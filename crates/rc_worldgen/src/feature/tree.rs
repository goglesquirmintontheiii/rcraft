//! `TreeFeature` with all vanilla trunk placers, foliage placers, root
//! placers and decorators.

use ahash::AHashMap;
use rc_core::{BlockPos, BlockStateId, Direction};
use rc_data::tags::BlockSet;
use serde_json::Value;

use super::Ctx;
use super::behaviour::bh;
use super::jset::JavaPosSet;
use super::predicate::StateProvider;
use crate::mth;
use crate::providers::IntProvider;
use crate::rng::Random;

const HORIZONTAL: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

fn step(d: Direction) -> (i32, i32) {
    let (x, _, z) = d.delta();
    (x, z)
}

fn random_horizontal(r: &mut Random) -> Direction {
    HORIZONTAL[r.next_int_bound(4) as usize]
}

#[derive(Clone, Debug)]
pub enum TrunkKind {
    Straight,
    Forking,
    Giant,
    MegaJungle,
    DarkOak,
    Fancy,
    Bending {
        min_height_for_leaves: i32,
        bend_length: IntProvider,
    },
    UpwardsBranching {
        extra_branch_steps: IntProvider,
        branch_prob: f32,
        extra_branch_length: IntProvider,
        can_grow_through: BlockSet,
    },
    Cherry {
        branch_count: IntProvider,
        branch_horizontal_length: IntProvider,
        branch_start_offset: (i32, i32),
        branch_end_offset: IntProvider,
    },
}

#[derive(Clone, Debug)]
pub struct Trunk {
    base: i32,
    a: i32,
    b: i32,
    kind: TrunkKind,
}

#[derive(Clone, Debug)]
pub enum FoliageKind {
    Blob {
        height: i32,
    },
    Fancy {
        height: i32,
    },
    Bush {
        height: i32,
    },
    Spruce {
        trunk_height: IntProvider,
    },
    Pine {
        height: IntProvider,
    },
    Acacia,
    Jungle {
        height: i32,
    },
    MegaPine {
        crown_height: IntProvider,
    },
    DarkOak,
    RandomSpread {
        foliage_height: IntProvider,
        attempts: i32,
    },
    Cherry {
        height: IntProvider,
        wide_bottom_layer_hole_chance: f32,
        corner_hole_chance: f32,
        hanging_leaves_chance: f32,
        hanging_leaves_extension_chance: f32,
    },
}

#[derive(Clone, Debug)]
pub struct Foliage {
    radius: IntProvider,
    offset: IntProvider,
    kind: FoliageKind,
}

#[derive(Clone, Debug)]
pub enum MinSize {
    Two {
        limit: i32,
        lower: i32,
        upper: i32,
        min_clipped: Option<i32>,
    },
    Three {
        limit: i32,
        upper_limit: i32,
        lower: i32,
        middle: i32,
        upper: i32,
        min_clipped: Option<i32>,
    },
}

impl MinSize {
    fn size_at(&self, i: i32, j: i32) -> i32 {
        match *self {
            Self::Two {
                limit,
                lower,
                upper,
                ..
            } => {
                if j < limit {
                    lower
                } else {
                    upper
                }
            }
            Self::Three {
                limit,
                upper_limit,
                lower,
                middle,
                upper,
                ..
            } => {
                if j < limit {
                    lower
                } else if j >= i - upper_limit {
                    upper
                } else {
                    middle
                }
            }
        }
    }

    fn min_clipped(&self) -> Option<i32> {
        match *self {
            Self::Two { min_clipped, .. } | Self::Three { min_clipped, .. } => min_clipped,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Decorator {
    Beehive(f32),
    LeaveVine(f32),
    TrunkVine,
    Cocoa(f32),
    AlterGround(StateProvider),
    AttachedToLeaves {
        probability: f32,
        xz: i32,
        y: i32,
        provider: StateProvider,
        required_empty: i32,
        directions: Vec<Direction>,
    },
}

#[derive(Clone, Debug)]
pub struct MangroveRoots {
    trunk_offset_y: IntProvider,
    root_provider: StateProvider,
    above_root: Option<(f32, StateProvider)>,
    can_grow_through: BlockSet,
    muddy_roots_in: BlockSet,
    muddy_roots_provider: StateProvider,
    max_root_width: i32,
    max_root_length: i32,
    random_skew_chance: f32,
}

#[derive(Clone, Debug)]
pub struct TreeConfig {
    trunk_provider: StateProvider,
    trunk: Trunk,
    foliage_provider: StateProvider,
    foliage: Foliage,
    root: Option<MangroveRoots>,
    dirt_provider: StateProvider,
    min_size: MinSize,
    decorators: Vec<Decorator>,
    ignore_vines: bool,
    force_dirt: bool,
}

fn ip(v: &Value) -> IntProvider {
    IntProvider::from_json(v)
}

fn i(v: &Value, k: &str) -> i32 {
    v[k].as_i64().unwrap_or(0) as i32
}

fn holder_set(v: &Value) -> BlockSet {
    let t = rc_data::tags();
    match v {
        Value::String(s) if s.starts_with('#') => t.block(s),
        Value::String(s) => {
            let mut b = t.block("#minecraft:__none__");
            b.insert(rc_data::blocks().index_of(s).unwrap());
            b
        }
        Value::Array(a) => {
            let mut b = t.block("#minecraft:__none__");
            for x in a {
                b.insert(rc_data::blocks().index_of(x.as_str().unwrap()).unwrap());
            }
            b
        }
        _ => t.block("#minecraft:__none__"),
    }
}

impl TreeConfig {
    pub fn from_json(c: &Value) -> Self {
        let tp = &c["trunk_placer"];
        let tkind = match tp["type"]
            .as_str()
            .unwrap()
            .trim_start_matches("minecraft:")
        {
            "straight_trunk_placer" => TrunkKind::Straight,
            "forking_trunk_placer" => TrunkKind::Forking,
            "giant_trunk_placer" => TrunkKind::Giant,
            "mega_jungle_trunk_placer" => TrunkKind::MegaJungle,
            "dark_oak_trunk_placer" => TrunkKind::DarkOak,
            "fancy_trunk_placer" => TrunkKind::Fancy,
            "bending_trunk_placer" => TrunkKind::Bending {
                min_height_for_leaves: tp
                    .get("min_height_for_leaves")
                    .and_then(Value::as_i64)
                    .unwrap_or(1) as i32,
                bend_length: ip(&tp["bend_length"]),
            },
            "upwards_branching_trunk_placer" => TrunkKind::UpwardsBranching {
                extra_branch_steps: ip(&tp["extra_branch_steps"]),
                branch_prob: tp["place_branch_per_log_probability"].as_f64().unwrap() as f32,
                extra_branch_length: ip(&tp["extra_branch_length"]),
                can_grow_through: holder_set(&tp["can_grow_through"]),
            },
            "cherry_trunk_placer" => {
                let so = &tp["branch_start_offset_from_top"];
                TrunkKind::Cherry {
                    branch_count: ip(&tp["branch_count"]),
                    branch_horizontal_length: ip(&tp["branch_horizontal_length"]),
                    branch_start_offset: (i(so, "min_inclusive"), i(so, "max_inclusive")),
                    branch_end_offset: ip(&tp["branch_end_offset_from_top"]),
                }
            }
            t => panic!("unknown trunk placer {t}"),
        };
        let trunk = Trunk {
            base: i(tp, "base_height"),
            a: i(tp, "height_rand_a"),
            b: i(tp, "height_rand_b"),
            kind: tkind,
        };
        let fp = &c["foliage_placer"];
        let fkind = match fp["type"]
            .as_str()
            .unwrap()
            .trim_start_matches("minecraft:")
        {
            "blob_foliage_placer" => FoliageKind::Blob {
                height: i(fp, "height"),
            },
            "fancy_foliage_placer" => FoliageKind::Fancy {
                height: i(fp, "height"),
            },
            "bush_foliage_placer" => FoliageKind::Bush {
                height: i(fp, "height"),
            },
            "spruce_foliage_placer" => FoliageKind::Spruce {
                trunk_height: ip(&fp["trunk_height"]),
            },
            "pine_foliage_placer" => FoliageKind::Pine {
                height: ip(&fp["height"]),
            },
            "acacia_foliage_placer" => FoliageKind::Acacia,
            "jungle_foliage_placer" => FoliageKind::Jungle {
                height: i(fp, "height"),
            },
            "mega_pine_foliage_placer" => FoliageKind::MegaPine {
                crown_height: ip(&fp["crown_height"]),
            },
            "dark_oak_foliage_placer" => FoliageKind::DarkOak,
            "random_spread_foliage_placer" => FoliageKind::RandomSpread {
                foliage_height: ip(&fp["foliage_height"]),
                attempts: i(fp, "leaf_placement_attempts"),
            },
            "cherry_foliage_placer" => FoliageKind::Cherry {
                height: ip(&fp["height"]),
                wide_bottom_layer_hole_chance: fp["wide_bottom_layer_hole_chance"].as_f64().unwrap()
                    as f32,
                corner_hole_chance: fp["corner_hole_chance"].as_f64().unwrap() as f32,
                hanging_leaves_chance: fp["hanging_leaves_chance"].as_f64().unwrap() as f32,
                hanging_leaves_extension_chance: fp["hanging_leaves_extension_chance"]
                    .as_f64()
                    .unwrap() as f32,
            },
            t => panic!("unknown foliage placer {t}"),
        };
        let foliage = Foliage {
            radius: ip(&fp["radius"]),
            offset: ip(&fp["offset"]),
            kind: fkind,
        };
        let ms = &c["minimum_size"];
        let min_clipped = ms
            .get("min_clipped_height")
            .and_then(Value::as_i64)
            .map(|v| v as i32);
        let min_size = match ms["type"]
            .as_str()
            .unwrap()
            .trim_start_matches("minecraft:")
        {
            "two_layers_feature_size" => MinSize::Two {
                limit: ms.get("limit").and_then(Value::as_i64).unwrap_or(1) as i32,
                lower: ms.get("lower_size").and_then(Value::as_i64).unwrap_or(0) as i32,
                upper: ms.get("upper_size").and_then(Value::as_i64).unwrap_or(1) as i32,
                min_clipped,
            },
            _ => MinSize::Three {
                limit: ms.get("limit").and_then(Value::as_i64).unwrap_or(1) as i32,
                upper_limit: ms.get("upper_limit").and_then(Value::as_i64).unwrap_or(1) as i32,
                lower: ms.get("lower_size").and_then(Value::as_i64).unwrap_or(0) as i32,
                middle: ms.get("middle_size").and_then(Value::as_i64).unwrap_or(1) as i32,
                upper: ms.get("upper_size").and_then(Value::as_i64).unwrap_or(1) as i32,
                min_clipped,
            },
        };
        let decorators = c["decorators"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(
                        |d| match d["type"].as_str().unwrap().trim_start_matches("minecraft:") {
                            "beehive" => {
                                Decorator::Beehive(d["probability"].as_f64().unwrap() as f32)
                            }
                            "leave_vine" => {
                                Decorator::LeaveVine(d["probability"].as_f64().unwrap() as f32)
                            }
                            "trunk_vine" => Decorator::TrunkVine,
                            "cocoa" => Decorator::Cocoa(d["probability"].as_f64().unwrap() as f32),
                            "alter_ground" => {
                                Decorator::AlterGround(StateProvider::from_json(&d["provider"]))
                            }
                            "attached_to_leaves" => Decorator::AttachedToLeaves {
                                probability: d["probability"].as_f64().unwrap() as f32,
                                xz: i(d, "exclusion_radius_xz"),
                                y: i(d, "exclusion_radius_y"),
                                provider: StateProvider::from_json(&d["block_provider"]),
                                required_empty: i(d, "required_empty_blocks"),
                                directions: d["directions"]
                                    .as_array()
                                    .unwrap()
                                    .iter()
                                    .map(|x| super::predicate::direction(x.as_str().unwrap()))
                                    .collect(),
                            },
                            t => panic!("unknown tree decorator {t}"),
                        },
                    )
                    .collect()
            })
            .unwrap_or_default();
        let root = c.get("root_placer").filter(|r| !r.is_null()).map(|rp| {
            let m = &rp["mangrove_root_placement"];
            MangroveRoots {
                trunk_offset_y: ip(&rp["trunk_offset_y"]),
                root_provider: StateProvider::from_json(&rp["root_provider"]),
                above_root: rp.get("above_root_placement").map(|a| {
                    (
                        a["above_root_placement_chance"].as_f64().unwrap() as f32,
                        StateProvider::from_json(&a["above_root_provider"]),
                    )
                }),
                can_grow_through: holder_set(&m["can_grow_through"]),
                muddy_roots_in: holder_set(&m["muddy_roots_in"]),
                muddy_roots_provider: StateProvider::from_json(&m["muddy_roots_provider"]),
                max_root_width: i(m, "max_root_width"),
                max_root_length: i(m, "max_root_length"),
                random_skew_chance: m["random_skew_chance"].as_f64().unwrap() as f32,
            }
        });
        Self {
            trunk_provider: StateProvider::from_json(&c["trunk_provider"]),
            trunk,
            foliage_provider: StateProvider::from_json(&c["foliage_provider"]),
            foliage,
            root,
            dirt_provider: StateProvider::from_json(&c["dirt_provider"]),
            min_size,
            decorators,
            ignore_vines: c["ignore_vines"].as_bool().unwrap_or(false),
            force_dirt: c["force_dirt"].as_bool().unwrap_or(false),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Attachment {
    pos: BlockPos,
    radius_offset: i32,
    double_trunk: bool,
}

/// Mutable state of one tree placement.
struct TreeGen<'c, 'r, 'a> {
    ctx: &'c mut Ctx<'r, 'a>,
    cfg: &'c TreeConfig,
    logs: JavaPosSet,
    roots: JavaPosSet,
    leaves: JavaPosSet,
    decorations: JavaPosSet,
}

impl TreeGen<'_, '_, '_> {
    fn get(&self, p: BlockPos) -> BlockStateId {
        self.ctx.region.get(p)
    }

    fn set(&mut self, p: BlockPos, s: BlockStateId) {
        self.ctx.region.set(p, s);
    }

    fn valid_tree_pos(&self, p: BlockPos) -> bool {
        let h = bh();
        let s = self.get(p);
        h.is_air(s) || h.replaceable_by_trees.contains(h.block(s))
    }

    fn trunk_valid_pos(&self, p: BlockPos) -> bool {
        if self.valid_tree_pos(p) {
            return true;
        }
        if let TrunkKind::UpwardsBranching {
            can_grow_through, ..
        } = &self.cfg.trunk.kind
        {
            return can_grow_through.contains(bh().block(self.get(p)));
        }
        false
    }

    fn is_free(&self, p: BlockPos) -> bool {
        let h = bh();
        self.trunk_valid_pos(p) || h.logs.contains(h.block(self.get(p)))
    }

    fn is_air_or_leaves(&self, p: BlockPos) -> bool {
        let h = bh();
        let s = self.get(p);
        h.is_air(s) || h.leaves.contains(h.block(s))
    }

    fn place_log(&mut self, r: &mut Random, p: BlockPos) -> bool {
        self.place_log_with(r, p, None)
    }

    fn place_log_with(&mut self, r: &mut Random, p: BlockPos, axis: Option<&str>) -> bool {
        if !self.trunk_valid_pos(p) {
            return false;
        }
        let mut s = self.cfg.trunk_provider.get(r, p);
        if let Some(a) = axis {
            s = rc_data::blocks().with(s, "axis", a);
        }
        self.logs.insert(p);
        self.set(p, s);
        true
    }

    fn place_log_if_free(&mut self, r: &mut Random, p: BlockPos) {
        if self.is_free(p) {
            self.place_log(r, p);
        }
    }

    fn set_dirt_at(&mut self, r: &mut Random, p: BlockPos) {
        let h = bh();
        let s = self.get(p);
        let bi = h.block(s);
        let is_dirt = h.dirt.contains(bi) && bi != h.idx.grass_block && bi != h.idx.mycelium;
        if self.cfg.force_dirt || !is_dirt {
            let d = self.cfg.dirt_provider.get(r, p);
            self.logs.insert(p);
            self.set(p, d);
        }
    }

    fn try_place_leaf(&mut self, r: &mut Random, p: BlockPos) -> bool {
        if !self.valid_tree_pos(p) {
            return false;
        }
        let b = rc_data::blocks();
        let mut s = self.cfg.foliage_provider.get(r, p);
        if b.prop_pos(s, "waterlogged").is_some() {
            let f = self.ctx.region.fluid(p);
            s = b.with(
                s,
                "waterlogged",
                if f.is_water() && f.is_source() {
                    "true"
                } else {
                    "false"
                },
            );
        }
        self.leaves.insert(p);
        self.set(p, s);
        true
    }

    // ---- trunk placers ------------------------------------------------------

    fn tree_height(&self, r: &mut Random) -> i32 {
        let t = &self.cfg.trunk;
        t.base + r.next_int_bound(t.a + 1) + r.next_int_bound(t.b + 1)
    }

    fn place_trunk(&mut self, r: &mut Random, height: i32, pos: BlockPos) -> Vec<Attachment> {
        let below = BlockPos::new(pos.x, pos.y - 1, pos.z);
        let at = |x: i32, y: i32, z: i32| BlockPos::new(x, y, z);
        let cfg = self.cfg;
        match &cfg.trunk.kind {
            TrunkKind::Straight => {
                self.set_dirt_at(r, below);
                for j in 0..height {
                    self.place_log(r, at(pos.x, pos.y + j, pos.z));
                }
                vec![Attachment {
                    pos: at(pos.x, pos.y + height, pos.z),
                    radius_offset: 0,
                    double_trunk: false,
                }]
            }
            TrunkKind::Forking => {
                self.set_dirt_at(r, below);
                let mut list = Vec::new();
                let d = random_horizontal(r);
                let j = height - r.next_int_bound(4) - 1;
                let mut k = 3 - r.next_int_bound(3);
                let mut l = pos.x;
                let mut m = pos.z;
                let mut oi: Option<i32> = None;
                for n in 0..height {
                    let o = pos.y + n;
                    if n >= j && k > 0 {
                        l += step(d).0;
                        m += step(d).1;
                        k -= 1;
                    }
                    if self.place_log(r, at(l, o, m)) {
                        oi = Some(o + 1);
                    }
                }
                if let Some(y) = oi {
                    list.push(Attachment {
                        pos: at(l, y, m),
                        radius_offset: 1,
                        double_trunk: false,
                    });
                }
                l = pos.x;
                m = pos.z;
                let d2 = random_horizontal(r);
                if d2 != d {
                    let o = j - r.next_int_bound(2) - 1;
                    let mut p = 1 + r.next_int_bound(3);
                    oi = None;
                    let mut q = o;
                    while q < height && p > 0 {
                        if q >= 1 {
                            let rr = pos.y + q;
                            l += step(d2).0;
                            m += step(d2).1;
                            if self.place_log(r, at(l, rr, m)) {
                                oi = Some(rr + 1);
                            }
                        }
                        q += 1;
                        p -= 1;
                    }
                    if let Some(y) = oi {
                        list.push(Attachment {
                            pos: at(l, y, m),
                            radius_offset: 0,
                            double_trunk: false,
                        });
                    }
                }
                list
            }
            TrunkKind::Giant | TrunkKind::MegaJungle => {
                let mut list = self.giant_trunk(r, height, pos);
                if let TrunkKind::MegaJungle = cfg.trunk.kind {
                    let mut j = height - 2 - r.next_int_bound(4);
                    while j > height / 2 {
                        let f = r.next_float() * (std::f32::consts::PI * 2.0);
                        let mut k = 0;
                        let mut l = 0;
                        for m in 0..5 {
                            k = (1.5 + mth::cos(f) * m as f32) as i32;
                            l = (1.5 + mth::sin(f) * m as f32) as i32;
                            self.place_log(r, at(pos.x + k, pos.y + j - 3 + m / 2, pos.z + l));
                        }
                        list.push(Attachment {
                            pos: at(pos.x + k, pos.y + j, pos.z + l),
                            radius_offset: -2,
                            double_trunk: false,
                        });
                        j -= 2 + r.next_int_bound(4);
                    }
                }
                list
            }
            TrunkKind::DarkOak => {
                let mut list = Vec::new();
                self.set_dirt_at(r, below);
                self.set_dirt_at(r, at(below.x + 1, below.y, below.z));
                self.set_dirt_at(r, at(below.x, below.y, below.z + 1));
                self.set_dirt_at(r, at(below.x + 1, below.y, below.z + 1));
                let d = random_horizontal(r);
                let j = height - r.next_int_bound(4);
                let mut k = 2 - r.next_int_bound(3);
                let (l, m, n) = (pos.x, pos.y, pos.z);
                let mut o = l;
                let mut p = n;
                let q = m + height - 1;
                for rr in 0..height {
                    if rr >= j && k > 0 {
                        o += step(d).0;
                        p += step(d).1;
                        k -= 1;
                    }
                    let bp = at(o, m + rr, p);
                    if self.is_air_or_leaves(bp) {
                        self.place_log(r, bp);
                        self.place_log(r, at(bp.x + 1, bp.y, bp.z));
                        self.place_log(r, at(bp.x, bp.y, bp.z + 1));
                        self.place_log(r, at(bp.x + 1, bp.y, bp.z + 1));
                    }
                }
                list.push(Attachment {
                    pos: at(o, q, p),
                    radius_offset: 0,
                    double_trunk: true,
                });
                for rr in -1..=2 {
                    for s in -1..=2 {
                        if ((0..=1).contains(&rr) && (0..=1).contains(&s))
                            || r.next_int_bound(3) > 0
                        {
                            continue;
                        }
                        let t = r.next_int_bound(3) + 2;
                        for v in 0..t {
                            self.place_log(r, at(l + rr, q - v - 1, n + s));
                        }
                        list.push(Attachment {
                            pos: at(l + rr, q, n + s),
                            radius_offset: 0,
                            double_trunk: false,
                        });
                    }
                }
                list
            }
            TrunkKind::Fancy => self.fancy_trunk(r, height, pos),
            TrunkKind::Bending {
                min_height_for_leaves,
                bend_length,
            } => {
                let d = random_horizontal(r);
                let j = height - 1;
                let mut mp = pos;
                self.set_dirt_at(r, below);
                let mut list = Vec::new();
                for k in 0..=j {
                    if k + 1 >= j + r.next_int_bound(2) {
                        mp = mp.offset(d);
                    }
                    if self.valid_tree_pos(mp) {
                        self.place_log(r, mp);
                    }
                    if k >= *min_height_for_leaves {
                        list.push(Attachment {
                            pos: mp,
                            radius_offset: 0,
                            double_trunk: false,
                        });
                    }
                    mp = mp.offset(Direction::Up);
                }
                let k = bend_length.sample(r);
                for _ in 0..=k {
                    if self.valid_tree_pos(mp) {
                        self.place_log(r, mp);
                    }
                    list.push(Attachment {
                        pos: mp,
                        radius_offset: 0,
                        double_trunk: false,
                    });
                    mp = mp.offset(d);
                }
                list
            }
            TrunkKind::UpwardsBranching {
                extra_branch_steps,
                branch_prob,
                extra_branch_length,
                ..
            } => {
                let mut list = Vec::new();
                for j in 0..height {
                    let k = pos.y + j;
                    let mp = at(pos.x, k, pos.z);
                    if self.place_log(r, mp) && j < height - 1 && r.next_float() < *branch_prob {
                        let d = random_horizontal(r);
                        let l = extra_branch_length.sample(r);
                        let m = (l - extra_branch_length.sample(r) - 1).max(0);
                        let n = extra_branch_steps.sample(r);
                        self.place_branch(r, height, &mut list, mp, k, d, m, n);
                    }
                    if j == height - 1 {
                        list.push(Attachment {
                            pos: at(pos.x, k + 1, pos.z),
                            radius_offset: 0,
                            double_trunk: false,
                        });
                    }
                }
                list
            }
            TrunkKind::Cherry { .. } => self.cherry_trunk(r, height, pos),
        }
    }

    fn giant_trunk(&mut self, r: &mut Random, height: i32, pos: BlockPos) -> Vec<Attachment> {
        let bp = BlockPos::new(pos.x, pos.y - 1, pos.z);
        self.set_dirt_at(r, bp);
        self.set_dirt_at(r, BlockPos::new(bp.x + 1, bp.y, bp.z));
        self.set_dirt_at(r, BlockPos::new(bp.x, bp.y, bp.z + 1));
        self.set_dirt_at(r, BlockPos::new(bp.x + 1, bp.y, bp.z + 1));
        for j in 0..height {
            self.place_log_if_free(r, BlockPos::new(pos.x, pos.y + j, pos.z));
            if j < height - 1 {
                self.place_log_if_free(r, BlockPos::new(pos.x + 1, pos.y + j, pos.z));
                self.place_log_if_free(r, BlockPos::new(pos.x + 1, pos.y + j, pos.z + 1));
                self.place_log_if_free(r, BlockPos::new(pos.x, pos.y + j, pos.z + 1));
            }
        }
        vec![Attachment {
            pos: BlockPos::new(pos.x, pos.y + height, pos.z),
            radius_offset: 0,
            double_trunk: true,
        }]
    }

    #[allow(clippy::too_many_arguments)]
    fn place_branch(
        &mut self,
        r: &mut Random,
        height: i32,
        list: &mut Vec<Attachment>,
        mp: BlockPos,
        j: i32,
        d: Direction,
        k: i32,
        mut l: i32,
    ) {
        let mut m = j + k;
        let mut n = mp.x;
        let mut o = mp.z;
        let mut p = k;
        while p < height && l > 0 {
            if p >= 1 {
                let q = j + p;
                m = q;
                n += step(d).0;
                o += step(d).1;
                let bp = BlockPos::new(n, q, o);
                if self.place_log(r, bp) {
                    m += 1;
                }
                list.push(Attachment {
                    pos: bp,
                    radius_offset: 0,
                    double_trunk: false,
                });
            }
            p += 1;
            l -= 1;
        }
        if m - j > 1 {
            let bp = BlockPos::new(n, m, o);
            list.push(Attachment {
                pos: bp,
                radius_offset: 0,
                double_trunk: false,
            });
            list.push(Attachment {
                pos: BlockPos::new(n, m - 2, o),
                radius_offset: 0,
                double_trunk: false,
            });
        }
    }

    fn fancy_trunk(&mut self, r: &mut Random, height: i32, pos: BlockPos) -> Vec<Attachment> {
        let l = height + 2;
        let m = mth::floor(f64::from(l) * 0.618);
        self.set_dirt_at(r, BlockPos::new(pos.x, pos.y - 1, pos.z));
        let n = 1.min(mth::floor(1.382 + (f64::from(l) / 13.0).powi(2)));
        let o = pos.y + m;
        // (attachment pos, branch base)
        let mut list: Vec<(BlockPos, i32)> = vec![(BlockPos::new(pos.x, pos.y + m, pos.z), o)];
        let mut k = l - 5;
        while k >= 0 {
            let f = tree_shape(l, k);
            if f >= 0.0 {
                for _ in 0..n {
                    let g = f64::from(f) * (f64::from(r.next_float()) + 0.328);
                    let h = f64::from(r.next_float() * 2.0) * std::f64::consts::PI;
                    let q = g * h.sin() + 0.5;
                    let rr = g * h.cos() + 0.5;
                    let bp =
                        BlockPos::new(pos.x + mth::floor(q), pos.y + k - 1, pos.z + mth::floor(rr));
                    let bp2 = BlockPos::new(bp.x, bp.y + 5, bp.z);
                    if !self.make_limb(r, bp, bp2, false) {
                        continue;
                    }
                    let s = pos.x - bp.x;
                    let t = pos.z - bp.z;
                    let u = f64::from(bp.y) - f64::from(s * s + t * t).sqrt() * 0.381;
                    let v = if u > f64::from(o) { o } else { u as i32 };
                    let bp3 = BlockPos::new(pos.x, v, pos.z);
                    if !self.make_limb(r, bp3, bp, false) {
                        continue;
                    }
                    list.push((bp, bp3.y));
                }
            }
            k -= 1;
        }
        self.make_limb(r, pos, BlockPos::new(pos.x, pos.y + m, pos.z), true);
        // makeBranches
        for &(apos, base) in &list {
            let bp = BlockPos::new(pos.x, base, pos.z);
            if bp == apos || !trim_branches(l, base - pos.y) {
                continue;
            }
            self.make_limb(r, bp, apos, true);
        }
        list.iter()
            .filter(|(_, base)| trim_branches(l, base - pos.y))
            .map(|&(p, _)| Attachment {
                pos: p,
                radius_offset: 0,
                double_trunk: false,
            })
            .collect()
    }

    fn make_limb(&mut self, r: &mut Random, a: BlockPos, b: BlockPos, place: bool) -> bool {
        if !place && a == b {
            return true;
        }
        let d = (b.x - a.x, b.y - a.y, b.z - a.z);
        let i = d.0.abs().max(d.1.abs()).max(d.2.abs());
        let f = d.0 as f32 / i as f32;
        let g = d.1 as f32 / i as f32;
        let h = d.2 as f32 / i as f32;
        for j in 0..=i {
            let bp2 = BlockPos::new(
                a.x + mth::floor(f64::from(0.5 + j as f32 * f)),
                a.y + mth::floor(f64::from(0.5 + j as f32 * g)),
                a.z + mth::floor(f64::from(0.5 + j as f32 * h)),
            );
            if place {
                let ax = log_axis(a, bp2);
                self.place_log_with(r, bp2, Some(ax));
                continue;
            }
            if !self.is_free(bp2) {
                return false;
            }
        }
        true
    }

    fn cherry_trunk(&mut self, r: &mut Random, i: i32, pos: BlockPos) -> Vec<Attachment> {
        let TrunkKind::Cherry {
            branch_count,
            branch_horizontal_length,
            branch_start_offset,
            branch_end_offset,
        } = self.cfg.trunk.kind.clone()
        else {
            unreachable!()
        };
        self.set_dirt_at(r, BlockPos::new(pos.x, pos.y - 1, pos.z));
        let j = (i - 1 + r.next_int_between(branch_start_offset.0, branch_start_offset.1)).max(0);
        let mut k =
            (i - 1 + r.next_int_between(branch_start_offset.0, branch_start_offset.1 - 1)).max(0);
        if k >= j {
            k += 1;
        }
        let l = branch_count.sample(r);
        let bl = l == 3;
        let bl2 = l >= 2;
        let m = if bl {
            i
        } else if bl2 {
            j.max(k) + 1
        } else {
            j + 1
        };
        for n in 0..m {
            self.place_log(r, BlockPos::new(pos.x, pos.y + n, pos.z));
        }
        let mut list = Vec::new();
        if bl {
            list.push(Attachment {
                pos: BlockPos::new(pos.x, pos.y + m, pos.z),
                radius_offset: 0,
                double_trunk: false,
            });
        }
        let d = random_horizontal(r);
        let axis = if matches!(d, Direction::East | Direction::West) {
            "x"
        } else {
            "z"
        };
        let a = self.cherry_branch(
            r,
            i,
            pos,
            axis,
            d,
            j,
            j < m - 1,
            &branch_horizontal_length,
            &branch_end_offset,
        );
        list.push(a);
        if bl2 {
            let b = self.cherry_branch(
                r,
                i,
                pos,
                axis,
                d.opposite(),
                k,
                k < m - 1,
                &branch_horizontal_length,
                &branch_end_offset,
            );
            list.push(b);
        }
        list
    }

    #[allow(clippy::too_many_arguments)]
    fn cherry_branch(
        &mut self,
        r: &mut Random,
        i: i32,
        pos: BlockPos,
        axis: &str,
        d: Direction,
        j: i32,
        bl: bool,
        horiz: &IntProvider,
        end_off: &IntProvider,
    ) -> Attachment {
        let mut mp = BlockPos::new(pos.x, pos.y + j, pos.z);
        let k = i - 1 + end_off.sample(r);
        let bl2 = bl || k < j;
        let l = horiz.sample(r) + i32::from(bl2);
        let (sx, sz) = step(d);
        let bp = BlockPos::new(pos.x + sx * l, pos.y + k, pos.z + sz * l);
        let m = if bl2 { 2 } else { 1 };
        for _ in 0..m {
            mp = mp.offset(d);
            self.place_log_with(r, mp, Some(axis));
        }
        let d2 = if bp.y > mp.y {
            Direction::Up
        } else {
            Direction::Down
        };
        loop {
            let o = (bp.x - mp.x).abs() + (bp.y - mp.y).abs() + (bp.z - mp.z).abs();
            if o == 0 {
                break;
            }
            let g = (bp.y - mp.y).abs() as f32 / o as f32;
            let vertical = r.next_float() < g;
            mp = mp.offset(if vertical { d2 } else { d });
            if vertical {
                self.place_log(r, mp);
            } else {
                self.place_log_with(r, mp, Some(axis));
            }
        }
        Attachment {
            pos: BlockPos::new(bp.x, bp.y + 1, bp.z),
            radius_offset: 0,
            double_trunk: false,
        }
    }

    // ---- foliage placers ---------------------------------------------------

    fn foliage_height(&self, r: &mut Random, height: i32) -> i32 {
        match &self.cfg.foliage.kind {
            FoliageKind::Blob { height }
            | FoliageKind::Fancy { height }
            | FoliageKind::Bush { height }
            | FoliageKind::Jungle { height } => *height,
            FoliageKind::Spruce { trunk_height } => 4.max(height - trunk_height.sample(r)),
            FoliageKind::Pine { height } => height.sample(r),
            FoliageKind::Acacia => 0,
            FoliageKind::MegaPine { crown_height } => crown_height.sample(r),
            FoliageKind::DarkOak => 4,
            FoliageKind::RandomSpread { foliage_height, .. } => foliage_height.sample(r),
            FoliageKind::Cherry { height, .. } => height.sample(r),
        }
    }

    fn foliage_radius(&self, r: &mut Random, i: i32) -> i32 {
        let base = self.cfg.foliage.radius.sample(r);
        if let FoliageKind::Pine { .. } = self.cfg.foliage.kind {
            base + r.next_int_bound((i + 1).max(1))
        } else {
            base
        }
    }

    fn should_skip(&self, r: &mut Random, i: i32, j: i32, k: i32, l: i32, dt: bool) -> bool {
        match &self.cfg.foliage.kind {
            FoliageKind::Blob { .. } => i == l && k == l && (r.next_int_bound(2) == 0 || j == 0),
            FoliageKind::Fancy { .. } => {
                mth::square(f64::from(i as f32 + 0.5)) as f32
                    + mth::square(f64::from(k as f32 + 0.5)) as f32
                    > (l * l) as f32
            }
            FoliageKind::Bush { .. } => i == l && k == l && r.next_int_bound(2) == 0,
            FoliageKind::Spruce { .. } | FoliageKind::Pine { .. } => i == l && k == l && l > 0,
            FoliageKind::Acacia => {
                if j == 0 {
                    (i > 1 || k > 1) && i != 0 && k != 0
                } else {
                    i == l && k == l && l > 0
                }
            }
            FoliageKind::Jungle { .. } | FoliageKind::MegaPine { .. } => {
                i + k >= 7 || i * i + k * k > l * l
            }
            FoliageKind::DarkOak => {
                if j == -1 && !dt {
                    i == l && k == l
                } else if j == 1 {
                    i + k > l * 2 - 2
                } else {
                    false
                }
            }
            FoliageKind::RandomSpread { .. } => false,
            FoliageKind::Cherry {
                wide_bottom_layer_hole_chance,
                corner_hole_chance,
                ..
            } => {
                if j == -1 && (i == l || k == l) && r.next_float() < *wide_bottom_layer_hole_chance
                {
                    return true;
                }
                let corner = i == l && k == l;
                let wide = l > 2;
                if wide {
                    corner || (i + k > l * 2 - 2 && r.next_float() < *corner_hole_chance)
                } else {
                    corner && r.next_float() < *corner_hole_chance
                }
            }
        }
    }

    fn should_skip_signed(&self, r: &mut Random, l: i32, j: i32, m: i32, k: i32, dt: bool) -> bool {
        if let FoliageKind::DarkOak = self.cfg.foliage.kind
            && j == 0
            && dt
            && (l == -k || l >= k)
            && (m == -k || m >= k)
        {
            return true;
        }
        let (n, o) = if dt {
            ((l.abs()).min((l - 1).abs()), (m.abs()).min((m - 1).abs()))
        } else {
            (l.abs(), m.abs())
        };
        self.should_skip(r, n, j, o, k, dt)
    }

    fn leaves_row(&mut self, r: &mut Random, pos: BlockPos, radius: i32, y: i32, dt: bool) {
        let k = i32::from(dt);
        for l in -radius..=radius + k {
            for m in -radius..=radius + k {
                if self.should_skip_signed(r, l, y, m, radius, dt) {
                    continue;
                }
                self.try_place_leaf(r, BlockPos::new(pos.x + l, pos.y + y, pos.z + m));
            }
        }
    }

    fn create_foliage(&mut self, r: &mut Random, att: Attachment, j: i32, k: i32) {
        let l = self.cfg.foliage.offset.sample(r);
        let p = att.pos;
        let dt = att.double_trunk;
        match self.cfg.foliage.kind.clone() {
            FoliageKind::Blob { .. } => {
                let mut m = l;
                while m >= l - j {
                    let n = (k + att.radius_offset - 1 - m / 2).max(0);
                    self.leaves_row(r, p, n, m, dt);
                    m -= 1;
                }
            }
            FoliageKind::Fancy { .. } => {
                let mut m = l;
                while m >= l - j {
                    let n = k + if m == l || m == l - j { 0 } else { 1 };
                    self.leaves_row(r, p, n, m, dt);
                    m -= 1;
                }
            }
            FoliageKind::Bush { .. } => {
                let mut m = l;
                while m >= l - j {
                    let n = k + att.radius_offset - 1 - m;
                    self.leaves_row(r, p, n, m, dt);
                    m -= 1;
                }
            }
            FoliageKind::Spruce { .. } => {
                let mut m = r.next_int_bound(2);
                let mut n = 1;
                let mut o = 0;
                let mut q = l;
                while q >= -j {
                    self.leaves_row(r, p, m, q, dt);
                    if m >= n {
                        m = o;
                        o = 1;
                        n = (n + 1).min(k + att.radius_offset);
                    } else {
                        m += 1;
                    }
                    q -= 1;
                }
            }
            FoliageKind::Pine { .. } => {
                let mut m = 0;
                let mut n = l;
                while n >= l - j {
                    self.leaves_row(r, p, m, n, dt);
                    if m >= 1 && n == l - j + 1 {
                        m -= 1;
                    } else if m < k + att.radius_offset {
                        m += 1;
                    }
                    n -= 1;
                }
            }
            FoliageKind::Acacia => {
                let bp = BlockPos::new(p.x, p.y + l, p.z);
                self.leaves_row(r, bp, k + att.radius_offset, -1 - j, dt);
                self.leaves_row(r, bp, k - 1, -j, dt);
                self.leaves_row(r, bp, k + att.radius_offset - 1, 0, dt);
            }
            FoliageKind::Jungle { .. } => {
                let m = if dt { j } else { 1 + r.next_int_bound(2) };
                let mut n = l;
                while n >= l - m {
                    let o = k + att.radius_offset + 1 - n;
                    self.leaves_row(r, p, o, n, dt);
                    n -= 1;
                }
            }
            FoliageKind::MegaPine { .. } => {
                let mut m = 0;
                for n in (p.y - j + l)..=(p.y + l) {
                    let o = p.y - n;
                    let pp =
                        k + att.radius_offset + mth::floor(f64::from(o as f32 / j as f32 * 3.5));
                    let q = if o > 0 && pp == m && n & 1 == 0 {
                        pp + 1
                    } else {
                        pp
                    };
                    self.leaves_row(r, BlockPos::new(p.x, n, p.z), q, 0, dt);
                    m = pp;
                }
            }
            FoliageKind::DarkOak => {
                let bp = BlockPos::new(p.x, p.y + l, p.z);
                if dt {
                    self.leaves_row(r, bp, k + 2, -1, dt);
                    self.leaves_row(r, bp, k + 3, 0, dt);
                    self.leaves_row(r, bp, k + 2, 1, dt);
                    if r.next_bool() {
                        self.leaves_row(r, bp, k, 2, dt);
                    }
                } else {
                    self.leaves_row(r, bp, k + 2, -1, dt);
                    self.leaves_row(r, bp, k + 1, 0, dt);
                }
            }
            FoliageKind::RandomSpread { attempts, .. } => {
                for _ in 0..attempts {
                    let dx = r.next_int_bound(k) - r.next_int_bound(k);
                    let dy = r.next_int_bound(j) - r.next_int_bound(j);
                    let dz = r.next_int_bound(k) - r.next_int_bound(k);
                    self.try_place_leaf(r, BlockPos::new(p.x + dx, p.y + dy, p.z + dz));
                }
            }
            FoliageKind::Cherry {
                hanging_leaves_chance,
                hanging_leaves_extension_chance,
                ..
            } => {
                let bp = BlockPos::new(p.x, p.y + l, p.z);
                let m = k + att.radius_offset - 1;
                self.leaves_row(r, bp, m - 2, j - 3, dt);
                self.leaves_row(r, bp, m - 1, j - 4, dt);
                let mut n = j - 5;
                while n >= 0 {
                    self.leaves_row(r, bp, m, n, dt);
                    n -= 1;
                }
                self.hanging_row(
                    r,
                    bp,
                    m,
                    -1,
                    dt,
                    hanging_leaves_chance,
                    hanging_leaves_extension_chance,
                );
                self.hanging_row(
                    r,
                    bp,
                    m - 1,
                    -2,
                    dt,
                    hanging_leaves_chance,
                    hanging_leaves_extension_chance,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn hanging_row(
        &mut self,
        r: &mut Random,
        pos: BlockPos,
        i: i32,
        j: i32,
        dt: bool,
        f: f32,
        g: f32,
    ) {
        self.leaves_row(r, pos, i, j, dt);
        let k = i32::from(dt);
        let bp = BlockPos::new(pos.x, pos.y - 1, pos.z);
        for d in HORIZONTAL {
            let d2 = clockwise(d);
            let l = if matches!(d2, Direction::East | Direction::South) {
                i + k
            } else {
                i
            };
            let (sx, sz) = step(d);
            let (tx, tz) = step(d2);
            let mut mp = BlockPos::new(
                pos.x + tx * l - sx * i,
                pos.y + j - 1,
                pos.z + tz * l - sz * i,
            );
            let mut m = -i;
            while m < i + k {
                let up = BlockPos::new(mp.x, mp.y + 1, mp.z);
                let set_above = self.leaves.contains(&up);
                if set_above && self.try_place_extension(r, f, bp, mp) {
                    let down = BlockPos::new(mp.x, mp.y - 1, mp.z);
                    self.try_place_extension(r, g, bp, down);
                }
                m += 1;
                mp = BlockPos::new(mp.x + sx, mp.y, mp.z + sz);
            }
        }
    }

    fn try_place_extension(
        &mut self,
        r: &mut Random,
        f: f32,
        origin: BlockPos,
        p: BlockPos,
    ) -> bool {
        let dist = (p.x - origin.x).abs() + (p.y - origin.y).abs() + (p.z - origin.z).abs();
        if dist >= 7 {
            return false;
        }
        if r.next_float() > f {
            return false;
        }
        self.try_place_leaf(r, p)
    }

    // ---- roots ------------------------------------------------------------

    fn trunk_origin(&self, r: &mut Random, pos: BlockPos) -> BlockPos {
        match &self.cfg.root {
            Some(rp) => BlockPos::new(pos.x, pos.y + rp.trunk_offset_y.sample(r), pos.z),
            None => pos,
        }
    }

    fn place_roots(&mut self, r: &mut Random, pos: BlockPos, trunk_origin: BlockPos) -> bool {
        let Some(rp) = self.cfg.root.clone() else {
            return true;
        };
        let mut list: Vec<BlockPos> = Vec::new();
        let mut mp = pos;
        while mp.y < trunk_origin.y {
            if !self.root_can_place(&rp, mp) {
                return false;
            }
            mp = mp.offset(Direction::Up);
        }
        list.push(BlockPos::new(
            trunk_origin.x,
            trunk_origin.y - 1,
            trunk_origin.z,
        ));
        for d in HORIZONTAL {
            let bp = trunk_origin.offset(d);
            let mut list2 = Vec::new();
            if !self.simulate_roots(&rp, r, bp, d, trunk_origin, &mut list2, 0) {
                return false;
            }
            list.extend(list2);
            list.push(trunk_origin.offset(d));
        }
        for bp in list {
            self.place_root(&rp, r, bp);
        }
        true
    }

    fn root_can_place(&self, rp: &MangroveRoots, p: BlockPos) -> bool {
        self.valid_tree_pos(p) || rp.can_grow_through.contains(bh().block(self.get(p)))
    }

    #[allow(clippy::too_many_arguments)]
    fn simulate_roots(
        &mut self,
        rp: &MangroveRoots,
        r: &mut Random,
        pos: BlockPos,
        d: Direction,
        origin: BlockPos,
        list: &mut Vec<BlockPos>,
        depth: i32,
    ) -> bool {
        let i = rp.max_root_length;
        if depth == i || list.len() > i as usize {
            return false;
        }
        for bp in self.potential_root_positions(rp, pos, d, r, origin) {
            if !self.root_can_place(rp, bp) {
                continue;
            }
            list.push(bp);
            if !self.simulate_roots(rp, r, bp, d, origin, list, depth + 1) {
                return false;
            }
        }
        true
    }

    fn potential_root_positions(
        &self,
        rp: &MangroveRoots,
        pos: BlockPos,
        d: Direction,
        r: &mut Random,
        origin: BlockPos,
    ) -> Vec<BlockPos> {
        let below = BlockPos::new(pos.x, pos.y - 1, pos.z);
        let bp = pos.offset(d);
        let i = (pos.x - origin.x).abs().max((pos.z - origin.z).abs());
        let j = rp.max_root_width;
        let f = rp.random_skew_chance;
        if i > j - 3 && i <= j {
            return if r.next_float() < f {
                vec![below, BlockPos::new(bp.x, bp.y - 1, bp.z)]
            } else {
                vec![below]
            };
        }
        if i > j {
            return vec![below];
        }
        if r.next_float() < f {
            return vec![below];
        }
        if r.next_bool() { vec![bp] } else { vec![below] }
    }

    fn place_root(&mut self, rp: &MangroveRoots, r: &mut Random, p: BlockPos) {
        if !self.root_can_place(rp, p) {
            return;
        }
        let b = rc_data::blocks();
        let mut s = rp.root_provider.get(r, p);
        if rp.muddy_roots_in.contains(bh().block(self.get(p))) {
            s = rp.muddy_roots_provider.get(r, p);
        }
        if b.prop_pos(s, "waterlogged").is_some() {
            let f = self.ctx.region.fluid(p);
            s = b.with(
                s,
                "waterlogged",
                if f.is_water() && f.is_source() {
                    "true"
                } else {
                    "false"
                },
            );
        }
        self.roots.insert(p);
        self.set(p, s);
        if let Some((chance, prov)) = &rp.above_root {
            let above = BlockPos::new(p.x, p.y + 1, p.z);
            if self.valid_tree_pos(above) && r.next_float() < *chance {
                let mut s2 = prov.get(r, above);
                if b.prop_pos(s2, "waterlogged").is_some() {
                    let f = self.ctx.region.fluid(above);
                    s2 = b.with(
                        s2,
                        "waterlogged",
                        if f.is_water() && f.is_source() {
                            "true"
                        } else {
                            "false"
                        },
                    );
                }
                self.roots.insert(above);
                self.set(above, s2);
            }
        }
    }

    // ---- decorators -------------------------------------------------------

    fn deco_set(&mut self, p: BlockPos, s: BlockStateId) {
        self.decorations.insert(p);
        self.set(p, s);
    }

    fn is_air(&self, p: BlockPos) -> bool {
        bh().is_air(self.get(p))
    }

    fn place_vine(&mut self, p: BlockPos, face: &str) {
        let b = rc_data::blocks();
        let v = b.with(b.default_state("vine"), face, "true");
        self.deco_set(p, v);
    }

    fn decorate(&mut self, r: &mut Random) {
        if self.cfg.decorators.is_empty() {
            return;
        }
        let logs = self.logs.sorted_by_y();
        let leaves = self.leaves.sorted_by_y();
        let roots = self.roots.sorted_by_y();
        let b = rc_data::blocks();
        for d in &self.cfg.decorators.clone() {
            match d {
                Decorator::Beehive(p) => {
                    if r.next_float() >= *p {
                        continue;
                    }
                    let i = if !leaves.is_empty() {
                        (leaves[0].y - 1).max(logs[0].y + 1)
                    } else {
                        (logs[0].y + 1 + r.next_int_bound(3)).min(logs[logs.len() - 1].y)
                    };
                    let mut list3: Vec<BlockPos> = logs
                        .iter()
                        .filter(|p| p.y == i)
                        .flat_map(|p| {
                            [Direction::East, Direction::South, Direction::West]
                                .map(|d| p.offset(d))
                        })
                        .collect();
                    if list3.is_empty() {
                        continue;
                    }
                    shuffle(&mut list3, r);
                    let found = list3
                        .iter()
                        .copied()
                        .find(|p| self.is_air(*p) && self.is_air(p.offset(Direction::South)));
                    if let Some(p) = found {
                        let nest = b.with(b.default_state("bee_nest"), "facing", "south");
                        self.deco_set(p, nest);
                        let _bees = 2 + r.next_int_bound(2);
                    }
                }
                Decorator::LeaveVine(p) => {
                    for &lp in &leaves {
                        for (dir, face) in [
                            (Direction::West, "east"),
                            (Direction::East, "west"),
                            (Direction::North, "south"),
                            (Direction::South, "north"),
                        ] {
                            if r.next_float() < *p {
                                let bp = lp.offset(dir);
                                if self.is_air(bp) {
                                    self.place_vine(bp, face);
                                    let mut q = BlockPos::new(bp.x, bp.y - 1, bp.z);
                                    let mut i = 4;
                                    while self.is_air(q) && i > 0 {
                                        self.place_vine(q, face);
                                        q = BlockPos::new(q.x, q.y - 1, q.z);
                                        i -= 1;
                                    }
                                }
                            }
                        }
                    }
                }
                Decorator::TrunkVine => {
                    for &lp in &logs {
                        for (dir, face) in [
                            (Direction::West, "east"),
                            (Direction::East, "west"),
                            (Direction::North, "south"),
                            (Direction::South, "north"),
                        ] {
                            if r.next_int_bound(3) > 0 {
                                let bp = lp.offset(dir);
                                if self.is_air(bp) {
                                    self.place_vine(bp, face);
                                }
                            }
                        }
                    }
                }
                Decorator::Cocoa(p) => {
                    if r.next_float() >= *p || logs.is_empty() {
                        continue;
                    }
                    let i = logs[0].y;
                    for &lp in logs.iter().filter(|p| p.y - i <= 2) {
                        for d in HORIZONTAL {
                            if r.next_float() <= 0.25 {
                                let o = d.opposite();
                                let bp = BlockPos::new(lp.x + step(o).0, lp.y, lp.z + step(o).1);
                                if self.is_air(bp) {
                                    let age = r.next_int_bound(3);
                                    let mut s =
                                        b.with(b.default_state("cocoa"), "age", &age.to_string());
                                    s = b.with(s, "facing", dir_name(d));
                                    self.deco_set(bp, s);
                                }
                            }
                        }
                    }
                }
                Decorator::AlterGround(prov) => {
                    let mut list: Vec<BlockPos> = Vec::new();
                    if roots.is_empty() {
                        list.extend(&logs);
                    } else if !logs.is_empty() && roots[0].y == logs[0].y {
                        list.extend(&logs);
                        list.extend(&roots);
                    } else {
                        list.extend(&roots);
                    }
                    if list.is_empty() {
                        continue;
                    }
                    let i = list[0].y;
                    for p in list.into_iter().filter(|p| p.y == i) {
                        self.alter_circle(r, prov, BlockPos::new(p.x - 1, p.y, p.z - 1));
                        self.alter_circle(r, prov, BlockPos::new(p.x + 2, p.y, p.z - 1));
                        self.alter_circle(r, prov, BlockPos::new(p.x - 1, p.y, p.z + 2));
                        self.alter_circle(r, prov, BlockPos::new(p.x + 2, p.y, p.z + 2));
                        for _ in 0..5 {
                            let k = r.next_int_bound(64);
                            let l = k % 8;
                            let m = k / 8;
                            if l == 0 || l == 7 || m == 0 || m == 7 {
                                self.alter_circle(
                                    r,
                                    prov,
                                    BlockPos::new(p.x - 3 + l, p.y, p.z - 3 + m),
                                );
                            }
                        }
                    }
                }
                Decorator::AttachedToLeaves {
                    probability,
                    xz,
                    y,
                    provider,
                    required_empty,
                    directions,
                } => {
                    let mut excluded: ahash::AHashSet<BlockPos> = ahash::AHashSet::new();
                    let mut shuffled = leaves.clone();
                    shuffle(&mut shuffled, r);
                    for bp in shuffled {
                        let d = directions[r.next_int_bound(directions.len() as i32) as usize];
                        let bp2 = bp.offset(d);
                        if excluded.contains(&bp2) || r.next_float() >= *probability {
                            continue;
                        }
                        let mut ok = true;
                        let mut q = bp;
                        for _ in 1..=*required_empty {
                            q = q.offset(d);
                            if !self.is_air(q) {
                                ok = false;
                                break;
                            }
                        }
                        if !ok {
                            continue;
                        }
                        for ex in (bp2.x - xz)..=(bp2.x + xz) {
                            for ey in (bp2.y - y)..=(bp2.y + y) {
                                for ez in (bp2.z - xz)..=(bp2.z + xz) {
                                    excluded.insert(BlockPos::new(ex, ey, ez));
                                }
                            }
                        }
                        let s = provider.get(r, bp2);
                        self.deco_set(bp2, s);
                    }
                }
            }
        }
    }

    fn alter_circle(&mut self, r: &mut Random, prov: &StateProvider, p: BlockPos) {
        for i in -2..=2 {
            for j in -2..=2 {
                if i32::abs(i) == 2 && i32::abs(j) == 2 {
                    continue;
                }
                let q = BlockPos::new(p.x + i, p.y, p.z + j);
                for k in (-3..=2).rev() {
                    let bp = BlockPos::new(q.x, q.y + k, q.z);
                    if bh().is_dirt(self.get(bp)) {
                        let s = prov.get(r, q);
                        self.deco_set(bp, s);
                        break;
                    }
                    if !self.is_air(bp) && k < 0 {
                        break;
                    }
                }
            }
        }
    }

    // ---- leaf distances ---------------------------------------------------

    fn update_leaves(&mut self) {
        let all: Vec<BlockPos> = self
            .logs
            .iter_inserted()
            .chain(self.roots.iter_inserted())
            .chain(self.leaves.iter_inserted())
            .chain(self.decorations.iter_inserted())
            .copied()
            .collect();
        if all.is_empty() {
            return;
        }
        let (mut min, mut max) = (all[0], all[0]);
        for p in &all {
            min = BlockPos::new(min.x.min(p.x), min.y.min(p.y), min.z.min(p.z));
            max = BlockPos::new(max.x.max(p.x), max.y.max(p.y), max.z.max(p.z));
        }
        let inside = |p: BlockPos| {
            p.x >= min.x
                && p.x <= max.x
                && p.y >= min.y
                && p.y <= max.y
                && p.z >= min.z
                && p.z <= max.z
        };
        let b = rc_data::blocks();
        let h = bh();
        let mut full: ahash::AHashSet<BlockPos> = ahash::AHashSet::new();
        for p in self
            .decorations
            .iter_inserted()
            .chain(self.roots.iter_inserted())
        {
            if inside(*p) {
                full.insert(*p);
            }
        }
        let mut levels: Vec<Vec<BlockPos>> = vec![Vec::new(); 7];
        levels[0] = self.logs.java_order();
        let mut queued: AHashMap<BlockPos, i32> = AHashMap::new();
        let mut k = 0usize;
        loop {
            while k < 7 && levels[k].is_empty() {
                k += 1;
            }
            if k >= 7 {
                break;
            }
            let p = levels[k].remove(0);
            if !inside(p) {
                continue;
            }
            if full.contains(&p) {
                continue;
            }
            if k != 0 {
                let s = self.get(p);
                if b.prop_pos(s, "distance").is_some() {
                    self.set(p, b.with(s, "distance", &k.to_string()));
                }
            }
            full.insert(p);
            for d in Direction::ALL {
                let q = p.offset(d);
                if !inside(q) || full.contains(&q) {
                    continue;
                }
                let s = self.get(q);
                let o = if h.logs.contains(h.block(s)) {
                    Some(0)
                } else if b.prop_pos(s, "distance").is_some() {
                    b.get(s, "distance").and_then(|v| v.parse::<i32>().ok())
                } else {
                    None
                };
                let Some(o) = o else { continue };
                let pp = o.min(k as i32 + 1);
                if pp < 7 {
                    let e = queued.entry(q).or_insert(i32::MAX);
                    if pp < *e {
                        *e = pp;
                        levels[pp as usize].push(q);
                        k = k.min(pp as usize);
                    }
                }
            }
        }
    }
}

fn clockwise(d: Direction) -> Direction {
    match d {
        Direction::North => Direction::East,
        Direction::East => Direction::South,
        Direction::South => Direction::West,
        Direction::West => Direction::North,
        o => o,
    }
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

/// `Util.shuffle`.
pub fn shuffle<T>(v: &mut [T], r: &mut Random) {
    let mut i = v.len();
    while i > 1 {
        let k = r.next_int_bound(i as i32) as usize;
        v.swap(i - 1, k);
        i -= 1;
    }
}

fn tree_shape(i: i32, j: i32) -> f32 {
    if (j as f32) < i as f32 * 0.3 {
        return -1.0;
    }
    let f = i as f32 / 2.0;
    let g = f - j as f32;
    let mut h = (f * f - g * g).sqrt();
    if g == 0.0 {
        h = f;
    } else if g.abs() >= f {
        return 0.0;
    }
    h * 0.5
}

fn trim_branches(i: i32, j: i32) -> bool {
    f64::from(j) >= f64::from(i) * 0.2
}

fn log_axis(a: BlockPos, b: BlockPos) -> &'static str {
    let i = (b.x - a.x).abs();
    let j = (b.z - a.z).abs();
    let k = i.max(j);
    if k > 0 {
        if i == k { "x" } else { "z" }
    } else {
        "y"
    }
}

/// `TreeFeature.place`.
pub fn place(ctx: &mut Ctx<'_, '_>, cfg: &TreeConfig, r: &mut Random, pos: BlockPos) -> bool {
    let mut t = TreeGen {
        ctx,
        cfg,
        logs: JavaPosSet::new(),
        roots: JavaPosSet::new(),
        leaves: JavaPosSet::new(),
        decorations: JavaPosSet::new(),
    };
    let i = t.tree_height(r);
    let j = t.foliage_height(r, i);
    let k = i - j;
    let l = t.foliage_radius(r, k);
    let bp = t.trunk_origin(r, pos);
    let m = pos.y.min(bp.y);
    let n = pos.y.max(bp.y) + i + 1;
    if m < t.ctx.region.min_y + 1 || n > t.ctx.region.max_y {
        return false;
    }
    let oi = cfg.min_size.min_clipped();
    // getMaxFreeTreeHeight
    let o = 'free: {
        for jj in 0..=i + 1 {
            let kk = cfg.min_size.size_at(i, jj);
            for ll in -kk..=kk {
                for mm in -kk..=kk {
                    let p = BlockPos::new(bp.x + ll, bp.y + jj, bp.z + mm);
                    let vine = bh().block(t.get(p)) == bh().idx.vine;
                    if t.is_free(p) && (cfg.ignore_vines || !vine) {
                        continue;
                    }
                    break 'free jj - 2;
                }
            }
        }
        i
    };
    if o < i && oi.is_none_or(|v| o < v) {
        return false;
    }
    if cfg.root.is_some() && !t.place_roots(r, pos, bp) {
        return false;
    }
    let atts = t.place_trunk(r, o, bp);
    for a in atts {
        t.create_foliage(r, a, j, l);
    }
    if t.logs.is_empty() && t.leaves.is_empty() {
        return false;
    }
    t.decorate(r);
    t.update_leaves();
    true
}
