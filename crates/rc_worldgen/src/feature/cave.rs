//! Cave and vegetation features: glow lichen, vines, bamboo, vegetation
//! patches, root systems, geodes, dripstone.

use rc_core::{BlockPos, BlockStateId, Direction};
use rc_data::tags::BlockSet;
use serde_json::Value;

use super::basic::{bp, holder_set, off};
use super::behaviour::{bh, can_survive};
use super::jset::JavaPosSet;
use super::predicate::{BlockPredicate, StateProvider};
use super::tree::shuffle;
use super::{Ctx, PlacedId, Registry};
use crate::noise::{NoiseParams, NormalNoise};
use crate::proto::Heightmap;
use crate::providers::{FloatProvider, IntProvider};
use crate::rng::Random;

const HORIZONTAL: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

#[derive(Clone, Debug)]
pub struct Multiface {
    block: u16,
    search_range: i32,
    directions: Vec<Direction>,
    chance_of_spreading: f32,
    can_be_placed_on: BlockSet,
}

#[derive(Clone, Debug)]
pub struct VegPatch {
    replaceable: BlockSet,
    ground: StateProvider,
    vegetation: PlacedId,
    ceiling: bool,
    depth: IntProvider,
    extra_bottom_block_chance: f32,
    vertical_range: i32,
    vegetation_chance: f32,
    xz_radius: IntProvider,
    extra_edge_column_chance: f32,
    waterlogged: bool,
}

#[derive(Clone, Debug)]
pub struct RootSystem {
    feature: PlacedId,
    required_vertical_space_for_tree: i32,
    root_radius: i32,
    root_replaceable: BlockSet,
    root_state: StateProvider,
    root_placement_attempts: i32,
    root_column_max_height: i32,
    hanging_root_radius: i32,
    hanging_roots_vertical_span: i32,
    hanging_root_state: StateProvider,
    hanging_root_placement_attempts: i32,
    allowed_vertical_water_for_tree: i32,
    allowed_tree_position: BlockPredicate,
}

#[derive(Clone, Debug)]
pub struct Geode {
    filling: StateProvider,
    inner: StateProvider,
    alternate_inner: StateProvider,
    middle: StateProvider,
    outer: StateProvider,
    inner_placements: Vec<BlockStateId>,
    cannot_replace: BlockSet,
    invalid_blocks: BlockSet,
    l_filling: f64,
    l_inner: f64,
    l_middle: f64,
    l_outer: f64,
    generate_crack_chance: f64,
    base_crack_size: f64,
    crack_point_offset: i32,
    use_potential_placements_chance: f64,
    use_alternate_layer0_chance: f64,
    placements_require_layer0_alternate: bool,
    outer_wall_distance: IntProvider,
    distribution_points: IntProvider,
    point_offset: IntProvider,
    min_gen_offset: i32,
    max_gen_offset: i32,
    noise_multiplier: f64,
    invalid_blocks_threshold: i32,
}

#[derive(Clone, Debug)]
pub struct PointedDripstone {
    taller: f32,
    directional: f32,
    radius2: f32,
    radius3: f32,
}

#[derive(Clone, Debug)]
pub enum CaveKind {
    Multiface(Multiface),
    Vines,
    Bamboo(f32),
    VegetationPatch(Box<VegPatch>),
    RootSystem(Box<RootSystem>),
    Geode(Box<Geode>),
    PointedDripstone(PointedDripstone),
    DripstoneCluster(Box<super::dripstone::Cluster>),
    LargeDripstone(Box<super::dripstone::Large>),
    Todo(&'static str),
}

fn fl(v: &Value, k: &str) -> f32 {
    v[k].as_f64().unwrap_or_else(|| panic!("missing {k}")) as f32
}

fn int(v: &Value, k: &str, d: i64) -> i32 {
    v.get(k).and_then(Value::as_i64).unwrap_or(d) as i32
}

impl CaveKind {
    pub fn from_json(reg: &mut Registry, t: &str, c: &Value) -> Option<Self> {
        let b = rc_data::blocks();
        Some(match t {
            "multiface_growth" => {
                let mut directions = Vec::new();
                if c.get("can_place_on_ceiling")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    directions.push(Direction::Up);
                }
                if c.get("can_place_on_floor")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    directions.push(Direction::Down);
                }
                if c.get("can_place_on_wall")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    directions.extend(HORIZONTAL);
                }
                Self::Multiface(Multiface {
                    block: b.index_of(c["block"].as_str().unwrap()).unwrap(),
                    search_range: int(c, "search_range", 10),
                    directions,
                    chance_of_spreading: c
                        .get("chance_of_spreading")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.5) as f32,
                    can_be_placed_on: holder_set(&c["can_be_placed_on"]),
                })
            }
            "vines" => Self::Vines,
            "bamboo" => Self::Bamboo(fl(c, "probability")),
            "vegetation_patch" | "waterlogged_vegetation_patch" => {
                Self::VegetationPatch(Box::new(VegPatch {
                    replaceable: holder_set(&c["replaceable"]),
                    ground: StateProvider::from_json(&c["ground_state"]),
                    vegetation: reg.placed_value(&c["vegetation_feature"]),
                    ceiling: c["surface"].as_str() == Some("ceiling"),
                    depth: IntProvider::from_json(&c["depth"]),
                    extra_bottom_block_chance: fl(c, "extra_bottom_block_chance"),
                    vertical_range: int(c, "vertical_range", 1),
                    vegetation_chance: fl(c, "vegetation_chance"),
                    xz_radius: IntProvider::from_json(&c["xz_radius"]),
                    extra_edge_column_chance: fl(c, "extra_edge_column_chance"),
                    waterlogged: t == "waterlogged_vegetation_patch",
                }))
            }
            "root_system" => Self::RootSystem(Box::new(RootSystem {
                feature: reg.placed_value(&c["feature"]),
                required_vertical_space_for_tree: int(c, "required_vertical_space_for_tree", 1),
                root_radius: int(c, "root_radius", 1),
                root_replaceable: holder_set(&c["root_replaceable"]),
                root_state: StateProvider::from_json(&c["root_state_provider"]),
                root_placement_attempts: int(c, "root_placement_attempts", 1),
                root_column_max_height: int(c, "root_column_max_height", 1),
                hanging_root_radius: int(c, "hanging_root_radius", 1),
                hanging_roots_vertical_span: int(c, "hanging_roots_vertical_span", 0),
                hanging_root_state: StateProvider::from_json(&c["hanging_root_state_provider"]),
                hanging_root_placement_attempts: int(c, "hanging_root_placement_attempts", 1),
                allowed_vertical_water_for_tree: int(c, "allowed_vertical_water_for_tree", 1),
                allowed_tree_position: BlockPredicate::from_json(&c["allowed_tree_position"]),
            })),
            "geode" => {
                let bl = &c["blocks"];
                let ls = &c["layers"];
                let cs = &c["crack"];
                let d =
                    |v: &Value, k: &str, def: f64| v.get(k).and_then(Value::as_f64).unwrap_or(def);
                Self::Geode(Box::new(Geode {
                    filling: StateProvider::from_json(&bl["filling_provider"]),
                    inner: StateProvider::from_json(&bl["inner_layer_provider"]),
                    alternate_inner: StateProvider::from_json(
                        &bl["alternate_inner_layer_provider"],
                    ),
                    middle: StateProvider::from_json(&bl["middle_layer_provider"]),
                    outer: StateProvider::from_json(&bl["outer_layer_provider"]),
                    inner_placements: bl["inner_placements"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(super::predicate::state)
                        .collect(),
                    cannot_replace: holder_set(&bl["cannot_replace"]),
                    invalid_blocks: holder_set(&bl["invalid_blocks"]),
                    l_filling: d(ls, "filling", 1.7),
                    l_inner: d(ls, "inner_layer", 2.2),
                    l_middle: d(ls, "middle_layer", 3.2),
                    l_outer: d(ls, "outer_layer", 4.2),
                    generate_crack_chance: d(cs, "generate_crack_chance", 1.0),
                    base_crack_size: d(cs, "base_crack_size", 2.0),
                    crack_point_offset: int(cs, "crack_point_offset", 2),
                    use_potential_placements_chance: d(c, "use_potential_placements_chance", 0.35),
                    use_alternate_layer0_chance: d(c, "use_alternate_layer0_chance", 0.0),
                    placements_require_layer0_alternate: c
                        .get("placements_require_layer0_alternate")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                    outer_wall_distance: IntProvider::from_json(&c["outer_wall_distance"]),
                    distribution_points: IntProvider::from_json(&c["distribution_points"]),
                    point_offset: IntProvider::from_json(&c["point_offset"]),
                    min_gen_offset: int(c, "min_gen_offset", -16),
                    max_gen_offset: int(c, "max_gen_offset", 16),
                    noise_multiplier: d(c, "noise_multiplier", 0.05),
                    invalid_blocks_threshold: int(c, "invalid_blocks_threshold", 0),
                }))
            }
            "pointed_dripstone" => Self::PointedDripstone(PointedDripstone {
                taller: c
                    .get("chance_of_taller_dripstone")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.2) as f32,
                directional: c
                    .get("chance_of_directional_spread")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.7) as f32,
                radius2: c
                    .get("chance_of_spread_radius2")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.5) as f32,
                radius3: c
                    .get("chance_of_spread_radius3")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.5) as f32,
            }),
            "dripstone_cluster" => {
                Self::DripstoneCluster(Box::new(super::dripstone::Cluster::from_json(c)))
            }
            "large_dripstone" => {
                Self::LargeDripstone(Box::new(super::dripstone::Large::from_json(c)))
            }
            "sculk_patch" => Self::Todo("sculk_patch"),
            "fossil" => Self::Todo("fossil"),
            _ => return None,
        })
    }

    pub fn place(&self, ctx: &mut Ctx<'_, '_>, r: &mut Random, pos: BlockPos) -> bool {
        match self {
            Self::Multiface(m) => multiface(ctx, m, r, pos),
            Self::Vines => vines(ctx, pos),
            Self::Bamboo(p) => bamboo(ctx, *p, r, pos),
            Self::VegetationPatch(v) => veg_patch(ctx, v, r, pos),
            Self::RootSystem(rs) => root_system(ctx, rs, r, pos),
            Self::Geode(g) => geode(ctx, g, r, pos),
            Self::PointedDripstone(p) => pointed_dripstone(ctx, p, r, pos),
            Self::DripstoneCluster(c) => super::dripstone::cluster(ctx, c, r, pos),
            Self::LargeDripstone(c) => super::dripstone::large(ctx, c, r, pos),
            Self::Todo(_) => false,
        }
    }
}

pub(super) fn face_prop(d: Direction) -> &'static str {
    match d {
        Direction::North => "north",
        Direction::South => "south",
        Direction::East => "east",
        Direction::West => "west",
        Direction::Up => "up",
        Direction::Down => "down",
    }
}

// ---- multiface growth (glow lichen) -------------------------------------

fn has_face(s: BlockStateId, d: Direction) -> bool {
    rc_data::blocks().get(s, face_prop(d)) == Some("true")
}

/// `MultifaceBlock.getStateForPlacement`.
fn multiface_state_for(
    ctx: &Ctx<'_, '_>,
    block: u16,
    current: BlockStateId,
    p: BlockPos,
    d: Direction,
) -> Option<BlockStateId> {
    let h = bh();
    let b = rc_data::blocks();
    let is_this = h.block(current) == block;
    if is_this && has_face(current, d) {
        return None;
    }
    let q = p.offset(d);
    if !h.can_attach_to(ctx.region.get(q), d) {
        return None;
    }
    let base = if is_this {
        current
    } else {
        let def = b.defs[block as usize].default;
        let f = b.fluid(current);
        if b.prop_pos(def, "waterlogged").is_some() && f.is_water() && f.is_source() {
            b.with(def, "waterlogged", "true")
        } else {
            def
        }
    };
    Some(b.with(base, face_prop(d), "true"))
}

fn is_air_or_water(s: BlockStateId) -> bool {
    let h = bh();
    h.is_air(s) || h.block(s) == h.idx.water
}

fn multiface_spread(
    ctx: &mut Ctx<'_, '_>,
    m: &Multiface,
    state: BlockStateId,
    p: BlockPos,
    face: Direction,
    r: &mut Random,
) {
    let mut dirs = Direction::ALL.to_vec();
    shuffle(&mut dirs, r);
    let axis = |d: Direction| match d {
        Direction::Up | Direction::Down => 1,
        Direction::North | Direction::South => 2,
        _ => 0,
    };
    for d in dirs {
        if axis(d) == axis(face) {
            continue;
        }
        if !(has_face(state, face) && !has_face(state, d)) {
            continue;
        }
        let candidates = [
            (p, d),
            (p.offset(d), face),
            (p.offset(d).offset(face), d.opposite()),
        ];
        for (sp, sf) in candidates {
            let s = ctx.region.get(sp);
            let replaceable = {
                let h = bh();
                let f = rc_data::blocks().fluid(s);
                h.is_air(s) || h.block(s) == m.block || (h.block(s) == h.idx.water && f.is_source())
            };
            if !replaceable {
                continue;
            }
            let Some(ns) = multiface_state_for(ctx, m.block, s, sp, sf) else {
                continue;
            };
            ctx.region.set(sp, ns);
            ctx.region.mark_post_process(sp);
            return;
        }
    }
}

fn place_growth(
    ctx: &mut Ctx<'_, '_>,
    m: &Multiface,
    p: BlockPos,
    current: BlockStateId,
    r: &mut Random,
    list: &[Direction],
) -> bool {
    let h = bh();
    for &d in list {
        let q = p.offset(d);
        if !m.can_be_placed_on.contains(h.block(ctx.region.get(q))) {
            continue;
        }
        let Some(ns) = multiface_state_for(ctx, m.block, current, p, d) else {
            return false;
        };
        ctx.region.set(p, ns);
        ctx.region.mark_post_process(p);
        if r.next_float() < m.chance_of_spreading {
            multiface_spread(ctx, m, ns, p, d, r);
        }
        return true;
    }
    false
}

fn multiface(ctx: &mut Ctx<'_, '_>, m: &Multiface, r: &mut Random, pos: BlockPos) -> bool {
    let cur = ctx.region.get(pos);
    if !is_air_or_water(cur) {
        return false;
    }
    let mut list = m.directions.clone();
    shuffle(&mut list, r);
    if place_growth(ctx, m, pos, cur, r, &list) {
        return true;
    }
    for &d in &list {
        let mut list2: Vec<Direction> = m
            .directions
            .iter()
            .copied()
            .filter(|x| *x != d.opposite())
            .collect();
        shuffle(&mut list2, r);
        for _ in 0..m.search_range {
            // Vanilla re-checks the same adjacent position every iteration.
            let q = pos.offset(d);
            let s = ctx.region.get(q);
            if !is_air_or_water(s) && bh().block(s) != m.block {
                break;
            }
            if place_growth(ctx, m, q, s, r, &list2) {
                return true;
            }
        }
    }
    false
}

// ---- vines / bamboo ------------------------------------------------------

fn vines(ctx: &mut Ctx<'_, '_>, pos: BlockPos) -> bool {
    let h = bh();
    let b = rc_data::blocks();
    if !ctx.region.is_air(pos) {
        return false;
    }
    for d in Direction::ALL {
        if d == Direction::Down {
            continue;
        }
        if !h.can_attach_to(ctx.region.get(pos.offset(d)), d) {
            continue;
        }
        ctx.region
            .set(pos, b.with(b.default_state("vine"), face_prop(d), "true"));
        return true;
    }
    false
}

fn bamboo(ctx: &mut Ctx<'_, '_>, prob: f32, r: &mut Random, pos: BlockPos) -> bool {
    let h = bh();
    let b = rc_data::blocks();
    let trunk = {
        let s = b.default_state("bamboo");
        let s = b.with(s, "age", "1");
        let s = b.with(s, "leaves", "none");
        b.with(s, "stage", "0")
    };
    let top_large = b.with(trunk, "leaves", "large");
    let final_large = b.with(top_large, "stage", "1");
    let top_small = b.with(trunk, "leaves", "small");
    let mut placed = false;
    if ctx.region.is_air(pos) {
        if can_survive(ctx.region, b.default_state("bamboo"), pos) {
            let j = r.next_int_bound(12) + 5;
            if r.next_float() < prob {
                let k = r.next_int_bound(4) + 1;
                let podzol = b.default_state("podzol");
                for l in pos.x - k..=pos.x + k {
                    for m in pos.z - k..=pos.z + k {
                        let n = l - pos.x;
                        let o = m - pos.z;
                        if n * n + o * o > k * k {
                            continue;
                        }
                        let y = ctx.region.height(Heightmap::WorldSurface, l, m) - 1;
                        let q = bp(l, y, m);
                        if h.is_dirt(ctx.region.get(q)) {
                            ctx.region.set(q, podzol);
                        }
                    }
                }
            }
            let mut mp = pos;
            let mut k = 0;
            while k < j && ctx.region.is_air(mp) {
                ctx.region.set(mp, trunk);
                mp = off(mp, 0, 1, 0);
                k += 1;
            }
            if mp.y - pos.y >= 3 {
                ctx.region.set(mp, final_large);
                mp = off(mp, 0, -1, 0);
                ctx.region.set(mp, top_large);
                mp = off(mp, 0, -1, 0);
                ctx.region.set(mp, top_small);
            }
        }
        placed = true;
    }
    placed
}

// ---- vegetation patches --------------------------------------------------

fn veg_patch(ctx: &mut Ctx<'_, '_>, v: &VegPatch, r: &mut Random, pos: BlockPos) -> bool {
    let h = bh();
    let i = v.xz_radius.sample(r) + 1;
    let j = v.xz_radius.sample(r) + 1;
    let d = if v.ceiling {
        Direction::Up
    } else {
        Direction::Down
    };
    let d2 = d.opposite();
    let mut set = JavaPosSet::new();
    for k in -i..=i {
        let bl = k == -i || k == i;
        for l in -j..=j {
            let bl2 = l == -j || l == j;
            let bl3 = bl || bl2;
            let bl4 = bl && bl2;
            let bl5 = bl3 && !bl4;
            if bl4
                || (bl5
                    && (v.extra_edge_column_chance == 0.0
                        || r.next_float() > v.extra_edge_column_chance))
            {
                continue;
            }
            let mut mp = off(pos, k, 0, l);
            let mut m = 0;
            while ctx.region.is_air(mp) && m < v.vertical_range {
                mp = mp.offset(d);
                m += 1;
            }
            m = 0;
            while !ctx.region.is_air(mp) && m < v.vertical_range {
                mp = mp.offset(d2);
                m += 1;
            }
            let mut mp2 = mp.offset(d);
            let bs = ctx.region.get(mp2);
            if !ctx.region.is_air(mp) || !h.is_face_sturdy(bs, d.opposite()) {
                continue;
            }
            let n = v.depth.sample(r)
                + i32::from(
                    v.extra_bottom_block_chance > 0.0
                        && r.next_float() < v.extra_bottom_block_chance,
                );
            let start = mp2;
            // placeGround
            let mut ok = true;
            for jj in 0..n {
                let s = v.ground.get(r, mp2);
                let cur = ctx.region.get(mp2);
                if h.block(s) == h.block(cur) {
                    continue;
                }
                if !v.replaceable.contains(h.block(cur)) {
                    ok = jj != 0;
                    break;
                }
                ctx.region.set(mp2, s);
                mp2 = mp2.offset(d);
            }
            if ok {
                set.insert(start);
            }
        }
    }
    let final_set = if v.waterlogged {
        let mut set2 = JavaPosSet::new();
        for p in set.java_order() {
            let exposed = [
                Direction::North,
                Direction::East,
                Direction::South,
                Direction::West,
                Direction::Down,
            ]
            .iter()
            .any(|dd| !h.is_face_sturdy(ctx.region.get(p.offset(*dd)), dd.opposite()));
            if !exposed {
                set2.insert(p);
            }
        }
        let water = rc_data::blocks().default_state("water");
        for p in set2.java_order() {
            ctx.region.set(p, water);
        }
        set2
    } else {
        set
    };
    for p in final_set.java_order() {
        if v.vegetation_chance > 0.0 && r.next_float() < v.vegetation_chance {
            if v.waterlogged {
                let q = off(p, 0, -1, 0).offset(d.opposite());
                if ctx.place_placed(v.vegetation, r, q) {
                    let b = rc_data::blocks();
                    let s = ctx.region.get(p);
                    if b.get(s, "waterlogged") == Some("false") {
                        ctx.region.set(p, b.with(s, "waterlogged", "true"));
                    }
                }
            } else {
                ctx.place_placed(v.vegetation, r, p.offset(d.opposite()));
            }
        }
    }
    !final_set.is_empty()
}

// ---- root system (azalea) ------------------------------------------------

fn root_system(ctx: &mut Ctx<'_, '_>, c: &RootSystem, r: &mut Random, pos: BlockPos) -> bool {
    let h = bh();
    if !ctx.region.is_air(pos) {
        return false;
    }
    let mut mp = pos;
    let mut placed_tree = false;
    for i in 0..c.root_column_max_height {
        mp = off(mp, 0, 1, 0);
        if !c.allowed_tree_position.test(ctx.region, mp) {
            continue;
        }
        // spaceForTree
        let mut space = true;
        let mut q = mp;
        for k in 1..=c.required_vertical_space_for_tree {
            q = off(q, 0, 1, 0);
            let s = ctx.region.get(q);
            let ok = h.is_air(s)
                || (k + 1 <= c.allowed_vertical_water_for_tree
                    && rc_data::blocks().fluid(s).is_water());
            if !ok {
                space = false;
                break;
            }
        }
        if !space {
            continue;
        }
        let below = off(mp, 0, -1, 0);
        if ctx.region.fluid(below).is_lava() || !h.is_solid(ctx.region.get(below)) {
            return true;
        }
        if !ctx.place_placed(c.feature, r, mp) {
            continue;
        }
        // placeDirt
        for l in pos.y..pos.y + i {
            let mut d = bp(pos.x, l, pos.z);
            for _ in 0..c.root_placement_attempts {
                let dx = r.next_int_bound(c.root_radius) - r.next_int_bound(c.root_radius);
                let dz = r.next_int_bound(c.root_radius) - r.next_int_bound(c.root_radius);
                d = off(d, dx, 0, dz);
                if c.root_replaceable.contains(h.block(ctx.region.get(d))) {
                    let s = c.root_state.get(r, d);
                    ctx.region.set(d, s);
                }
                d = bp(pos.x, d.y, pos.z);
            }
        }
        placed_tree = true;
        break;
    }
    if placed_tree {
        let i = c.hanging_root_radius;
        let j = c.hanging_roots_vertical_span;
        for _ in 0..c.hanging_root_placement_attempts {
            let dx = r.next_int_bound(i) - r.next_int_bound(i);
            let dy = r.next_int_bound(j) - r.next_int_bound(j);
            let dz = r.next_int_bound(i) - r.next_int_bound(i);
            let q = off(pos, dx, dy, dz);
            if !ctx.region.is_air(q) {
                continue;
            }
            let s = c.hanging_root_state.get(r, q);
            if !can_survive(ctx.region, s, q)
                || !h.is_face_sturdy(ctx.region.get(off(q, 0, 1, 0)), Direction::Down)
            {
                continue;
            }
            ctx.region.set(q, s);
        }
    }
    true
}

// ---- geode ---------------------------------------------------------------

fn geode(ctx: &mut Ctx<'_, '_>, g: &Geode, r: &mut Random, pos: BlockPos) -> bool {
    let h = bh();
    let b = rc_data::blocks();
    let i = g.min_gen_offset;
    let j = g.max_gen_offset;
    let mut points: Vec<(BlockPos, i32)> = Vec::new();
    let k2 = g.distribution_points.sample(r);
    let noise = NormalNoise::new(
        &mut Random::legacy(ctx.region.dg.seed),
        &NoiseParams {
            first_octave: -4,
            amplitudes: vec![1.0],
        },
    );
    let mut cracks: Vec<BlockPos> = Vec::new();
    let d = f64::from(k2) / f64::from(g.outer_wall_distance.max_value());
    let e = 1.0 / g.l_filling.sqrt();
    let f = 1.0 / (g.l_inner + d).sqrt();
    let gg = 1.0 / (g.l_middle + d).sqrt();
    let hh = 1.0 / (g.l_outer + d).sqrt();
    let l2 =
        1.0 / (g.base_crack_size + r.next_double() / 2.0 + if k2 > 3 { d } else { 0.0 }).sqrt();
    let crack = f64::from(r.next_float()) < g.generate_crack_chance;
    let mut m = 0;
    for _ in 0..k2 {
        let n = g.outer_wall_distance.sample(r);
        let o = g.outer_wall_distance.sample(r);
        let p = g.outer_wall_distance.sample(r);
        let q = off(pos, n, o, p);
        let s = ctx.region.get(q);
        if h.is_air(s) || g.invalid_blocks.contains(h.block(s)) {
            m += 1;
            if m > g.invalid_blocks_threshold {
                return false;
            }
        }
        points.push((q, g.point_offset.sample(r)));
    }
    if crack {
        let k = r.next_int_bound(4);
        let l = k2 * 2 + 1;
        let (cx, cz) = match k {
            0 => (l, 0),
            1 => (0, l),
            2 => (l, l),
            _ => (0, 0),
        };
        for y in [7, 5, 1] {
            cracks.push(off(pos, cx, y, cz));
        }
    }
    let mut potential: Vec<BlockPos> = Vec::new();
    let can_replace =
        |ctx: &Ctx<'_, '_>, p: BlockPos| !g.cannot_replace.contains(h.block(ctx.region.get(p)));
    let dist = |a: BlockPos, c: BlockPos| {
        let dx = f64::from(a.x - c.x);
        let dy = f64::from(a.y - c.y);
        let dz = f64::from(a.z - c.z);
        dx * dx + dy * dy + dz * dz
    };
    for z in pos.z + i..=pos.z + j {
        for y in pos.y + i..=pos.y + j {
            for x in pos.x + i..=pos.x + j {
                let p = bp(x, y, z);
                let rr =
                    noise.get_value(f64::from(x), f64::from(y), f64::from(z)) * g.noise_multiplier;
                let mut s = 0.0;
                let mut t = 0.0;
                for (pt, off_) in &points {
                    s += 1.0 / (dist(p, *pt) + f64::from(*off_)).sqrt() + rr;
                }
                for c in &cracks {
                    t += 1.0 / (dist(p, *c) + f64::from(g.crack_point_offset)).sqrt() + rr;
                }
                if s < hh {
                    continue;
                }
                if crack && t >= l2 && s < e {
                    if can_replace(ctx, p) {
                        ctx.region.set(p, BlockStateId::AIR);
                    }
                    continue;
                }
                if s >= e {
                    let st = g.filling.get(r, p);
                    if can_replace(ctx, p) {
                        ctx.region.set(p, st);
                    }
                    continue;
                }
                if s >= f {
                    let alt = f64::from(r.next_float()) < g.use_alternate_layer0_chance;
                    let st = if alt {
                        g.alternate_inner.get(r, p)
                    } else {
                        g.inner.get(r, p)
                    };
                    if can_replace(ctx, p) {
                        ctx.region.set(p, st);
                    }
                    if (g.placements_require_layer0_alternate && !alt)
                        || !(f64::from(r.next_float()) < g.use_potential_placements_chance)
                    {
                        continue;
                    }
                    potential.push(p);
                    continue;
                }
                if s >= gg {
                    let st = g.middle.get(r, p);
                    if can_replace(ctx, p) {
                        ctx.region.set(p, st);
                    }
                    continue;
                }
                let st = g.outer.get(r, p);
                if can_replace(ctx, p) {
                    ctx.region.set(p, st);
                }
            }
        }
    }
    'outer: for p in potential {
        let mut st = g.inner_placements[r.next_int_bound(g.inner_placements.len() as i32) as usize];
        for d3 in Direction::ALL {
            if b.prop_pos(st, "facing").is_some() {
                st = b.with(st, "facing", face_prop(d3));
            }
            let q = p.offset(d3);
            let s3 = ctx.region.get(q);
            if b.prop_pos(st, "waterlogged").is_some() {
                st = b.with(
                    st,
                    "waterlogged",
                    if b.fluid(s3).is_source() {
                        "true"
                    } else {
                        "false"
                    },
                );
            }
            let f3 = b.fluid(s3);
            let grow = h.is_air(s3) || (h.block(s3) == h.idx.water && f3.amount == 8);
            if !grow {
                continue;
            }
            if can_replace(ctx, q) {
                ctx.region.set(q, st);
            }
            continue 'outer;
        }
    }
    true
}

// ---- pointed dripstone ---------------------------------------------------

pub(super) fn is_dripstone_base(s: BlockStateId) -> bool {
    let h = bh();
    let t = rc_data::tags();
    h.name(s) == "dripstone_block"
        || t.block_ref("dripstone_replaceable_blocks")
            .is_some_and(|x| x.contains(h.block(s)))
}

pub(super) fn is_empty_or_water(s: BlockStateId) -> bool {
    let h = bh();
    h.is_air(s) || h.block(s) == h.idx.water
}

pub(super) fn place_dripstone_block_if_possible(ctx: &mut Ctx<'_, '_>, p: BlockPos) -> bool {
    let h = bh();
    let t = rc_data::tags();
    let s = ctx.region.get(p);
    if t.block_ref("dripstone_replaceable_blocks")
        .is_some_and(|x| x.contains(h.block(s)))
    {
        ctx.region
            .set(p, rc_data::blocks().default_state("dripstone_block"));
        return true;
    }
    false
}

pub(super) fn pointed(d: Direction, thickness: &str) -> BlockStateId {
    let b = rc_data::blocks();
    let s = b.default_state("pointed_dripstone");
    let s = b.with(
        s,
        "vertical_direction",
        if d == Direction::Up { "up" } else { "down" },
    );
    b.with(s, "thickness", thickness)
}

/// `DripstoneUtils.growPointedDripstone`.
pub(super) fn grow_pointed_dripstone(
    ctx: &mut Ctx<'_, '_>,
    pos: BlockPos,
    d: Direction,
    i: i32,
    merge: bool,
) {
    if !is_dripstone_base(ctx.region.get(pos.offset(d.opposite()))) {
        return;
    }
    let mut seq = Vec::new();
    if i >= 3 {
        seq.push(pointed(d, "base"));
        for _ in 0..i - 3 {
            seq.push(pointed(d, "middle"));
        }
    }
    if i >= 2 {
        seq.push(pointed(d, "frustum"));
    }
    if i >= 1 {
        seq.push(pointed(d, if merge { "tip_merge" } else { "tip" }));
    }
    let b = rc_data::blocks();
    let mut mp = pos;
    for s in seq {
        let w = ctx.region.fluid(mp).is_water();
        ctx.region.set(
            mp,
            b.with(s, "waterlogged", if w { "true" } else { "false" }),
        );
        mp = mp.offset(d);
    }
}

fn random_direction(r: &mut Random) -> Direction {
    Direction::ALL[r.next_int_bound(6) as usize]
}

fn pointed_dripstone(
    ctx: &mut Ctx<'_, '_>,
    c: &PointedDripstone,
    r: &mut Random,
    pos: BlockPos,
) -> bool {
    let up = is_dripstone_base(ctx.region.get(off(pos, 0, 1, 0)));
    let down = is_dripstone_base(ctx.region.get(off(pos, 0, -1, 0)));
    let dir = if up && down {
        if r.next_bool() {
            Direction::Down
        } else {
            Direction::Up
        }
    } else if up {
        Direction::Down
    } else if down {
        Direction::Up
    } else {
        return false;
    };
    let base = pos.offset(dir.opposite());
    place_dripstone_block_if_possible(ctx, base);
    for d in HORIZONTAL {
        if r.next_float() > c.directional {
            continue;
        }
        let q = base.offset(d);
        place_dripstone_block_if_possible(ctx, q);
        if r.next_float() > c.radius2 {
            continue;
        }
        let q2 = q.offset(random_direction(r));
        place_dripstone_block_if_possible(ctx, q2);
        if r.next_float() > c.radius3 {
            continue;
        }
        let q3 = q2.offset(random_direction(r));
        place_dripstone_block_if_possible(ctx, q3);
    }
    let i = if r.next_float() < c.taller && is_empty_or_water(ctx.region.get(pos.offset(dir))) {
        2
    } else {
        1
    };
    grow_pointed_dripstone(ctx, pos, dir, i, false);
    true
}

#[allow(dead_code)]
fn _fp(v: &Value) -> FloatProvider {
    FloatProvider::from_json(v)
}
