//! Block behaviour: fluid flow, neighbour shape updates (fences, panes,
//! walls, stairs), random ticks (crops, grass, leaves, saplings, cane,
//! melting) — following the rules described on minecraft.wiki.

use rc_core::{BlockPos, BlockStateId};
use rc_data::fluid::FluidKind;

use crate::world::World;

pub const H4: [(i32, i32, &str); 4] = [
    (0, -1, "north"),
    (1, 0, "east"),
    (0, 1, "south"),
    (-1, 0, "west"),
];

#[inline]
fn off(p: BlockPos, dx: i32, dy: i32, dz: i32) -> BlockPos {
    BlockPos::new(p.x + dx, p.y + dy, p.z + dz)
}

fn name(s: BlockStateId) -> &'static str {
    rc_data::blocks().def(s).name.as_str()
}

/// Can a fluid flow into this block (and wash it away)?
#[must_use]
pub fn fluid_can_replace(s: BlockStateId) -> bool {
    let b = rc_data::blocks();
    if s == BlockStateId::AIR || name(s).ends_with("air") {
        return true;
    }
    if !b.fluid(s).is_empty() {
        return false;
    }
    b.collision(s).is_empty()
        && b.get(s, "waterlogged").is_none()
        && !name(s).contains("sign")
        && !name(s).contains("portal")
}

/// Fluid at `p`: (kind, amount 1..8, is_source, falling).
fn fluid(world: &World, p: BlockPos) -> Option<(FluidKind, u8, bool, bool)> {
    let f = rc_data::blocks().fluid(world.get_block(p));
    if f.is_empty() {
        return None;
    }
    Some((f.kind, f.amount, f.is_source(), f.falling))
}

/// Delay between fluid updates (overworld).
#[must_use]
pub fn fluid_delay(k: FluidKind) -> u64 {
    if matches!(k, FluidKind::Lava) { 30 } else { 5 }
}

fn fluid_state(k: FluidKind, amount: u8, falling: bool) -> BlockStateId {
    let b = rc_data::blocks();
    let block = if matches!(k, FluidKind::Lava) {
        "lava"
    } else {
        "water"
    };
    let level = if amount >= 8 && !falling {
        0
    } else if falling {
        8
    } else {
        8 - amount
    };
    b.with(b.default_state(block), "level", &level.to_string())
}

/// Outcome of a fluid tick: blocks to set, and positions to re-tick.
#[derive(Debug, Default)]
pub struct FluidResult {
    pub set: Vec<(BlockPos, BlockStateId)>,
    pub schedule: Vec<(BlockPos, u64)>,
    pub fizz: Option<BlockPos>,
}

/// Distance (≤ `max`) to a spot where the fluid could flow down, searching
/// horizontally from `p` without going back toward `from`.
fn slope_distance(world: &World, p: BlockPos, depth: u32, max: u32, from: (i32, i32)) -> u32 {
    let mut best = 1000;
    for &(dx, dz, _) in &H4 {
        if (dx, dz) == from {
            continue;
        }
        let n = off(p, dx, 0, dz);
        let s = world.get_block(n);
        if !fluid_can_replace(s) && fluid(world, n).is_none_or(|f| f.2) {
            continue;
        }
        if fluid_can_replace(world.get_block(off(n, 0, -1, 0)))
            || fluid(world, off(n, 0, -1, 0)).is_some()
        {
            return depth;
        }
        if depth < max {
            best = best.min(slope_distance(world, n, depth + 1, max, (-dx, -dz)));
        }
    }
    best
}

/// One scheduled fluid update at `p`.
#[must_use]
pub fn fluid_tick(world: &World, p: BlockPos) -> FluidResult {
    let mut out = FluidResult::default();
    let b = rc_data::blocks();
    let Some((kind, amount, source, _falling)) = fluid(world, p) else {
        return out;
    };
    let is_lava = matches!(kind, FluidKind::Lava);
    let drop = if is_lava { 2 } else { 1 };
    let delay = fluid_delay(kind);
    let state = world.get_block(p);
    let waterlogged = b.get(state, "waterlogged") == Some("true");
    // Lava meeting water hardens.
    if is_lava {
        for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, 1, 0)] {
            if fluid(world, off(p, dx, dy, dz)).is_some_and(|f| matches!(f.0, FluidKind::Water)) {
                let to = if source { "obsidian" } else { "cobblestone" };
                out.set.push((p, b.default_state(to)));
                out.fizz = Some(p);
                return out;
            }
        }
    }
    // Recompute a flowing block's level from its neighbours.
    let mut amount = amount;
    let mut falling_now = false;
    if !source && !waterlogged {
        let above = fluid(world, off(p, 0, 1, 0)).filter(|f| f.0 == kind);
        let mut best = 0u8;
        let mut sources = 0;
        for &(dx, dz, _) in &H4 {
            if let Some(f) = fluid(world, off(p, dx, 0, dz)).filter(|f| f.0 == kind) {
                best = best.max(f.1);
                if f.2 {
                    sources += 1;
                }
            }
        }
        let below = world.get_block(off(p, 0, -1, 0));
        let below_solid = !b.collision(below).is_empty()
            || fluid(world, off(p, 0, -1, 0)).is_some_and(|f| f.2 && f.0 == kind);
        let new = if !is_lava && sources >= 2 && below_solid {
            out.set.push((p, fluid_state(kind, 8, false)));
            out.schedule.push((p, delay));
            return out;
        } else if above.is_some() {
            falling_now = true;
            8
        } else {
            best.saturating_sub(drop)
        };
        if new == 0 {
            out.set.push((p, BlockStateId::AIR));
            for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 0, 1), (0, 0, -1), (0, -1, 0)] {
                out.schedule.push((off(p, dx, dy, dz), delay));
            }
            return out;
        }
        let ns = fluid_state(kind, new, falling_now);
        if ns != state {
            out.set.push((p, ns));
            out.schedule.push((p, delay));
            for &(dx, dz, _) in &H4 {
                out.schedule.push((off(p, dx, 0, dz), delay));
            }
        }
        amount = new;
    }
    // Spread down first.
    let below_p = off(p, 0, -1, 0);
    let below = world.get_block(below_p);
    if below_p.y >= -64 {
        if fluid_can_replace(below) {
            out.set.push((below_p, fluid_state(kind, 8, true)));
            out.schedule.push((below_p, delay));
            if !source {
                return out;
            }
        } else if is_lava && fluid(world, below_p).is_some_and(|f| matches!(f.0, FluidKind::Water))
        {
            out.set.push((below_p, b.default_state("stone")));
            out.fizz = Some(below_p);
            return out;
        } else if fluid(world, below_p).is_some_and(|f| f.0 == kind && !f.2) && !source {
            return out;
        }
    }
    // Then sideways toward the nearest drop.
    let spread = if falling_now && !source {
        8 - drop
    } else {
        amount.saturating_sub(drop)
    };
    if spread == 0 {
        return out;
    }
    let max = if is_lava { 2 } else { 4 };
    let mut dirs: Vec<(i32, i32, u32)> = Vec::new();
    for &(dx, dz, _) in &H4 {
        let n = off(p, dx, 0, dz);
        let s = world.get_block(n);
        let can = fluid_can_replace(s)
            || fluid(world, n).is_some_and(|f| f.0 == kind && !f.2 && f.1 < spread);
        if !can {
            continue;
        }
        let down = fluid_can_replace(world.get_block(off(n, 0, -1, 0)))
            || fluid(world, off(n, 0, -1, 0)).is_some_and(|f| f.0 == kind);
        let d = if down {
            0
        } else {
            slope_distance(world, n, 1, max, (-dx, -dz))
        };
        dirs.push((dx, dz, d));
    }
    let best = dirs.iter().map(|d| d.2).min().unwrap_or(1000);
    for (dx, dz, d) in dirs {
        if d != best && best < 1000 {
            continue;
        }
        let n = off(p, dx, 0, dz);
        if is_lava && fluid(world, n).is_some_and(|f| matches!(f.0, FluidKind::Water)) {
            continue;
        }
        out.set.push((n, fluid_state(kind, spread, false)));
        out.schedule.push((n, delay));
    }
    out
}

/// Recompute connection/shape properties after a neighbour changed.
#[must_use]
pub fn update_shape(world: &World, p: BlockPos) -> Option<BlockStateId> {
    let b = rc_data::blocks();
    let s = world.get_block(p);
    let n = name(s);
    let solid = |q: BlockPos| {
        let t = world.get_block(q);
        b.info(t).flags & rc_data::blocks::FLAG_FULL_CUBE != 0 && !b.def(t).transparent
    };
    let mut out = s;
    let family = |t: BlockStateId, wanted: &str| -> bool {
        let tn = name(t);
        match wanted {
            "fence" => {
                (tn.ends_with("_fence") && tn.contains("nether") == n.contains("nether"))
                    || tn.ends_with("fence_gate")
            }
            "pane" => tn.ends_with("glass_pane") || tn == "iron_bars" || tn.ends_with("_wall"),
            "wall" => {
                tn.ends_with("_wall")
                    || tn.ends_with("glass_pane")
                    || tn == "iron_bars"
                    || tn.ends_with("fence_gate")
            }
            _ => false,
        }
    };
    let kind = if n.ends_with("_fence") {
        "fence"
    } else if n.ends_with("glass_pane") || n == "iron_bars" {
        "pane"
    } else if n.ends_with("_wall") && b.get(s, "up").is_some() {
        "wall"
    } else {
        ""
    };
    if !kind.is_empty() {
        for &(dx, dz, dir) in &H4 {
            let q = off(p, dx, 0, dz);
            let t = world.get_block(q);
            let c = family(t, kind) || solid(q);
            out = if kind == "wall" {
                b.with(out, dir, if c { "low" } else { "none" })
            } else {
                b.with(out, dir, if c { "true" } else { "false" })
            };
        }
        if kind == "wall" {
            let ns = b.get(out, "north") != Some("none");
            let ss = b.get(out, "south") != Some("none");
            let es = b.get(out, "east") != Some("none");
            let ws = b.get(out, "west") != Some("none");
            let straight = (ns && ss && !es && !ws) || (es && ws && !ns && !ss);
            let above = world.get_block(off(p, 0, 1, 0));
            out = b.with(
                out,
                "up",
                if !straight
                    || name(above).ends_with("_wall")
                    || !b.collision(above).is_empty() && solid(off(p, 0, 1, 0))
                {
                    "true"
                } else {
                    "false"
                },
            );
        }
    } else if n.ends_with("_stairs") {
        out = b.with(out, "shape", &stair_shape(world, p, s));
    } else if n == "grass_block" || n == "podzol" || n == "mycelium" {
        let above = name(world.get_block(off(p, 0, 1, 0)));
        out = b.with(
            out,
            "snowy",
            if above == "snow" || above == "snow_block" || above == "powder_snow" {
                "true"
            } else {
                "false"
            },
        );
    }
    (out != s).then_some(out)
}

fn facing_vec(f: &str) -> (i32, i32) {
    match f {
        "north" => (0, -1),
        "south" => (0, 1),
        "east" => (1, 0),
        _ => (-1, 0),
    }
}

fn rot_left(f: &str) -> &'static str {
    match f {
        "north" => "west",
        "west" => "south",
        "south" => "east",
        _ => "north",
    }
}

/// Vanilla stair shape rule (inner/outer corners from neighbouring stairs).
fn stair_shape(world: &World, p: BlockPos, s: BlockStateId) -> String {
    let b = rc_data::blocks();
    let facing = b.get(s, "facing").unwrap_or("north").to_owned();
    let half = b.get(s, "half").unwrap_or("bottom").to_owned();
    let stair = |q: BlockPos| -> Option<(String, String)> {
        let t = world.get_block(q);
        name(t).ends_with("_stairs").then(|| {
            (
                b.get(t, "facing").unwrap_or("north").to_owned(),
                b.get(t, "half").unwrap_or("bottom").to_owned(),
            )
        })
    };
    let (fx, fz) = facing_vec(&facing);
    let left = rot_left(&facing);
    let is_axis_diff =
        |a: &str, c: &str| (a == "north" || a == "south") != (c == "north" || c == "south");
    if let Some((f2, h2)) = stair(off(p, fx, 0, fz))
        && h2 == half
        && is_axis_diff(&f2, &facing)
    {
        return if f2 == left {
            "outer_left".into()
        } else {
            "outer_right".into()
        };
    }
    if let Some((f2, h2)) = stair(off(p, -fx, 0, -fz))
        && h2 == half
        && is_axis_diff(&f2, &facing)
    {
        return if f2 == left {
            "inner_left".into()
        } else {
            "inner_right".into()
        };
    }
    "straight".into()
}

/// A change produced by a random tick.
#[derive(Debug)]
pub enum Change {
    Set(BlockPos, BlockStateId),
    /// Grow a tree of `kind` at the sapling position.
    Tree(BlockPos, String),
    /// Break with drops.
    Break(BlockPos),
}

/// One random tick at `p`.
#[must_use]
pub fn random_tick(world: &World, p: BlockPos, rng: &mut impl rand::Rng) -> Option<Change> {
    let b = rc_data::blocks();
    let s = world.get_block(p);
    let n = name(s);
    let light_above = || {
        let (sky, blk) = world.light(off(p, 0, 1, 0));
        sky.max(blk)
    };
    if matches!(
        n,
        "wheat"
            | "carrots"
            | "potatoes"
            | "beetroots"
            | "nether_wart"
            | "torchflower_crop"
            | "melon_stem"
            | "pumpkin_stem"
            | "sweet_berry_bush"
    ) {
        let max = match n {
            "beetroots" | "nether_wart" | "sweet_berry_bush" => 3,
            "torchflower_crop" => 2,
            _ => 7,
        };
        let age: u32 = b.get(s, "age").and_then(|a| a.parse().ok()).unwrap_or(0);
        if age < max && light_above() >= 9 && rng.random_range(0..5) == 0 {
            return Some(Change::Set(p, b.with(s, "age", &(age + 1).to_string())));
        }
        return None;
    }
    if n.ends_with("_sapling") || n == "mangrove_propagule" {
        if light_above() >= 9 && rng.random_range(0..7) == 0 {
            let stage = b.get(s, "stage").unwrap_or("0");
            if stage == "0" {
                return Some(Change::Set(p, b.with(s, "stage", "1")));
            }
            return Some(Change::Tree(p, n.trim_end_matches("_sapling").to_owned()));
        }
        return None;
    }
    if n == "grass_block" || n == "mycelium" {
        let above = world.get_block(off(p, 0, 1, 0));
        if b.info(above).opacity >= 15 {
            return Some(Change::Set(p, b.default_state("dirt")));
        }
        if light_above() >= 9 {
            let q = off(
                p,
                rng.random_range(-1..=1),
                rng.random_range(-3..=1),
                rng.random_range(-1..=1),
            );
            if name(world.get_block(q)) == "dirt"
                && b.info(world.get_block(off(q, 0, 1, 0))).opacity < 15
            {
                let (sky, blk) = world.light(off(q, 0, 1, 0));
                if sky.max(blk) >= 4 {
                    return Some(Change::Set(q, b.default_state(n)));
                }
            }
        }
        return None;
    }
    if n.ends_with("leaves")
        && b.get(s, "persistent") == Some("false")
        && b.get(s, "distance") == Some("7")
    {
        return Some(Change::Break(p));
    }
    if n == "sugar_cane" || n == "cactus" {
        let above = off(p, 0, 1, 0);
        if world.get_block(above) == BlockStateId::AIR {
            let mut h = 1;
            while h < 3 && name(world.get_block(off(p, 0, -h, 0))) == n {
                h += 1;
            }
            if h < 3 {
                let age: u32 = b.get(s, "age").and_then(|a| a.parse().ok()).unwrap_or(0);
                if age >= 15 {
                    return Some(Change::Set(above, b.default_state(n)));
                }
                return Some(Change::Set(p, b.with(s, "age", &(age + 1).to_string())));
            }
        }
        return None;
    }
    if (n == "ice" || n == "snow") && world.light(p).1 > 11 {
        return Some(Change::Set(
            p,
            if n == "ice" {
                b.default_state("water")
            } else {
                BlockStateId::AIR
            },
        ));
    }
    None
}

/// Recompute a leaf block's distance to the nearest log.
#[must_use]
pub fn leaf_distance(world: &World, p: BlockPos) -> Option<BlockStateId> {
    let b = rc_data::blocks();
    let s = world.get_block(p);
    if !name(s).ends_with("leaves") {
        return None;
    }
    let logs = rc_data::tags().block("logs");
    let mut best = 7u32;
    for (dx, dy, dz) in [
        (1, 0, 0),
        (-1, 0, 0),
        (0, 1, 0),
        (0, -1, 0),
        (0, 0, 1),
        (0, 0, -1),
    ] {
        let t = world.get_block(off(p, dx, dy, dz));
        if logs.contains_state(b, t) {
            best = 1;
            break;
        }
        if name(t).ends_with("leaves")
            && let Some(d) = b.get(t, "distance").and_then(|d| d.parse::<u32>().ok())
        {
            best = best.min(d + 1);
        }
    }
    let ns = b.with(s, "distance", &best.min(7).to_string());
    (ns != s).then_some(ns)
}

/// Blocks of a simple grown tree: trunk and leaf blob (sapling growth).
#[must_use]
pub fn tree_blocks(
    world: &World,
    p: BlockPos,
    kind: &str,
    rng: &mut impl rand::Rng,
) -> Vec<(BlockPos, BlockStateId)> {
    let b = rc_data::blocks();
    let (log, leaves) = match kind {
        "birch" | "spruce" | "jungle" | "acacia" | "dark_oak" | "cherry" => {
            (format!("{kind}_log"), format!("{kind}_leaves"))
        }
        "mangrove" | "mangrove_propagule" => ("mangrove_log".into(), "mangrove_leaves".into()),
        _ => ("oak_log".into(), "oak_leaves".into()),
    };
    let log = b.default_state(&log);
    let leaves = b.with(b.default_state(&leaves), "persistent", "false");
    let h = match kind {
        "spruce" => rng.random_range(6..9),
        "birch" => rng.random_range(5..8),
        _ => rng.random_range(4..7),
    };
    // Need space.
    for y in 1..=h {
        let t = world.get_block(off(p, 0, y, 0));
        if t != BlockStateId::AIR && !name(t).ends_with("leaves") {
            return Vec::new();
        }
    }
    let mut out = Vec::new();
    let top = p.y + h;
    let leaf = |q: BlockPos, out: &mut Vec<(BlockPos, BlockStateId)>| {
        if world.get_block(q) == BlockStateId::AIR {
            out.push((q, leaves));
        }
    };
    if kind == "spruce" {
        let mut r = 0;
        for y in (p.y + 2..=top + 1).rev() {
            for dx in -r..=r {
                for dz in -r..=r {
                    if dx * dx + dz * dz <= r * r + 1 && (dx != 0 || dz != 0 || y > top) {
                        leaf(BlockPos::new(p.x + dx, y, p.z + dz), &mut out);
                    }
                }
            }
            r = if r >= 2 { 1 } else { r + 1 };
        }
    } else {
        for y in top - 3..=top + 1 {
            let r: i32 = if y >= top { 1 } else { 2 };
            for dx in -r..=r {
                for dz in -r..=r {
                    let corner = dx.abs() == r && dz.abs() == r;
                    if corner && (y >= top || rng.random_range(0..2) == 0) {
                        continue;
                    }
                    leaf(BlockPos::new(p.x + dx, y, p.z + dz), &mut out);
                }
            }
        }
    }
    for y in 0..h {
        out.push((off(p, 0, y, 0), log));
    }
    // Leaves just placed are distance 1..3 from the trunk; mark them by distance.
    let trunk: Vec<BlockPos> = (0..h).map(|y| off(p, 0, y, 0)).collect();
    for (q, s) in &mut out {
        if *s == leaves {
            let d = trunk
                .iter()
                .map(|t| (t.x - q.x).abs() + (t.y - q.y).abs() + (t.z - q.z).abs())
                .min()
                .unwrap_or(7);
            *s = b.with(leaves, "distance", &(d.clamp(1, 7)).to_string());
        }
    }
    out
}
