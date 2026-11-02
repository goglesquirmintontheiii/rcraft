//! Feature decoration: placed / configured feature registry, placement
//! modifiers, the global feature ordering (`FeatureSorter`) and the per-chunk
//! decoration driver (`ChunkGenerator.applyBiomeDecoration`).

pub mod behaviour;
pub mod predicate;
pub mod region;

mod basic;
mod cave;
mod dripstone;
mod end;
mod jset;
mod nether;
mod tree;
mod water;

use std::collections::BTreeMap;
use std::sync::OnceLock;

use ahash::{AHashMap, AHashSet};
use rc_core::{BiomeId, BlockPos};
use serde_json::Value;

use self::predicate::BlockPredicate;
use self::region::Region;
use crate::generator::DimensionGen;
use crate::proto::Heightmap;
use crate::providers::{HeightProvider, IntProvider};
use crate::rng::Random;

pub type PlacedId = u32;
pub type ConfiguredId = u32;

#[derive(Clone, Debug)]
pub enum Modifier {
    Count(IntProvider),
    InSquare,
    Heightmap(Heightmap),
    HeightRange(HeightProvider),
    Biome,
    Rarity(i32),
    Predicate(BlockPredicate),
    SurfaceWaterDepth(i32),
    SurfaceRelative {
        hm: Heightmap,
        min: i64,
        max: i64,
    },
    RandomOffset {
        xz: IntProvider,
        y: IntProvider,
    },
    EnvironmentScan {
        dir: rc_core::Direction,
        target: BlockPredicate,
        allowed: BlockPredicate,
        max_steps: i32,
    },
    CountOnEveryLayer(IntProvider),
    NoiseBasedCount {
        ratio: i32,
        factor: f64,
        offset: f64,
    },
    NoiseThresholdCount {
        level: f64,
        below: i32,
        above: i32,
    },
    Fixed(Vec<BlockPos>),
    CarvingMask,
}

#[derive(Clone, Debug)]
pub struct PlacedFeature {
    pub feature: ConfiguredId,
    pub modifiers: Vec<Modifier>,
}

pub use basic::Kind;

#[derive(Clone, Debug)]
pub struct ConfiguredFeature {
    pub kind: Kind,
}

/// All placed and configured features, with inline ones interned.
pub struct Registry {
    pub placed: Vec<PlacedFeature>,
    pub configured: Vec<ConfiguredFeature>,
    placed_by_name: AHashMap<String, PlacedId>,
    configured_by_name: AHashMap<String, ConfiguredId>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "FeatureRegistry({} placed, {} configured)",
            self.placed.len(),
            self.configured.len()
        )
    }
}

fn int(v: &Value) -> IntProvider {
    IntProvider::from_json(v)
}

impl Registry {
    fn new() -> Self {
        let mut r = Self {
            placed: Vec::new(),
            configured: Vec::new(),
            placed_by_name: AHashMap::new(),
            configured_by_name: AHashMap::new(),
        };
        for n in rc_data::files().list("worldgen/placed_feature") {
            r.placed_ref(&format!("minecraft:{n}"));
        }
        r
    }

    /// Resolve (and intern) a placed feature by name.
    pub fn placed_ref(&mut self, name: &str) -> PlacedId {
        let key = if name.contains(':') {
            name.to_owned()
        } else {
            format!("minecraft:{name}")
        };
        if let Some(&i) = self.placed_by_name.get(&key) {
            return i;
        }
        let j = rc_data::files()
            .registry_json("worldgen/placed_feature", &key)
            .unwrap_or_else(|| panic!("placed feature {key}"));
        let id = self.placed_inline(&j);
        self.placed_by_name.insert(key, id);
        id
    }

    /// A placed feature given as a name or inline object.
    pub fn placed_value(&mut self, v: &Value) -> PlacedId {
        match v {
            Value::String(s) => self.placed_ref(s),
            o => self.placed_inline(o),
        }
    }

    fn placed_inline(&mut self, v: &Value) -> PlacedId {
        let feature = self.configured_value(&v["feature"]);
        let modifiers = v["placement"]
            .as_array()
            .map_or_else(Vec::new, |a| a.iter().map(Self::modifier).collect());
        self.placed.push(PlacedFeature { feature, modifiers });
        (self.placed.len() - 1) as PlacedId
    }

    pub fn configured_value(&mut self, v: &Value) -> ConfiguredId {
        match v {
            Value::String(s) => {
                let key = if s.contains(':') {
                    s.clone()
                } else {
                    format!("minecraft:{s}")
                };
                if let Some(&i) = self.configured_by_name.get(&key) {
                    return i;
                }
                let j = rc_data::files()
                    .registry_json("worldgen/configured_feature", &key)
                    .unwrap_or_else(|| panic!("configured feature {key}"));
                let id = self.configured_inline(&j);
                self.configured_by_name.insert(key, id);
                id
            }
            o => self.configured_inline(o),
        }
    }

    fn configured_inline(&mut self, v: &Value) -> ConfiguredId {
        let kind = Kind::from_json(self, v["type"].as_str().unwrap(), &v["config"]);
        self.configured.push(ConfiguredFeature { kind });
        (self.configured.len() - 1) as ConfiguredId
    }

    fn modifier(v: &Value) -> Modifier {
        let t = v["type"].as_str().unwrap().trim_start_matches("minecraft:");
        let hm = |k: &str| Heightmap::from_name(v[k].as_str().unwrap()).unwrap();
        match t {
            "count" => Modifier::Count(int(&v["count"])),
            "in_square" => Modifier::InSquare,
            "heightmap" => Modifier::Heightmap(hm("heightmap")),
            "height_range" => Modifier::HeightRange(HeightProvider::from_json(&v["height"])),
            "biome" => Modifier::Biome,
            "rarity_filter" => Modifier::Rarity(v["chance"].as_i64().unwrap() as i32),
            "block_predicate_filter" => {
                Modifier::Predicate(BlockPredicate::from_json(&v["predicate"]))
            }
            "surface_water_depth_filter" => {
                Modifier::SurfaceWaterDepth(v["max_water_depth"].as_i64().unwrap() as i32)
            }
            "surface_relative_threshold_filter" => Modifier::SurfaceRelative {
                hm: hm("heightmap"),
                min: v
                    .get("min_inclusive")
                    .and_then(Value::as_i64)
                    .unwrap_or(i64::from(i32::MIN)),
                max: v
                    .get("max_inclusive")
                    .and_then(Value::as_i64)
                    .unwrap_or(i64::from(i32::MAX)),
            },
            "random_offset" => Modifier::RandomOffset {
                xz: int(&v["xz_spread"]),
                y: int(&v["y_spread"]),
            },
            "environment_scan" => Modifier::EnvironmentScan {
                dir: predicate::direction(v["direction_of_search"].as_str().unwrap()),
                target: BlockPredicate::from_json(&v["target_condition"]),
                allowed: v
                    .get("allowed_search_condition")
                    .map_or(BlockPredicate::True, BlockPredicate::from_json),
                max_steps: v["max_steps"].as_i64().unwrap() as i32,
            },
            "count_on_every_layer" => Modifier::CountOnEveryLayer(int(&v["count"])),
            "noise_based_count" => Modifier::NoiseBasedCount {
                ratio: v["noise_to_count_ratio"].as_i64().unwrap() as i32,
                factor: v["noise_factor"].as_f64().unwrap(),
                offset: v.get("noise_offset").and_then(Value::as_f64).unwrap_or(0.0),
            },
            "noise_threshold_count" => Modifier::NoiseThresholdCount {
                level: v["noise_level"].as_f64().unwrap(),
                below: v["below_noise"].as_i64().unwrap() as i32,
                above: v["above_noise"].as_i64().unwrap() as i32,
            },
            "fixed_placement" => Modifier::Fixed(
                v["positions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| {
                        let a = p.as_array().unwrap();
                        BlockPos::new(
                            a[0].as_i64().unwrap() as i32,
                            a[1].as_i64().unwrap() as i32,
                            a[2].as_i64().unwrap() as i32,
                        )
                    })
                    .collect(),
            ),
            "carving_mask" => Modifier::CarvingMask,
            other => panic!("unknown placement modifier {other}"),
        }
    }

    /// Name of a top-level placed feature (debugging).
    #[must_use]
    pub fn placed_name(&self, id: PlacedId) -> String {
        self.placed_by_name
            .iter()
            .find(|(_, v)| **v == id)
            .map_or_else(|| format!("#{id}"), |(k, _)| k.clone())
    }

    #[must_use]
    pub fn placed_id(&self, name: &str) -> Option<PlacedId> {
        self.placed_by_name.get(name).copied()
    }
}

/// The global feature registry.
pub fn registry() -> &'static Registry {
    static R: OnceLock<Registry> = OnceLock::new();
    R.get_or_init(Registry::new)
}

/// Biome noise used by count modifiers (`Biome.BIOME_INFO_NOISE`).
fn biome_info_noise() -> &'static crate::noise::PerlinSimplexNoise {
    static N: OnceLock<crate::noise::PerlinSimplexNoise> = OnceLock::new();
    N.get_or_init(|| crate::noise::PerlinSimplexNoise::new(&mut Random::legacy(2345), &[0]))
}

/// Per-biome feature sets (for the biome placement filter).
pub struct BiomeFeatures {
    /// `[biome][step]` → placed feature ids.
    pub steps: Vec<Vec<Vec<PlacedId>>>,
    pub sets: Vec<AHashSet<PlacedId>>,
}

/// `FeatureSorter.buildFeaturesPerStep` over a generator's possible biomes.
pub struct StepOrder {
    pub steps: Vec<Vec<PlacedId>>,
    pub index: Vec<AHashMap<PlacedId, usize>>,
}

impl StepOrder {
    #[must_use]
    pub fn build(biomes: &[BiomeId]) -> (Self, BiomeFeatures) {
        let reg = registry();
        let breg = crate::biome::registry();
        let mut bsteps: Vec<Vec<Vec<PlacedId>>> = vec![Vec::new(); breg.len()];
        let mut bsets: Vec<AHashSet<PlacedId>> = vec![AHashSet::new(); breg.len()];
        for (bi, b) in breg.biomes.iter().enumerate() {
            let steps: Vec<Vec<PlacedId>> = b
                .features
                .iter()
                .map(|s| s.iter().filter_map(|n| reg.placed_id(n)).collect())
                .collect();
            bsets[bi] = steps.iter().flatten().copied().collect();
            bsteps[bi] = steps;
        }
        // (step, featureIndex) keyed nodes.
        let mut feature_index: AHashMap<PlacedId, usize> = AHashMap::new();
        let mut counter = 0usize;
        let mut graph: BTreeMap<
            (usize, usize),
            (PlacedId, std::collections::BTreeSet<(usize, usize)>),
        > = BTreeMap::new();
        let mut max_steps = 0;
        for &b in biomes {
            let steps = &bsteps[b.index()];
            max_steps = max_steps.max(steps.len());
            let mut list: Vec<(usize, usize, PlacedId)> = Vec::new();
            for (j, s) in steps.iter().enumerate() {
                for &pf in s {
                    let fi = *feature_index.entry(pf).or_insert_with(|| {
                        counter += 1;
                        counter - 1
                    });
                    list.push((fi, j, pf));
                }
            }
            for j in 0..list.len() {
                let (fi, step, pf) = list[j];
                let e = graph
                    .entry((step, fi))
                    .or_insert_with(|| (pf, std::collections::BTreeSet::new()));
                if j + 1 < list.len() {
                    e.1.insert((list[j + 1].1, list[j + 1].0));
                }
            }
        }
        let mut visited = std::collections::BTreeSet::new();
        let mut in_progress = std::collections::BTreeSet::new();
        let mut out: Vec<(usize, usize)> = Vec::new();
        fn dfs(
            g: &BTreeMap<(usize, usize), (PlacedId, std::collections::BTreeSet<(usize, usize)>)>,
            visited: &mut std::collections::BTreeSet<(usize, usize)>,
            in_progress: &mut std::collections::BTreeSet<(usize, usize)>,
            out: &mut Vec<(usize, usize)>,
            n: (usize, usize),
        ) -> bool {
            if visited.contains(&n) {
                return false;
            }
            if in_progress.contains(&n) {
                return true;
            }
            in_progress.insert(n);
            if let Some((_, next)) = g.get(&n) {
                for &t in next {
                    if dfs(g, visited, in_progress, out, t) {
                        return true;
                    }
                }
            }
            in_progress.remove(&n);
            visited.insert(n);
            out.push(n);
            false
        }
        let keys: Vec<(usize, usize)> = graph.keys().copied().collect();
        for k in keys {
            if visited.contains(&k) {
                continue;
            }
            if dfs(&graph, &mut visited, &mut in_progress, &mut out, k) {
                panic!("feature order cycle");
            }
        }
        out.reverse();
        let mut steps = vec![Vec::new(); max_steps];
        for (step, fi) in out {
            steps[step].push(graph[&(step, fi)].0);
        }
        let index = steps
            .iter()
            .map(|s| s.iter().enumerate().map(|(i, &pf)| (pf, i)).collect())
            .collect();
        (
            Self { steps, index },
            BiomeFeatures {
                steps: bsteps,
                sets: bsets,
            },
        )
    }
}

fn debug_feature() -> &'static Option<String> {
    static F: OnceLock<Option<String>> = OnceLock::new();
    F.get_or_init(|| std::env::var("RC_DEBUG_FEATURE").ok())
}

fn unsafe_env_set(on: bool) {
    DEBUG_PLACE.with(|d| d.set(on));
}

thread_local! {
    static DEBUG_PLACE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Context passed to features.
pub struct Ctx<'r, 'a> {
    pub region: &'r mut Region<'a>,
    pub reg: &'static Registry,
    pub biome_features: &'r BiomeFeatures,
    /// Top-level placed feature (for the biome filter).
    pub top: Option<PlacedId>,
}

impl Ctx<'_, '_> {
    /// `PlacedFeature.place` (nested, no biome check).
    pub fn place_placed(&mut self, id: PlacedId, r: &mut Random, pos: BlockPos) -> bool {
        let saved = self.top.take();
        let res = self.place_with_context(id, r, pos);
        self.top = saved;
        res
    }

    /// `ConfiguredFeature.place`.
    pub fn place_configured(&mut self, id: ConfiguredId, r: &mut Random, pos: BlockPos) -> bool {
        let reg = self.reg;
        reg.configured[id as usize].kind.place(self, r, pos)
    }

    fn place_with_context(&mut self, id: PlacedId, r: &mut Random, pos: BlockPos) -> bool {
        let pf = &self.reg.placed[id as usize];
        let mut placed = false;
        self.apply_modifiers(pf, 0, r, pos, &mut placed);
        placed
    }

    fn apply_modifiers(
        &mut self,
        pf: &PlacedFeature,
        i: usize,
        r: &mut Random,
        pos: BlockPos,
        placed: &mut bool,
    ) {
        if DEBUG_PLACE.with(std::cell::Cell::get) && self.top.is_none() && i == 0 {
            let before = self.region.get(pos);
            eprintln!(
                "  try {:?} on {} below {}",
                pos,
                rc_data::blocks().to_string(before),
                rc_data::blocks().to_string(self.region.get(BlockPos::new(
                    pos.x,
                    pos.y - 1,
                    pos.z
                )))
            );
        }
        if i == pf.modifiers.len() {
            if self.place_configured(pf.feature, r, pos) {
                *placed = true;
            }
            return;
        }
        let out = self.positions(&pf.modifiers[i], r, pos);
        for p in out {
            self.apply_modifiers(pf, i + 1, r, p, placed);
        }
        if DEBUG_PLACE.with(std::cell::Cell::get) && self.top.is_none() && i == 0 {
            eprintln!(
                "  after {} {} {} {}",
                pos.x,
                pos.y,
                pos.z,
                rc_data::blocks().to_string(self.region.get(pos))
            );
        }
    }

    fn positions(&mut self, m: &Modifier, r: &mut Random, pos: BlockPos) -> Vec<BlockPos> {
        let one = |p: BlockPos| vec![p];
        match m {
            Modifier::Count(c) => vec![pos; c.sample(r).max(0) as usize],
            Modifier::InSquare => {
                let i = r.next_int_bound(16) + pos.x;
                let j = r.next_int_bound(16) + pos.z;
                one(BlockPos::new(i, pos.y, j))
            }
            Modifier::Heightmap(h) => {
                let k = self.region.height(*h, pos.x, pos.z);
                if k > self.region.min_y {
                    one(BlockPos::new(pos.x, k, pos.z))
                } else {
                    Vec::new()
                }
            }
            Modifier::HeightRange(h) => {
                let y = h.sample(r, self.region.min_y, self.region.max_y - self.region.min_y);
                one(BlockPos::new(pos.x, y, pos.z))
            }
            Modifier::Biome => {
                let Some(top) = self.top else {
                    return Vec::new();
                };
                let b = self.region.biome(pos);
                if self.biome_features.sets[b.index()].contains(&top) {
                    one(pos)
                } else {
                    Vec::new()
                }
            }
            Modifier::Rarity(chance) => {
                if r.next_float() < 1.0 / *chance as f32 {
                    one(pos)
                } else {
                    Vec::new()
                }
            }
            Modifier::Predicate(p) => {
                if p.test(self.region, pos) {
                    one(pos)
                } else {
                    Vec::new()
                }
            }
            Modifier::SurfaceWaterDepth(max) => {
                let i = self.region.height(Heightmap::OceanFloor, pos.x, pos.z);
                let j = self.region.height(Heightmap::WorldSurface, pos.x, pos.z);
                if j - i <= *max { one(pos) } else { Vec::new() }
            }
            Modifier::SurfaceRelative { hm, min, max } => {
                let l = i64::from(self.region.height(*hm, pos.x, pos.z));
                let y = i64::from(pos.y);
                if l + min <= y && y <= l + max {
                    one(pos)
                } else {
                    Vec::new()
                }
            }
            Modifier::RandomOffset { xz, y } => {
                let i = pos.x + xz.sample(r);
                let j = pos.y + y.sample(r);
                let k = pos.z + xz.sample(r);
                one(BlockPos::new(i, j, k))
            }
            Modifier::EnvironmentScan {
                dir,
                target,
                allowed,
                max_steps,
            } => {
                let mut m = pos;
                if !allowed.test(self.region, m) {
                    return Vec::new();
                }
                for _ in 0..*max_steps {
                    if target.test(self.region, m) {
                        return one(m);
                    }
                    m = m.offset(*dir);
                    if self.region.is_outside_build_height(m.y) {
                        return Vec::new();
                    }
                    if !allowed.test(self.region, m) {
                        break;
                    }
                }
                if target.test(self.region, m) {
                    one(m)
                } else {
                    Vec::new()
                }
            }
            Modifier::CountOnEveryLayer(c) => {
                let mut out = Vec::new();
                let mut layer = 0;
                loop {
                    let mut any = false;
                    let mut j = 0;
                    while j < c.sample(r) {
                        let k = r.next_int_bound(16) + pos.x;
                        let l = r.next_int_bound(16) + pos.z;
                        let m = self.region.height(Heightmap::MotionBlocking, k, l);
                        let n = self.find_on_ground(k, m, l, layer);
                        j += 1;
                        if n == i32::MAX {
                            continue;
                        }
                        out.push(BlockPos::new(k, n, l));
                        any = true;
                    }
                    layer += 1;
                    if !any {
                        break;
                    }
                }
                out
            }
            Modifier::NoiseBasedCount {
                ratio,
                factor,
                offset,
            } => {
                let d = biome_info_noise().get_value(
                    f64::from(pos.x) / factor,
                    f64::from(pos.z) / factor,
                    false,
                );
                let i = ((d + offset) * f64::from(*ratio)).ceil() as i32;
                vec![pos; i.max(0) as usize]
            }
            Modifier::NoiseThresholdCount {
                level,
                below,
                above,
            } => {
                let d = biome_info_noise().get_value(
                    f64::from(pos.x) / 200.0,
                    f64::from(pos.z) / 200.0,
                    false,
                );
                let n = if d < *level { *below } else { *above };
                vec![pos; n.max(0) as usize]
            }
            Modifier::Fixed(list) => {
                let (cx, cz) = (pos.x >> 4, pos.z >> 4);
                list.iter()
                    .filter(|p| p.x >> 4 == cx && p.z >> 4 == cz)
                    .copied()
                    .collect()
            }
            Modifier::CarvingMask => {
                let Some(c) = self.region.chunk_at(pos.x, pos.z) else {
                    return Vec::new();
                };
                let mut out = Vec::new();
                let (bx, bz) = (c.pos.x << 4, c.pos.z << 4);
                for (wi, w) in c.carving_mask.iter().enumerate() {
                    let mut w = *w;
                    while w != 0 {
                        let bit = w.trailing_zeros() as usize;
                        w &= w - 1;
                        let i = wi * 64 + bit;
                        let x = (i & 15) as i32;
                        let z = ((i >> 4) & 15) as i32;
                        let y = (i >> 8) as i32 + c.min_y;
                        out.push(BlockPos::new(bx + x, y, bz + z));
                    }
                }
                out
            }
        }
    }

    fn find_on_ground(&mut self, x: i32, y: i32, z: i32, layer: i32) -> i32 {
        let h = behaviour::bh();
        let is_empty = |s| {
            let bi = h.block(s);
            h.is_air(s) || bi == h.idx.water || bi == h.idx.lava
        };
        let mut n = 0;
        let mut bs = self.region.get_xyz(x, y, z);
        let mut o = y;
        while o >= self.region.min_y + 1 {
            let bs2 = self.region.get_xyz(x, o - 1, z);
            if !is_empty(bs2) && is_empty(bs) && h.block(bs2) != h.idx.bedrock {
                if n == layer {
                    return o;
                }
                n += 1;
            }
            bs = bs2;
            o -= 1;
        }
        i32::MAX
    }
}

impl DimensionGen {
    /// Feature ordering for this generator (built once).
    pub fn feature_order(&self) -> &(StepOrder, BiomeFeatures) {
        self.feature_order
            .get_or_init(|| StepOrder::build(&self.possible_biomes()))
    }

    /// Decorate the center chunk of `region` (`applyBiomeDecoration`).
    pub fn decorate(&self, region: &mut Region<'_>) {
        let (order, bf) = self.feature_order();
        let reg = registry();
        let origin_x = region.center.x << 4;
        let origin_z = region.center.z << 4;
        let origin = BlockPos::new(origin_x, self.settings.min_y, origin_z);
        // Biomes present in the 3×3 chunks.
        let mut present: AHashSet<BiomeId> = AHashSet::new();
        for c in region.chunks.iter().flatten() {
            present.extend(c.biomes.iter().copied());
        }
        let possible: AHashSet<BiomeId> = self.possible_biomes().into_iter().collect();
        present.retain(|b| possible.contains(b));
        let mut wr = Random::worldgen_xoroshiro(0);
        let l = wr.set_decoration_seed(self.seed, origin_x, origin_z);
        let n_steps = order.steps.len();
        for k in 0..n_steps.max(11) {
            if k >= n_steps {
                continue;
            }
            let mut idx: Vec<usize> = Vec::new();
            for b in &present {
                let steps = &bf.steps[b.index()];
                if k >= steps.len() {
                    continue;
                }
                for pf in &steps[k] {
                    if let Some(&i) = order.index[k].get(pf) {
                        idx.push(i);
                    }
                }
            }
            idx.sort_unstable();
            idx.dedup();
            for p in idx {
                let pf = order.steps[k][p];
                wr.set_feature_seed(l, p as i32, k as i32);
                region::TRACE.with(|t| {
                    let mut t = t.borrow_mut();
                    if t.0.is_some() {
                        t.1 = format!(
                            "chunk {:?} step {k} idx {p} feature {}",
                            region.center,
                            reg.placed_name(pf)
                        );
                    }
                });
                let dbg = debug_feature().as_ref().is_some_and(|f| {
                    let (c, n) = f.split_once('|').unwrap();
                    c == format!("{},{}", region.center.x, region.center.z)
                        && n == reg.placed_name(pf)
                });
                if dbg {
                    unsafe_env_set(true);
                }
                let mut ctx = Ctx {
                    region,
                    reg,
                    biome_features: bf,
                    top: Some(pf),
                };
                ctx.place_with_context(pf, &mut wr, origin);
                if dbg {
                    unsafe_env_set(false);
                }
            }
        }
    }
}
