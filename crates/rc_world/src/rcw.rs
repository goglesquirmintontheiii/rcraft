//! RCW on-disk format: region files, chunk records and the world header.
//!
//! Full specification: `docs/WORLD_FORMAT.md`. Region files are append-only:
//! a chunk rewrite appends a new record and repoints the header entry, so a
//! crash mid-write leaves the previous version intact. Every record carries an
//! xxh3-64 checksum.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use ahash::AHashMap;
use anyhow::{Context as _, Result, bail, ensure};
use parking_lot::Mutex;
use rc_core::{BiomeId, BlockStateId, ChunkPos, SECTION_COUNT};
use serde::{Deserialize, Serialize};

use crate::{Chunk, PalettedContainer, Section};

pub const REGION_MAGIC: &[u8; 4] = b"RCWR";
pub const CHUNK_MAGIC: &[u8; 4] = b"RCWC";
pub const WORLD_MAGIC: &[u8; 4] = b"RCWW";
pub const FORMAT_VERSION: u16 = 1;
pub const SECTOR_BYTES: usize = 4096;
/// 4 sectors: 8-byte preamble + 1024 × 12-byte chunk table entries (12 296 B), padded.
pub const REGION_HEADER_BYTES: usize = 4 * SECTOR_BYTES;
const _: () = assert!(CHUNKS_PER_REGION * 12 + 8 <= REGION_HEADER_BYTES);
pub const CHUNKS_PER_REGION: usize = 32 * 32;
const HEADER_SECTORS: u32 = (REGION_HEADER_BYTES / SECTOR_BYTES) as u32;
/// Record header: magic 4 + checksum 8 + cx 4 + cz 4 + data_ver 4 + status 1 +
/// flags 1 + raw_len 4.
const RECORD_HEADER: usize = 30;
const ZSTD_LEVEL: i32 = 3;

/// Checksum used for every record.
#[must_use]
pub fn checksum(bytes: &[u8]) -> u64 {
    xxhash_rust::xxh3::xxh3_64(bytes)
}

// ---------------------------------------------------------------------------
// Id mapping
// ---------------------------------------------------------------------------

/// Translates between runtime registry ids and the world's stable ids.
#[derive(Clone, Debug, Default)]
pub struct IdMap {
    pub block_to_stable: Vec<u32>,
    pub block_from_stable: Vec<BlockStateId>,
    pub biome_to_stable: Vec<u32>,
    pub biome_from_stable: Vec<BiomeId>,
}

impl IdMap {
    /// Identity mapping for `blocks` states and `biomes` biomes.
    #[must_use]
    pub fn identity(blocks: usize, biomes: usize) -> Self {
        Self {
            block_to_stable: (0..blocks as u32).collect(),
            block_from_stable: (0..blocks as u32).map(BlockStateId).collect(),
            biome_to_stable: (0..biomes as u32).collect(),
            biome_from_stable: (0..biomes as u32).map(BiomeId).collect(),
        }
    }

    /// Build from the world's stable name tables and the runtime resolvers.
    /// Unknown names map to `fallback`.
    pub fn from_tables(
        block_names: &[String],
        biome_names: &[String],
        runtime_blocks: usize,
        runtime_biomes: usize,
        resolve_block: impl Fn(&str) -> Option<BlockStateId>,
        resolve_biome: impl Fn(&str) -> Option<BiomeId>,
    ) -> Self {
        let mut m = Self {
            block_to_stable: vec![0; runtime_blocks],
            block_from_stable: Vec::with_capacity(block_names.len()),
            biome_to_stable: vec![0; runtime_biomes],
            biome_from_stable: Vec::with_capacity(biome_names.len()),
        };
        for (i, n) in block_names.iter().enumerate() {
            let id = resolve_block(n).unwrap_or(BlockStateId::AIR);
            m.block_from_stable.push(id);
            if let Some(slot) = m.block_to_stable.get_mut(id.0 as usize) {
                *slot = i as u32;
            }
        }
        for (i, n) in biome_names.iter().enumerate() {
            let id = resolve_biome(n).unwrap_or(BiomeId(0));
            m.biome_from_stable.push(id);
            if let Some(slot) = m.biome_to_stable.get_mut(id.0 as usize) {
                *slot = i as u32;
            }
        }
        m
    }
}

// ---------------------------------------------------------------------------
// Chunk codec
// ---------------------------------------------------------------------------

struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i16(&mut self, v: i16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        ensure!(self.p + n <= self.b.len(), "truncated chunk payload");
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into()?))
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into()?))
    }
}

fn write_container<const N: usize>(
    w: &mut Writer,
    c: &PalettedContainer<N>,
    map: &[u32],
    wide_len: bool,
) {
    // Direct containers store runtime ids; write them as a wide palette of
    // stable ids so the on-disk form is always palette-indexed.
    if c.bits() >= PalettedContainer::<N>::DIRECT_BITS {
        let mut palette: Vec<u32> = Vec::new();
        let mut lookup: AHashMap<u32, u32> = AHashMap::new();
        let idx: Vec<u32> = (0..N)
            .map(|i| {
                let v = map.get(c.get(i) as usize).copied().unwrap_or(0);
                *lookup.entry(v).or_insert_with(|| {
                    palette.push(v);
                    palette.len() as u32 - 1
                })
            })
            .collect();
        let bits = (u32::BITS - (palette.len() as u32 - 1).leading_zeros()).max(4) as usize;
        let per = 64 / bits;
        let mut data = vec![0u64; N.div_ceil(per)];
        for (i, &v) in idx.iter().enumerate() {
            data[i / per] |= u64::from(v) << ((i % per) * bits);
        }
        w.u16(palette.len() as u16);
        for p in palette {
            w.u32(p);
        }
        w.u8(bits as u8);
        for d in data {
            w.u64(d);
        }
        return;
    }
    if wide_len {
        w.u16(c.palette().len() as u16);
    } else {
        w.u8(c.palette().len() as u8);
    }
    for &p in c.palette() {
        w.u32(map.get(p as usize).copied().unwrap_or(0));
    }
    w.u8(c.bits());
    for &d in c.raw_data() {
        w.u64(d);
    }
}

fn read_container<const N: usize, T: Copy>(
    r: &mut Reader<'_>,
    map: &[T],
    to_u32: impl Fn(T) -> u32,
    wide_len: bool,
) -> Result<PalettedContainer<N>> {
    let len = if wide_len {
        r.u16()? as usize
    } else {
        r.u8()? as usize
    };
    let mut palette = Vec::with_capacity(len);
    for _ in 0..len {
        let s = r.u32()? as usize;
        palette.push(map.get(s).map_or(0, |&v| to_u32(v)));
    }
    let bits = r.u8()?;
    let words = if bits == 0 {
        0
    } else {
        N.div_ceil(64 / bits as usize)
    };
    let mut data = Vec::with_capacity(words);
    for _ in 0..words {
        data.push(r.u64()?);
    }
    // Two stable ids may map to the same runtime id; from_parts tolerates a
    // palette with duplicates.
    PalettedContainer::from_parts(palette, bits, data).context("invalid paletted container")
}

fn write_light(w: &mut Writer, l: Option<&[u8; 2048]>) {
    match l {
        None => w.u8(0),
        Some(l) if l.iter().all(|&b| b == l[0]) && (l[0] >> 4) == (l[0] & 15) => {
            w.u8(1);
            w.u8(l[0] & 15);
        }
        Some(l) => {
            w.u8(2);
            w.0.extend_from_slice(l);
        }
    }
}

fn read_light(r: &mut Reader<'_>) -> Result<Option<Box<[u8; 2048]>>> {
    Ok(match r.u8()? {
        0 => None,
        1 => {
            let v = r.u8()? & 15;
            Some(Box::new([v | (v << 4); 2048]))
        }
        2 => {
            let mut b = Box::new([0u8; 2048]);
            b.copy_from_slice(r.take(2048)?);
            Some(b)
        }
        t => bail!("bad light tag {t}"),
    })
}

/// Generation/lighting status persisted with a chunk.
pub mod status {
    pub const FEATURES: u8 = 4;
    pub const LIGHT: u8 = 5;
    pub const FULL: u8 = 6;
}

/// Serialize a chunk column into a full record (header + compressed payload).
#[must_use]
pub fn encode_chunk(c: &Chunk, map: &IdMap, data_version: u32) -> Vec<u8> {
    let mut w = Writer(Vec::with_capacity(64 * 1024));
    w.u32(SECTION_COUNT as u32);
    for s in c.sections.iter() {
        write_container(&mut w, &s.blocks, &map.block_to_stable, true);
        write_container(&mut w, &s.biomes, &map.biome_to_stable, false);
        write_light(&mut w, s.sky_light.as_deref());
        write_light(&mut w, s.block_light.as_deref());
    }
    for &h in &c.height_surface {
        w.i16(h);
    }
    w.u32(c.block_entities.len() as u32);
    for (pos, data) in &c.block_entities {
        w.u32(*pos);
        w.u32(data.len() as u32);
        w.0.extend_from_slice(data);
    }
    w.u32(c.entities.len() as u32);
    for data in &c.entities {
        w.u32(data.len() as u32);
        w.0.extend_from_slice(data);
    }
    // scheduled ticks, post-processing
    w.u32(0);
    w.u32(0);
    let raw = w.0;
    let comp = zstd::bulk::compress(&raw, ZSTD_LEVEL).expect("zstd compress");
    let mut out = Vec::with_capacity(RECORD_HEADER + comp.len());
    out.extend_from_slice(CHUNK_MAGIC);
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&c.pos.x.to_le_bytes());
    out.extend_from_slice(&c.pos.z.to_le_bytes());
    out.extend_from_slice(&data_version.to_le_bytes());
    out.push(if c.light_valid {
        status::FULL
    } else {
        status::FEATURES
    });
    out.push(u8::from(c.light_valid));
    out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    out.extend_from_slice(&comp);
    let sum = checksum(&out[12..]);
    out[4..12].copy_from_slice(&sum.to_le_bytes());
    out
}

/// Parse a record produced by [`encode_chunk`].
pub fn decode_chunk(rec: &[u8], map: &IdMap) -> Result<Chunk> {
    ensure!(rec.len() >= RECORD_HEADER, "record too short");
    ensure!(&rec[0..4] == CHUNK_MAGIC, "bad chunk magic");
    let sum = u64::from_le_bytes(rec[4..12].try_into()?);
    ensure!(checksum(&rec[12..]) == sum, "chunk checksum mismatch");
    let cx = i32::from_le_bytes(rec[12..16].try_into()?);
    let cz = i32::from_le_bytes(rec[16..20].try_into()?);
    let flags = rec[25];
    let raw_len = u32::from_le_bytes(rec[26..30].try_into()?) as usize;
    let raw = zstd::bulk::decompress(&rec[RECORD_HEADER..], raw_len)?;
    let mut r = Reader { b: &raw, p: 0 };
    let n = r.u32()? as usize;
    ensure!(n == SECTION_COUNT, "section count {n} unsupported");
    let mut chunk = Chunk::new_empty(ChunkPos::new(cx, cz));
    for i in 0..n {
        let blocks = read_container::<4096, _>(&mut r, &map.block_from_stable, |b| b.0, true)?;
        let biomes = read_container::<64, _>(&mut r, &map.biome_from_stable, |b| b.0, false)?;
        let sky_light = read_light(&mut r)?;
        let block_light = read_light(&mut r)?;
        chunk.sections[i] = Section {
            blocks,
            biomes,
            sky_light,
            block_light,
        };
    }
    for h in &mut chunk.height_surface {
        *h = r.i16()?;
    }
    if r.p < raw.len() {
        let n = r.u32()? as usize;
        for _ in 0..n {
            let pos = r.u32()?;
            let len = r.u32()? as usize;
            chunk.block_entities.push((pos, r.take(len)?.to_vec()));
        }
        let n = r.u32()? as usize;
        for _ in 0..n {
            let len = r.u32()? as usize;
            chunk.entities.push(r.take(len)?.to_vec());
        }
    }
    chunk.light_valid = flags & 1 != 0;
    chunk.dirty = false;
    Ok(chunk)
}

// ---------------------------------------------------------------------------
// Region files
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Entry {
    /// In sectors; 0 ⇒ absent.
    offset: u32,
    length: u32,
    ver: u32,
}

/// One open `.rcr` file.
pub struct RegionFile {
    file: File,
    table: Box<[Entry; CHUNKS_PER_REGION]>,
    end_sector: u32,
}

impl std::fmt::Debug for RegionFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RegionFile({} sectors)", self.end_sector)
    }
}

#[inline]
fn local_index(pos: ChunkPos) -> usize {
    ((pos.z & 31) * 32 + (pos.x & 31)) as usize
}

impl RegionFile {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .with_context(|| format!("opening {}", path.display()))?;
        let len = file.metadata()?.len();
        let mut table = Box::new([Entry::default(); CHUNKS_PER_REGION]);
        if len < REGION_HEADER_BYTES as u64 {
            let mut h = vec![0u8; REGION_HEADER_BYTES];
            h[0..4].copy_from_slice(REGION_MAGIC);
            h[4..6].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
            file.seek(SeekFrom::Start(0))?;
            file.write_all(&h)?;
            file.set_len(REGION_HEADER_BYTES as u64)?;
            return Ok(Self {
                file,
                table,
                end_sector: HEADER_SECTORS,
            });
        }
        let mut h = vec![0u8; REGION_HEADER_BYTES];
        file.seek(SeekFrom::Start(0))?;
        file.read_exact(&mut h)?;
        ensure!(
            &h[0..4] == REGION_MAGIC,
            "{} is not an RCW region",
            path.display()
        );
        let ver = u16::from_le_bytes([h[4], h[5]]);
        ensure!(
            ver <= FORMAT_VERSION,
            "region format {ver} is newer than supported"
        );
        let mut end = HEADER_SECTORS;
        for (i, e) in table.iter_mut().enumerate() {
            let o = 8 + i * 12;
            *e = Entry {
                offset: u32::from_le_bytes(h[o..o + 4].try_into()?),
                length: u32::from_le_bytes(h[o + 4..o + 8].try_into()?),
                ver: u32::from_le_bytes(h[o + 8..o + 12].try_into()?),
            };
            if e.offset != 0 {
                end = end.max(e.offset + (e.length as usize).div_ceil(SECTOR_BYTES) as u32);
            }
        }
        let file_sectors = len.div_ceil(SECTOR_BYTES as u64) as u32;
        Ok(Self {
            file,
            table,
            end_sector: end.max(file_sectors),
        })
    }

    #[must_use]
    pub fn contains(&self, pos: ChunkPos) -> bool {
        self.table[local_index(pos)].offset != 0
    }

    /// Raw record bytes for `pos`, if present.
    pub fn read(&mut self, pos: ChunkPos) -> Result<Option<Vec<u8>>> {
        let e = self.table[local_index(pos)];
        if e.offset == 0 {
            return Ok(None);
        }
        let mut buf = vec![0u8; e.length as usize];
        self.file
            .seek(SeekFrom::Start(u64::from(e.offset) * SECTOR_BYTES as u64))?;
        self.file.read_exact(&mut buf)?;
        Ok(Some(buf))
    }

    /// Append a record for `pos` and repoint the table entry.
    pub fn write(&mut self, pos: ChunkPos, record: &[u8]) -> Result<()> {
        let i = local_index(pos);
        let sectors = record.len().div_ceil(SECTOR_BYTES);
        let off = self.end_sector;
        let mut padded = Vec::with_capacity(sectors * SECTOR_BYTES);
        padded.extend_from_slice(record);
        padded.resize(sectors * SECTOR_BYTES, 0);
        self.file
            .seek(SeekFrom::Start(u64::from(off) * SECTOR_BYTES as u64))?;
        self.file.write_all(&padded)?;
        self.end_sector = off + sectors as u32;
        let e = Entry {
            offset: off,
            length: record.len() as u32,
            ver: self.table[i].ver.wrapping_add(1),
        };
        self.table[i] = e;
        let mut b = [0u8; 12];
        b[0..4].copy_from_slice(&e.offset.to_le_bytes());
        b[4..8].copy_from_slice(&e.length.to_le_bytes());
        b[8..12].copy_from_slice(&e.ver.to_le_bytes());
        self.file.seek(SeekFrom::Start(8 + i as u64 * 12))?;
        self.file.write_all(&b)?;
        Ok(())
    }

    /// Fraction of the body occupied by superseded records.
    #[must_use]
    pub fn dead_fraction(&self) -> f64 {
        let live: u64 = self
            .table
            .iter()
            .filter(|e| e.offset != 0)
            .map(|e| (e.length as usize).div_ceil(SECTOR_BYTES) as u64)
            .sum();
        let body = u64::from(self.end_sector - HEADER_SECTORS);
        if body == 0 {
            0.0
        } else {
            1.0 - live as f64 / body as f64
        }
    }

    pub fn flush(&mut self) -> Result<()> {
        self.file.sync_data()?;
        Ok(())
    }

    /// Rewrite `path` keeping only live records.
    pub fn compact(path: &Path) -> Result<()> {
        let mut old = Self::open(path)?;
        let tmp = path.with_extension("rcr.tmp");
        let _ = std::fs::remove_file(&tmp);
        {
            let mut new = Self::open(&tmp)?;
            for i in 0..CHUNKS_PER_REGION {
                let pos = ChunkPos::new((i % 32) as i32, (i / 32) as i32);
                if let Some(rec) = old.read(pos)? {
                    new.write(pos, &rec)?;
                }
            }
            new.flush()?;
        }
        drop(old);
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

/// Region-file cache for one dimension directory.
pub struct DimStorage {
    dir: PathBuf,
    regions: Mutex<AHashMap<(i32, i32), RegionFile>>,
}

impl std::fmt::Debug for DimStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DimStorage({})", self.dir.display())
    }
}

impl DimStorage {
    pub fn new(dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            regions: Mutex::new(AHashMap::new()),
        })
    }

    fn region_path(&self, rx: i32, rz: i32) -> PathBuf {
        self.dir.join(format!("r.{rx}.{rz}.rcr"))
    }

    fn with_region<T>(
        &self,
        pos: ChunkPos,
        create: bool,
        f: impl FnOnce(&mut RegionFile) -> Result<T>,
    ) -> Result<Option<T>> {
        let key = (pos.x >> 5, pos.z >> 5);
        let mut regions = self.regions.lock();
        if !regions.contains_key(&key) {
            let path = self.region_path(key.0, key.1);
            if !create && !path.exists() {
                return Ok(None);
            }
            if regions.len() >= 64 {
                regions.clear();
            }
            regions.insert(key, RegionFile::open(&path)?);
        }
        f(regions.get_mut(&key).expect("just inserted")).map(Some)
    }

    /// Raw record for a chunk.
    pub fn read_record(&self, pos: ChunkPos) -> Result<Option<Vec<u8>>> {
        Ok(self.with_region(pos, false, |r| r.read(pos))?.flatten())
    }

    pub fn write_record(&self, pos: ChunkPos, rec: &[u8]) -> Result<()> {
        self.with_region(pos, true, |r| r.write(pos, rec))?;
        Ok(())
    }

    /// Load and decode a chunk; corrupt records are reported and ignored.
    pub fn load(&self, pos: ChunkPos, map: &IdMap) -> Result<Option<Chunk>> {
        match self.read_record(pos)? {
            None => Ok(None),
            Some(rec) => decode_chunk(&rec, map).map(Some),
        }
    }

    pub fn save(&self, c: &Chunk, map: &IdMap, data_version: u32) -> Result<()> {
        let rec = encode_chunk(c, map, data_version);
        self.write_record(c.pos, &rec)
    }

    /// Flush and compact regions with too much dead space.
    pub fn close(&self) -> Result<()> {
        let mut regions = self.regions.lock();
        let mut compact = Vec::new();
        for (k, r) in regions.iter_mut() {
            r.flush()?;
            if r.dead_fraction() > 0.25 {
                compact.push(*k);
            }
        }
        regions.clear();
        for (rx, rz) in compact {
            RegionFile::compact(&self.region_path(rx, rz))?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// World header
// ---------------------------------------------------------------------------

/// Persistent per-world data (`world.rcw`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorldMeta {
    pub name: String,
    pub seed: i64,
    pub data_version: u32,
    pub game_version: String,
    pub spawn: [i32; 3],
    pub time: i64,
    pub day_time: i64,
    pub last_played_ms: u64,
    #[serde(default)]
    pub game_mode: String,
    #[serde(default)]
    pub difficulty: String,
    #[serde(default)]
    pub player: Option<serde_json::Value>,
    /// Index ⇒ stable id used in region files.
    pub block_states: Vec<String>,
    pub biomes: Vec<String>,
    #[serde(default)]
    pub raining: bool,
    #[serde(default)]
    pub thundering: bool,
    #[serde(default)]
    pub rain_time: i64,
    #[serde(default)]
    pub thunder_time: i64,
    #[serde(default)]
    pub clear_weather_time: i64,
    /// Game rules as strings (vanilla names, e.g. `doDaylightCycle`).
    #[serde(default)]
    pub rules: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub world_type: String,
}

impl WorldMeta {
    pub fn write(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_vec(self)?;
        let comp = zstd::bulk::compress(&json, 9)?;
        let mut out = Vec::with_capacity(comp.len() + 22);
        out.extend_from_slice(WORLD_MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&checksum(&comp).to_le_bytes());
        out.extend_from_slice(&(json.len() as u64).to_le_bytes());
        out.extend_from_slice(&comp);
        let tmp = path.with_extension("rcw.tmp");
        std::fs::write(&tmp, &out)?;
        if path.exists() {
            let _ = std::fs::copy(path, path.with_extension("rcw.bak"));
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn read(path: &Path) -> Result<Self> {
        match Self::read_one(path) {
            Ok(m) => Ok(m),
            Err(e) => {
                let bak = path.with_extension("rcw.bak");
                Self::read_one(&bak).map_err(|_| e)
            }
        }
    }

    fn read_one(path: &Path) -> Result<Self> {
        let b = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        ensure!(
            b.len() >= 22 && &b[0..4] == WORLD_MAGIC,
            "not an RCW world header"
        );
        let ver = u16::from_le_bytes([b[4], b[5]]);
        ensure!(
            ver <= FORMAT_VERSION,
            "world format {ver} is newer than supported"
        );
        let sum = u64::from_le_bytes(b[6..14].try_into()?);
        let len = u64::from_le_bytes(b[14..22].try_into()?) as usize;
        ensure!(checksum(&b[22..]) == sum, "world header checksum mismatch");
        let json = zstd::bulk::decompress(&b[22..], len)?;
        Ok(serde_json::from_slice(&json)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rc_core::BlockPos;

    #[test]
    fn checksum_is_stable() {
        assert_eq!(super::checksum(b""), 0x2d06_8005_38d3_94c2);
    }

    fn sample_chunk(pos: ChunkPos) -> Chunk {
        let mut c = Chunk::new_empty(pos);
        for x in 0..16 {
            for z in 0..16 {
                for y in -64..(x * 3 + z) {
                    let id = BlockStateId(1 + ((x + y + z) as u32 % 7));
                    c.set_block(BlockPos::new(pos.x * 16 + x, y, pos.z * 16 + z), id);
                }
            }
        }
        c.sections[3].sky_light = Some(Box::new([0xF3; 2048]));
        c.sections[20].sky_light = Some(Box::new([0xFF; 2048]));
        c.light_valid = true;
        c
    }

    #[test]
    fn chunk_round_trip() {
        let map = IdMap::identity(100, 10);
        let c = sample_chunk(ChunkPos::new(-5, 9));
        let rec = encode_chunk(&c, &map, 3955);
        let d = decode_chunk(&rec, &map).unwrap();
        assert_eq!(d.pos, c.pos);
        assert!(d.light_valid);
        for (a, b) in c.sections.iter().zip(d.sections.iter()) {
            for i in 0..4096 {
                assert_eq!(a.blocks.get(i), b.blocks.get(i));
            }
            assert_eq!(a.sky_light, b.sky_light);
        }
        assert_eq!(c.height_surface, d.height_surface);
        let mut bad = rec.clone();
        let n = bad.len();
        bad[n - 3] ^= 1;
        assert!(decode_chunk(&bad, &map).is_err());
    }

    #[test]
    fn region_append_and_compact() {
        let dir = std::env::temp_dir().join(format!("rcw-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let st = DimStorage::new(dir.clone()).unwrap();
        let map = IdMap::identity(100, 10);
        for i in 0..3 {
            for x in -2..2 {
                let mut c = sample_chunk(ChunkPos::new(x, 33));
                c.set_block(BlockPos::new(x * 16, 200, 33 * 16), BlockStateId(50 + i));
                st.save(&c, &map, 1).unwrap();
            }
        }
        let c = st.load(ChunkPos::new(-1, 33), &map).unwrap().unwrap();
        assert_eq!(c.get_block(BlockPos::new(-16, 200, 528)), BlockStateId(52));
        assert!(st.load(ChunkPos::new(5, 5), &map).unwrap().is_none());
        st.close().unwrap();
        let st = DimStorage::new(dir.clone()).unwrap();
        let c = st.load(ChunkPos::new(1, 33), &map).unwrap().unwrap();
        assert_eq!(c.get_block(BlockPos::new(16, 200, 528)), BlockStateId(52));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn world_meta_round_trip() {
        let dir = std::env::temp_dir().join(format!("rcw-meta-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("world.rcw");
        let m = WorldMeta {
            name: "Test".into(),
            seed: -42,
            data_version: 3955,
            game_version: "0.1.0".into(),
            spawn: [1, 70, 2],
            time: 5,
            day_time: 6000,
            last_played_ms: 0,
            game_mode: "survival".into(),
            difficulty: "normal".into(),
            player: None,
            block_states: vec!["minecraft:air".into()],
            biomes: vec!["minecraft:plains".into()],
            raining: false,
            thundering: false,
            rain_time: 0,
            thunder_time: 0,
            clear_weather_time: 0,
            rules: Default::default(),
            world_type: String::new(),
        };
        m.write(&p).unwrap();
        m.write(&p).unwrap();
        let r = WorldMeta::read(&p).unwrap();
        assert_eq!(r.seed, -42);
        assert_eq!(r.spawn, [1, 70, 2]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
