//! Nether portals (minecraft.wiki "Nether portal"): an obsidian frame with a
//! 2×3 to 21×21 interior (corners optional) lights when fire is placed inside
//! it; standing in the portal for 4 seconds (instantly in creative) moves the
//! player between the Overworld and the Nether at an 8:1 scale. Arriving, the
//! nearest portal around the scaled position is used, or a new one is built.

use rc_core::{BlockPos, BlockStateId};
use rc_worldgen::Dimension;

use crate::world::World;

/// Interior limits (frame 4×5 .. 23×23).
const MIN_W: i32 = 2;
const MIN_H: i32 = 3;
const MAX_INNER: i32 = 21;

fn name(s: BlockStateId) -> &'static str {
    rc_data::blocks().def(s).name.as_str()
}

fn is_frame(s: BlockStateId) -> bool {
    name(s) == "obsidian"
}

fn is_inner(s: BlockStateId) -> bool {
    matches!(
        name(s),
        "air" | "cave_air" | "fire" | "soul_fire" | "nether_portal"
    )
}

/// A lit portal's interior: lower corner, axis ("x" or "z"), width, height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub origin: BlockPos,
    pub axis: &'static str,
    pub width: i32,
    pub height: i32,
}

impl Frame {
    /// Every interior position.
    pub fn cells(self) -> impl Iterator<Item = BlockPos> {
        let (dx, dz) = if self.axis == "x" { (1, 0) } else { (0, 1) };
        (0..self.height).flat_map(move |h| {
            (0..self.width).map(move |i| {
                BlockPos::new(
                    self.origin.x + dx * i,
                    self.origin.y + h,
                    self.origin.z + dz * i,
                )
            })
        })
    }
}

/// Find a complete frame around `p` (an interior cell) along `axis`.
#[must_use]
pub fn find_frame(w: &World, p: BlockPos, axis: &'static str) -> Option<Frame> {
    let (dx, dz) = if axis == "x" { (1, 0) } else { (0, 1) };
    let at = |x: i32, y: i32, z: i32| w.get_block(BlockPos::new(x, y, z));
    if !is_inner(at(p.x, p.y, p.z)) {
        return None;
    }
    // Down to the floor.
    let mut y = p.y;
    let mut n = 0;
    while is_inner(at(p.x, y - 1, p.z)) {
        y -= 1;
        n += 1;
        if n > MAX_INNER {
            return None;
        }
    }
    if !is_frame(at(p.x, y - 1, p.z)) {
        return None;
    }
    // Back to the left edge.
    let (mut x, mut z) = (p.x, p.z);
    n = 0;
    while is_inner(at(x - dx, y, z - dz)) && is_frame(at(x - dx, y - 1, z - dz)) {
        x -= dx;
        z -= dz;
        n += 1;
        if n > MAX_INNER {
            return None;
        }
    }
    if !is_frame(at(x - dx, y, z - dz)) {
        return None;
    }
    // Width along the floor.
    let mut width = 0;
    while width <= MAX_INNER
        && is_inner(at(x + dx * width, y, z + dz * width))
        && is_frame(at(x + dx * width, y - 1, z + dz * width))
    {
        width += 1;
    }
    if !(MIN_W..=MAX_INNER).contains(&width) || !is_frame(at(x + dx * width, y, z + dz * width)) {
        return None;
    }
    // Height: rows of interior with frame on both sides, closed by a frame row.
    let mut height = 0;
    loop {
        if height > MAX_INNER {
            return None;
        }
        let row = y + height;
        let all_frame = (0..width).all(|i| is_frame(at(x + dx * i, row, z + dz * i)));
        if all_frame {
            break;
        }
        let ok = (0..width).all(|i| is_inner(at(x + dx * i, row, z + dz * i)))
            && is_frame(at(x - dx, row, z - dz))
            && is_frame(at(x + dx * width, row, z + dz * width));
        if !ok {
            return None;
        }
        height += 1;
    }
    (MIN_H..=MAX_INNER).contains(&height).then_some(Frame {
        origin: BlockPos::new(x, y, z),
        axis,
        width,
        height,
    })
}

/// A portal frame that fire placed at `p` would light (X axis first).
#[must_use]
pub fn frame_for_fire(w: &World, p: BlockPos) -> Option<Frame> {
    if w.dimension == Dimension::End {
        return None;
    }
    find_frame(w, p, "x").or_else(|| find_frame(w, p, "z"))
}

/// Whether a portal block at `p` still has portal or frame on all four
/// in-plane sides (otherwise it breaks, taking the rest with it).
#[must_use]
pub fn portal_block_valid(w: &World, p: BlockPos, s: BlockStateId) -> bool {
    let axis = rc_data::blocks().get(s, "axis").unwrap_or("x");
    let (dx, dz) = if axis == "x" { (1, 0) } else { (0, 1) };
    let ok = |x: i32, y: i32, z: i32| {
        let n = w.get_block(BlockPos::new(x, y, z));
        is_frame(n) || name(n) == "nether_portal"
    };
    ok(p.x, p.y + 1, p.z)
        && ok(p.x, p.y - 1, p.z)
        && ok(p.x + dx, p.y, p.z + dz)
        && ok(p.x - dx, p.y, p.z - dz)
}

/// Destination dimension and position for a portal in `from` at `pos`.
#[must_use]
pub fn destination(from: Dimension, pos: [f64; 3]) -> Option<(&'static str, [f64; 3])> {
    match from {
        Dimension::Overworld => Some((
            "the_nether",
            [pos[0] / 8.0, pos[1].clamp(32.0, 118.0), pos[2] / 8.0],
        )),
        Dimension::Nether => Some(("overworld", [pos[0] * 8.0, pos[1], pos[2] * 8.0])),
        Dimension::End => None,
    }
}

/// Nearest portal block within `r` blocks horizontally of `p` (loaded
/// chunks only).
#[must_use]
pub fn find_portal_near(w: &World, p: BlockPos, r: i32) -> Option<BlockPos> {
    let portal = rc_data::blocks().default_state("nether_portal");
    let (lo, hi) = match w.dimension {
        Dimension::Nether => (1, 127),
        _ => (-63, 319),
    };
    let mut best: Option<(i64, BlockPos)> = None;
    for dx in -r..=r {
        for dz in -r..=r {
            let (x, z) = (p.x + dx, p.z + dz);
            if !w.is_loaded(BlockPos::new(x, 0, z)) {
                continue;
            }
            for y in lo..hi {
                let q = BlockPos::new(x, y, z);
                let s = w.get_block(q);
                if name(s) != "nether_portal" && s != portal {
                    continue;
                }
                // Lowest block of this column of the portal.
                if name(w.get_block(BlockPos::new(x, y - 1, z))) == "nether_portal" {
                    continue;
                }
                let d = i64::from(dx).pow(2) + i64::from(dz).pow(2) + i64::from(y - p.y).pow(2);
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, q));
                }
            }
        }
    }
    best.map(|(_, q)| q)
}

/// Blocks to set to build a new 2×3 portal with its frame at `base` (lowest
/// interior cell), along the X axis, with an obsidian ledge to stand on and
/// air cleared on both sides.
#[must_use]
pub fn build_portal(base: BlockPos) -> Vec<(BlockPos, BlockStateId)> {
    let b = rc_data::blocks();
    let obsidian = b.default_state("obsidian");
    let air = BlockStateId::AIR;
    let portal = b.with(b.default_state("nether_portal"), "axis", "x");
    let mut out = Vec::new();
    for i in -1..=2 {
        for h in -1..=3 {
            let q = BlockPos::new(base.x + i, base.y + h, base.z);
            let edge = i == -1 || i == 2 || h == -1 || h == 3;
            out.push((q, if edge { obsidian } else { portal }));
            if (0..=2).contains(&h) {
                for dz in [-1, 1] {
                    out.push((BlockPos::new(q.x, q.y, q.z + dz), air));
                }
            }
        }
        // Ledges either side to stand on.
        for dz in [-1, 1] {
            out.push((BlockPos::new(base.x + i, base.y - 1, base.z + dz), obsidian));
        }
    }
    out
}

/// A spot near `p` for a new portal: free 4×5×3 space on solid ground if
/// any is near, otherwise `p` itself (the portal brings its own ledge).
#[must_use]
pub fn site_for_new_portal(w: &World, p: BlockPos) -> BlockPos {
    let b = rc_data::blocks();
    let solid = |q: BlockPos| !b.collision(w.get_block(q)).is_empty();
    let free = |q: BlockPos| {
        let s = w.get_block(q);
        b.collision(s).is_empty() && b.fluid(s).is_empty()
    };
    let (lo, hi) = match w.dimension {
        Dimension::Nether => (32, 118),
        _ => (w.surface_y(p.x, p.z).unwrap_or(64).max(-60) - 8, 300),
    };
    for r in 0i32..16 {
        for dx in -r..=r {
            for dz in -r..=r {
                if dx.abs() != r && dz.abs() != r {
                    continue;
                }
                let (x, z) = (p.x + dx, p.z + dz);
                if !w.is_loaded(BlockPos::new(x, 0, z)) {
                    continue;
                }
                for y in lo..hi {
                    let ok = (-1..=2).all(|i| {
                        solid(BlockPos::new(x + i, y - 1, z))
                            && (0..4).all(|h| {
                                (-1..=1).all(|dz2| free(BlockPos::new(x + i, y + h, z + dz2)))
                            })
                    });
                    if ok {
                        return BlockPos::new(x, y, z);
                    }
                }
            }
        }
    }
    let y = match w.dimension {
        Dimension::Nether => p.y.clamp(32, 110),
        _ => w.surface_y(p.x, p.z).map_or(p.y, |s| s + 1),
    };
    BlockPos::new(p.x, y, p.z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destinations_scale_by_eight() {
        let (d, p) = destination(Dimension::Overworld, [800.0, 70.0, -160.0]).unwrap();
        assert_eq!(d, "the_nether");
        assert_eq!(p, [100.0, 70.0, -20.0]);
        let (d, p) = destination(Dimension::Nether, [100.0, 70.0, -20.0]).unwrap();
        assert_eq!(d, "overworld");
        assert_eq!(p, [800.0, 70.0, -160.0]);
        assert!(destination(Dimension::End, [0.0; 3]).is_none());
    }

    fn empty_world() -> World {
        use std::sync::Arc;
        let dir = std::env::temp_dir().join(format!("rc-portal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let models = Arc::new(rc_assets::fallback::block_models(rc_data::blocks(), 1));
        let mesh = Arc::new(crate::world::MeshShared {
            tables: rc_render::mesh::MeshTables::new(&models, true),
            models,
            colors: crate::world::biome_colors(None, None),
            smooth: true,
        });
        let mut w = World::open(&dir, "t", 1, 1, mesh).unwrap();
        for cx in -1..=1 {
            for cz in -1..=1 {
                let c = rc_world::Chunk::new_empty(rc_core::ChunkPos::new(cx, cz));
                w.chunks.insert(c.pos, Arc::new(c));
            }
        }
        w
    }

    #[test]
    fn finds_frames_of_any_size_without_corners() {
        let mut w = empty_world();
        let obsidian = rc_data::blocks().default_state("obsidian");
        // 4 wide × 5 tall interior along Z at x = 2, corners left out.
        let (w_in, h_in) = (4, 5);
        for i in 0..w_in {
            w.set_block(BlockPos::new(2, 69, 1 + i), obsidian);
            w.set_block(BlockPos::new(2, 70 + h_in, 1 + i), obsidian);
        }
        for h in 0..h_in {
            w.set_block(BlockPos::new(2, 70 + h, 0), obsidian);
            w.set_block(BlockPos::new(2, 70 + h, 1 + w_in), obsidian);
        }
        let f = frame_for_fire(&w, BlockPos::new(2, 72, 3)).expect("frame");
        assert_eq!(f.axis, "z");
        assert_eq!((f.width, f.height), (w_in, h_in));
        assert_eq!(f.origin, BlockPos::new(2, 70, 1));
        assert_eq!(f.cells().count(), (w_in * h_in) as usize);
        // A gap in the frame: no portal.
        w.set_block(BlockPos::new(2, 72, 0), BlockStateId::AIR);
        assert!(frame_for_fire(&w, BlockPos::new(2, 72, 3)).is_none());
        // Too small (1 wide).
        assert!(find_frame(&w, BlockPos::new(9, 70, 9), "x").is_none());
    }

    #[test]
    fn built_portal_has_frame_and_interior() {
        let blocks = build_portal(BlockPos::new(0, 70, 0));
        let portals = blocks
            .iter()
            .filter(|(_, s)| name(*s) == "nether_portal")
            .count();
        assert_eq!(portals, 6);
        let frame = blocks
            .iter()
            .filter(|(q, s)| name(*s) == "obsidian" && q.z == 0)
            .count();
        assert_eq!(frame, 14);
    }
}
