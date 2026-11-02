//! `.mca` region files: 1024 chunk slots, 4 KiB sectors, per-chunk compression.

use std::io::Read;
use std::path::Path;

use anyhow::{Context as _, Result, bail};

use crate::nbt::{self, Tag};

pub struct RegionFile {
    data: Vec<u8>,
}

impl std::fmt::Debug for RegionFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RegionFile({} bytes)", self.data.len())
    }
}

impl RegionFile {
    pub fn open(path: &Path) -> Result<Self> {
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Ok(Self { data })
    }

    #[must_use]
    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self { data }
    }

    /// Whether slot `(x & 31, z & 31)` holds a chunk.
    #[must_use]
    pub fn has_chunk(&self, x: i32, z: i32) -> bool {
        let i = (((z & 31) << 5) | (x & 31)) as usize * 4;
        self.data.len() >= 4096 && self.data[i..i + 3] != [0, 0, 0]
    }

    /// Indices `(local_x, local_z)` of all present chunks.
    #[must_use]
    pub fn present(&self) -> Vec<(i32, i32)> {
        (0..1024)
            .filter(|&i| self.data.len() >= 4096 && self.data[i * 4..i * 4 + 3] != [0, 0, 0])
            .map(|i| ((i & 31) as i32, (i >> 5) as i32))
            .collect()
    }

    /// Read the raw (decompressed) chunk NBT bytes.
    pub fn read_raw(&self, x: i32, z: i32) -> Result<Option<Vec<u8>>> {
        let i = (((z & 31) << 5) | (x & 31)) as usize * 4;
        if self.data.len() < 8192 {
            return Ok(None);
        }
        let off = (u32::from(self.data[i]) << 16)
            | (u32::from(self.data[i + 1]) << 8)
            | u32::from(self.data[i + 2]);
        if off == 0 {
            return Ok(None);
        }
        let o = off as usize * 4096;
        if o + 5 > self.data.len() {
            bail!("chunk offset out of file");
        }
        let len = u32::from_be_bytes(self.data[o..o + 4].try_into()?) as usize;
        let comp = self.data[o + 4];
        let body = self
            .data
            .get(o + 5..o + 4 + len)
            .context("truncated chunk")?;
        let mut out = Vec::new();
        match comp & 0x7f {
            1 => {
                flate2::read::GzDecoder::new(body).read_to_end(&mut out)?;
            }
            2 => {
                flate2::read::ZlibDecoder::new(body).read_to_end(&mut out)?;
            }
            3 => out.extend_from_slice(body),
            c => bail!("unsupported chunk compression {c}"),
        }
        Ok(Some(out))
    }

    pub fn read_chunk(&self, x: i32, z: i32) -> Result<Option<Tag>> {
        match self.read_raw(x, z)? {
            Some(b) => Ok(Some(nbt::parse(&b)?)),
            None => Ok(None),
        }
    }
}

/// Writes a complete `.mca` region from chunk NBT documents.
#[derive(Debug, Default)]
pub struct RegionWriter {
    chunks: Vec<((i32, i32), Vec<u8>)>,
}

impl RegionWriter {
    /// Add a chunk (local coords 0..32) as an NBT document.
    pub fn add(&mut self, x: i32, z: i32, tag: &Tag) -> Result<()> {
        use std::io::Write as _;
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(&nbt::write(tag))?;
        self.chunks.push(((x & 31, z & 31), e.finish()?));
        Ok(())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Serialize the region file.
    #[must_use]
    pub fn finish(self, timestamp: u32) -> Vec<u8> {
        let mut out = vec![0u8; 8192];
        let mut sector = 2u32;
        for ((x, z), data) in self.chunks {
            let i = ((z << 5) | x) as usize * 4;
            let len = data.len() + 1;
            let sectors = (len + 4).div_ceil(4096) as u32;
            out[i] = (sector >> 16) as u8;
            out[i + 1] = (sector >> 8) as u8;
            out[i + 2] = sector as u8;
            out[i + 3] = sectors.min(255) as u8;
            out[4096 + i..4096 + i + 4].copy_from_slice(&timestamp.to_be_bytes());
            out.extend_from_slice(&(len as u32).to_be_bytes());
            out.push(2);
            out.extend_from_slice(&data);
            out.resize((sector + sectors) as usize * 4096, 0);
            sector += sectors;
        }
        out
    }
}
