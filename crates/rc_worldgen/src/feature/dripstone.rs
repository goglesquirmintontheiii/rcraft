//! Dripstone cluster and large dripstone features.

use rc_core::{BlockPos, BlockStateId, Direction};
use serde_json::Value;

use super::Ctx;
use super::basic::{bp, off};
use super::behaviour::bh;
use super::cave::{
    grow_pointed_dripstone, is_dripstone_base, is_empty_or_water, place_dripstone_block_if_possible,
};
use crate::mth;
use crate::proto::Heightmap;
use crate::providers::{FloatProvider, IntProvider};
use crate::rng::Random;

const HORIZONTAL: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

/// `Column`: floor/ceiling y of the bounding blocks, if found.
#[derive(Clone, Copy, Debug)]
struct Column {
    floor: Option<i32>,
    ceiling: Option<i32>,
}

impl Column {
    fn height(&self) -> Option<i32> {
        match (self.floor, self.ceiling) {
            (Some(f), Some(c)) => Some(c - f - 1),
            _ => None,
        }
    }
}

fn scan(
    ctx: &Ctx<'_, '_>,
    pos: BlockPos,
    range: i32,
    inside: impl Fn(BlockStateId) -> bool,
    edge: impl Fn(BlockStateId) -> bool,
) -> Option<Column> {
    if !inside(ctx.region.get(pos)) {
        return None;
    }
    let dir = |dy: i32| -> Option<i32> {
        let mut y = pos.y;
        let mut k = 1;
        while k < range && inside(ctx.region.get_xyz(pos.x, y, pos.z)) {
            y += dy;
            k += 1;
        }
        if edge(ctx.region.get_xyz(pos.x, y, pos.z)) {
            Some(y)
        } else {
            None
        }
    };
    let ceiling = dir(1);
    let floor = dir(-1);
    Some(Column { floor, ceiling })
}

fn neither_empty_nor_water(s: BlockStateId) -> bool {
    let h = bh();
    !h.is_air(s) && h.block(s) != h.idx.water
}

fn empty_water_or_lava(s: BlockStateId) -> bool {
    let h = bh();
    let bi = h.block(s);
    h.is_air(s) || bi == h.idx.water || bi == h.idx.lava
}

fn is_lava(ctx: &Ctx<'_, '_>, p: BlockPos) -> bool {
    bh().block(ctx.region.get(p)) == bh().idx.lava
}

#[derive(Clone, Debug)]
pub struct Cluster {
    floor_to_ceiling_search_range: i32,
    height: IntProvider,
    radius: IntProvider,
    max_stalagmite_stalactite_height_diff: i32,
    height_deviation: i32,
    dripstone_block_layer_thickness: IntProvider,
    density: FloatProvider,
    wetness: FloatProvider,
    chance_of_dripstone_column_at_max_distance_from_center: f32,
    max_distance_from_edge_affecting_chance_of_dripstone_column: i32,
    max_distance_from_center_affecting_height_bias: i32,
}

impl Cluster {
    pub fn from_json(c: &Value) -> Self {
        let i = |k: &str| c[k].as_i64().unwrap() as i32;
        Self {
            floor_to_ceiling_search_range: i("floor_to_ceiling_search_range"),
            height: IntProvider::from_json(&c["height"]),
            radius: IntProvider::from_json(&c["radius"]),
            max_stalagmite_stalactite_height_diff: i("max_stalagmite_stalactite_height_diff"),
            height_deviation: i("height_deviation"),
            dripstone_block_layer_thickness: IntProvider::from_json(
                &c["dripstone_block_layer_thickness"],
            ),
            density: FloatProvider::from_json(&c["density"]),
            wetness: FloatProvider::from_json(&c["wetness"]),
            chance_of_dripstone_column_at_max_distance_from_center:
                c["chance_of_dripstone_column_at_max_distance_from_center"]
                    .as_f64()
                    .unwrap() as f32,
            max_distance_from_edge_affecting_chance_of_dripstone_column: i(
                "max_distance_from_edge_affecting_chance_of_dripstone_column",
            ),
            max_distance_from_center_affecting_height_bias: i(
                "max_distance_from_center_affecting_height_bias",
            ),
        }
    }
}

fn clamped_map_f(v: f32, a: f32, b: f32, c: f32, d: f32) -> f32 {
    let t = (v - a) / (b - a);
    if t < 0.0 {
        c
    } else if t > 1.0 {
        d
    } else {
        c + t * (d - c)
    }
}

fn dripstone_height(r: &mut Random, i: i32, j: i32, f: f32, k: i32, c: &Cluster) -> i32 {
    if r.next_float() > f {
        return 0;
    }
    let l = i.abs() + j.abs();
    let g = mth::clamped_map(
        f64::from(l),
        0.0,
        f64::from(c.max_distance_from_center_affecting_height_bias),
        f64::from(k) / 2.0,
        0.0,
    ) as f32;
    // randomBetweenBiased → ClampedNormalFloat.sample(random, mean g, dev, 0, k)
    let v = g + r.next_gaussian() as f32 * c.height_deviation as f32;
    v.clamp(0.0, k as f32) as i32
}

fn can_be_adjacent_to_water(ctx: &Ctx<'_, '_>, p: BlockPos) -> bool {
    let h = bh();
    let s = ctx.region.get(p);
    h.base_stone_overworld.contains(h.block(s)) || rc_data::blocks().fluid(s).is_water()
}

fn can_place_pool(ctx: &Ctx<'_, '_>, p: BlockPos) -> bool {
    let h = bh();
    let s = ctx.region.get(p);
    let n = h.name(s);
    if h.block(s) == h.idx.water || n == "dripstone_block" || n == "pointed_dripstone" {
        return false;
    }
    if ctx.region.fluid(off(p, 0, 1, 0)).is_water() {
        return false;
    }
    for d in HORIZONTAL {
        if !can_be_adjacent_to_water(ctx, p.offset(d)) {
            return false;
        }
    }
    can_be_adjacent_to_water(ctx, off(p, 0, -1, 0))
}

fn replace_with_dripstone(ctx: &mut Ctx<'_, '_>, p: BlockPos, n: i32, d: Direction) {
    let mut mp = p;
    for _ in 0..n {
        if !place_dripstone_block_if_possible(ctx, mp) {
            return;
        }
        mp = mp.offset(d);
    }
}

pub fn cluster(ctx: &mut Ctx<'_, '_>, c: &Cluster, r: &mut Random, pos: BlockPos) -> bool {
    if !is_empty_or_water(ctx.region.get(pos)) {
        return false;
    }
    let i = c.height.sample(r);
    let f = c.wetness.sample(r);
    let g = c.density.sample(r);
    let j = c.radius.sample(r);
    let k = c.radius.sample(r);
    for l in -j..=j {
        for m in -k..=k {
            let mm = (j - l.abs()).min(k - m.abs());
            let d = f64::from(clamped_map_f(
                mm as f32,
                0.0,
                c.max_distance_from_edge_affecting_chance_of_dripstone_column as f32,
                c.chance_of_dripstone_column_at_max_distance_from_center,
                1.0,
            ));
            let p = off(pos, l, 0, m);
            cluster_column(ctx, c, r, p, l, m, f, d, i, g);
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn cluster_column(
    ctx: &mut Ctx<'_, '_>,
    c: &Cluster,
    r: &mut Random,
    pos: BlockPos,
    i: i32,
    j: i32,
    f: f32,
    d: f64,
    k: i32,
    g: f32,
) {
    let Some(col) = scan(
        ctx,
        pos,
        c.floor_to_ceiling_search_range,
        is_empty_or_water,
        neither_empty_nor_water,
    ) else {
        return;
    };
    let ceiling = col.ceiling;
    let floor = col.floor;
    if ceiling.is_none() && floor.is_none() {
        return;
    }
    let wet = r.next_float() < f;
    let column = if wet && floor.is_some_and(|fl| can_place_pool(ctx, bp(pos.x, fl, pos.z))) {
        let l = floor.unwrap();
        ctx.region.set(
            bp(pos.x, l, pos.z),
            rc_data::blocks().default_state("water"),
        );
        Column {
            floor: Some(l - 1),
            ceiling,
        }
    } else {
        col
    };
    let floor3 = column.floor;
    let bl3 = r.next_double() < d;
    let n = if let Some(ce) = ceiling
        && bl3
        && !is_lava(ctx, bp(pos.x, ce, pos.z))
    {
        let m = c.dripstone_block_layer_thickness.sample(r);
        replace_with_dripstone(ctx, bp(pos.x, ce, pos.z), m, Direction::Up);
        let n2 = floor3.map_or(k, |fl| k.min(ce - fl));
        dripstone_height(r, i, j, g, n2, c)
    } else {
        0
    };
    let bl4 = r.next_double() < d;
    let m = if let Some(fl) = floor3
        && bl4
        && !is_lava(ctx, bp(pos.x, fl, pos.z))
    {
        let rr = c.dripstone_block_layer_thickness.sample(r);
        replace_with_dripstone(ctx, bp(pos.x, fl, pos.z), rr, Direction::Down);
        if ceiling.is_some() {
            let dd = c.max_stalagmite_stalactite_height_diff;
            (n + r.next_int_between(-dd, dd)).max(0)
        } else {
            dripstone_height(r, i, j, g, k, c)
        }
    } else {
        0
    };
    let (u, v) = match (ceiling, floor3) {
        (Some(p), Some(o)) if p - n <= o + m => {
            let q = (p - n).max(o + 1);
            let rr = (o + m).min(p - 1);
            let s = r.next_int_between(q, rr + 1);
            let t = s - 1;
            (p - s, t - o)
        }
        _ => (n, m),
    };
    let merge = r.next_bool() && u > 0 && v > 0 && column.height().is_some_and(|h| u + v == h);
    if let Some(ce) = ceiling {
        grow_pointed_dripstone(ctx, bp(pos.x, ce - 1, pos.z), Direction::Down, u, merge);
    }
    if let Some(fl) = floor3 {
        grow_pointed_dripstone(ctx, bp(pos.x, fl + 1, pos.z), Direction::Up, v, merge);
    }
}

// ---- large dripstone -------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Large {
    floor_to_ceiling_search_range: i32,
    column_radius: IntProvider,
    height_scale: FloatProvider,
    max_column_radius_to_cave_height_ratio: f32,
    stalactite_bluntness: FloatProvider,
    stalagmite_bluntness: FloatProvider,
    wind_speed: FloatProvider,
    min_radius_for_wind: i32,
    min_bluntness_for_wind: f32,
}

impl Large {
    pub fn from_json(c: &Value) -> Self {
        Self {
            floor_to_ceiling_search_range: c["floor_to_ceiling_search_range"].as_i64().unwrap()
                as i32,
            column_radius: IntProvider::from_json(&c["column_radius"]),
            height_scale: FloatProvider::from_json(&c["height_scale"]),
            max_column_radius_to_cave_height_ratio: c["max_column_radius_to_cave_height_ratio"]
                .as_f64()
                .unwrap() as f32,
            stalactite_bluntness: FloatProvider::from_json(&c["stalactite_bluntness"]),
            stalagmite_bluntness: FloatProvider::from_json(&c["stalagmite_bluntness"]),
            wind_speed: FloatProvider::from_json(&c["wind_speed"]),
            min_radius_for_wind: c["min_radius_for_wind"].as_i64().unwrap() as i32,
            min_bluntness_for_wind: c["min_bluntness_for_wind"].as_f64().unwrap() as f32,
        }
    }
}

struct Wind {
    origin_y: i32,
    speed: Option<(f64, f64)>,
}

impl Wind {
    fn offset(&self, p: BlockPos) -> BlockPos {
        let Some((sx, sz)) = self.speed else { return p };
        let i = f64::from(self.origin_y - p.y);
        bp(p.x + mth::floor(sx * i), p.y, p.z + mth::floor(sz * i))
    }
}

fn dripstone_height_at(d: f64, e: f64, f: f64, g: f64) -> f64 {
    let d = if d < g { g } else { d };
    let i = d / e * 0.384;
    let j = 0.75 * i.powf(1.333_333_333_333_333_3);
    let k = i.powf(0.666_666_666_666_666_6);
    let l = 0.333_333_333_333_333_3 * i.ln();
    let m = (f * (j - k - l)).max(0.0);
    m / 0.384 * e
}

struct LargeDs {
    root: BlockPos,
    up: bool,
    radius: i32,
    bluntness: f64,
    scale: f64,
}

impl LargeDs {
    fn height_at(&self, f: f32) -> i32 {
        dripstone_height_at(
            f64::from(f),
            f64::from(self.radius),
            self.scale,
            self.bluntness,
        ) as i32
    }

    fn suitable_for_wind(&self, c: &Large) -> bool {
        self.radius >= c.min_radius_for_wind
            && self.bluntness >= f64::from(c.min_bluntness_for_wind)
    }

    fn settle(&mut self, ctx: &Ctx<'_, '_>, w: &Wind) -> bool {
        while self.radius > 1 {
            let mut mp = self.root;
            let i = 10.min(self.height_at(0.0));
            for _ in 0..i {
                if is_lava(ctx, mp) {
                    return false;
                }
                if circle_embedded(ctx, w.offset(mp), self.radius) {
                    self.root = mp;
                    return true;
                }
                mp = off(mp, 0, if self.up { -1 } else { 1 }, 0);
            }
            self.radius /= 2;
        }
        false
    }

    fn place(&self, ctx: &mut Ctx<'_, '_>, r: &mut Random, w: &Wind) {
        let h = bh();
        let dripstone = rc_data::blocks().default_state("dripstone_block");
        for i in -self.radius..=self.radius {
            'col: for j in -self.radius..=self.radius {
                let f = mth::sqrt_f((i * i + j * j) as f32);
                if f > self.radius as f32 {
                    continue;
                }
                let mut k = self.height_at(f);
                if k <= 0 {
                    continue;
                }
                if f64::from(r.next_float()) < 0.2 {
                    k = (k as f32 * (r.next_float() * (1.0 - 0.8) + 0.8)) as i32;
                }
                let mut mp = off(self.root, i, 0, j);
                let mut bl = false;
                let l = if self.up {
                    ctx.region.height(Heightmap::WorldSurfaceWg, mp.x, mp.z)
                } else {
                    i32::MAX
                };
                let mut m = 0;
                while m < k && mp.y < l {
                    let q = w.offset(mp);
                    if empty_water_or_lava(ctx.region.get(q)) {
                        bl = true;
                        ctx.region.set(q, dripstone);
                    } else if bl && h.base_stone_overworld.contains(h.block(ctx.region.get(q))) {
                        continue 'col;
                    }
                    mp = off(mp, 0, if self.up { 1 } else { -1 }, 0);
                    m += 1;
                }
            }
        }
    }
}

fn circle_embedded(ctx: &Ctx<'_, '_>, p: BlockPos, i: i32) -> bool {
    if empty_water_or_lava(ctx.region.get(p)) {
        return false;
    }
    let g = 6.0f32 / i as f32;
    let mut hh = 0.0f32;
    while hh < std::f32::consts::PI * 2.0 {
        let j = (mth::cos(hh) * i as f32) as i32;
        let k = (mth::sin(hh) * i as f32) as i32;
        if empty_water_or_lava(ctx.region.get(off(p, j, 0, k))) {
            return false;
        }
        hh += g;
    }
    true
}

pub fn large(ctx: &mut Ctx<'_, '_>, c: &Large, r: &mut Random, pos: BlockPos) -> bool {
    if !is_empty_or_water(ctx.region.get(pos)) {
        return false;
    }
    let edge = |s: BlockStateId| is_dripstone_base(s) || bh().block(s) == bh().idx.lava;
    let Some(col) = scan(
        ctx,
        pos,
        c.floor_to_ceiling_search_range,
        is_empty_or_water,
        edge,
    ) else {
        return false;
    };
    let (Some(floor), Some(ceiling)) = (col.floor, col.ceiling) else {
        return false;
    };
    let height = ceiling - floor - 1;
    if height < 4 {
        return false;
    }
    let i = (height as f32 * c.max_column_radius_to_cave_height_ratio) as i32;
    let j = i.clamp(c.column_radius.min_value(), c.column_radius.max_value());
    let k = r.next_int_between(c.column_radius.min_value(), j);
    let b1 = f64::from(c.stalactite_bluntness.sample(r));
    let s1 = f64::from(c.height_scale.sample(r));
    let mut ld = LargeDs {
        root: bp(pos.x, ceiling - 1, pos.z),
        up: false,
        radius: k,
        bluntness: b1,
        scale: s1,
    };
    let b2 = f64::from(c.stalagmite_bluntness.sample(r));
    let s2 = f64::from(c.height_scale.sample(r));
    let mut ld2 = LargeDs {
        root: bp(pos.x, floor + 1, pos.z),
        up: true,
        radius: k,
        bluntness: b2,
        scale: s2,
    };
    let wind = if ld.suitable_for_wind(c) && ld2.suitable_for_wind(c) {
        let f = c.wind_speed.sample(r);
        let g = r.next_float() * std::f32::consts::PI;
        Wind {
            origin_y: pos.y,
            speed: Some((f64::from(mth::cos(g) * f), f64::from(mth::sin(g) * f))),
        }
    } else {
        Wind {
            origin_y: 0,
            speed: None,
        }
    };
    let bl = ld.settle(ctx, &wind);
    let bl2 = ld2.settle(ctx, &wind);
    if bl {
        ld.place(ctx, r, &wind);
    }
    if bl2 {
        ld2.place(ctx, r, &wind);
    }
    true
}
