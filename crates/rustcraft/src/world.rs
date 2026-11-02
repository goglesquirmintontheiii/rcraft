//! The loaded world: chunk cache, background generation, lighting, meshing
//! and saving.
//!
//! Threads:
//! - a **generation thread** owns the worldgen [`ChunkStore`] (decoration
//!   needs exclusive 3×3 windows); terrain noise inside it fans out on the
//!   worker pool;
//! - the **worker pool** (sized from the CPU limit option) runs chunk loads,
//!   lighting and meshing jobs;
//! - an **IO thread** writes region files.
//!
//! The main thread only applies finished results, so frames never wait on
//! chunk work.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ahash::{AHashMap, AHashSet};
use anyhow::{Context as _, Result};
use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::Mutex;
use rc_core::{BiomeId, BlockPos, BlockStateId, ChunkPos, SECTION_COUNT, WORLD_MIN_Y};
use rc_render::mesh::{BiomeColors, MeshTables, SectionMesh, Snapshot, mesh_section};
use rc_world::light::{self, ChunkLight};
use rc_world::rcw::{DimStorage, IdMap, WorldMeta};
use rc_world::{Chunk, PalettedContainer};
use rc_worldgen::pipeline::ChunkStore;
use rc_worldgen::{Dimension, DimensionGen, ProtoChunk, Status};
use tracing::{info, warn};

/// Shared, read-only data the mesher needs.
pub struct MeshShared {
    pub tables: MeshTables,
    pub models: Arc<rc_assets::BlockModels>,
    pub colors: BiomeColors,
    pub smooth: bool,
}

enum Done {
    Loaded(ChunkPos, Option<Chunk>),
    Generated(Chunk),
    Lit(ChunkPos, u32, ChunkLight, (usize, usize)),
    Meshed(ChunkPos, usize, u32, SectionMesh),
}

enum GenMsg {
    Want(ChunkPos),
    Cancel(ChunkPos),
    Stop,
}

enum IoMsg {
    Save(Arc<Chunk>),
    Flush(Sender<()>),
    Stop,
}

/// Convert a finished worldgen chunk into a runtime chunk column.
#[must_use]
pub fn export_chunk(p: &ProtoChunk) -> Chunk {
    let mut c = Chunk::new_empty(p.pos);
    let mut vals = vec![0u32; 4096];
    let mut bvals = vec![0u32; 64];
    for (s, sec) in c.sections.iter_mut().enumerate() {
        let y0 = s * 16;
        if (y0 as i32) >= p.height {
            break;
        }
        let base = y0 * 256;
        for (i, v) in vals.iter_mut().enumerate() {
            *v = p.blocks[base + i].0;
        }
        sec.blocks = PalettedContainer::from_values(&vals);
        let qbase = s * 64;
        for (i, v) in bvals.iter_mut().enumerate() {
            *v = p.biomes.get(qbase + i).map_or(0, |b| b.0);
        }
        sec.biomes = PalettedContainer::from_values(&bvals);
    }
    for z in 0..16 {
        for x in 0..16 {
            let mut h = WORLD_MIN_Y - 1;
            for y in (0..p.height).rev() {
                if p.blocks[((y as usize * 16 + z) * 16) + x].0 != 0 {
                    h = p.min_y + y;
                    break;
                }
            }
            c.height_surface[z * 16 + x] = h as i16;
        }
    }
    c.dirty = true;
    c
}

fn gen_thread(
    dg: Arc<DimensionGen>,
    pool: Arc<rayon::ThreadPool>,
    rx: Receiver<GenMsg>,
    tx: Sender<Done>,
    center: Arc<Mutex<(ChunkPos, i32)>>,
) {
    let mut store = ChunkStore::new(&dg);
    let mut wanted: AHashSet<ChunkPos> = AHashSet::new();
    let mut finalized: AHashSet<ChunkPos> = AHashSet::new();
    let mut since_evict = 0u32;
    loop {
        // Drain messages; block only when idle.
        let msg = if wanted.is_empty() {
            match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => return,
            }
        } else {
            rx.try_recv().ok()
        };
        let mut stop = false;
        let mut handle = |m: GenMsg, wanted: &mut AHashSet<ChunkPos>| match m {
            GenMsg::Want(p) => {
                wanted.insert(p);
            }
            GenMsg::Cancel(p) => {
                wanted.remove(&p);
            }
            GenMsg::Stop => stop = true,
        };
        if let Some(m) = msg {
            handle(m, &mut wanted);
        }
        while let Ok(m) = rx.try_recv() {
            handle(m, &mut wanted);
        }
        if stop {
            return;
        }
        let (c, rd) = *center.lock();
        let Some(&target) = wanted
            .iter()
            .min_by_key(|p| ((p.x - c.x).pow(2) + (p.z - c.z).pow(2), p.x, p.z))
        else {
            continue;
        };
        wanted.remove(&target);
        // Decorate the 3×3 around the target (needs 5×5 carved).
        let mut need_dec = Vec::new();
        for dx in -1..=1 {
            for dz in -1..=1 {
                let n = ChunkPos::new(target.x + dx, target.z + dz);
                if finalized.contains(&n) {
                    continue;
                }
                if store
                    .chunks
                    .get(&n)
                    .is_some_and(|ch| ch.status >= Status::Features)
                {
                    continue;
                }
                need_dec.push(n);
            }
        }
        let mut carve = Vec::new();
        for n in &need_dec {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    let m = ChunkPos::new(n.x + dx, n.z + dz);
                    if !finalized.contains(&m) && !store.chunks.contains_key(&m) {
                        carve.push(m);
                    }
                }
            }
        }
        carve.sort_by_key(|p| (p.x, p.z));
        carve.dedup();
        pool.install(|| store.ensure_carved(&carve));
        need_dec.sort_by_key(|p| (p.x, p.z));
        for n in need_dec {
            store.decorate(n);
        }
        if let Some(pc) = store.chunks.remove(&target) {
            let chunk = export_chunk(&pc);
            finalized.insert(target);
            if tx.send(Done::Generated(chunk)).is_err() {
                return;
            }
        }
        // Evict far work-in-progress chunks to bound memory.
        since_evict += 1;
        if since_evict >= 16 {
            since_evict = 0;
            let keep = rd + 6;
            let far: Vec<ChunkPos> = store
                .chunks
                .keys()
                .copied()
                .filter(|p| (p.x - c.x).abs() > keep || (p.z - c.z).abs() > keep)
                .collect();
            for p in far {
                if let Some(ch) = store.chunks.remove(&p)
                    && ch.status >= Status::Features
                {
                    // Neighbours finalized while this chunk was decorated must
                    // be redecorated if this chunk is ever regenerated.
                    for dx in -1..=1 {
                        for dz in -1..=1 {
                            finalized.remove(&ChunkPos::new(p.x + dx, p.z + dz));
                        }
                    }
                }
            }
            finalized.retain(|p| (p.x - c.x).abs() <= keep * 2 && (p.z - c.z).abs() <= keep * 2);
        }
    }
}

fn io_thread(storage: Arc<DimStorage>, map: Arc<IdMap>, rx: Receiver<IoMsg>) {
    while let Ok(m) = rx.recv() {
        match m {
            IoMsg::Save(c) => {
                if let Err(e) = storage.save(&c, &map, rc_core::version::TARGET_DATA_VERSION as u32)
                {
                    warn!("saving chunk {:?}: {e:#}", c.pos);
                }
            }
            IoMsg::Flush(done) => {
                if let Err(e) = storage.close() {
                    warn!("flushing regions: {e:#}");
                }
                let _ = done.send(());
            }
            IoMsg::Stop => {
                let _ = storage.close();
                return;
            }
        }
    }
}

/// Per-chunk bookkeeping.
#[derive(Default, Clone, Copy)]
struct State {
    version: u32,
    light_pending: bool,
    lit_version: u32,
    meshed_version: u32,
    mesh_pending: u32,
    section_ver: [u32; SECTION_COUNT],
}

/// The single-player world.
pub struct World {
    pub dir: PathBuf,
    pub meta: WorldMeta,
    storage: Arc<DimStorage>,
    map: Arc<IdMap>,
    pub chunks: AHashMap<ChunkPos, Arc<Chunk>>,
    state: AHashMap<ChunkPos, State>,
    requested: AHashSet<ChunkPos>,
    in_gen: AHashSet<ChunkPos>,
    pool: Arc<rayon::ThreadPool>,
    done_tx: Sender<Done>,
    done_rx: Receiver<Done>,
    gen_tx: Sender<GenMsg>,
    io_tx: Sender<IoMsg>,
    gen_handle: Option<JoinHandle<()>>,
    io_handle: Option<JoinHandle<()>>,
    center: Arc<Mutex<(ChunkPos, i32)>>,
    mesh: Arc<MeshShared>,
    /// Finished meshes for the renderer: (section origin, mesh).
    pub mesh_out: Vec<([i32; 3], SectionMesh)>,
    /// Chunks dropped from memory (renderer frees their sections).
    pub unloaded: Vec<ChunkPos>,
    /// Sections queued for remeshing: (chunk, lo, hi) section range.
    remesh: AHashMap<ChunkPos, (usize, usize)>,
    in_flight: usize,
    pub max_in_flight: usize,
    last_autosave: Instant,
    pub generated_count: u64,
    shutdown: Arc<AtomicBool>,
}

impl std::fmt::Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "World({}, {} chunks)", self.meta.name, self.chunks.len())
    }
}

/// Stable registry tables for a new world.
fn registry_tables() -> (Vec<String>, Vec<String>) {
    let b = rc_data::blocks();
    let blocks = (0..b.len() as u32)
        .map(|i| b.to_string(BlockStateId(i)))
        .collect();
    let reg = rc_worldgen::biome::registry();
    let biomes = reg.biomes.iter().map(|x| x.name.clone()).collect();
    (blocks, biomes)
}

/// Biome tint table using the colormaps (vanilla) or an approximation.
#[must_use]
pub fn biome_colors(
    grass_map: Option<&rc_assets::Image>,
    foliage_map: Option<&rc_assets::Image>,
) -> BiomeColors {
    let reg = rc_worldgen::biome::registry();
    let sample = |map: Option<&rc_assets::Image>, t: f32, d: f32, fallback: u32| -> u32 {
        let t = t.clamp(0.0, 1.0);
        let d = d.clamp(0.0, 1.0) * t;
        let i = ((1.0 - t) * 255.0) as u32;
        let j = ((1.0 - d) * 255.0) as u32;
        match map {
            Some(m) if m.w >= 256 && m.h >= 256 => {
                let c = m.get(i.min(255), j.min(255));
                (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2])
            }
            _ => {
                // Blend between dry yellow-green and lush green.
                let lerp = |a: u32, b: u32, k: f32| -> u32 {
                    let ch = |s: u32| {
                        ((a >> s & 255) as f32 * (1.0 - k) + (b >> s & 255) as f32 * k) as u32
                    };
                    (ch(16) << 16) | (ch(8) << 8) | ch(0)
                };
                let k = (d * 0.7 + (1.0 - t) * 0.3).clamp(0.0, 1.0);
                lerp(fallback, 0x4c_a0_2c, k)
            }
        }
    };
    let mut out = BiomeColors::default();
    for b in &reg.biomes {
        let mut grass = b
            .grass_color
            .unwrap_or_else(|| sample(grass_map, b.temperature, b.downfall, 0xbf_b7_55));
        let foliage = b
            .foliage_color
            .unwrap_or_else(|| sample(foliage_map, b.temperature, b.downfall, 0xae_a4_2a));
        match b.grass_modifier.as_str() {
            "dark_forest" => grass = ((grass & 0xfe_fe_fe) + 0x28_34_0a) >> 1,
            "swamp" => grass = 0x6a_70_39,
            _ => {}
        }
        out.grass.push(grass);
        out.foliage.push(foliage);
        out.water.push(b.water_color);
    }
    out
}

impl World {
    /// Open `dir`, creating a new world with `seed` if it has no header.
    pub fn open(
        dir: &Path,
        name: &str,
        seed: i64,
        threads: usize,
        mesh: Arc<MeshShared>,
    ) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let header = dir.join("world.rcw");
        let meta = if header.exists() {
            WorldMeta::read(&header).context("reading world header")?
        } else {
            let (block_states, biomes) = registry_tables();
            let m = WorldMeta {
                name: name.to_owned(),
                seed,
                data_version: rc_core::version::TARGET_DATA_VERSION as u32,
                game_version: rc_core::version::VERSION.to_owned(),
                spawn: [0, 64, 0],
                time: 0,
                day_time: 6000,
                last_played_ms: 0,
                game_mode: "survival".into(),
                difficulty: "normal".into(),
                player: None,
                block_states,
                biomes,
            };
            m.write(&header)?;
            m
        };
        let b = rc_data::blocks();
        let reg = rc_worldgen::biome::registry();
        let map = Arc::new(IdMap::from_tables(
            &meta.block_states,
            &meta.biomes,
            b.len(),
            reg.biomes.len(),
            |s| b.parse_state(s).ok(),
            |s| reg.id(s),
        ));
        let storage = Arc::new(DimStorage::new(dir.join("dim/overworld"))?);
        let dg = Arc::new(DimensionGen::new(Dimension::Overworld, meta.seed));
        let pool = Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads.max(1))
                .thread_name(|i| format!("rc-worker-{i}"))
                .build()?,
        );
        let (done_tx, done_rx) = unbounded();
        let (gen_tx, gen_rx) = unbounded();
        let (io_tx, io_rx) = unbounded();
        let center = Arc::new(Mutex::new((ChunkPos::new(0, 0), 8)));
        let gen_handle = {
            let dg = dg.clone();
            let pool = pool.clone();
            let tx = done_tx.clone();
            let center = center.clone();
            std::thread::Builder::new()
                .name("rc-worldgen".into())
                .spawn(move || gen_thread(dg, pool, gen_rx, tx, center))?
        };
        let io_handle = {
            let st = storage.clone();
            let map = map.clone();
            std::thread::Builder::new()
                .name("rc-io".into())
                .spawn(move || io_thread(st, map, io_rx))?
        };
        info!(name = %meta.name, seed = meta.seed, "world opened");
        Ok(Self {
            dir: dir.to_owned(),
            meta,
            storage,
            map,
            chunks: AHashMap::new(),
            state: AHashMap::new(),
            requested: AHashSet::new(),
            in_gen: AHashSet::new(),
            pool,
            done_tx,
            done_rx,
            gen_tx,
            io_tx,
            gen_handle: Some(gen_handle),
            io_handle: Some(io_handle),
            center,
            mesh,
            mesh_out: Vec::new(),
            unloaded: Vec::new(),
            remesh: AHashMap::new(),
            in_flight: 0,
            max_in_flight: threads.max(1) * 3,
            last_autosave: Instant::now(),
            generated_count: 0,
            shutdown: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn set_mesh_shared(&mut self, mesh: Arc<MeshShared>) {
        self.mesh = mesh;
        let all: Vec<ChunkPos> = self.chunks.keys().copied().collect();
        for p in all {
            self.queue_remesh(p, 0, SECTION_COUNT - 1);
        }
    }

    #[must_use]
    pub fn threads(&self) -> usize {
        self.pool.current_num_threads()
    }

    // ---- block access ----

    #[must_use]
    pub fn get_block(&self, p: BlockPos) -> BlockStateId {
        self.chunks
            .get(&ChunkPos::new(p.x >> 4, p.z >> 4))
            .map_or(BlockStateId::AIR, |c| c.get_block(p))
    }

    /// Whether the chunk containing `p` is loaded.
    #[must_use]
    pub fn is_loaded(&self, p: BlockPos) -> bool {
        self.chunks.contains_key(&ChunkPos::new(p.x >> 4, p.z >> 4))
    }

    #[must_use]
    pub fn light(&self, p: BlockPos) -> (u8, u8) {
        self.chunks
            .get(&ChunkPos::new(p.x >> 4, p.z >> 4))
            .map_or((15, 0), |c| light::light_at(c, p.x, p.y, p.z))
    }

    #[must_use]
    pub fn biome(&self, p: BlockPos) -> BiomeId {
        let Some(c) = self.chunks.get(&ChunkPos::new(p.x >> 4, p.z >> 4)) else {
            return BiomeId(0);
        };
        let sy = ((p.y - WORLD_MIN_Y) >> 4).clamp(0, SECTION_COUNT as i32 - 1) as usize;
        c.sections[sy].biome(
            (p.x & 15) as usize,
            (p.y & 15) as usize,
            (p.z & 15) as usize,
        )
    }

    /// Highest non-air block at a column, if loaded.
    #[must_use]
    pub fn surface_y(&self, x: i32, z: i32) -> Option<i32> {
        self.chunks
            .get(&ChunkPos::new(x >> 4, z >> 4))
            .map(|c| i32::from(c.height_surface[(((z & 15) << 4) | (x & 15)) as usize]))
    }

    /// Set a block, relight and remesh as needed. Returns false when the
    /// chunk is not loaded or `p` is out of bounds.
    pub fn set_block(&mut self, p: BlockPos, s: BlockStateId) -> bool {
        let cp = ChunkPos::new(p.x >> 4, p.z >> 4);
        let Some(c) = self.chunks.get_mut(&cp) else {
            return false;
        };
        let chunk = Arc::make_mut(c);
        if !chunk.set_block(p, s) {
            return false;
        }
        chunk.dirty = true;
        let sy = ((p.y - WORLD_MIN_Y) >> 4) as usize;
        let st = self.state.entry(cp).or_default();
        st.version += 1;
        // Immediate remesh of affected sections with the old light, then relight.
        let lx = p.x & 15;
        let lz = p.z & 15;
        let ly = (p.y - WORLD_MIN_Y) & 15;
        let lo = sy.saturating_sub(usize::from(ly == 0));
        let hi = (sy + usize::from(ly == 15)).min(SECTION_COUNT - 1);
        self.queue_remesh(cp, lo, hi);
        if lx == 0 {
            self.queue_remesh(ChunkPos::new(cp.x - 1, cp.z), sy, sy);
        }
        if lx == 15 {
            self.queue_remesh(ChunkPos::new(cp.x + 1, cp.z), sy, sy);
        }
        if lz == 0 {
            self.queue_remesh(ChunkPos::new(cp.x, cp.z - 1), sy, sy);
        }
        if lz == 15 {
            self.queue_remesh(ChunkPos::new(cp.x, cp.z + 1), sy, sy);
        }
        let range = (sy.saturating_sub(1), (sy + 1).min(SECTION_COUNT - 1));
        for dx in -1..=1 {
            for dz in -1..=1 {
                let n = ChunkPos::new(cp.x + dx, cp.z + dz);
                if dx != 0 || dz != 0 {
                    // Light can cross into neighbours only within 15 blocks.
                    let near_x = match dx {
                        -1 => lx < 15,
                        1 => lx > 0,
                        _ => true,
                    };
                    let near_z = match dz {
                        -1 => lz < 15,
                        1 => lz > 0,
                        _ => true,
                    };
                    if !(near_x && near_z) {
                        continue;
                    }
                }
                self.request_light(n, range);
            }
        }
        true
    }

    fn request_light(&mut self, p: ChunkPos, range: (usize, usize)) {
        if !self.chunks.contains_key(&p) {
            return;
        }
        let st = self.state.entry(p).or_default();
        st.version += 1;
        st.light_pending = false;
        let e = self.remesh.entry(p).or_insert(range);
        e.0 = e.0.min(range.0);
        e.1 = e.1.max(range.1);
        if let Some(c) = self.chunks.get_mut(&p) {
            Arc::make_mut(c).light_valid = false;
        }
    }

    fn queue_remesh(&mut self, p: ChunkPos, lo: usize, hi: usize) {
        if !self.chunks.contains_key(&p) {
            return;
        }
        let e = self.remesh.entry(p).or_insert((lo, hi));
        e.0 = e.0.min(lo);
        e.1 = e.1.max(hi);
    }

    // ---- streaming ----

    fn neighbours_present(&self, p: ChunkPos) -> bool {
        (-1..=1).all(|dx| {
            (-1..=1).all(|dz| self.chunks.contains_key(&ChunkPos::new(p.x + dx, p.z + dz)))
        })
    }

    fn neighbours_lit(&self, p: ChunkPos) -> bool {
        (-1..=1).all(|dx| {
            (-1..=1).all(|dz| {
                self.chunks
                    .get(&ChunkPos::new(p.x + dx, p.z + dz))
                    .is_some_and(|c| c.light_valid)
            })
        })
    }

    fn neighbourhood(&self, p: ChunkPos) -> [Option<Arc<Chunk>>; 9] {
        std::array::from_fn(|i| {
            let dx = i as i32 / 3 - 1;
            let dz = i as i32 % 3 - 1;
            self.chunks.get(&ChunkPos::new(p.x + dx, p.z + dz)).cloned()
        })
    }

    /// Stream chunks around `player` and apply finished work. Spends at most
    /// `budget` on the main thread.
    pub fn update(&mut self, player: [f64; 3], render_distance: i32, budget: Duration) {
        let start = Instant::now();
        let pc = ChunkPos::new(
            (player[0].floor() as i32) >> 4,
            (player[2].floor() as i32) >> 4,
        );
        *self.center.lock() = (pc, render_distance);
        // Meshing a chunk needs lit neighbours, and lighting needs theirs.
        let load_r = render_distance + 2;
        // Request missing chunks nearest-first.
        let mut want: Vec<ChunkPos> = Vec::new();
        for dx in -load_r..=load_r {
            for dz in -load_r..=load_r {
                let p = ChunkPos::new(pc.x + dx, pc.z + dz);
                if !self.chunks.contains_key(&p) && !self.requested.contains(&p) {
                    want.push(p);
                }
            }
        }
        want.sort_by_key(|p| (p.x - pc.x).pow(2) + (p.z - pc.z).pow(2));
        for p in want {
            self.requested.insert(p);
            let st = self.storage.clone();
            let map = self.map.clone();
            let tx = self.done_tx.clone();
            self.pool.spawn(move || {
                let r = match st.load(p, &map) {
                    Ok(c) => c,
                    Err(e) => {
                        warn!("chunk {p:?} unreadable, regenerating: {e:#}");
                        None
                    }
                };
                let _ = tx.send(Done::Loaded(p, r));
            });
        }
        // Unload far chunks.
        let unload_r = render_distance + 3;
        let far: Vec<ChunkPos> = self
            .chunks
            .keys()
            .copied()
            .filter(|p| (p.x - pc.x).abs() > unload_r || (p.z - pc.z).abs() > unload_r)
            .collect();
        for p in far {
            if let Some(c) = self.chunks.remove(&p) {
                if c.dirty {
                    let _ = self.io_tx.send(IoMsg::Save(c));
                }
                self.state.remove(&p);
                self.remesh.remove(&p);
                self.unloaded.push(p);
            }
        }
        let stale: Vec<ChunkPos> = self
            .requested
            .iter()
            .copied()
            .filter(|p| (p.x - pc.x).abs() > unload_r || (p.z - pc.z).abs() > unload_r)
            .collect();
        for p in stale {
            self.requested.remove(&p);
            if self.in_gen.remove(&p) {
                let _ = self.gen_tx.send(GenMsg::Cancel(p));
            }
        }
        // Apply results.
        while start.elapsed() < budget {
            let Ok(d) = self.done_rx.try_recv() else {
                break;
            };
            self.apply(d, pc, unload_r);
        }
        // Schedule lighting and meshing, nearest first.
        let mut cands: Vec<(i32, ChunkPos)> = self
            .chunks
            .keys()
            .map(|p| ((p.x - pc.x).pow(2) + (p.z - pc.z).pow(2), *p))
            .collect();
        cands.sort_unstable_by_key(|c| (c.0, c.1.x, c.1.z));
        for (_, p) in cands {
            if self.in_flight >= self.max_in_flight {
                break;
            }
            let lit = self.chunks[&p].light_valid;
            let st = self.state.get(&p).copied().unwrap_or_default();
            if !lit {
                if !st.light_pending && self.neighbours_present(p) {
                    self.spawn_light(p);
                }
                continue;
            }
            let in_view =
                (p.x - pc.x).abs() <= render_distance && (p.z - pc.z).abs() <= render_distance;
            if !in_view {
                continue;
            }
            let needs_full = st.meshed_version == 0 && st.mesh_pending == 0;
            let range = if needs_full {
                Some((0, SECTION_COUNT - 1))
            } else {
                self.remesh.get(&p).copied()
            };
            if let Some((lo, hi)) = range
                && self.neighbours_lit(p)
            {
                self.remesh.remove(&p);
                self.spawn_mesh(p, lo, hi);
            }
        }
        if self.last_autosave.elapsed() > Duration::from_secs(60) {
            self.save_dirty();
            self.last_autosave = Instant::now();
        }
    }

    fn apply(&mut self, d: Done, pc: ChunkPos, unload_r: i32) {
        let far = |p: ChunkPos| (p.x - pc.x).abs() > unload_r || (p.z - pc.z).abs() > unload_r;
        match d {
            Done::Loaded(p, Some(c)) => {
                if far(p) || !self.requested.remove(&p) {
                    return;
                }
                self.insert_chunk(c);
            }
            Done::Loaded(p, None) => {
                if far(p) || !self.requested.contains(&p) {
                    self.requested.remove(&p);
                    return;
                }
                self.in_gen.insert(p);
                let _ = self.gen_tx.send(GenMsg::Want(p));
            }
            Done::Generated(c) => {
                let p = c.pos;
                self.in_gen.remove(&p);
                self.generated_count += 1;
                if far(p) || !self.requested.remove(&p) {
                    // Still worth keeping on disk: generation is the expensive part.
                    let _ = self.io_tx.send(IoMsg::Save(Arc::new(c)));
                    return;
                }
                self.insert_chunk(c);
            }
            Done::Lit(p, ver, l, range) => {
                self.in_flight = self.in_flight.saturating_sub(1);
                let Some(st) = self.state.get_mut(&p) else {
                    return;
                };
                st.light_pending = false;
                if st.version != ver {
                    return;
                }
                st.lit_version = ver;
                let first = st.meshed_version == 0;
                if let Some(c) = self.chunks.get_mut(&p) {
                    let c = Arc::make_mut(c);
                    light::apply(c, l);
                    compact_light(c);
                }
                if !first {
                    self.queue_remesh(p, range.0, range.1);
                }
                // Neighbours may now be meshable at the borders.
                for dx in -1..=1 {
                    for dz in -1..=1 {
                        if dx == 0 && dz == 0 {
                            continue;
                        }
                        let n = ChunkPos::new(p.x + dx, p.z + dz);
                        if self.state.get(&n).is_some_and(|s| s.meshed_version != 0) && !first {
                            self.queue_remesh(n, range.0, range.1);
                        }
                    }
                }
            }
            Done::Meshed(p, sy, ver, mesh) => {
                self.in_flight = self.in_flight.saturating_sub(1);
                let Some(st) = self.state.get_mut(&p) else {
                    return;
                };
                st.mesh_pending = st.mesh_pending.saturating_sub(1);
                st.meshed_version = st.meshed_version.max(ver).max(1);
                if ver < st.section_ver[sy] {
                    return;
                }
                st.section_ver[sy] = ver;
                let origin = [p.x * 16, WORLD_MIN_Y + sy as i32 * 16, p.z * 16];
                self.mesh_out.push((origin, mesh));
            }
        }
    }

    fn insert_chunk(&mut self, c: Chunk) {
        let p = c.pos;
        let lit = c.light_valid;
        self.chunks.insert(p, Arc::new(c));
        let st = self.state.entry(p).or_default();
        st.version += 1;
        if lit {
            st.lit_version = st.version;
        }
        // A new chunk changes the light/mesh borders of its neighbours.
        for dx in -1..=1 {
            for dz in -1..=1 {
                let n = ChunkPos::new(p.x + dx, p.z + dz);
                if (dx != 0 || dz != 0)
                    && let Some(s) = self.state.get(&n)
                    && s.meshed_version != 0
                {
                    self.queue_remesh(n, 0, SECTION_COUNT - 1);
                }
            }
        }
    }

    fn spawn_light(&mut self, p: ChunkPos) {
        let n = self.neighbourhood(p);
        let st = self.state.entry(p).or_default();
        st.light_pending = true;
        let ver = st.version;
        let range = self
            .remesh
            .get(&p)
            .copied()
            .unwrap_or((0, SECTION_COUNT - 1));
        let tx = self.done_tx.clone();
        self.in_flight += 1;
        self.pool.spawn(move || {
            let refs: [Option<&Chunk>; 9] = std::array::from_fn(|i| n[i].as_deref());
            let l = light::compute(&refs);
            let _ = tx.send(Done::Lit(p, ver, l, range));
        });
    }

    fn spawn_mesh(&mut self, p: ChunkPos, lo: usize, hi: usize) {
        let n = self.neighbourhood(p);
        let st = self.state.entry(p).or_default();
        let ver = st.version.max(1);
        let centre = n[4].clone().expect("centre present");
        for sy in lo..=hi {
            // Skip sections that are empty with empty neighbours above/below.
            let empty = |s: usize| centre.sections.get(s).is_none_or(|x| x.is_empty());
            if empty(sy) {
                // An empty section may still need its old mesh cleared.
                let origin = [p.x * 16, WORLD_MIN_Y + sy as i32 * 16, p.z * 16];
                self.mesh_out.push((origin, SectionMesh::default()));
                continue;
            }
            st.mesh_pending += 1;
            self.in_flight += 1;
            let n = n.clone();
            let shared = self.mesh.clone();
            let tx = self.done_tx.clone();
            self.pool.spawn(move || {
                let snap = Snapshot::capture(&n, sy);
                let m = mesh_section(
                    &snap,
                    &shared.tables,
                    &shared.models,
                    &shared.colors,
                    shared.smooth,
                );
                let _ = tx.send(Done::Meshed(p, sy, ver, m));
            });
        }
        if st.mesh_pending == 0 {
            st.meshed_version = ver;
        }
    }

    /// Progress for the loading screen: (ready, total) chunks in view.
    #[must_use]
    pub fn spawn_progress(&self, centre: ChunkPos, r: i32) -> (usize, usize) {
        let mut ready = 0;
        let mut total = 0;
        for dx in -r..=r {
            for dz in -r..=r {
                total += 1;
                let p = ChunkPos::new(centre.x + dx, centre.z + dz);
                if self
                    .state
                    .get(&p)
                    .is_some_and(|s| s.meshed_version != 0 && s.mesh_pending == 0)
                {
                    ready += 1;
                }
            }
        }
        (ready, total)
    }

    /// Queue every dirty chunk for saving.
    pub fn save_dirty(&mut self) {
        for c in self.chunks.values_mut() {
            if c.dirty {
                let _ = self.io_tx.send(IoMsg::Save(c.clone()));
                Arc::make_mut(c).dirty = false;
            }
        }
    }

    /// Save everything and wait for the writes to finish.
    pub fn save_all(&mut self) -> Result<()> {
        self.save_dirty();
        self.meta.last_played_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        self.meta.write(&self.dir.join("world.rcw"))?;
        let (tx, rx) = unbounded();
        let _ = self.io_tx.send(IoMsg::Flush(tx));
        let _ = rx.recv_timeout(Duration::from_secs(30));
        Ok(())
    }

    /// Number of chunks waiting in the generator.
    #[must_use]
    pub fn pending_generation(&self) -> usize {
        self.in_gen.len()
    }
}

/// Drop light arrays that equal the implicit defaults (sky 15, block 0).
fn compact_light(c: &mut Chunk) {
    for s in c.sections.iter_mut() {
        if s.sky_light
            .as_ref()
            .is_some_and(|a| a.iter().all(|&b| b == 0xFF))
        {
            s.sky_light = None;
        }
        if s.block_light
            .as_ref()
            .is_some_and(|a| a.iter().all(|&b| b == 0))
        {
            s.block_light = None;
        }
    }
}

impl Drop for World {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = self.save_all();
        let _ = self.gen_tx.send(GenMsg::Stop);
        let _ = self.io_tx.send(IoMsg::Stop);
        if let Some(h) = self.gen_handle.take() {
            let _ = h.join();
        }
        if let Some(h) = self.io_handle.take() {
            let _ = h.join();
        }
    }
}

impl rc_physics::Collider for World {
    fn boxes(&self, area: &rc_physics::Aabb, out: &mut Vec<rc_physics::Aabb>) {
        let b = rc_data::blocks();
        let x0 = area.min[0].floor() as i32;
        let x1 = area.max[0].floor() as i32;
        let y0 = area.min[1].floor() as i32 - 1;
        let y1 = area.max[1].floor() as i32;
        let z0 = area.min[2].floor() as i32;
        let z1 = area.max[2].floor() as i32;
        for x in x0..=x1 {
            for z in z0..=z1 {
                let loaded = self.is_loaded(BlockPos::new(x, 0, z));
                for y in y0..=y1 {
                    let (fx, fy, fz) = (f64::from(x), f64::from(y), f64::from(z));
                    if !loaded {
                        // Unloaded chunks are solid so nobody falls out of the world.
                        out.push(rc_physics::Aabb::new(
                            [fx, fy, fz],
                            [fx + 1.0, fy + 1.0, fz + 1.0],
                        ));
                        continue;
                    }
                    let s = self.get_block(BlockPos::new(x, y, z));
                    if s == BlockStateId::AIR {
                        continue;
                    }
                    for bb in b.collision(s) {
                        out.push(rc_physics::Aabb::new(
                            [fx + bb[0], fy + bb[1], fz + bb[2]],
                            [fx + bb[3], fy + bb[4], fz + bb[5]],
                        ));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_matches_proto() {
        let dg = DimensionGen::new(Dimension::Overworld, 1);
        let mut store = ChunkStore::new(&dg);
        store.generate(&[ChunkPos::new(0, 0)]);
        let pc = store.chunks.remove(&ChunkPos::new(0, 0)).unwrap();
        let c = export_chunk(&pc);
        for y in (-64..320).step_by(7) {
            for x in 0..16 {
                assert_eq!(c.get_block(BlockPos::new(x, y, 5)), pc.get(x, y, 5));
            }
        }
    }
}

#[cfg(test)]
mod debug_dump {
    #[test]
    #[ignore = "manual diagnostic"]
    fn dump_surface() {
        let dir = std::path::PathBuf::from(std::env::var("RC_WORLD").unwrap());
        let st = rc_world::rcw::DimStorage::new(dir.join("dim/overworld")).unwrap();
        let b = rc_data::blocks();
        let map = rc_world::rcw::IdMap::identity(b.len(), 64);
        let mut counts = std::collections::BTreeMap::new();
        for cx in -2..2 {
            for cz in -2..2 {
                let c = st
                    .load(rc_core::ChunkPos::new(cx, cz), &map)
                    .unwrap()
                    .unwrap();
                for i in 0..256 {
                    let h = i32::from(c.height_surface[i]);
                    let p = rc_core::BlockPos::new(
                        cx * 16 + (i as i32 & 15),
                        h,
                        cz * 16 + (i as i32 >> 4),
                    );
                    let s = c.get_block(p);
                    *counts.entry(b.def(s).name.clone()).or_insert(0) += 1;
                    let below = c.get_block(rc_core::BlockPos::new(p.x, h - 1, p.z));
                    *counts
                        .entry(format!("below:{}", b.def(below).name))
                        .or_insert(0) += 1;
                }
            }
        }
        eprintln!("{counts:#?}");
        // Mesh the river section at the origin and report water vertex colours.
        let a = rc_assets::Assets::discover(
            std::env::var_os("RC_ASSETS")
                .as_deref()
                .map(std::path::Path::new),
        );
        let models = std::sync::Arc::new(a.block_models(4));
        let tables = rc_render::mesh::MeshTables::new(&models, true);
        let colors = super::biome_colors(None, None);
        let n: [Option<std::sync::Arc<rc_world::Chunk>>; 9] = std::array::from_fn(|i| {
            st.load(
                rc_core::ChunkPos::new(i as i32 / 3 - 1, i as i32 % 3 - 1),
                &map,
            )
            .unwrap()
            .map(std::sync::Arc::new)
        });
        let snap = rc_render::mesh::Snapshot::capture(&n, 7);
        let m = rc_render::mesh::mesh_section(&snap, &tables, &models, &colors, true);
        eprintln!(
            "translucent verts {}, first colours {:?}",
            m.translucent.len(),
            m.translucent
                .iter()
                .take(4)
                .map(|v| v.color)
                .collect::<Vec<_>>()
        );
        eprintln!("water colours sample {:?}", &colors.water[..4]);
    }
}
