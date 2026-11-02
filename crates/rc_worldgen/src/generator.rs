//! Per-dimension generator state (`RandomState` + `NoiseBasedChunkGenerator`)
//! and the biome / noise stages.

use std::cell::Cell;

use ahash::AHashMap;
use rc_core::{BiomeId, BlockStateId, ChunkPos};

use crate::aquifer::{Aquifer, AquiferFns, GlobalFluid, PreliminarySurface};
use crate::biome::{self, BiomeRegistry};
use crate::climate::{self, RTree, Target};
use crate::density::{ChunkCache, Graph, Id, LerpOrder, Mode};
use crate::mth;
use crate::proto::{Heightmap, ProtoChunk, Status};
use crate::rng::{Positional, Random};
use crate::surface::SurfaceSystem;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dimension {
    Overworld,
    Nether,
    End,
}

impl Dimension {
    #[must_use]
    pub fn settings_name(self) -> &'static str {
        match self {
            Self::Overworld => "overworld",
            Self::Nether => "nether",
            Self::End => "end",
        }
    }
}

/// Parsed `noise_settings`.
#[derive(Clone, Debug)]
pub struct NoiseSettings {
    pub min_y: i32,
    pub height: i32,
    pub cell_width: i32,
    pub cell_height: i32,
    pub sea_level: i32,
    pub default_block: BlockStateId,
    pub default_fluid: BlockStateId,
    pub aquifers: bool,
    pub ore_veins: bool,
    pub legacy_random: bool,
}

/// Router function ids.
#[derive(Clone, Copy, Debug)]
pub struct Router {
    pub barrier: Id,
    pub floodedness: Id,
    pub spread: Id,
    pub lava: Id,
    pub temperature: Id,
    pub vegetation: Id,
    pub continents: Id,
    pub erosion: Id,
    pub depth: Id,
    pub ridges: Id,
    pub initial_density: Id,
    pub final_density: Id,
    pub vein_toggle: Id,
    pub vein_ridged: Id,
    pub vein_gap: Id,
}

pub enum BiomeSource {
    MultiNoise {
        tree: RTree,
        biomes: Vec<BiomeId>,
    },
    End {
        end: BiomeId,
        highlands: BiomeId,
        midlands: BiomeId,
        small_islands: BiomeId,
        barrens: BiomeId,
    },
}

thread_local! {
    static LAST_LEAF: Cell<(u64, Option<usize>)> = const { Cell::new((0, None)) };
}

/// All seed-dependent generator state for one dimension.
pub struct DimensionGen {
    pub dim: Dimension,
    pub seed: i64,
    pub settings: NoiseSettings,
    pub graph: Graph,
    pub router: Router,
    pub random: Positional,
    pub aquifer_random: Positional,
    pub ore_random: Positional,
    pub biome_source: BiomeSource,
    pub surface: SurfaceSystem,
    pub biomes: &'static BiomeRegistry,
    /// `BiomeManager.obfuscateSeed(seed)`.
    pub biome_zoom_seed: i64,
    pub(crate) states: WgStates,
    pub(crate) feature_order:
        std::sync::OnceLock<(crate::feature::StepOrder, crate::feature::BiomeFeatures)>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WgStates {
    pub air: BlockStateId,
    pub water: BlockStateId,
    pub lava: BlockStateId,
    pub copper_ore: BlockStateId,
    pub raw_copper_block: BlockStateId,
    pub granite: BlockStateId,
    pub deepslate_iron_ore: BlockStateId,
    pub raw_iron_block: BlockStateId,
    pub tuff: BlockStateId,
}

impl std::fmt::Debug for DimensionGen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DimensionGen({:?}, seed {})", self.dim, self.seed)
    }
}

fn obfuscate_seed(seed: i64) -> i64 {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(seed.to_le_bytes());
    i64::from_le_bytes(h[0..8].try_into().unwrap())
}

impl DimensionGen {
    #[must_use]
    pub fn new(dim: Dimension, seed: i64) -> Self {
        let files = rc_data::files();
        let blocks = rc_data::blocks();
        let ns = files
            .json(&format!(
                "worldgen/noise_settings/{}.json",
                dim.settings_name()
            ))
            .unwrap();
        let noise = &ns["noise"];
        let settings = NoiseSettings {
            min_y: noise["min_y"].as_i64().unwrap() as i32,
            height: noise["height"].as_i64().unwrap() as i32,
            cell_width: noise["size_horizontal"].as_i64().unwrap() as i32 * 4,
            cell_height: noise["size_vertical"].as_i64().unwrap() as i32 * 4,
            sea_level: ns["sea_level"].as_i64().unwrap() as i32,
            default_block: blocks.from_json(&ns["default_block"]).unwrap(),
            default_fluid: blocks.from_json(&ns["default_fluid"]).unwrap(),
            aquifers: ns["aquifers_enabled"].as_bool().unwrap(),
            ore_veins: ns["ore_veins_enabled"].as_bool().unwrap(),
            legacy_random: ns["legacy_random_source"].as_bool().unwrap(),
        };
        let mut base = if settings.legacy_random {
            Random::legacy(seed)
        } else {
            Random::xoroshiro(seed)
        };
        let random = base.fork_positional();
        let aquifer_random = random.from_hash_of("minecraft:aquifer").fork_positional();
        let ore_random = random.from_hash_of("minecraft:ore").fork_positional();
        let mut graph = Graph::new(seed, settings.legacy_random, random);
        let r = &ns["noise_router"];
        let router = Router {
            barrier: graph.compile(&r["barrier"]),
            floodedness: graph.compile(&r["fluid_level_floodedness"]),
            spread: graph.compile(&r["fluid_level_spread"]),
            lava: graph.compile(&r["lava"]),
            temperature: graph.compile(&r["temperature"]),
            vegetation: graph.compile(&r["vegetation"]),
            continents: graph.compile(&r["continents"]),
            erosion: graph.compile(&r["erosion"]),
            depth: graph.compile(&r["depth"]),
            ridges: graph.compile(&r["ridges"]),
            initial_density: graph.compile(&r["initial_density_without_jaggedness"]),
            final_density: graph.compile(&r["final_density"]),
            vein_toggle: graph.compile(&r["vein_toggle"]),
            vein_ridged: graph.compile(&r["vein_ridged"]),
            vein_gap: graph.compile(&r["vein_gap"]),
        };
        let biomes = biome::registry();
        let dim_json = files
            .json(&format!(
                "dimension/{}.json",
                match dim {
                    Dimension::Overworld => "overworld",
                    Dimension::Nether => "the_nether",
                    Dimension::End => "the_end",
                }
            ))
            .unwrap();
        let bs = &dim_json["generator"]["biome_source"];
        let biome_source = if bs["type"].as_str() == Some("minecraft:the_end") {
            let id = |n: &str| biomes.id(n).unwrap();
            BiomeSource::End {
                end: id("the_end"),
                highlands: id("end_highlands"),
                midlands: id("end_midlands"),
                small_islands: id("small_end_islands"),
                barrens: id("end_barrens"),
            }
        } else {
            let (leaves, names) = climate::parse_parameter_list(bs["biomes"].as_array().unwrap());
            BiomeSource::MultiNoise {
                tree: RTree::build(&leaves),
                biomes: names.iter().map(|n| biomes.id(n).unwrap()).collect(),
            }
        };
        let surface = SurfaceSystem::new(&mut graph, random, &ns["surface_rule"], &settings);
        let states = WgStates {
            air: blocks.default_state("air"),
            water: blocks.default_state("water"),
            lava: blocks.default_state("lava"),
            copper_ore: blocks.default_state("copper_ore"),
            raw_copper_block: blocks.default_state("raw_copper_block"),
            granite: blocks.default_state("granite"),
            deepslate_iron_ore: blocks.default_state("deepslate_iron_ore"),
            raw_iron_block: blocks.default_state("raw_iron_block"),
            tuff: blocks.default_state("tuff"),
        };
        Self {
            dim,
            seed,
            settings,
            graph,
            router,
            random,
            aquifer_random,
            ore_random,
            biome_source,
            surface,
            biomes,
            biome_zoom_seed: obfuscate_seed(seed),
            states,
            feature_order: std::sync::OnceLock::new(),
        }
    }

    /// Possible biomes in first-appearance order (feature sorting input).
    #[must_use]
    pub fn possible_biomes(&self) -> Vec<BiomeId> {
        match &self.biome_source {
            BiomeSource::MultiNoise { biomes, .. } => biomes.clone(),
            BiomeSource::End {
                end,
                highlands,
                midlands,
                small_islands,
                barrens,
            } => {
                vec![*end, *highlands, *midlands, *small_islands, *barrens]
            }
        }
    }

    /// Climate target at quart coordinates (`Climate.Sampler.sample`).
    #[must_use]
    pub fn climate(&self, qx: i32, qy: i32, qz: i32) -> Target {
        let (x, y, z) = (qx << 2, qy << 2, qz << 2);
        let g = &self.graph;
        let e = |id| g.eval(id, x, y, z, Mode::Direct, None) as f32;
        Target::new(
            e(self.router.temperature),
            e(self.router.vegetation),
            e(self.router.continents),
            e(self.router.erosion),
            e(self.router.depth),
            e(self.router.ridges),
        )
    }

    /// Noise biome at quart coordinates (`BiomeSource.getNoiseBiome`).
    #[must_use]
    pub fn noise_biome(&self, qx: i32, qy: i32, qz: i32) -> BiomeId {
        match &self.biome_source {
            BiomeSource::MultiNoise { tree, biomes } => {
                let t = self.climate(qx, qy, qz);
                let i = LAST_LEAF.with(|last| tree.search(&t, last));
                biomes[i as usize]
            }
            BiomeSource::End {
                end,
                highlands,
                midlands,
                small_islands,
                barrens,
            } => {
                let i = qx << 2;
                let j = qy << 2;
                let k = qz << 2;
                let l = i >> 4;
                let m = k >> 4;
                if i64::from(l) * i64::from(l) + i64::from(m) * i64::from(m) <= 4096 {
                    return *end;
                }
                let n = ((i >> 4) * 2 + 1) * 8;
                let o = ((k >> 4) * 2 + 1) * 8;
                let d = self
                    .graph
                    .eval(self.router.erosion, n, j, o, Mode::Direct, None);
                if d > 0.25 {
                    *highlands
                } else if d >= -0.0625 {
                    *midlands
                } else if d < -0.21875 {
                    *small_islands
                } else {
                    *barrens
                }
            }
        }
    }

    /// `BiomeManager.getBiome(pos)` over the noise biome source (fuzzy zoom).
    #[must_use]
    pub fn zoomed_biome(
        &self,
        x: i32,
        y: i32,
        z: i32,
        lookup: &mut dyn FnMut(i32, i32, i32) -> BiomeId,
    ) -> BiomeId {
        let i = x - 2;
        let j = y - 2;
        let k = z - 2;
        let l = i >> 2;
        let m = j >> 2;
        let n = k >> 2;
        let d = f64::from(i & 3) / 4.0;
        let e = f64::from(j & 3) / 4.0;
        let f = f64::from(k & 3) / 4.0;
        let mut o = 0;
        let mut g = f64::INFINITY;
        for p in 0..8 {
            let bl = p & 4 == 0;
            let bl2 = p & 2 == 0;
            let bl3 = p & 1 == 0;
            let q = if bl { l } else { l + 1 };
            let r = if bl2 { m } else { m + 1 };
            let s = if bl3 { n } else { n + 1 };
            let h = if bl { d } else { d - 1.0 };
            let t = if bl2 { e } else { e - 1.0 };
            let u = if bl3 { f } else { f - 1.0 };
            let v = fiddled_distance(self.biome_zoom_seed, q, r, s, h, t, u);
            if g > v {
                o = p;
                g = v;
            }
        }
        let px = if o & 4 == 0 { l } else { l + 1 };
        let py = if o & 2 == 0 { m } else { m + 1 };
        let pz = if o & 1 == 0 { n } else { n + 1 };
        lookup(px, py, pz)
    }

    /// Fill the chunk's quart biomes from noise.
    pub fn fill_biomes(&self, chunk: &mut ProtoChunk) {
        let qx0 = chunk.pos.x << 2;
        let qz0 = chunk.pos.z << 2;
        let qy0 = chunk.min_y >> 2;
        let ny = chunk.height >> 2;
        // Vanilla iterates per section, then x, y, z within the section.
        for sy in 0..(ny / 4) {
            for x in 0..4 {
                for y in 0..4 {
                    for z in 0..4 {
                        let qy = qy0 + sy * 4 + y;
                        let b = self.noise_biome(qx0 + x, qy, qz0 + z);
                        let i = (((sy * 4 + y) << 4) | (z << 2) | x) as usize;
                        chunk.biomes[i] = b;
                    }
                }
            }
        }
        chunk.status = Status::Biomes;
    }

    /// Per-chunk density cache for this generator.
    #[must_use]
    pub fn chunk_cache(&self, pos: ChunkPos) -> ChunkCache {
        let s = &self.settings;
        let mut cc = ChunkCache::new(
            &self.graph,
            pos.x << 4,
            pos.z << 4,
            s.cell_width,
            s.cell_height,
            s.min_y,
            s.height,
        );
        self.graph.fill_flat(&mut cc);
        cc
    }

    #[must_use]
    pub fn new_aquifer(&self, pos: ChunkPos) -> Aquifer {
        let s = &self.settings;
        Aquifer::new(
            s.aquifers,
            AquiferFns {
                barrier: self.router.barrier,
                floodedness: self.router.floodedness,
                spread: self.router.spread,
                lava: self.router.lava,
                erosion: self.router.erosion,
                depth: self.router.depth,
            },
            self.aquifer_random,
            self.global_fluid(),
            self.states.air,
            pos.x << 4,
            pos.z << 4,
            s.min_y,
            s.height,
        )
    }

    #[must_use]
    pub fn global_fluid(&self) -> GlobalFluid {
        GlobalFluid {
            sea_level: self.settings.sea_level,
            sea_fluid: self.settings.default_fluid,
            lava: self.states.lava,
            water: self.states.water,
        }
    }

    /// Noise stage: fill terrain, aquifers and ore veins.
    pub fn fill_noise(
        &self,
        chunk: &mut ProtoChunk,
        cc: &mut ChunkCache,
        aquifer: &mut Aquifer,
        prelim: &mut PrelimSurface,
    ) {
        cc.fill_grids(&self.graph);
        let s = &self.settings;
        let cw = s.cell_width;
        let ch = s.cell_height;
        let min_x = chunk.pos.x << 4;
        let min_z = chunk.pos.z << 4;
        let g = &self.graph;
        for cx in 0..cc.cells_xz {
            for cz in 0..cc.cells_xz {
                for cy in (0..cc.cells_y).rev() {
                    for iy in (0..ch).rev() {
                        let y = (cc.min_cell_y + cy) * ch + iy;
                        for ix in 0..cw {
                            let x = min_x + cx * cw + ix;
                            for iz in 0..cw {
                                let z = min_z + cz * cw + iz;
                                cc.cell_x = cx;
                                cc.cell_y = cy;
                                cc.cell_z = cz;
                                cc.in_x = ix;
                                cc.in_y = iy;
                                cc.in_z = iz;
                                let density = g.eval(
                                    self.router.final_density,
                                    x,
                                    y,
                                    z,
                                    Mode::Cell(LerpOrder::Lerp3),
                                    Some(cc),
                                );
                                let mut pb = prelim.bind(self, cc);
                                let mut state = aquifer.compute_substance(
                                    g,
                                    cc,
                                    Mode::Cell(LerpOrder::Sequential),
                                    &mut pb,
                                    x,
                                    y,
                                    z,
                                    density,
                                );
                                if state.is_none() && s.ore_veins {
                                    state = self.ore_vein(cc, x, y, z);
                                }
                                let state = state.unwrap_or(s.default_block);
                                if state != self.states.air {
                                    chunk.set(x, y, z, state);
                                }
                            }
                        }
                    }
                }
            }
        }
        chunk.status = Status::Noise;
    }

    fn ore_vein(&self, cc: &ChunkCache, x: i32, y: i32, z: i32) -> Option<BlockStateId> {
        let g = &self.graph;
        let mode = Mode::Cell(LerpOrder::Sequential);
        let d = g.eval(self.router.vein_toggle, x, y, z, mode, Some(cc));
        let copper = d > 0.0;
        let (ore, raw, filler, min_y, max_y) = if copper {
            (
                self.states.copper_ore,
                self.states.raw_copper_block,
                self.states.granite,
                0,
                50,
            )
        } else {
            (
                self.states.deepslate_iron_ore,
                self.states.raw_iron_block,
                self.states.tuff,
                -60,
                -8,
            )
        };
        let e = d.abs();
        let j = max_y - y;
        let k = y - min_y;
        if k < 0 || j < 0 {
            return None;
        }
        let l = j.min(k);
        let f = mth::clamped_map(f64::from(l), 0.0, 20.0, -0.2, 0.0);
        if e + f < f64::from(0.4f32) {
            return None;
        }
        let mut r = self.ore_random.at(x, y, z);
        if r.next_float() > 0.7 {
            return None;
        }
        if g.eval(self.router.vein_ridged, x, y, z, mode, Some(cc)) >= 0.0 {
            return None;
        }
        let gg = mth::clamped_map(
            e,
            f64::from(0.4f32),
            f64::from(0.6f32),
            f64::from(0.1f32),
            f64::from(0.3f32),
        );
        if f64::from(r.next_float()) < gg
            && g.eval(self.router.vein_gap, x, y, z, mode, Some(cc)) > f64::from(-0.3f32)
        {
            return Some(if r.next_float() < 0.02 { raw } else { ore });
        }
        Some(filler)
    }

    /// `NoiseChunk.computePreliminarySurfaceLevel` at block (x, z) (already
    /// quart-aligned by the caller).
    #[must_use]
    pub fn compute_preliminary_surface(&self, cc: &ChunkCache, x: i32, z: i32) -> i32 {
        let s = &self.settings;
        let mut m = s.min_y + s.height;
        while m >= s.min_y {
            if self.graph.eval(
                self.router.initial_density,
                x,
                m,
                z,
                Mode::ChunkPoint,
                Some(cc),
            ) > 0.390_625
            {
                return m;
            }
            m -= s.cell_height;
        }
        i32::MAX
    }

    /// Run biomes + noise + surface + carvers on a fresh chunk.
    pub fn generate_to_carvers(&self, pos: ChunkPos) -> ProtoChunk {
        let s = &self.settings;
        let mut chunk = ProtoChunk::new(pos, s.min_y, s.height);
        self.fill_biomes(&mut chunk);
        let mut cc = self.chunk_cache(pos);
        let mut aquifer = self.new_aquifer(pos);
        let mut prelim = PrelimSurface::default();
        self.fill_noise(&mut chunk, &mut cc, &mut aquifer, &mut prelim);
        self.build_surface(&mut chunk, &cc, &mut prelim);
        self.apply_carvers(&mut chunk, &cc, &mut aquifer, &mut prelim);
        chunk
    }

    /// Heightmap read used by the surface step (`WORLD_SURFACE_WG`), returns
    /// `getHeight(...)`, i.e. topmost block y.
    pub fn wg_height(chunk: &mut ProtoChunk, x: i32, z: i32) -> i32 {
        chunk.first_available(Heightmap::WorldSurfaceWg, x, z) - 1
    }
}

/// Lazily-computed preliminary surface levels for one chunk
/// (`NoiseChunk.preliminarySurfaceLevel` cache).
#[derive(Default)]
pub struct PrelimSurface {
    cache: AHashMap<(i32, i32), i32>,
}

impl PrelimSurface {
    /// Borrow as an aquifer callback for one call.
    pub fn bind<'a>(&'a mut self, dg: &'a DimensionGen, cc: &'a ChunkCache) -> PrelimBound<'a> {
        PrelimBound {
            cache: &mut self.cache,
            dg,
            cc,
        }
    }

    pub fn get(&mut self, dg: &DimensionGen, cc: &ChunkCache, x: i32, z: i32) -> i32 {
        self.bind(dg, cc).preliminary_surface_level(x, z)
    }
}

pub struct PrelimBound<'a> {
    cache: &'a mut AHashMap<(i32, i32), i32>,
    dg: &'a DimensionGen,
    cc: &'a ChunkCache,
}

impl PreliminarySurface for PrelimBound<'_> {
    fn preliminary_surface_level(&mut self, x: i32, z: i32) -> i32 {
        let k = (x >> 2 << 2, z >> 2 << 2);
        if let Some(&v) = self.cache.get(&k) {
            return v;
        }
        let v = self.dg.compute_preliminary_surface(self.cc, k.0, k.1);
        self.cache.insert(k, v);
        v
    }
}

#[inline]
fn lcg_next(l: i64, m: i64) -> i64 {
    let l2 = l.wrapping_mul(
        l.wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407),
    );
    l2.wrapping_add(m)
}

#[inline]
fn fiddle(l: i64) -> f64 {
    let d = mth::floor_mod_l(l >> 24, 1024) as f64 / 1024.0;
    (d - 0.5) * 0.9
}

#[allow(clippy::many_single_char_names)]
fn fiddled_distance(seed: i64, x: i32, y: i32, z: i32, dx: f64, dy: f64, dz: f64) -> f64 {
    let mut m = lcg_next(seed, i64::from(x));
    m = lcg_next(m, i64::from(y));
    m = lcg_next(m, i64::from(z));
    m = lcg_next(m, i64::from(x));
    m = lcg_next(m, i64::from(y));
    m = lcg_next(m, i64::from(z));
    let g = fiddle(m);
    m = lcg_next(m, seed);
    let h = fiddle(m);
    m = lcg_next(m, seed);
    let n = fiddle(m);
    mth::square(dz + n) + mth::square(dy + h) + mth::square(dx + g)
}
