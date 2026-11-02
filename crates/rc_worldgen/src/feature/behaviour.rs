//! Block behaviour needed by features: `canSurvive`, face sturdiness,
//! solidity, replaceability. Mirrors the relevant `Block` subclasses for
//! naturally generated blocks.

use std::sync::OnceLock;

use rc_core::{BlockPos, BlockStateId, Direction};
use rc_data::Blocks;
use rc_data::tags::BlockSet;

use super::region::Region;

/// Cached tags and block indices.
pub struct Bh {
    pub b: &'static Blocks,
    pub dirt: BlockSet,
    pub sand: BlockSet,
    pub leaves: BlockSet,
    pub logs: BlockSet,
    pub replaceable: BlockSet,
    pub replaceable_by_trees: BlockSet,
    pub features_cannot_replace: BlockSet,
    pub base_stone_overworld: BlockSet,
    pub dead_bush_may_place_on: BlockSet,
    pub mushroom_grow_block: BlockSet,
    pub snow_cannot: BlockSet,
    pub snow_can: BlockSet,
    pub nylium: BlockSet,
    pub small_dripleaf_placeable: BlockSet,
    pub big_dripleaf_placeable: BlockSet,
    pub bamboo_plantable_on: BlockSet,
    pub azalea_grows_on: BlockSet,
    pub wart_blocks: BlockSet,
    pub corals: BlockSet,
    pub idx: Idx,
}

/// Frequently compared block indices.
#[allow(missing_docs)]
pub struct Idx {
    pub air: u16,
    pub water: u16,
    pub lava: u16,
    pub farmland: u16,
    pub sugar_cane: u16,
    pub cactus: u16,
    pub frosted_ice: u16,
    pub snow: u16,
    pub magma_block: u16,
    pub kelp: u16,
    pub kelp_plant: u16,
    pub clay: u16,
    pub moss_block: u16,
    pub bedrock: u16,
    pub vine: u16,
    pub soul_soil: u16,
    pub big_dripleaf: u16,
    pub big_dripleaf_stem: u16,
    pub mycelium: u16,
    pub podzol: u16,
    pub grass_block: u16,
    pub ice: u16,
    pub packed_ice: u16,
    pub blue_ice: u16,
    pub netherrack: u16,
    pub warped_nylium: u16,
    pub crimson_nylium: u16,
}

pub fn bh() -> &'static Bh {
    static B: OnceLock<Bh> = OnceLock::new();
    B.get_or_init(|| {
        let b = rc_data::blocks();
        let t = rc_data::tags();
        let i = |n: &str| b.index_of(n).unwrap_or_else(|| panic!("block {n}"));
        Bh {
            b,
            dirt: t.block("dirt"),
            sand: t.block("sand"),
            leaves: t.block("leaves"),
            logs: t.block("logs"),
            replaceable: t.block("replaceable"),
            replaceable_by_trees: t.block("replaceable_by_trees"),
            features_cannot_replace: t.block("features_cannot_replace"),
            base_stone_overworld: t.block("base_stone_overworld"),
            dead_bush_may_place_on: t.block("dead_bush_may_place_on"),
            mushroom_grow_block: t.block("mushroom_grow_block"),
            snow_cannot: t.block("snow_layer_cannot_survive_on"),
            snow_can: t.block("snow_layer_can_survive_on"),
            nylium: t.block("nylium"),
            small_dripleaf_placeable: t.block("small_dripleaf_placeable"),
            big_dripleaf_placeable: t.block("big_dripleaf_placeable"),
            bamboo_plantable_on: t.block("bamboo_plantable_on"),
            azalea_grows_on: t.block("azalea_grows_on"),
            wart_blocks: t.block("wart_blocks"),
            corals: t.block("corals"),
            idx: Idx {
                air: i("air"),
                water: i("water"),
                lava: i("lava"),
                farmland: i("farmland"),
                sugar_cane: i("sugar_cane"),
                cactus: i("cactus"),
                frosted_ice: i("frosted_ice"),
                snow: i("snow"),
                magma_block: i("magma_block"),
                kelp: i("kelp"),
                kelp_plant: i("kelp_plant"),
                clay: i("clay"),
                moss_block: i("moss_block"),
                bedrock: i("bedrock"),
                vine: i("vine"),
                soul_soil: i("soul_soil"),
                big_dripleaf: i("big_dripleaf"),
                big_dripleaf_stem: i("big_dripleaf_stem"),
                mycelium: i("mycelium"),
                podzol: i("podzol"),
                grass_block: i("grass_block"),
                ice: i("ice"),
                packed_ice: i("packed_ice"),
                blue_ice: i("blue_ice"),
                netherrack: i("netherrack"),
                warped_nylium: i("warped_nylium"),
                crimson_nylium: i("crimson_nylium"),
            },
        }
    })
}

impl Bh {
    #[inline]
    #[must_use]
    pub fn block(&self, s: BlockStateId) -> u16 {
        self.b.block_index(s)
    }

    #[inline]
    #[must_use]
    pub fn name(&self, s: BlockStateId) -> &str {
        &self.b.def(s).name
    }

    #[inline]
    #[must_use]
    pub fn is_air(&self, s: BlockStateId) -> bool {
        self.b.is_air(s)
    }

    /// `BlockState.isSolid()` (legacy solidity).
    #[inline]
    #[must_use]
    pub fn is_solid(&self, s: BlockStateId) -> bool {
        self.b.info(s).flags & rc_data::blocks::FLAG_LEGACY_SOLID != 0
    }

    /// `BlockState.liquid()` (the block is a liquid block).
    #[inline]
    #[must_use]
    pub fn is_liquid_block(&self, s: BlockStateId) -> bool {
        let i = self.block(s);
        i == self.idx.water || i == self.idx.lava
    }

    /// `BlockState.canBeReplaced()`.
    #[inline]
    #[must_use]
    pub fn can_be_replaced(&self, s: BlockStateId) -> bool {
        self.replaceable.contains(self.block(s))
    }

    /// Full-cube collision shape.
    #[inline]
    #[must_use]
    pub fn is_full_cube(&self, s: BlockStateId) -> bool {
        self.b.info(s).flags & rc_data::blocks::FLAG_FULL_CUBE != 0
    }

    /// `isSolidRender` — opaque full cubes.
    #[must_use]
    pub fn is_solid_render(&self, s: BlockStateId) -> bool {
        self.is_full_cube(s) && !self.b.def(s).transparent && !self.leaves.contains(self.block(s))
    }

    /// `isFaceSturdy(level, pos, dir)` using the collision shape.
    #[must_use]
    pub fn is_face_sturdy(&self, s: BlockStateId, d: Direction) -> bool {
        // Leaves have an empty block-support shape.
        if self.leaves.contains(self.block(s)) {
            return false;
        }
        if self.is_full_cube(s) {
            return true;
        }
        face_full(self.b.collision(s), d)
    }

    /// `MultifaceBlock.canAttachTo`: the support shape *or* the collision
    /// shape has a full face towards us.
    #[must_use]
    pub fn can_attach_to(&self, s: BlockStateId, d: Direction) -> bool {
        let opp = d.opposite();
        self.is_face_sturdy(s, opp) || self.is_full_cube(s) || face_full(self.b.collision(s), opp)
    }

    #[inline]
    #[must_use]
    pub fn is_dirt(&self, s: BlockStateId) -> bool {
        self.dirt.contains(self.block(s))
    }

    #[inline]
    #[must_use]
    pub fn has_prop(&self, s: BlockStateId, p: &str) -> bool {
        self.b.prop_pos(s, p).is_some()
    }
}

/// Does the union of `boxes` cover the full unit face in direction `d`?
fn face_full(boxes: &[[f64; 6]], d: Direction) -> bool {
    let (axis, at_max) = match d {
        Direction::Down => (1, false),
        Direction::Up => (1, true),
        Direction::North => (2, false),
        Direction::South => (2, true),
        Direction::West => (0, false),
        Direction::East => (0, true),
    };
    let (u, v) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let mut grid = [[false; 16]; 16];
    for b in boxes {
        let touches = if at_max {
            b[axis + 3] >= 1.0 - 1e-7
        } else {
            b[axis] <= 1e-7
        };
        if !touches {
            continue;
        }
        for (gu, row) in grid.iter_mut().enumerate() {
            let cu = (gu as f64 + 0.5) / 16.0;
            if cu < b[u] || cu > b[u + 3] {
                continue;
            }
            for (gv, cell) in row.iter_mut().enumerate() {
                let cv = (gv as f64 + 0.5) / 16.0;
                if cv >= b[v] && cv <= b[v + 3] {
                    *cell = true;
                }
            }
        }
    }
    grid.iter().all(|r| r.iter().all(|&c| c))
}

/// `BlockState.canSurvive(level, pos)` for the blocks features place.
#[must_use]
pub fn can_survive(r: &Region<'_>, s: BlockStateId, p: BlockPos) -> bool {
    let h = bh();
    let below = BlockPos::new(p.x, p.y - 1, p.z);
    let above = BlockPos::new(p.x, p.y + 1, p.z);
    let bs_below = r.get(below);
    let bi_below = h.block(bs_below);
    let name = h.name(s);
    let bush = || h.dirt.contains(bi_below) || bi_below == h.idx.farmland;
    match name {
        // ---- bushes ----
        "short_grass" | "fern" | "dandelion" | "torchflower" | "poppy" | "blue_orchid"
        | "allium" | "azure_bluet" | "red_tulip" | "orange_tulip" | "white_tulip"
        | "pink_tulip" | "oxeye_daisy" | "cornflower" | "lily_of_the_valley" | "wither_rose"
        | "oak_sapling" | "spruce_sapling" | "birch_sapling" | "jungle_sapling"
        | "acacia_sapling" | "dark_oak_sapling" | "cherry_sapling" | "sweet_berry_bush"
        | "pink_petals" => bush(),
        "azalea" | "flowering_azalea" => bi_below == h.idx.clay || bush(),
        "mangrove_propagule" => {
            if h.b.get(s, "hanging") == Some("true") {
                let a = r.get(above);
                h.leaves.contains(h.block(a)) && h.name(a) == "mangrove_leaves"
            } else {
                bush() || bi_below == h.idx.clay
            }
        }
        "dead_bush" => h.dead_bush_may_place_on.contains(bi_below),
        "brown_mushroom" | "red_mushroom" => {
            // Unlit proto chunks report raw brightness 15 during features.
            h.mushroom_grow_block.contains(bi_below)
        }
        "tall_grass" | "large_fern" | "sunflower" | "lilac" | "rose_bush" | "peony"
        | "pitcher_plant" => {
            if h.b.get(s, "half") == Some("upper") {
                h.b.def(bs_below).name == name && h.b.get(bs_below, "half") == Some("lower")
            } else {
                bush()
            }
        }
        "tall_seagrass" => {
            if h.b.get(s, "half") == Some("upper") {
                h.b.def(bs_below).name == name && h.b.get(bs_below, "half") == Some("lower")
            } else {
                h.is_face_sturdy(bs_below, Direction::Up) && bi_below != h.idx.magma_block
            }
        }
        "seagrass" => h.is_face_sturdy(bs_below, Direction::Up) && bi_below != h.idx.magma_block,
        "lily_pad" => {
            let f = h.b.fluid(bs_below);
            let f2 = r.fluid(p);
            (f.is_water() && f.is_source()
                || matches!(
                    h.name(bs_below),
                    "ice" | "packed_ice" | "blue_ice" | "frosted_ice"
                ))
                && f2.is_empty()
        }
        "sugar_cane" => {
            if bi_below == h.idx.sugar_cane {
                return true;
            }
            if h.dirt.contains(bi_below) || h.sand.contains(bi_below) {
                for d in [
                    Direction::North,
                    Direction::East,
                    Direction::South,
                    Direction::West,
                ] {
                    let q = below.offset(d);
                    let bs2 = r.get(q);
                    if h.b.fluid(bs2).is_water() || h.block(bs2) == h.idx.frosted_ice {
                        return true;
                    }
                }
            }
            false
        }
        "cactus" => {
            for d in [
                Direction::North,
                Direction::East,
                Direction::South,
                Direction::West,
            ] {
                let q = p.offset(d);
                let bs = r.get(q);
                if h.is_solid(bs) || h.b.fluid(bs).is_lava() {
                    return false;
                }
            }
            (bi_below == h.idx.cactus || h.sand.contains(bi_below))
                && !h.is_liquid_block(r.get(above))
        }
        "snow" => {
            if h.snow_cannot.contains(bi_below) {
                return false;
            }
            if h.snow_can.contains(bi_below) {
                return true;
            }
            h.is_face_sturdy(bs_below, Direction::Up)
                || (bi_below == h.idx.snow && h.b.get(bs_below, "layers") == Some("8"))
        }
        "kelp" | "kelp_plant" => {
            bi_below != h.idx.magma_block
                && (bi_below == h.idx.kelp
                    || bi_below == h.idx.kelp_plant
                    || h.is_face_sturdy(bs_below, Direction::Up))
        }
        "small_dripleaf" => {
            if h.b.get(s, "half") == Some("upper") {
                h.name(bs_below) == "small_dripleaf"
            } else {
                h.small_dripleaf_placeable.contains(bi_below)
                    || (r.fluid(p).is_water() && r.fluid(p).is_source() && bush())
            }
        }
        "big_dripleaf" | "big_dripleaf_stem" => {
            bi_below == h.idx.big_dripleaf_stem
                || bi_below == h.idx.big_dripleaf
                || h.big_dripleaf_placeable.contains(bi_below)
        }
        "moss_carpet" => !h.is_air(bs_below),
        "hanging_roots" | "spore_blossom" => {
            let a = r.get(above);
            h.is_face_sturdy(a, Direction::Down)
                && !(name == "spore_blossom" && !r.fluid(p).is_empty())
        }
        "crimson_roots" | "warped_roots" | "nether_sprouts" | "crimson_fungus"
        | "warped_fungus" => {
            h.nylium.contains(bi_below)
                || bi_below == h.idx.soul_soil
                || (name.ends_with("fungus") && bi_below == h.idx.mycelium)
                || bush()
        }
        "bamboo" | "bamboo_sapling" => h.bamboo_plantable_on.contains(bi_below),
        n if n.ends_with("coral") || n.ends_with("coral_fan") => {
            h.is_face_sturdy(bs_below, Direction::Up)
        }
        "sea_pickle" => {
            !h.b.collision(bs_below).is_empty() && face_any(h.b.collision(bs_below))
                || h.is_face_sturdy(bs_below, Direction::Up)
        }
        _ => true,
    }
}

/// Collision shape has any area on its top face.
fn face_any(boxes: &[[f64; 6]]) -> bool {
    boxes.iter().any(|b| b[4] >= 1.0 - 1e-7)
}
