//! Configured feature kinds and the general-purpose features.

use rc_core::{BlockPos, BlockStateId, Direction};
use rc_data::tags::BlockSet;
use serde_json::Value;

use super::behaviour::{bh, can_survive};
use super::predicate::{BlockPredicate, RuleTest, StateProvider, direction, state};
use super::{Ctx, PlacedId, Registry, cave, end, nether, tree, water};
use crate::mth;
use crate::proto::Heightmap;
use crate::providers::IntProvider;
use crate::rng::Random;

#[derive(Clone, Debug)]
pub struct OreConfig {
    pub size: i32,
    pub discard: f32,
    pub targets: Vec<(RuleTest, BlockStateId)>,
}

#[derive(Clone, Debug)]
pub enum Kind {
    Ore(OreConfig),
    ScatteredOre(OreConfig),
    SimpleBlock(StateProvider),
    RandomPatch {
        tries: i32,
        xz: i32,
        y: i32,
        feature: PlacedId,
    },
    RandomSelector {
        features: Vec<(f32, PlacedId)>,
        default: PlacedId,
    },
    SimpleRandomSelector(Vec<PlacedId>),
    RandomBooleanSelector(PlacedId, PlacedId),
    BlockColumn {
        layers: Vec<(IntProvider, StateProvider)>,
        dir: Direction,
        allowed: BlockPredicate,
        prioritize_tip: bool,
    },
    Disk {
        fallback: StateProvider,
        rules: Vec<(BlockPredicate, StateProvider)>,
        target: BlockPredicate,
        radius: IntProvider,
        half_height: i32,
    },
    Spring {
        state: BlockStateId,
        requires_below: bool,
        rock_count: i32,
        hole_count: i32,
        valid: BlockSet,
    },
    Lake {
        fluid: StateProvider,
        barrier: StateProvider,
    },
    FreezeTopLayer,
    ForestRock(BlockStateId),
    IceSpike,
    BlueIce,
    HugeMushroom {
        red: bool,
        cap: StateProvider,
        stem: StateProvider,
        radius: i32,
    },
    MonsterRoom,
    DesertWell,
    Tree(Box<tree::TreeConfig>),
    Water(water::WaterKind),
    Cave(cave::CaveKind),
    Nether(nether::NetherKind),
    End(end::EndKind),
    /// Not part of natural generation (bonus chest, spawn platforms) or
    /// arriving with structures in Phase 3.
    NoOp,
}

fn i(v: &Value, k: &str) -> i32 {
    v[k].as_i64()
        .unwrap_or_else(|| panic!("missing {k} in {v}")) as i32
}

fn f(v: &Value, k: &str) -> f32 {
    v[k].as_f64()
        .unwrap_or_else(|| panic!("missing {k} in {v}")) as f32
}

pub(super) fn holder_set(v: &Value) -> BlockSet {
    let t = rc_data::tags();
    let b = rc_data::blocks();
    let mut set = t.block("#minecraft:__none__");
    match v {
        Value::String(s) if s.starts_with('#') => return t.block(s),
        Value::String(s) => set.insert(b.index_of(s).unwrap()),
        Value::Array(a) => {
            for x in a {
                set.insert(b.index_of(x.as_str().unwrap()).unwrap());
            }
        }
        _ => {}
    }
    set
}

fn ore(c: &Value) -> OreConfig {
    OreConfig {
        size: i(c, "size"),
        discard: f(c, "discard_chance_on_air_exposure"),
        targets: c["targets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| (RuleTest::from_json(&t["target"]), state(&t["state"])))
            .collect(),
    }
}

impl Kind {
    pub fn from_json(reg: &mut Registry, t: &str, c: &Value) -> Self {
        let t = t.trim_start_matches("minecraft:");
        match t {
            "ore" => Self::Ore(ore(c)),
            "scattered_ore" => Self::ScatteredOre(ore(c)),
            "simple_block" => Self::SimpleBlock(StateProvider::from_json(&c["to_place"])),
            "random_patch" | "flower" | "no_bonemeal_flower" => Self::RandomPatch {
                tries: c.get("tries").and_then(Value::as_i64).unwrap_or(128) as i32,
                xz: c.get("xz_spread").and_then(Value::as_i64).unwrap_or(7) as i32,
                y: c.get("y_spread").and_then(Value::as_i64).unwrap_or(3) as i32,
                feature: reg.placed_value(&c["feature"]),
            },
            "random_selector" => Self::RandomSelector {
                features: c["features"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| (f(e, "chance"), reg.placed_value(&e["feature"])))
                    .collect(),
                default: reg.placed_value(&c["default"]),
            },
            "simple_random_selector" => {
                let fs = match &c["features"] {
                    Value::Array(a) => a.iter().map(|e| reg.placed_value(e)).collect(),
                    other => vec![reg.placed_value(other)],
                };
                Self::SimpleRandomSelector(fs)
            }
            "random_boolean_selector" => Self::RandomBooleanSelector(
                reg.placed_value(&c["feature_true"]),
                reg.placed_value(&c["feature_false"]),
            ),
            "block_column" => Self::BlockColumn {
                layers: c["layers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|l| {
                        (
                            IntProvider::from_json(&l["height"]),
                            StateProvider::from_json(&l["provider"]),
                        )
                    })
                    .collect(),
                dir: direction(c["direction"].as_str().unwrap()),
                allowed: BlockPredicate::from_json(&c["allowed_placement"]),
                prioritize_tip: c["prioritize_tip"].as_bool().unwrap_or(false),
            },
            "disk" => {
                let sp = &c["state_provider"];
                Self::Disk {
                    fallback: StateProvider::from_json(&sp["fallback"]),
                    rules: sp["rules"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|r| {
                            (
                                BlockPredicate::from_json(&r["if_true"]),
                                StateProvider::from_json(&r["then"]),
                            )
                        })
                        .collect(),
                    target: BlockPredicate::from_json(&c["target"]),
                    radius: IntProvider::from_json(&c["radius"]),
                    half_height: i(c, "half_height"),
                }
            }
            "spring_feature" => {
                let fs = &c["state"];
                let name = fs["Name"].as_str().unwrap();
                let block = if name.contains("lava") {
                    "lava"
                } else {
                    "water"
                };
                Self::Spring {
                    state: rc_data::blocks().default_state(block),
                    requires_below: c["requires_block_below"].as_bool().unwrap_or(true),
                    rock_count: i(c, "rock_count"),
                    hole_count: i(c, "hole_count"),
                    valid: holder_set(&c["valid_blocks"]),
                }
            }
            "lake" => Self::Lake {
                fluid: StateProvider::from_json(&c["fluid"]),
                barrier: StateProvider::from_json(&c["barrier"]),
            },
            "freeze_top_layer" => Self::FreezeTopLayer,
            "forest_rock" => Self::ForestRock(state(&c["state"])),
            "ice_spike" => Self::IceSpike,
            "blue_ice" => Self::BlueIce,
            "huge_red_mushroom" | "huge_brown_mushroom" => Self::HugeMushroom {
                red: t == "huge_red_mushroom",
                cap: StateProvider::from_json(&c["cap_provider"]),
                stem: StateProvider::from_json(&c["stem_provider"]),
                radius: c.get("foliage_radius").and_then(Value::as_i64).unwrap_or(2) as i32,
            },
            "monster_room" => Self::MonsterRoom,
            "desert_well" => Self::DesertWell,
            "tree" => Self::Tree(Box::new(tree::TreeConfig::from_json(c))),
            "bonus_chest" | "void_start_platform" | "end_platform" | "block_pile" => Self::NoOp,
            other => {
                if let Some(k) = water::WaterKind::from_json(reg, other, c) {
                    Self::Water(k)
                } else if let Some(k) = cave::CaveKind::from_json(reg, other, c) {
                    Self::Cave(k)
                } else if let Some(k) = nether::NetherKind::from_json(reg, other, c) {
                    Self::Nether(k)
                } else if let Some(k) = end::EndKind::from_json(reg, other, c) {
                    Self::End(k)
                } else {
                    Self::NoOp
                }
            }
        }
    }

    pub fn place(&self, ctx: &mut Ctx<'_, '_>, r: &mut Random, pos: BlockPos) -> bool {
        match self {
            Self::Ore(c) => place_ore(ctx, c, r, pos),
            Self::ScatteredOre(c) => place_scattered_ore(ctx, c, r, pos),
            Self::SimpleBlock(p) => place_simple_block(ctx, p, r, pos),
            Self::RandomPatch {
                tries,
                xz,
                y,
                feature,
            } => {
                let mut n = 0;
                let j = xz + 1;
                let k = y + 1;
                for _ in 0..*tries {
                    let dx = r.next_int_bound(j) - r.next_int_bound(j);
                    let dy = r.next_int_bound(k) - r.next_int_bound(k);
                    let dz = r.next_int_bound(j) - r.next_int_bound(j);
                    if ctx.place_placed(
                        *feature,
                        r,
                        BlockPos::new(pos.x + dx, pos.y + dy, pos.z + dz),
                    ) {
                        n += 1;
                    }
                }
                n > 0
            }
            Self::RandomSelector { features, default } => {
                for (chance, pf) in features {
                    if r.next_float() < *chance {
                        return ctx.place_placed(*pf, r, pos);
                    }
                }
                ctx.place_placed(*default, r, pos)
            }
            Self::SimpleRandomSelector(list) => {
                let i = r.next_int_bound(list.len() as i32) as usize;
                ctx.place_placed(list[i], r, pos)
            }
            Self::RandomBooleanSelector(t, f) => {
                let b = r.next_bool();
                ctx.place_placed(if b { *t } else { *f }, r, pos)
            }
            Self::BlockColumn {
                layers,
                dir,
                allowed,
                prioritize_tip,
            } => place_block_column(ctx, layers, *dir, allowed, *prioritize_tip, r, pos),
            Self::Disk {
                fallback,
                rules,
                target,
                radius,
                half_height,
            } => place_disk(ctx, fallback, rules, target, radius, *half_height, r, pos),
            Self::Spring {
                state,
                requires_below,
                rock_count,
                hole_count,
                valid,
            } => place_spring(
                ctx,
                *state,
                *requires_below,
                *rock_count,
                *hole_count,
                valid,
                pos,
            ),
            Self::Lake { fluid, barrier } => place_lake(ctx, fluid, barrier, r, pos),
            Self::FreezeTopLayer => place_freeze_top_layer(ctx, pos),
            Self::ForestRock(s) => place_forest_rock(ctx, *s, r, pos),
            Self::IceSpike => place_ice_spike(ctx, r, pos),
            Self::BlueIce => place_blue_ice(ctx, r, pos),
            Self::HugeMushroom {
                red,
                cap,
                stem,
                radius,
            } => place_huge_mushroom(ctx, *red, cap, stem, *radius, r, pos),
            Self::MonsterRoom => place_monster_room(ctx, r, pos),
            Self::DesertWell => place_desert_well(ctx, r, pos),
            Self::Tree(c) => tree::place(ctx, c, r, pos),
            Self::Water(k) => k.place(ctx, r, pos),
            Self::Cave(k) => k.place(ctx, r, pos),
            Self::Nether(k) => k.place(ctx, r, pos),
            Self::End(k) => k.place(ctx, r, pos),
            Self::NoOp => false,
        }
    }
}

// ---- helpers ----------------------------------------------------------------

#[inline]
pub(super) fn bp(x: i32, y: i32, z: i32) -> BlockPos {
    BlockPos::new(x, y, z)
}

#[inline]
pub(super) fn off(p: BlockPos, x: i32, y: i32, z: i32) -> BlockPos {
    BlockPos::new(p.x + x, p.y + y, p.z + z)
}

pub(super) fn is_stone(s: BlockStateId) -> bool {
    let h = bh();
    h.base_stone_overworld.contains(h.block(s))
}

/// `DoublePlantBlock.placeAt`.
pub(super) fn place_double_plant(ctx: &mut Ctx<'_, '_>, s: BlockStateId, p: BlockPos) {
    let b = rc_data::blocks();
    let above = off(p, 0, 1, 0);
    let wl = |ctx: &Ctx<'_, '_>, s: BlockStateId, q: BlockPos| {
        if b.prop_pos(s, "waterlogged").is_some() {
            let f = ctx.region.fluid(q);
            b.with(
                s,
                "waterlogged",
                if f.is_water() && f.is_source() {
                    "true"
                } else {
                    "false"
                },
            )
        } else {
            s
        }
    };
    let lower = wl(ctx, b.with(s, "half", "lower"), p);
    ctx.region.set(p, lower);
    let upper = wl(ctx, b.with(s, "half", "upper"), above);
    ctx.region.set(above, upper);
}

fn is_double_plant(s: BlockStateId) -> bool {
    matches!(
        bh().name(s),
        "tall_grass"
            | "large_fern"
            | "sunflower"
            | "lilac"
            | "rose_bush"
            | "peony"
            | "pitcher_plant"
            | "tall_seagrass"
    ) || (bh().name(s) == "small_dripleaf")
}

// ---- ore --------------------------------------------------------------------

fn can_place_ore(
    ctx: &Ctx<'_, '_>,
    bs: BlockStateId,
    r: &mut Random,
    c: &OreConfig,
    t: &RuleTest,
    p: BlockPos,
) -> bool {
    if !t.test(bs, r) {
        return false;
    }
    // shouldSkipAirCheck
    let skip = if c.discard <= 0.0 {
        true
    } else if c.discard >= 1.0 {
        false
    } else {
        r.next_float() >= c.discard
    };
    if skip {
        return true;
    }
    !Direction::ALL
        .iter()
        .any(|d| ctx.region.is_air(p.offset(*d)))
}

fn place_ore(ctx: &mut Ctx<'_, '_>, c: &OreConfig, r: &mut Random, pos: BlockPos) -> bool {
    let ff = r.next_float() * std::f32::consts::PI;
    let g = c.size as f32 / 8.0;
    let i = mth_ceil_f((c.size as f32 / 16.0 * 2.0 + 1.0) / 2.0);
    let fd = f64::from(ff);
    let d = f64::from(pos.x) + fd.sin() * f64::from(g);
    let e = f64::from(pos.x) - fd.sin() * f64::from(g);
    let h = f64::from(pos.z) + fd.cos() * f64::from(g);
    let j = f64::from(pos.z) - fd.cos() * f64::from(g);
    let l = f64::from(pos.y + r.next_int_bound(3) - 2);
    let m = f64::from(pos.y + r.next_int_bound(3) - 2);
    let n = pos.x - mth_ceil_f(g) - i;
    let o = pos.y - 2 - i;
    let p = pos.z - mth_ceil_f(g) - i;
    let q = 2 * (mth_ceil_f(g) + i);
    let rr = 2 * (2 + i);
    for s in n..=n + q {
        for t in p..=p + q {
            if o <= ctx.region.height(Heightmap::OceanFloorWg, s, t) {
                return do_place_ore(ctx, c, r, d, e, h, j, l, m, n, o, p, q, rr);
            }
        }
    }
    false
}

fn mth_ceil_f(v: f32) -> i32 {
    let i = v as i32;
    if v > i as f32 { i + 1 } else { i }
}

#[allow(clippy::too_many_arguments)]
fn do_place_ore(
    ctx: &mut Ctx<'_, '_>,
    c: &OreConfig,
    r: &mut Random,
    d: f64,
    e: f64,
    f: f64,
    g: f64,
    h: f64,
    i: f64,
    j: i32,
    k: i32,
    l: i32,
    m: i32,
    n: i32,
) -> bool {
    let mut count = 0;
    let mut bits = vec![false; (m * n * m) as usize];
    let p = c.size as usize;
    let mut ds = vec![0f64; p * 4];
    for q in 0..p {
        let rr = q as f32 / p as f32;
        let s = mth::lerp(f64::from(rr), d, e);
        let t = mth::lerp(f64::from(rr), h, i);
        let u = mth::lerp(f64::from(rr), f, g);
        let v = r.next_double() * p as f64 / 16.0;
        let w = (f64::from(mth::sin(std::f32::consts::PI * rr) + 1.0) * v + 1.0) / 2.0;
        ds[q * 4] = s;
        ds[q * 4 + 1] = t;
        ds[q * 4 + 2] = u;
        ds[q * 4 + 3] = w;
    }
    for q in 0..p.saturating_sub(1) {
        if ds[q * 4 + 3] <= 0.0 {
            continue;
        }
        for x in q + 1..p {
            if ds[x * 4 + 3] <= 0.0 {
                continue;
            }
            let s = ds[q * 4] - ds[x * 4];
            let t = ds[q * 4 + 1] - ds[x * 4 + 1];
            let u = ds[q * 4 + 2] - ds[x * 4 + 2];
            let v = ds[q * 4 + 3] - ds[x * 4 + 3];
            if !(v * v > s * s + t * t + u * u) {
                continue;
            }
            if v > 0.0 {
                ds[x * 4 + 3] = -1.0;
            } else {
                ds[q * 4 + 3] = -1.0;
            }
        }
    }
    for x in 0..p {
        let s = ds[x * 4 + 3];
        if s < 0.0 {
            continue;
        }
        let t = ds[x * 4];
        let u = ds[x * 4 + 1];
        let v = ds[x * 4 + 2];
        let y0 = mth::floor(t - s).max(j);
        let z0 = mth::floor(u - s).max(k);
        let aa = mth::floor(v - s).max(l);
        let ab = mth::floor(t + s).max(y0);
        let ac = mth::floor(u + s).max(z0);
        let ad = mth::floor(v + s).max(aa);
        for ae in y0..=ab {
            let af = (f64::from(ae) + 0.5 - t) / s;
            if !(af * af < 1.0) {
                continue;
            }
            for ag in z0..=ac {
                let ah = (f64::from(ag) + 0.5 - u) / s;
                if !(af * af + ah * ah < 1.0) {
                    continue;
                }
                'inner: for ai in aa..=ad {
                    let aj = (f64::from(ai) + 0.5 - v) / s;
                    if !(af * af + ah * ah + aj * aj < 1.0)
                        || ctx.region.is_outside_build_height(ag)
                    {
                        continue;
                    }
                    let ak = (ae - j + (ag - k) * m + (ai - l) * m * n) as usize;
                    if bits[ak] {
                        continue;
                    }
                    bits[ak] = true;
                    let mp = bp(ae, ag, ai);
                    if !ctx.region.can_write(mp) {
                        continue;
                    }
                    let bs = ctx.region.get(mp);
                    for (test, st) in &c.targets {
                        if can_place_ore(ctx, bs, r, c, test, mp) {
                            ctx.region.set_raw(mp, *st);
                            count += 1;
                            continue 'inner;
                        }
                    }
                }
            }
        }
    }
    count > 0
}

fn place_scattered_ore(
    ctx: &mut Ctx<'_, '_>,
    c: &OreConfig,
    r: &mut Random,
    pos: BlockPos,
) -> bool {
    let n = r.next_int_bound(c.size + 1);
    let axis = |r: &mut Random, i: i32| -> i32 {
        let v = (r.next_float() - r.next_float()) * i as f32;
        (v + 0.5).floor() as i32
    };
    'outer: for j in 0..n {
        let k = j.min(7);
        let dx = axis(r, k);
        let dy = axis(r, k);
        let dz = axis(r, k);
        let mp = off(pos, dx, dy, dz);
        let bs = ctx.region.get(mp);
        for (test, st) in &c.targets {
            if can_place_ore(ctx, bs, r, c, test, mp) {
                ctx.region.set(mp, *st);
                continue 'outer;
            }
        }
    }
    true
}

// ---- simple block / column / disk -----------------------------------------

fn place_simple_block(
    ctx: &mut Ctx<'_, '_>,
    p: &StateProvider,
    r: &mut Random,
    pos: BlockPos,
) -> bool {
    let s = p.get(r, pos);
    if !can_survive(ctx.region, s, pos) {
        return false;
    }
    if is_double_plant(s) && bh().has_prop(s, "half") {
        if !ctx.region.is_air(off(pos, 0, 1, 0)) {
            return false;
        }
        place_double_plant(ctx, s, pos);
    } else {
        ctx.region.set(pos, s);
    }
    true
}

fn place_block_column(
    ctx: &mut Ctx<'_, '_>,
    layers: &[(IntProvider, StateProvider)],
    dir: Direction,
    allowed: &BlockPredicate,
    prioritize_tip: bool,
    r: &mut Random,
    pos: BlockPos,
) -> bool {
    let n = layers.len();
    let mut is = vec![0i32; n];
    let mut total = 0;
    for (k, (h, _)) in layers.iter().enumerate() {
        is[k] = h.sample(r);
        total += is[k];
    }
    if total == 0 {
        return false;
    }
    let mut mp2 = pos;
    for l in 0..total {
        if !allowed.test(ctx.region, mp2) {
            // truncate
            let mut kk = total - l;
            let order: Vec<usize> = if prioritize_tip {
                (0..n).collect()
            } else {
                (0..n).rev().collect()
            };
            for o in order {
                if kk <= 0 {
                    break;
                }
                let q = is[o].min(kk);
                kk -= q;
                is[o] -= q;
            }
            break;
        }
        mp2 = mp2.offset(dir);
    }
    let mut mp = pos;
    for (l, (_, prov)) in layers.iter().enumerate() {
        for _ in 0..is[l] {
            let s = prov.get(r, mp);
            ctx.region.set(mp, s);
            mp = mp.offset(dir);
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn place_disk(
    ctx: &mut Ctx<'_, '_>,
    fallback: &StateProvider,
    rules: &[(BlockPredicate, StateProvider)],
    target: &BlockPredicate,
    radius: &IntProvider,
    half_height: i32,
    r: &mut Random,
    pos: BlockPos,
) -> bool {
    let mut placed = false;
    let top = pos.y + half_height;
    let bottom = pos.y - half_height - 1;
    let l = radius.sample(r);
    for z in pos.z - l..=pos.z + l {
        for x in pos.x - l..=pos.x + l {
            let m = x - pos.x;
            let n = z - pos.z;
            if m * m + n * n > l * l {
                continue;
            }
            let mut k = top;
            while k > bottom {
                let mp = bp(x, k, z);
                if target.test(ctx.region, mp) {
                    let mut s = None;
                    for (pred, prov) in rules {
                        if pred.test(ctx.region, mp) {
                            s = Some(prov.get(r, mp));
                            break;
                        }
                    }
                    let s = s.unwrap_or_else(|| fallback.get(r, mp));
                    ctx.region.set(mp, s);
                    placed = true;
                }
                k -= 1;
            }
        }
    }
    placed
}

fn place_spring(
    ctx: &mut Ctx<'_, '_>,
    state: BlockStateId,
    requires_below: bool,
    rock_count: i32,
    hole_count: i32,
    valid: &BlockSet,
    pos: BlockPos,
) -> bool {
    let h = bh();
    let is_valid = |ctx: &Ctx<'_, '_>, p: BlockPos| valid.contains(h.block(ctx.region.get(p)));
    if !is_valid(ctx, off(pos, 0, 1, 0)) {
        return false;
    }
    if requires_below && !is_valid(ctx, off(pos, 0, -1, 0)) {
        return false;
    }
    let bs = ctx.region.get(pos);
    if !h.is_air(bs) && !valid.contains(h.block(bs)) {
        return false;
    }
    let sides = [
        off(pos, -1, 0, 0),
        off(pos, 1, 0, 0),
        off(pos, 0, 0, -1),
        off(pos, 0, 0, 1),
        off(pos, 0, -1, 0),
    ];
    let rocks = sides.iter().filter(|p| is_valid(ctx, **p)).count() as i32;
    let holes = sides.iter().filter(|p| ctx.region.is_air(**p)).count() as i32;
    if rocks == rock_count && holes == hole_count {
        ctx.region.set(pos, state);
        return true;
    }
    false
}

fn place_lake(
    ctx: &mut Ctx<'_, '_>,
    fluid: &StateProvider,
    barrier: &StateProvider,
    r: &mut Random,
    origin: BlockPos,
) -> bool {
    let h = bh();
    if origin.y <= ctx.region.min_y + 4 {
        return false;
    }
    let pos = off(origin, 0, -4, 0);
    let mut bls = vec![false; 2048];
    let i = r.next_int_bound(4) + 4;
    for _ in 0..i {
        let d = r.next_double() * 6.0 + 3.0;
        let e = r.next_double() * 4.0 + 2.0;
        let f = r.next_double() * 6.0 + 3.0;
        let g = r.next_double() * (16.0 - d - 2.0) + 1.0 + d / 2.0;
        let hh = r.next_double() * (8.0 - e - 4.0) + 2.0 + e / 2.0;
        let k = r.next_double() * (16.0 - f - 2.0) + 1.0 + f / 2.0;
        for l in 1..15 {
            for m in 1..15 {
                for n in 1..7 {
                    let o = (f64::from(l) - g) / (d / 2.0);
                    let p = (f64::from(n) - hh) / (e / 2.0);
                    let q = (f64::from(m) - k) / (f / 2.0);
                    if o * o + p * p + q * q < 1.0 {
                        bls[((l * 16 + m) * 8 + n) as usize] = true;
                    }
                }
            }
        }
    }
    let at = |s: i32, m: i32, n: i32| bls[((s * 16 + m) * 8 + n) as usize];
    let edge = |s: i32, m: i32, n: i32| {
        !at(s, m, n)
            && ((s < 15 && at(s + 1, m, n))
                || (s > 0 && at(s - 1, m, n))
                || (m < 15 && at(s, m + 1, n))
                || (m > 0 && at(s, m - 1, n))
                || (n < 7 && at(s, m, n + 1))
                || (n > 0 && at(s, m, n - 1)))
    };
    let fs = fluid.get(r, pos);
    for s in 0..16 {
        for m in 0..16 {
            for n in 0..8 {
                if !edge(s, m, n) {
                    continue;
                }
                let bs2 = ctx.region.get(off(pos, s, n, m));
                if n >= 4 && h.is_liquid_block(bs2) {
                    return false;
                }
                if n < 4 && !h.is_solid(bs2) && bs2 != fs {
                    return false;
                }
            }
        }
    }
    let air = BlockStateId::AIR;
    for s in 0..16 {
        for m in 0..16 {
            for n in 0..8 {
                if !at(s, m, n) {
                    continue;
                }
                let p = off(pos, s, n, m);
                if h.features_cannot_replace
                    .contains(h.block(ctx.region.get(p)))
                {
                    continue;
                }
                ctx.region.set(p, if n >= 4 { air } else { fs });
            }
        }
    }
    let bs3 = barrier.get(r, pos);
    if !h.is_air(bs3) {
        let lava_stone = rc_data::tags().block("lava_pool_stone_cannot_replace");
        for m in 0..16 {
            for n in 0..16 {
                for t in 0..8 {
                    if !edge(m, n, t) {
                        continue;
                    }
                    if t >= 4 && r.next_int_bound(2) == 0 {
                        continue;
                    }
                    let p = off(pos, m, t, n);
                    let bs4 = ctx.region.get(p);
                    if !h.is_solid(bs4) || lava_stone.contains(h.block(bs4)) {
                        continue;
                    }
                    ctx.region.set(p, bs3);
                }
            }
        }
    }
    true
}

// ---- snow / ice ------------------------------------------------------------

pub(super) fn should_freeze(ctx: &Ctx<'_, '_>, biome: rc_core::BiomeId, p: BlockPos) -> bool {
    let reg = ctx.region.dg.biomes;
    if reg.temperature_at(biome, p.x, p.y, p.z) >= 0.15 {
        return false;
    }
    if ctx.region.is_outside_build_height(p.y) {
        return false;
    }
    let s = ctx.region.get(p);
    let f = rc_data::blocks().fluid(s);
    f.is_water() && f.is_source() && bh().block(s) == bh().idx.water
}

pub(super) fn should_snow(ctx: &Ctx<'_, '_>, biome: rc_core::BiomeId, p: BlockPos) -> bool {
    let reg = ctx.region.dg.biomes;
    if reg.temperature_at(biome, p.x, p.y, p.z) >= 0.15 {
        return false;
    }
    if ctx.region.is_outside_build_height(p.y) {
        return false;
    }
    let h = bh();
    let s = ctx.region.get(p);
    (h.is_air(s) || h.block(s) == h.idx.snow)
        && can_survive(ctx.region, rc_data::blocks().default_state("snow"), p)
}

fn place_freeze_top_layer(ctx: &mut Ctx<'_, '_>, pos: BlockPos) -> bool {
    let b = rc_data::blocks();
    let ice = b.default_state("ice");
    let snow = b.default_state("snow");
    for i in 0..16 {
        for j in 0..16 {
            let k = pos.x + i;
            let l = pos.z + j;
            let m = ctx.region.height(Heightmap::MotionBlocking, k, l);
            let mp = bp(k, m, l);
            let mp2 = bp(k, m - 1, l);
            let biome = ctx.region.biome(mp);
            if should_freeze(ctx, biome, mp2) {
                ctx.region.set(mp2, ice);
            }
            if !should_snow(ctx, biome, mp) {
                continue;
            }
            ctx.region.set(mp, snow);
            let below = ctx.region.get(mp2);
            if b.prop_pos(below, "snowy").is_some() {
                ctx.region.set(mp2, b.with(below, "snowy", "true"));
            }
        }
    }
    true
}

fn place_forest_rock(
    ctx: &mut Ctx<'_, '_>,
    s: BlockStateId,
    r: &mut Random,
    origin: BlockPos,
) -> bool {
    let h = bh();
    let mut pos = origin;
    while pos.y > ctx.region.min_y + 3 {
        let below = ctx.region.get(off(pos, 0, -1, 0));
        if !h.is_air(below) && (h.is_dirt(below) || is_stone(below)) {
            break;
        }
        pos = off(pos, 0, -1, 0);
    }
    if pos.y <= ctx.region.min_y + 3 {
        return false;
    }
    for _ in 0..3 {
        let j = r.next_int_bound(2);
        let k = r.next_int_bound(2);
        let l = r.next_int_bound(2);
        let f = (j + k + l) as f32 * 0.333 + 0.5;
        for z in -l..=l {
            for y in -k..=k {
                for x in -j..=j {
                    let q = off(pos, x, y, z);
                    let d = f64::from(x * x + y * y + z * z);
                    if d <= f64::from(f * f) {
                        ctx.region.set(q, s);
                    }
                }
            }
        }
        let dx = -1 + r.next_int_bound(2);
        let dy = -r.next_int_bound(2);
        let dz = -1 + r.next_int_bound(2);
        pos = off(pos, dx, dy, dz);
    }
    true
}

fn place_ice_spike(ctx: &mut Ctx<'_, '_>, r: &mut Random, origin: BlockPos) -> bool {
    let h = bh();
    let b = rc_data::blocks();
    let packed = b.default_state("packed_ice");
    let snow_block = b.index_of("snow_block").unwrap();
    let mut pos = origin;
    while ctx.region.is_air(pos) && pos.y > ctx.region.min_y + 2 {
        pos = off(pos, 0, -1, 0);
    }
    if h.block(ctx.region.get(pos)) != snow_block {
        return false;
    }
    pos = off(pos, 0, r.next_int_bound(4), 0);
    let i = r.next_int_bound(4) + 7;
    let j = i / 4 + r.next_int_bound(2);
    if j > 1 && r.next_int_bound(60) == 0 {
        pos = off(pos, 0, 10 + r.next_int_bound(30), 0);
    }
    let ok = |s: BlockStateId| {
        let bi = h.block(s);
        h.is_air(s) || h.is_dirt(s) || bi == snow_block || bi == h.idx.ice
    };
    for k in 0..i {
        let f = (1.0 - k as f32 / i as f32) * j as f32;
        let l = mth_ceil_f(f);
        for m in -l..=l {
            let g = m.abs() as f32 - 0.25;
            for n in -l..=l {
                let hh = n.abs() as f32 - 0.25;
                if ((m != 0 || n != 0) && g * g + hh * hh > f * f)
                    || ((m == -l || m == l || n == -l || n == l) && r.next_float() > 0.75)
                {
                    continue;
                }
                let p1 = off(pos, m, k, n);
                if ok(ctx.region.get(p1)) {
                    ctx.region.set(p1, packed);
                }
                if k != 0 && l > 1 {
                    let p2 = off(pos, m, -k, n);
                    if ok(ctx.region.get(p2)) {
                        ctx.region.set(p2, packed);
                    }
                }
            }
        }
    }
    let k = (j - 1).clamp(0, 1);
    for l in -k..=k {
        for m in -k..=k {
            let mut q = off(pos, l, -1, m);
            let mut o = 50;
            if l.abs() == 1 && m.abs() == 1 {
                o = r.next_int_bound(5);
            }
            while q.y > 50 {
                let s = ctx.region.get(q);
                if !ok(s) && h.block(s) != h.idx.packed_ice {
                    break;
                }
                ctx.region.set(q, packed);
                q = off(q, 0, -1, 0);
                o -= 1;
                if o <= 0 {
                    q = off(q, 0, -(r.next_int_bound(5) + 1), 0);
                    o = r.next_int_bound(5);
                }
            }
        }
    }
    true
}

fn place_blue_ice(ctx: &mut Ctx<'_, '_>, r: &mut Random, pos: BlockPos) -> bool {
    let h = bh();
    let b = rc_data::blocks();
    let blue = b.default_state("blue_ice");
    if pos.y > ctx.region.dg.settings.sea_level - 1 {
        return false;
    }
    if h.block(ctx.region.get(pos)) != h.idx.water
        && h.block(ctx.region.get(off(pos, 0, -1, 0))) != h.idx.water
    {
        return false;
    }
    let mut any = false;
    for d in Direction::ALL {
        if d == Direction::Down {
            continue;
        }
        if h.block(ctx.region.get(pos.offset(d))) == h.idx.packed_ice {
            any = true;
            break;
        }
    }
    if !any {
        return false;
    }
    ctx.region.set(pos, blue);
    for _ in 0..200 {
        let j = r.next_int_bound(5) - r.next_int_bound(6);
        let mut k = 3;
        if j < 2 {
            k += j / 2;
        }
        if k < 1 {
            continue;
        }
        let q = off(
            pos,
            r.next_int_bound(k) - r.next_int_bound(k),
            j,
            r.next_int_bound(k) - r.next_int_bound(k),
        );
        let s = ctx.region.get(q);
        let bi = h.block(s);
        if !h.is_air(s) && bi != h.idx.water && bi != h.idx.packed_ice && bi != h.idx.ice {
            continue;
        }
        for d in Direction::ALL {
            if h.block(ctx.region.get(q.offset(d))) == h.idx.blue_ice {
                ctx.region.set(q, blue);
                break;
            }
        }
    }
    true
}

// ---- huge mushrooms ------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn place_huge_mushroom(
    ctx: &mut Ctx<'_, '_>,
    red: bool,
    cap: &StateProvider,
    stem: &StateProvider,
    radius: i32,
    r: &mut Random,
    pos: BlockPos,
) -> bool {
    let h = bh();
    let b = rc_data::blocks();
    let mut height = r.next_int_bound(3) + 4;
    if r.next_int_bound(12) == 0 {
        height *= 2;
    }
    let radius_for = |l: i32| -> i32 {
        if red {
            let j = -1;
            if (l < j && l >= j - 3) || l == j {
                radius
            } else {
                0
            }
        } else if l <= 3 {
            0
        } else {
            radius
        }
    };
    // isValidPosition
    let j = pos.y;
    if j < ctx.region.min_y + 1 || j + height + 1 >= ctx.region.max_y {
        return false;
    }
    let below = ctx.region.get(off(pos, 0, -1, 0));
    if !h.is_dirt(below) && !h.mushroom_grow_block.contains(h.block(below)) {
        return false;
    }
    for k in 0..=height {
        let l = radius_for(k);
        for m in -l..=l {
            for n in -l..=l {
                let s = ctx.region.get(off(pos, m, k, n));
                if !h.is_air(s) && !h.leaves.contains(h.block(s)) {
                    return false;
                }
            }
        }
    }
    // makeCap
    let set_faces = |s: BlockStateId, up: Option<bool>, w: bool, e: bool, n: bool, so: bool| {
        if b.prop_pos(s, "west").is_none() {
            return s;
        }
        let tf = |v: bool| if v { "true" } else { "false" };
        let mut s = s;
        if let Some(u) = up {
            s = b.with(s, "up", tf(u));
        }
        s = b.with(s, "west", tf(w));
        s = b.with(s, "east", tf(e));
        s = b.with(s, "north", tf(n));
        b.with(s, "south", tf(so))
    };
    if red {
        for jj in height - 3..=height {
            let k = if jj < height { radius } else { radius - 1 };
            let l = radius - 2;
            for m in -k..=k {
                for n in -k..=k {
                    let bl5 = m == -k || m == k;
                    let bl6 = n == -k || n == k;
                    if jj < height && bl5 == bl6 {
                        continue;
                    }
                    let mp = off(pos, m, jj, n);
                    if h.is_solid_render(ctx.region.get(mp)) {
                        continue;
                    }
                    let s = cap.get(r, pos);
                    let s = set_faces(s, Some(jj >= height - 1), m < -l, m > l, n < -l, n > l);
                    ctx.region.set(mp, s);
                }
            }
        }
    } else {
        let jr = radius;
        for k in -jr..=jr {
            for l in -jr..=jr {
                let bl = k == -jr;
                let bl2 = k == jr;
                let bl3 = l == -jr;
                let bl4 = l == jr;
                let bl5 = bl || bl2;
                let bl6 = bl3 || bl4;
                if bl5 && bl6 {
                    continue;
                }
                let mp = off(pos, k, height, l);
                if h.is_solid_render(ctx.region.get(mp)) {
                    continue;
                }
                let bl7 = bl || (bl6 && k == 1 - jr);
                let bl8 = bl2 || (bl6 && k == jr - 1);
                let bl9 = bl3 || (bl5 && l == 1 - jr);
                let bl10 = bl4 || (bl5 && l == jr - 1);
                let s = cap.get(r, pos);
                let s = set_faces(s, None, bl7, bl8, bl9, bl10);
                ctx.region.set(mp, s);
            }
        }
    }
    // placeTrunk
    for jj in 0..height {
        let mp = off(pos, 0, jj, 0);
        if h.is_solid_render(ctx.region.get(mp)) {
            continue;
        }
        let s = stem.get(r, pos);
        ctx.region.set(mp, s);
    }
    true
}

// ---- monster room / desert well ----------------------------------------

fn place_monster_room(ctx: &mut Ctx<'_, '_>, r: &mut Random, pos: BlockPos) -> bool {
    let h = bh();
    let b = rc_data::blocks();
    let chest = b.index_of("chest").unwrap();
    let spawner = b.index_of("spawner").unwrap();
    let j = r.next_int_bound(2) + 2;
    let k = -j - 1;
    let l = j + 1;
    let o = r.next_int_bound(2) + 2;
    let p = -o - 1;
    let q = o + 1;
    let mut openings = 0;
    for s in k..=l {
        for t in -1..=4 {
            for u in p..=q {
                let bpos = off(pos, s, t, u);
                let solid = h.is_solid(ctx.region.get(bpos));
                if t == -1 && !solid {
                    return false;
                }
                if t == 4 && !solid {
                    return false;
                }
                if (s == k || s == l || u == p || u == q)
                    && t == 0
                    && ctx.region.is_air(bpos)
                    && ctx.region.is_air(off(bpos, 0, 1, 0))
                {
                    openings += 1;
                }
            }
        }
    }
    if !(1..=5).contains(&openings) {
        return false;
    }
    let cannot = |ctx: &Ctx<'_, '_>, q: BlockPos| {
        h.features_cannot_replace
            .contains(h.block(ctx.region.get(q)))
    };
    let safe_set = |ctx: &mut Ctx<'_, '_>, q: BlockPos, s: BlockStateId| {
        if !cannot(ctx, q) {
            ctx.region.set(q, s);
        }
    };
    let cave_air = b.default_state("cave_air");
    let mossy = b.default_state("mossy_cobblestone");
    let cobble = b.default_state("cobblestone");
    for s in k..=l {
        for t in (-1..=3).rev() {
            for u in p..=q {
                let bpos = off(pos, s, t, u);
                let bs = ctx.region.get(bpos);
                if s == k || t == -1 || u == p || s == l || t == 4 || u == q {
                    if bpos.y >= ctx.region.min_y
                        && !h.is_solid(ctx.region.get(off(bpos, 0, -1, 0)))
                    {
                        ctx.region.set(bpos, cave_air);
                        continue;
                    }
                    if !h.is_solid(bs) || h.block(bs) == chest {
                        continue;
                    }
                    if t == -1 && r.next_int_bound(4) != 0 {
                        safe_set(ctx, bpos, mossy);
                    } else {
                        safe_set(ctx, bpos, cobble);
                    }
                    continue;
                }
                if h.block(bs) == chest || h.block(bs) == spawner {
                    continue;
                }
                safe_set(ctx, bpos, cave_air);
            }
        }
    }
    'chests: for _ in 0..2 {
        for _ in 0..3 {
            let u = pos.x + r.next_int_bound(j * 2 + 1) - j;
            let v = pos.z + r.next_int_bound(o * 2 + 1) - o;
            let cp = bp(u, pos.y, v);
            if !ctx.region.is_air(cp) {
                continue;
            }
            let mut x = 0;
            for d in [
                Direction::North,
                Direction::East,
                Direction::South,
                Direction::West,
            ] {
                if h.is_solid(ctx.region.get(cp.offset(d))) {
                    x += 1;
                }
            }
            if x != 1 {
                continue;
            }
            let facing = chest_facing(ctx, cp);
            let cs = b.with(b.default_state("chest"), "facing", facing);
            safe_set(ctx, cp, cs);
            let _loot_seed = r.next_long();
            continue 'chests;
        }
    }
    safe_set(ctx, pos, b.default_state("spawner"));
    let _mob = r.next_int_bound(4);
    true
}

/// `StructurePiece.reorient` for a chest (default facing north).
fn chest_facing(ctx: &Ctx<'_, '_>, p: BlockPos) -> &'static str {
    let h = bh();
    let chest = rc_data::blocks().index_of("chest").unwrap();
    let hz = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];
    let solid = |q: BlockPos| h.is_solid_render(ctx.region.get(q));
    let mut dir: Option<Direction> = None;
    for d in hz {
        let q = p.offset(d);
        let s = ctx.region.get(q);
        if h.block(s) == chest {
            return "north";
        }
        if !solid(q) {
            continue;
        }
        if dir.is_none() {
            dir = Some(d);
        } else {
            dir = None;
            break;
        }
    }
    let d = if let Some(d) = dir {
        d.opposite()
    } else {
        let cw = |d: Direction| match d {
            Direction::North => Direction::East,
            Direction::East => Direction::South,
            Direction::South => Direction::West,
            _ => Direction::North,
        };
        let mut d = Direction::North;
        if solid(p.offset(d)) {
            d = d.opposite();
        }
        if solid(p.offset(d)) {
            d = cw(d);
        }
        if solid(p.offset(d)) {
            d = d.opposite();
        }
        d
    };
    match d {
        Direction::North => "north",
        Direction::South => "south",
        Direction::East => "east",
        _ => "west",
    }
}

fn place_desert_well(ctx: &mut Ctx<'_, '_>, r: &mut Random, origin: BlockPos) -> bool {
    let b = rc_data::blocks();
    let sand = b.default_state("sand");
    let sandstone = b.default_state("sandstone");
    let slab = b.default_state("sandstone_slab");
    let water = b.default_state("water");
    let mut pos = off(origin, 0, 1, 0);
    while ctx.region.is_air(pos) && pos.y > ctx.region.min_y + 2 {
        pos = off(pos, 0, -1, 0);
    }
    if ctx.region.get(pos) != sand {
        return false;
    }
    for i in -2..=2 {
        for j in -2..=2 {
            if ctx.region.is_air(off(pos, i, -1, j)) && ctx.region.is_air(off(pos, i, -2, j)) {
                return false;
            }
        }
    }
    for i in -2..=0 {
        for j in -2..=2 {
            for k in -2..=2 {
                ctx.region.set(off(pos, j, i, k), sandstone);
            }
        }
    }
    ctx.region.set(pos, water);
    let hz = [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ];
    for d in hz {
        ctx.region.set(pos.offset(d), water);
    }
    let below = off(pos, 0, -1, 0);
    ctx.region.set(below, sand);
    for d in hz {
        ctx.region.set(below.offset(d), sand);
    }
    for j in -2..=2 {
        for k in -2..=2 {
            if j == -2 || j == 2 || k == -2 || k == 2 {
                ctx.region.set(off(pos, j, 1, k), sandstone);
            }
        }
    }
    ctx.region.set(off(pos, 2, 1, 0), slab);
    ctx.region.set(off(pos, -2, 1, 0), slab);
    ctx.region.set(off(pos, 0, 1, 2), slab);
    ctx.region.set(off(pos, 0, 1, -2), slab);
    for j in -1..=1 {
        for k in -1..=1 {
            ctx.region.set(
                off(pos, j, 4, k),
                if j == 0 && k == 0 { sandstone } else { slab },
            );
        }
    }
    for j in 1..=3 {
        ctx.region.set(off(pos, -1, j, -1), sandstone);
        ctx.region.set(off(pos, -1, j, 1), sandstone);
        ctx.region.set(off(pos, 1, j, -1), sandstone);
        ctx.region.set(off(pos, 1, j, 1), sandstone);
    }
    let list = [
        pos,
        off(pos, 1, 0, 0),
        off(pos, 0, 0, 1),
        off(pos, -1, 0, 0),
        off(pos, 0, 0, -1),
    ];
    let sus = b.default_state("suspicious_sand");
    let a = list[r.next_int_bound(5) as usize];
    ctx.region.set(off(a, 0, -1, 0), sus);
    let c = list[r.next_int_bound(5) as usize];
    ctx.region.set(off(c, 0, -2, 0), sus);
    true
}
