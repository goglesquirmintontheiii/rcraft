//! Surface system (`SurfaceSystem` + `SurfaceRules`).

use std::cell::{Cell, OnceCell};

use ahash::{AHashMap, AHashSet};
use rc_core::{BiomeId, BlockStateId};
use serde_json::Value;

use crate::density::{ChunkCache, Graph};
use crate::generator::{DimensionGen, NoiseSettings, PrelimSurface};
use crate::mth;
use crate::noise::NormalNoise;
use crate::proto::{Heightmap, ProtoChunk};
use crate::rng::{Positional, Random};

#[derive(Clone, Copy, Debug)]
pub enum Anchor {
    Absolute(i32),
    AboveBottom(i32),
    BelowTop(i32),
}

impl Anchor {
    #[must_use]
    pub fn from_json(v: &Value) -> Self {
        if let Some(n) = v.get("absolute") {
            Self::Absolute(n.as_i64().unwrap() as i32)
        } else if let Some(n) = v.get("above_bottom") {
            Self::AboveBottom(n.as_i64().unwrap() as i32)
        } else if let Some(n) = v.get("below_top") {
            Self::BelowTop(n.as_i64().unwrap() as i32)
        } else {
            panic!("bad vertical anchor {v}")
        }
    }

    #[inline]
    #[must_use]
    pub fn resolve(self, min_y: i32, height: i32) -> i32 {
        match self {
            Self::Absolute(y) => y,
            Self::AboveBottom(y) => min_y + y,
            Self::BelowTop(y) => min_y + height - 1 - y,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Cond {
    Biome(AHashSet<BiomeId>),
    NoiseThreshold {
        noise: u32,
        min: f64,
        max: f64,
    },
    VerticalGradient {
        random: Positional,
        true_at: Anchor,
        false_at: Anchor,
    },
    YAbove {
        anchor: Anchor,
        mul: i32,
        add_stone_depth: bool,
    },
    Water {
        offset: i32,
        mul: i32,
        add_stone_depth: bool,
    },
    Temperature,
    Steep,
    Not(Box<Cond>),
    Hole,
    AbovePreliminarySurface,
    StoneDepth {
        offset: i32,
        add_surface_depth: bool,
        secondary_range: i32,
        ceiling: bool,
    },
}

#[derive(Clone, Debug)]
pub enum Rule {
    Block(BlockStateId),
    Sequence(Vec<Rule>),
    Condition(Cond, Box<Rule>),
    Bandlands,
}

pub struct SurfaceSystem {
    pub rule: Rule,
    default_block: BlockStateId,
    sea_level: i32,
    min_y: i32,
    height: i32,
    noise_random: Positional,
    surface_noise: u32,
    surface_secondary_noise: u32,
    clay_bands_offset: u32,
    clay_bands: Vec<BlockStateId>,
    badlands_pillar: u32,
    badlands_pillar_roof: u32,
    badlands_surface: u32,
    iceberg_pillar: u32,
    iceberg_pillar_roof: u32,
    iceberg_surface: u32,
    eroded_badlands: Option<BiomeId>,
    frozen_ocean: Option<BiomeId>,
    deep_frozen_ocean: Option<BiomeId>,
    states: SurfaceStates,
}

#[derive(Clone, Copy, Debug)]
struct SurfaceStates {
    water: BlockStateId,
    snow_block: BlockStateId,
    packed_ice: BlockStateId,
    grass_block: u16,
    mycelium: u16,
    dirt: u16,
}

impl std::fmt::Debug for SurfaceSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SurfaceSystem")
    }
}

fn generate_bands(r: &mut Random) -> Vec<BlockStateId> {
    let b = rc_data::blocks();
    let s = |n: &str| b.default_state(n);
    let mut bs = vec![s("terracotta"); 192];
    let mut i = 0usize;
    while i < bs.len() {
        i += r.next_int_bound(5) as usize + 1;
        if i >= bs.len() {
            break;
        }
        bs[i] = s("orange_terracotta");
        i += 1;
    }
    make_bands(r, &mut bs, 1, s("yellow_terracotta"));
    make_bands(r, &mut bs, 2, s("brown_terracotta"));
    make_bands(r, &mut bs, 1, s("red_terracotta"));
    let j = r.next_int_between(9, 15);
    let mut k = 0;
    let mut l = 0usize;
    while k < j && l < bs.len() {
        bs[l] = s("white_terracotta");
        if l >= 2 && r.next_bool() {
            bs[l - 1] = s("light_gray_terracotta");
        }
        if l + 1 < bs.len() && r.next_bool() {
            bs[l + 1] = s("light_gray_terracotta");
        }
        k += 1;
        l += r.next_int_bound(16) as usize + 4;
    }
    bs
}

fn make_bands(r: &mut Random, bs: &mut [BlockStateId], i: i32, state: BlockStateId) {
    let j = r.next_int_between(6, 15);
    for _ in 0..j {
        let l = i + r.next_int_bound(3);
        let m = r.next_int_bound(bs.len() as i32) as usize;
        let mut n = 0;
        while m + n < bs.len() && (n as i32) < l {
            bs[m + n] = state;
            n += 1;
        }
    }
}

impl SurfaceSystem {
    pub(crate) fn new(
        graph: &mut Graph,
        random: Positional,
        rule: &Value,
        settings: &NoiseSettings,
    ) -> Self {
        let b = rc_data::blocks();
        let biomes = crate::biome::registry();
        let mut gradients: AHashMap<String, Positional> = AHashMap::new();
        let rule = compile_rule(rule, graph, random, &mut gradients);
        let mut clay_random = random.from_hash_of("minecraft:clay_bands");
        Self {
            rule,
            default_block: settings.default_block,
            sea_level: settings.sea_level,
            min_y: settings.min_y,
            height: settings.height,
            noise_random: random,
            surface_noise: graph.noise_by_key("minecraft:surface"),
            surface_secondary_noise: graph.noise_by_key("minecraft:surface_secondary"),
            clay_bands_offset: graph.noise_by_key("minecraft:clay_bands_offset"),
            clay_bands: generate_bands(&mut clay_random),
            badlands_pillar: graph.noise_by_key("minecraft:badlands_pillar"),
            badlands_pillar_roof: graph.noise_by_key("minecraft:badlands_pillar_roof"),
            badlands_surface: graph.noise_by_key("minecraft:badlands_surface"),
            iceberg_pillar: graph.noise_by_key("minecraft:iceberg_pillar"),
            iceberg_pillar_roof: graph.noise_by_key("minecraft:iceberg_pillar_roof"),
            iceberg_surface: graph.noise_by_key("minecraft:iceberg_surface"),
            eroded_badlands: biomes.id("eroded_badlands"),
            frozen_ocean: biomes.id("frozen_ocean"),
            deep_frozen_ocean: biomes.id("deep_frozen_ocean"),
            states: SurfaceStates {
                water: b.default_state("water"),
                snow_block: b.default_state("snow_block"),
                packed_ice: b.default_state("packed_ice"),
                grass_block: b.index_of("grass_block").unwrap(),
                mycelium: b.index_of("mycelium").unwrap(),
                dirt: b.index_of("dirt").unwrap(),
            },
        }
    }

    fn noise<'g>(g: &'g Graph, id: u32) -> &'g NormalNoise {
        &g.noises[id as usize]
    }

    #[must_use]
    pub fn surface_depth(&self, g: &Graph, x: i32, z: i32) -> i32 {
        let d = Self::noise(g, self.surface_noise).get_value(f64::from(x), 0.0, f64::from(z));
        (d * 2.75 + 3.0 + self.noise_random.at(x, 0, z).next_double() * 0.25) as i32
    }

    fn band(&self, g: &Graph, x: i32, y: i32, z: i32) -> BlockStateId {
        let l = mth::round(
            Self::noise(g, self.clay_bands_offset).get_value(f64::from(x), 0.0, f64::from(z)) * 4.0,
        ) as i32;
        let n = self.clay_bands.len() as i32;
        self.clay_bands[((y + l + n) % n) as usize]
    }
}

fn compile_cond(
    v: &Value,
    g: &mut Graph,
    random: Positional,
    grads: &mut AHashMap<String, Positional>,
) -> Cond {
    let t = v["type"].as_str().unwrap().trim_start_matches("minecraft:");
    let i = |k: &str| v[k].as_i64().unwrap_or(0) as i32;
    let bo = |k: &str| v[k].as_bool().unwrap_or(false);
    match t {
        "biome" => {
            let reg = crate::biome::registry();
            Cond::Biome(
                v["biome_is"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|b| reg.id(b.as_str().unwrap()))
                    .collect(),
            )
        }
        "noise_threshold" => Cond::NoiseThreshold {
            noise: g.noise_by_key(v["noise"].as_str().unwrap()),
            min: v["min_threshold"].as_f64().unwrap(),
            max: v["max_threshold"].as_f64().unwrap(),
        },
        "vertical_gradient" => {
            let name = v["random_name"].as_str().unwrap().to_owned();
            let r = *grads
                .entry(name.clone())
                .or_insert_with(|| random.from_hash_of(&name).fork_positional());
            Cond::VerticalGradient {
                random: r,
                true_at: Anchor::from_json(&v["true_at_and_below"]),
                false_at: Anchor::from_json(&v["false_at_and_above"]),
            }
        }
        "y_above" => Cond::YAbove {
            anchor: Anchor::from_json(&v["anchor"]),
            mul: i("surface_depth_multiplier"),
            add_stone_depth: bo("add_stone_depth"),
        },
        "water" => Cond::Water {
            offset: i("offset"),
            mul: i("surface_depth_multiplier"),
            add_stone_depth: bo("add_stone_depth"),
        },
        "temperature" => Cond::Temperature,
        "steep" => Cond::Steep,
        "not" => Cond::Not(Box::new(compile_cond(&v["invert"], g, random, grads))),
        "hole" => Cond::Hole,
        "above_preliminary_surface" => Cond::AbovePreliminarySurface,
        "stone_depth" => Cond::StoneDepth {
            offset: i("offset"),
            add_surface_depth: bo("add_surface_depth"),
            secondary_range: i("secondary_depth_range"),
            ceiling: v["surface_type"].as_str() == Some("ceiling"),
        },
        other => panic!("unknown surface condition {other}"),
    }
}

fn compile_rule(
    v: &Value,
    g: &mut Graph,
    random: Positional,
    grads: &mut AHashMap<String, Positional>,
) -> Rule {
    let t = v["type"].as_str().unwrap().trim_start_matches("minecraft:");
    match t {
        "block" => Rule::Block(rc_data::blocks().from_json(&v["result_state"]).unwrap()),
        "sequence" => Rule::Sequence(
            v["sequence"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| compile_rule(r, g, random, grads))
                .collect(),
        ),
        "condition" => Rule::Condition(
            compile_cond(&v["if_true"], g, random, grads),
            Box::new(compile_rule(&v["then_run"], g, random, grads)),
        ),
        "bandlands" => Rule::Bandlands,
        other => panic!("unknown surface rule {other}"),
    }
}

/// Values `SurfaceRules.Context` caches per XZ update.
#[derive(Default)]
pub struct ColumnCache {
    surface_secondary: Cell<Option<f64>>,
    min_surface: Cell<Option<i32>>,
    steep: Cell<Option<bool>>,
}

/// `SurfaceRules.Context` for one position.
pub struct Ctx<'a> {
    pub dg: &'a DimensionGen,
    pub cc: &'a ChunkCache,
    pub block_x: i32,
    pub block_y: i32,
    pub block_z: i32,
    pub surface_depth: i32,
    pub stone_depth_above: i32,
    pub stone_depth_below: i32,
    pub water_height: i32,
    col: &'a ColumnCache,
    biome: OnceCell<BiomeId>,
}

impl<'a> Ctx<'a> {
    fn surface_secondary(&self) -> f64 {
        if let Some(v) = self.col.surface_secondary.get() {
            return v;
        }
        let s = &self.dg.surface;
        let v = SurfaceSystem::noise(&self.dg.graph, s.surface_secondary_noise).get_value(
            f64::from(self.block_x),
            0.0,
            f64::from(self.block_z),
        );
        self.col.surface_secondary.set(Some(v));
        v
    }

    fn min_surface_level(&self, prelim: &mut PrelimSurface) -> i32 {
        if let Some(v) = self.col.min_surface.get() {
            return v;
        }
        let i = self.block_x >> 4;
        let j = self.block_z >> 4;
        let p = |x: i32, z: i32, pr: &mut PrelimSurface| pr.get(self.dg, self.cc, x, z);
        let k = p(i << 4, j << 4, prelim);
        let l = p((i + 1) << 4, j << 4, prelim);
        let m = p(i << 4, (j + 1) << 4, prelim);
        let n = p((i + 1) << 4, (j + 1) << 4, prelim);
        let o = mth::floor(mth::lerp2(
            f64::from(self.block_x & 15) / 16.0,
            f64::from(self.block_z & 15) / 16.0,
            f64::from(k),
            f64::from(l),
            f64::from(m),
            f64::from(n),
        ));
        let v = o + self.surface_depth - 8;
        self.col.min_surface.set(Some(v));
        v
    }
}

/// Biome lookup over noise biomes with quart caching, as seen by a
/// `WorldGenRegion` (stored chunk biomes, y clamped to the chunk).
pub struct BiomeLookup<'a> {
    dg: &'a DimensionGen,
    cache: AHashMap<(i32, i32, i32), BiomeId>,
    min_q: i32,
    max_q: i32,
    clamp: bool,
}

impl<'a> BiomeLookup<'a> {
    #[must_use]
    pub fn new(dg: &'a DimensionGen, clamp: bool) -> Self {
        let min_q = dg.settings.min_y >> 2;
        let max_q = min_q + (dg.settings.height >> 2) - 1;
        Self {
            dg,
            cache: AHashMap::new(),
            min_q,
            max_q,
            clamp,
        }
    }

    pub fn quart(&mut self, qx: i32, qy: i32, qz: i32) -> BiomeId {
        let qy = if self.clamp {
            qy.clamp(self.min_q, self.max_q)
        } else {
            qy
        };
        let dg = self.dg;
        *self
            .cache
            .entry((qx, qy, qz))
            .or_insert_with(|| dg.noise_biome(qx, qy, qz))
    }

    pub fn zoomed(&mut self, x: i32, y: i32, z: i32) -> BiomeId {
        let dg = self.dg;
        dg.zoomed_biome(x, y, z, &mut |a, b, c| self.quart(a, b, c))
    }
}

impl SurfaceSystem {
    fn test(
        &self,
        c: &Cond,
        ctx: &Ctx<'_>,
        prelim: &mut PrelimSurface,
        chunk: &ProtoChunk,
        biomes: &mut BiomeLookup<'_>,
    ) -> bool {
        match c {
            Cond::Biome(set) => {
                let b = *ctx
                    .biome
                    .get_or_init(|| biomes.zoomed(ctx.block_x, ctx.block_y, ctx.block_z));
                set.contains(&b)
            }
            Cond::NoiseThreshold { noise, min, max } => {
                let d = SurfaceSystem::noise(&ctx.dg.graph, *noise).get_value(
                    f64::from(ctx.block_x),
                    0.0,
                    f64::from(ctx.block_z),
                );
                d >= *min && d <= *max
            }
            Cond::VerticalGradient {
                random,
                true_at,
                false_at,
            } => {
                let i = true_at.resolve(self.min_y, self.height);
                let j = false_at.resolve(self.min_y, self.height);
                let y = ctx.block_y;
                if y <= i {
                    return true;
                }
                if y >= j {
                    return false;
                }
                let d = mth::map(f64::from(y), f64::from(i), f64::from(j), 1.0, 0.0);
                let mut r = random.at(ctx.block_x, y, ctx.block_z);
                f64::from(r.next_float()) < d
            }
            Cond::YAbove {
                anchor,
                mul,
                add_stone_depth,
            } => {
                ctx.block_y
                    + if *add_stone_depth {
                        ctx.stone_depth_above
                    } else {
                        0
                    }
                    >= anchor.resolve(self.min_y, self.height) + ctx.surface_depth * mul
            }
            Cond::Water {
                offset,
                mul,
                add_stone_depth,
            } => {
                ctx.water_height == i32::MIN
                    || ctx.block_y
                        + if *add_stone_depth {
                            ctx.stone_depth_above
                        } else {
                            0
                        }
                        >= ctx.water_height + offset + ctx.surface_depth * mul
            }
            Cond::Temperature => {
                let b = *ctx
                    .biome
                    .get_or_init(|| biomes.zoomed(ctx.block_x, ctx.block_y, ctx.block_z));
                ctx.dg
                    .biomes
                    .cold_enough_to_snow(b, ctx.block_x, ctx.block_y, ctx.block_z)
            }
            Cond::Steep => {
                if let Some(v) = ctx.col.steep.get() {
                    return v;
                }
                let i = ctx.block_x & 15;
                let j = ctx.block_z & 15;
                let h = |x: i32, z: i32| {
                    chunk
                        .height_primed(Heightmap::WorldSurfaceWg, x, z)
                        .unwrap_or(chunk.min_y)
                        - 1
                };
                let k = (j - 1).max(0);
                let l = (j + 1).min(15);
                let m = h(i, k);
                let n = h(i, l);
                let v = if n >= m + 4 {
                    true
                } else {
                    let o = (i - 1).max(0);
                    let p = (i + 1).min(15);
                    let q = h(o, j);
                    let r = h(p, j);
                    q >= r + 4
                };
                ctx.col.steep.set(Some(v));
                v
            }
            Cond::Not(inner) => !self.test(inner, ctx, prelim, chunk, biomes),
            Cond::Hole => ctx.surface_depth <= 0,
            Cond::AbovePreliminarySurface => ctx.block_y >= ctx.min_surface_level(prelim),
            Cond::StoneDepth {
                offset,
                add_surface_depth,
                secondary_range,
                ceiling,
            } => {
                let i = if *ceiling {
                    ctx.stone_depth_below
                } else {
                    ctx.stone_depth_above
                };
                let j = if *add_surface_depth {
                    ctx.surface_depth
                } else {
                    0
                };
                let k = if *secondary_range == 0 {
                    0
                } else {
                    mth::map(
                        ctx.surface_secondary(),
                        -1.0,
                        1.0,
                        0.0,
                        f64::from(*secondary_range),
                    ) as i32
                };
                i <= 1 + offset + j + k
            }
        }
    }

    fn apply(
        &self,
        r: &Rule,
        ctx: &Ctx<'_>,
        prelim: &mut PrelimSurface,
        chunk: &ProtoChunk,
        biomes: &mut BiomeLookup<'_>,
    ) -> Option<BlockStateId> {
        match r {
            Rule::Block(s) => Some(*s),
            Rule::Sequence(rules) => {
                for r in rules {
                    if let Some(s) = self.apply(r, ctx, prelim, chunk, biomes) {
                        return Some(s);
                    }
                }
                None
            }
            Rule::Condition(c, then) => {
                if self.test(c, ctx, prelim, chunk, biomes) {
                    self.apply(then, ctx, prelim, chunk, biomes)
                } else {
                    None
                }
            }
            Rule::Bandlands => {
                Some(self.band(&ctx.dg.graph, ctx.block_x, ctx.block_y, ctx.block_z))
            }
        }
    }

    #[inline]
    fn is_stone(b: &rc_data::Blocks, s: BlockStateId) -> bool {
        !b.is_air(s) && b.fluid(s).is_empty()
    }
}

impl DimensionGen {
    /// Surface stage (`SurfaceSystem.buildSurface`).
    pub fn build_surface(
        &self,
        chunk: &mut ProtoChunk,
        cc: &ChunkCache,
        prelim: &mut PrelimSurface,
    ) {
        let b = rc_data::blocks();
        let ss = &self.surface;
        let mut biomes = BiomeLookup::new(self, true);
        let min_x = chunk.pos.x << 4;
        let min_z = chunk.pos.z << 4;
        let min_y = chunk.min_y;
        for k in 0..16 {
            for l in 0..16 {
                let m = min_x + k;
                let n = min_z + l;
                let o = DimensionGen::wg_height(chunk, k, l) + 1;
                let holder = biomes.zoomed(m, if self.settings.legacy_random { 0 } else { o }, n);
                if Some(holder) == ss.eroded_badlands {
                    self.eroded_badlands_extension(chunk, m, n, o);
                }
                let p = DimensionGen::wg_height(chunk, k, l) + 1;
                let surface_depth = ss.surface_depth(&self.graph, m, n);
                let column = ColumnCache::default();
                let mut q = 0;
                let mut r = i32::MIN;
                let mut s = i32::MAX;
                let mut u = p;
                while u >= min_y {
                    let bs = chunk.get(m, u, n);
                    if b.is_air(bs) {
                        q = 0;
                        r = i32::MIN;
                        u -= 1;
                        continue;
                    }
                    if !b.fluid(bs).is_empty() {
                        if r == i32::MIN {
                            r = u + 1;
                        }
                        u -= 1;
                        continue;
                    }
                    if s >= u {
                        s = crate::aquifer::WAY_BELOW_MIN_Y;
                        let mut v = u - 1;
                        while v >= min_y - 1 {
                            let bs2 = chunk.get(m, v, n);
                            if !SurfaceSystem::is_stone(b, bs2) {
                                s = v + 1;
                                break;
                            }
                            v -= 1;
                        }
                    }
                    q += 1;
                    let vv = u - s + 1;
                    if bs == ss.default_block {
                        let ctx = Ctx {
                            dg: self,
                            cc,
                            block_x: m,
                            block_y: u,
                            block_z: n,
                            surface_depth,
                            stone_depth_above: q,
                            stone_depth_below: vv,
                            water_height: r,
                            col: &column,
                            biome: OnceCell::new(),
                        };
                        let res = ss.apply(&ss.rule, &ctx, prelim, chunk, &mut biomes);
                        if let Some(st) = res {
                            chunk.set(m, u, n, st);
                            if !b.fluid(st).is_empty() {
                                chunk.post_process.push((m, u, n));
                            }
                        }
                    }
                    u -= 1;
                }
                if Some(holder) == ss.frozen_ocean || Some(holder) == ss.deep_frozen_ocean {
                    let ctx = Ctx {
                        dg: self,
                        cc,
                        block_x: m,
                        block_y: 0,
                        block_z: n,
                        surface_depth,
                        stone_depth_above: 0,
                        stone_depth_below: 0,
                        water_height: i32::MIN,
                        col: &column,
                        biome: OnceCell::new(),
                    };
                    let msl = ctx.min_surface_level(prelim);
                    self.frozen_ocean_extension(chunk, msl, holder, m, n, o);
                }
            }
        }
        chunk.status = crate::proto::Status::Surface;
    }

    fn eroded_badlands_extension(&self, chunk: &mut ProtoChunk, x: i32, z: i32, height: i32) {
        let b = rc_data::blocks();
        let ss = &self.surface;
        let g = &self.graph;
        let e = (SurfaceSystem::noise(g, ss.badlands_surface).get_value(
            f64::from(x),
            0.0,
            f64::from(z),
        ) * 8.25)
            .abs()
            .min(
                SurfaceSystem::noise(g, ss.badlands_pillar).get_value(
                    f64::from(x) * 0.2,
                    0.0,
                    f64::from(z) * 0.2,
                ) * 15.0,
            );
        if e <= 0.0 {
            return;
        }
        let h = (SurfaceSystem::noise(g, ss.badlands_pillar_roof).get_value(
            f64::from(x) * 0.75,
            0.0,
            f64::from(z) * 0.75,
        ) * 1.5)
            .abs();
        let l = 64.0 + (e * e * 2.5).min((h * 50.0).ceil() + 24.0);
        let m = mth::floor(l);
        if height > m {
            return;
        }
        let def_block = b.block_index(ss.default_block);
        let mut n = m;
        while n >= chunk.min_y {
            let bs = chunk.get(x, n, z);
            if b.block_index(bs) == def_block {
                break;
            }
            if bs == ss.states.water {
                return;
            }
            n -= 1;
        }
        let mut n = m;
        while n >= chunk.min_y && b.is_air(chunk.get(x, n, z)) {
            chunk.set(x, n, z, ss.default_block);
            n -= 1;
        }
    }

    fn frozen_ocean_extension(
        &self,
        chunk: &mut ProtoChunk,
        min_surface: i32,
        biome: BiomeId,
        x: i32,
        z: i32,
        height: i32,
    ) {
        let b = rc_data::blocks();
        let ss = &self.surface;
        let g = &self.graph;
        let e = (SurfaceSystem::noise(g, ss.iceberg_surface).get_value(
            f64::from(x),
            0.0,
            f64::from(z),
        ) * 8.25)
            .abs()
            .min(
                SurfaceSystem::noise(g, ss.iceberg_pillar).get_value(
                    f64::from(x) * 1.28,
                    0.0,
                    f64::from(z) * 1.28,
                ) * 15.0,
            );
        if e <= 1.8 {
            return;
        }
        let h = (SurfaceSystem::noise(g, ss.iceberg_pillar_roof).get_value(
            f64::from(x) * 1.17,
            0.0,
            f64::from(z) * 1.17,
        ) * 1.5)
            .abs();
        let mut l = (e * e * 1.2).min((h * 40.0).ceil() + 14.0);
        if self.biomes.temperature_at(biome, x, 63, z) > 0.1 {
            l -= 2.0;
        }
        let m;
        if l > 2.0 {
            m = f64::from(ss.sea_level) - l - 7.0;
            l += f64::from(ss.sea_level);
        } else {
            l = 0.0;
            m = 0.0;
        }
        let n = l;
        let mut rs = ss.noise_random.at(x, 0, z);
        let o = 2 + rs.next_int_bound(4);
        let p = ss.sea_level + 18 + rs.next_int_bound(10);
        let mut q = 0;
        let mut r = height.max(n as i32 + 1);
        while r >= min_surface {
            let cur = chunk.get(x, r, z);
            let a = b.is_air(cur) && r < n as i32 && rs.next_double() > 0.01;
            let w = !a
                && cur == ss.states.water
                && r > m as i32
                && r < ss.sea_level
                && m != 0.0
                && rs.next_double() > 0.15;
            if a || w {
                if q <= o && r > p {
                    chunk.set(x, r, z, ss.states.snow_block);
                    q += 1;
                } else {
                    chunk.set(x, r, z, ss.states.packed_ice);
                }
            }
            r -= 1;
        }
    }

    /// `SurfaceSystem.topMaterial` for carvers (grass/mycelium regrowth).
    pub fn top_material(
        &self,
        chunk: &ProtoChunk,
        cc: &ChunkCache,
        prelim: &mut PrelimSurface,
        biomes: &mut BiomeLookup<'_>,
        x: i32,
        y: i32,
        z: i32,
        has_fluid: bool,
    ) -> Option<BlockStateId> {
        let ss = &self.surface;
        let column = ColumnCache::default();
        let ctx = Ctx {
            dg: self,
            cc,
            block_x: x,
            block_y: y,
            block_z: z,
            surface_depth: ss.surface_depth(&self.graph, x, z),
            stone_depth_above: 1,
            stone_depth_below: 1,
            water_height: if has_fluid { y + 1 } else { i32::MIN },
            col: &column,
            biome: OnceCell::new(),
        };
        ss.apply(&ss.rule, &ctx, prelim, chunk, biomes)
    }

    pub(crate) fn surface_dirt_states(&self) -> (u16, u16, u16) {
        let s = &self.surface.states;
        (s.grass_block, s.mycelium, s.dirt)
    }
}
