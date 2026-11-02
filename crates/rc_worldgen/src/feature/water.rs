//! Aquatic features: seagrass, kelp, sea pickles, corals, underwater magma.

use rc_core::{BlockPos, BlockStateId, Direction};
use serde_json::Value;

use super::basic::{bp, off};
use super::behaviour::{bh, can_survive};
use super::tree::shuffle;
use super::{Ctx, Registry};
use crate::proto::Heightmap;
use crate::providers::IntProvider;
use crate::rng::Random;

const HORIZONTAL: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

#[derive(Clone, Copy, Debug)]
pub enum Coral {
    Tree,
    Claw,
    Mushroom,
}

#[derive(Clone, Debug)]
pub enum WaterKind {
    Seagrass(f32),
    Kelp,
    SeaPickle(IntProvider),
    Coral(Coral),
    UnderwaterMagma {
        floor_search_range: i32,
        radius: i32,
        probability: f32,
    },
    Iceberg(BlockStateId),
}

impl WaterKind {
    pub fn from_json(_reg: &mut Registry, t: &str, c: &Value) -> Option<Self> {
        Some(match t {
            "seagrass" => Self::Seagrass(c["probability"].as_f64().unwrap() as f32),
            "kelp" => Self::Kelp,
            "sea_pickle" => Self::SeaPickle(IntProvider::from_json(&c["count"])),
            "coral_tree" => Self::Coral(Coral::Tree),
            "coral_claw" => Self::Coral(Coral::Claw),
            "coral_mushroom" => Self::Coral(Coral::Mushroom),
            "underwater_magma" => Self::UnderwaterMagma {
                floor_search_range: c["floor_search_range"].as_i64().unwrap() as i32,
                radius: c["placement_radius_around_floor"].as_i64().unwrap() as i32,
                probability: c["placement_probability_per_valid_position"]
                    .as_f64()
                    .unwrap() as f32,
            },
            "iceberg" => Self::Iceberg(super::predicate::state(&c["state"])),
            _ => return None,
        })
    }

    pub fn place(&self, ctx: &mut Ctx<'_, '_>, r: &mut Random, pos: BlockPos) -> bool {
        match self {
            Self::Seagrass(p) => seagrass(ctx, *p, r, pos),
            Self::Kelp => kelp(ctx, r, pos),
            Self::SeaPickle(c) => sea_pickle(ctx, c, r, pos),
            Self::Coral(k) => coral(ctx, *k, r, pos),
            Self::UnderwaterMagma {
                floor_search_range,
                radius,
                probability,
            } => underwater_magma(ctx, *floor_search_range, *radius, *probability, r, pos),
            Self::Iceberg(_) => false,
        }
    }
}

fn is_water_block(ctx: &Ctx<'_, '_>, p: BlockPos) -> bool {
    bh().block(ctx.region.get(p)) == bh().idx.water
}

fn seagrass(ctx: &mut Ctx<'_, '_>, prob: f32, r: &mut Random, pos: BlockPos) -> bool {
    let b = rc_data::blocks();
    let i = r.next_int_bound(8) - r.next_int_bound(8);
    let j = r.next_int_bound(8) - r.next_int_bound(8);
    let k = ctx
        .region
        .height(Heightmap::OceanFloor, pos.x + i, pos.z + j);
    let p = bp(pos.x + i, k, pos.z + j);
    if !is_water_block(ctx, p) {
        return false;
    }
    let tall = r.next_double() < f64::from(prob);
    let s = if tall {
        b.default_state("tall_seagrass")
    } else {
        b.default_state("seagrass")
    };
    if !can_survive(ctx.region, s, p) {
        return false;
    }
    if tall {
        let upper = b.with(s, "half", "upper");
        let above = off(p, 0, 1, 0);
        if is_water_block(ctx, above) {
            ctx.region.set(p, s);
            ctx.region.set(above, upper);
        }
    } else {
        ctx.region.set(p, s);
    }
    true
}

fn kelp(ctx: &mut Ctx<'_, '_>, r: &mut Random, pos: BlockPos) -> bool {
    let b = rc_data::blocks();
    let mut n = 0;
    let j = ctx.region.height(Heightmap::OceanFloor, pos.x, pos.z);
    let mut p = bp(pos.x, j, pos.z);
    if !is_water_block(ctx, p) {
        return false;
    }
    let head = b.default_state("kelp");
    let plant = b.default_state("kelp_plant");
    let k = 1 + r.next_int_bound(10);
    for l in 0..=k {
        if is_water_block(ctx, p)
            && is_water_block(ctx, off(p, 0, 1, 0))
            && can_survive(ctx.region, plant, p)
        {
            if l == k {
                let age = r.next_int_bound(4) + 20;
                ctx.region.set(p, b.with(head, "age", &age.to_string()));
                n += 1;
            } else {
                ctx.region.set(p, plant);
            }
        } else if l > 0 {
            let below = off(p, 0, -1, 0);
            if !can_survive(ctx.region, head, below)
                || bh().block(ctx.region.get(off(below, 0, -1, 0))) == bh().idx.kelp
            {
                break;
            }
            let age = r.next_int_bound(4) + 20;
            ctx.region.set(below, b.with(head, "age", &age.to_string()));
            n += 1;
            break;
        }
        p = off(p, 0, 1, 0);
    }
    n > 0
}

fn sea_pickle(ctx: &mut Ctx<'_, '_>, count: &IntProvider, r: &mut Random, pos: BlockPos) -> bool {
    let b = rc_data::blocks();
    let mut n = 0;
    let j = count.sample(r);
    for _ in 0..j {
        let l = r.next_int_bound(8) - r.next_int_bound(8);
        let m = r.next_int_bound(8) - r.next_int_bound(8);
        let y = ctx
            .region
            .height(Heightmap::OceanFloor, pos.x + l, pos.z + m);
        let p = bp(pos.x + l, y, pos.z + m);
        let pickles = r.next_int_bound(4) + 1;
        let s = b.with(
            b.default_state("sea_pickle"),
            "pickles",
            &pickles.to_string(),
        );
        if !is_water_block(ctx, p) || !can_survive(ctx.region, s, p) {
            continue;
        }
        ctx.region.set(p, s);
        n += 1;
    }
    n > 0
}

fn random_of_tag(tag: &str, r: &mut Random) -> Option<BlockStateId> {
    let list = rc_data::tags().block_list(tag);
    if list.is_empty() {
        return None;
    }
    let b = rc_data::blocks();
    Some(b.defs[list[r.next_int_bound(list.len() as i32) as usize] as usize].default)
}

fn place_coral_block(
    ctx: &mut Ctx<'_, '_>,
    r: &mut Random,
    p: BlockPos,
    state: BlockStateId,
) -> bool {
    let h = bh();
    let b = rc_data::blocks();
    let above = off(p, 0, 1, 0);
    let s = ctx.region.get(p);
    if (!is_water_block(ctx, p) && !h.corals.contains(h.block(s))) || !is_water_block(ctx, above) {
        return false;
    }
    ctx.region.set(p, state);
    if r.next_float() < 0.25 {
        if let Some(c) = random_of_tag("corals", r) {
            ctx.region.set(above, c);
        }
    } else if r.next_float() < 0.05 {
        let pickles = r.next_int_bound(4) + 1;
        ctx.region.set(
            above,
            b.with(
                b.default_state("sea_pickle"),
                "pickles",
                &pickles.to_string(),
            ),
        );
    }
    for d in HORIZONTAL {
        if r.next_float() < 0.2 {
            let q = p.offset(d);
            if !is_water_block(ctx, q) {
                continue;
            }
            if let Some(mut w) = random_of_tag("wall_corals", r) {
                if b.prop_pos(w, "facing").is_some() {
                    w = b.with(w, "facing", dir_name(d));
                }
                ctx.region.set(q, w);
            }
        }
    }
    true
}

fn dir_name(d: Direction) -> &'static str {
    match d {
        Direction::North => "north",
        Direction::South => "south",
        Direction::East => "east",
        Direction::West => "west",
        Direction::Up => "up",
        Direction::Down => "down",
    }
}

fn cw(d: Direction) -> Direction {
    match d {
        Direction::North => Direction::East,
        Direction::East => Direction::South,
        Direction::South => Direction::West,
        Direction::West => Direction::North,
        o => o,
    }
}

fn ccw(d: Direction) -> Direction {
    match d {
        Direction::North => Direction::West,
        Direction::West => Direction::South,
        Direction::South => Direction::East,
        Direction::East => Direction::North,
        o => o,
    }
}

fn coral(ctx: &mut Ctx<'_, '_>, kind: Coral, r: &mut Random, pos: BlockPos) -> bool {
    let Some(state) = random_of_tag("coral_blocks", r) else {
        return false;
    };
    match kind {
        Coral::Tree => {
            let mut mp = pos;
            let i = r.next_int_bound(3) + 1;
            for _ in 0..i {
                if !place_coral_block(ctx, r, mp, state) {
                    return true;
                }
                mp = off(mp, 0, 1, 0);
            }
            let base = mp;
            let k = r.next_int_bound(3) + 2;
            let mut list = HORIZONTAL.to_vec();
            shuffle(&mut list, r);
            for &d in &list[..k as usize] {
                mp = base.offset(d);
                let l = r.next_int_bound(5) + 2;
                let mut m = 0;
                let mut n = 0;
                while n < l && place_coral_block(ctx, r, mp, state) {
                    m += 1;
                    mp = off(mp, 0, 1, 0);
                    if n == 0 || (m >= 2 && r.next_float() < 0.25) {
                        mp = mp.offset(d);
                        m = 0;
                    }
                    n += 1;
                }
            }
            true
        }
        Coral::Claw => {
            if !place_coral_block(ctx, r, pos, state) {
                return false;
            }
            let d = HORIZONTAL[r.next_int_bound(4) as usize];
            let i = r.next_int_bound(2) + 2;
            let mut list = vec![d, cw(d), ccw(d)];
            shuffle(&mut list, r);
            'outer: for &d2 in &list[..i as usize] {
                let mut mp = pos;
                let j = r.next_int_bound(2) + 1;
                mp = mp.offset(d2);
                let (d3, k) = if d2 == d {
                    (d, r.next_int_bound(3) + 2)
                } else {
                    mp = off(mp, 0, 1, 0);
                    let ds = [d2, Direction::Up];
                    let d3 = ds[r.next_int_bound(2) as usize];
                    (d3, r.next_int_bound(3) + 3)
                };
                let mut l = 0;
                while l < j && place_coral_block(ctx, r, mp, state) {
                    mp = mp.offset(d3);
                    l += 1;
                }
                mp = mp.offset(d3.opposite());
                mp = off(mp, 0, 1, 0);
                for _ in 0..k {
                    mp = mp.offset(d);
                    if !place_coral_block(ctx, r, mp, state) {
                        continue 'outer;
                    }
                    if r.next_float() < 0.25 {
                        mp = off(mp, 0, 1, 0);
                    }
                }
            }
            true
        }
        Coral::Mushroom => {
            let i = r.next_int_bound(3) + 3;
            let j = r.next_int_bound(3) + 3;
            let k = r.next_int_bound(3) + 3;
            let l = r.next_int_bound(3) + 1;
            for m in 0..=j {
                for n in 0..=i {
                    for o in 0..=k {
                        let mp = bp(m + pos.x, n + pos.y - l, o + pos.z);
                        let c1 = (m != 0 && m != j) || (n != 0 && n != i);
                        let c2 = (o != 0 && o != k) || (n != 0 && n != i);
                        let c3 = (m != 0 && m != j) || (o != 0 && o != k);
                        let c4 = m == 0 || m == j || n == 0 || n == i || o == 0 || o == k;
                        if c1 && c2 && c3 && c4 && r.next_float() >= 0.1 {
                            place_coral_block(ctx, r, mp, state);
                        }
                    }
                }
            }
            true
        }
    }
}

fn underwater_magma(
    ctx: &mut Ctx<'_, '_>,
    range: i32,
    radius: i32,
    prob: f32,
    r: &mut Random,
    pos: BlockPos,
) -> bool {
    let h = bh();
    let water = |s: BlockStateId| h.block(s) == h.idx.water;
    // Column.scan
    if !water(ctx.region.get(pos)) {
        return false;
    }
    let scan = |ctx: &Ctx<'_, '_>, dy: i32| -> Option<i32> {
        let mut y = pos.y;
        let mut k = 1;
        while k < range && water(ctx.region.get_xyz(pos.x, y, pos.z)) {
            y += dy;
            k += 1;
        }
        if !water(ctx.region.get_xyz(pos.x, y, pos.z)) {
            Some(y)
        } else {
            None
        }
    };
    let _ceiling = scan(ctx, 1);
    let Some(floor) = scan(ctx, -1) else {
        return false;
    };
    let center = bp(pos.x, floor, pos.z);
    let magma = rc_data::blocks().default_state("magma_block");
    let water_or_air = |ctx: &Ctx<'_, '_>, p: BlockPos| {
        let s = ctx.region.get(p);
        water(s) || h.is_air(s)
    };
    let mut n = 0;
    for z in center.z - radius..=center.z + radius {
        for y in center.y - radius..=center.y + radius {
            for x in center.x - radius..=center.x + radius {
                if r.next_float() >= prob {
                    continue;
                }
                let p = bp(x, y, z);
                if water_or_air(ctx, p) || water_or_air(ctx, off(p, 0, -1, 0)) {
                    continue;
                }
                if HORIZONTAL.iter().any(|d| water_or_air(ctx, p.offset(*d))) {
                    continue;
                }
                ctx.region.set(p, magma);
                n += 1;
            }
        }
    }
    n > 0
}
