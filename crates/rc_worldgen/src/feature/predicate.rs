//! Block predicates, ore rule tests and block-state providers.

use rc_core::{BlockPos, BlockStateId, Direction};
use rc_data::tags::BlockSet;
use serde_json::Value;

use super::behaviour::{bh, can_survive};
use super::region::Region;
use crate::mth;
use crate::noise::{NoiseParams, NormalNoise};
use crate::providers::IntProvider;
use crate::rng::Random;

pub fn state(v: &Value) -> BlockStateId {
    rc_data::blocks()
        .from_json(v)
        .unwrap_or_else(|e| panic!("bad block state {e:?}"))
}

fn block_list(v: &Value) -> BlockSet {
    let t = rc_data::tags();
    match v {
        Value::String(s) if s.starts_with('#') => t.block(s),
        Value::String(s) => {
            let mut set = t.block("#minecraft:__none__");
            set.insert(
                rc_data::blocks()
                    .index_of(s)
                    .unwrap_or_else(|| panic!("block {s}")),
            );
            set
        }
        Value::Array(a) => {
            let mut set = t.block("#minecraft:__none__");
            for x in a {
                set.insert(rc_data::blocks().index_of(x.as_str().unwrap()).unwrap());
            }
            set
        }
        _ => panic!("bad block list {v}"),
    }
}

fn offset(v: &Value) -> (i32, i32, i32) {
    v.get("offset")
        .and_then(Value::as_array)
        .map_or((0, 0, 0), |a| {
            (
                a[0].as_i64().unwrap() as i32,
                a[1].as_i64().unwrap() as i32,
                a[2].as_i64().unwrap() as i32,
            )
        })
}

#[must_use]
pub fn direction(s: &str) -> Direction {
    match s {
        "down" => Direction::Down,
        "up" => Direction::Up,
        "north" => Direction::North,
        "south" => Direction::South,
        "west" => Direction::West,
        _ => Direction::East,
    }
}

#[derive(Clone, Debug)]
pub enum BlockPredicate {
    True,
    MatchingBlocks(BlockSet, (i32, i32, i32)),
    MatchingFluids {
        water: bool,
        flowing_water: bool,
        lava: bool,
        flowing_lava: bool,
        off: (i32, i32, i32),
    },
    SturdyFace(Direction, (i32, i32, i32)),
    Solid((i32, i32, i32)),
    Replaceable((i32, i32, i32)),
    WouldSurvive(BlockStateId, (i32, i32, i32)),
    InsideWorldBounds((i32, i32, i32)),
    AllOf(Vec<BlockPredicate>),
    AnyOf(Vec<BlockPredicate>),
    Not(Box<BlockPredicate>),
}

impl BlockPredicate {
    #[must_use]
    pub fn from_json(v: &Value) -> Self {
        let t = v["type"].as_str().unwrap().trim_start_matches("minecraft:");
        let off = offset(v);
        match t {
            "true" => Self::True,
            "matching_blocks" => Self::MatchingBlocks(block_list(&v["blocks"]), off),
            "matching_block_tag" => {
                Self::MatchingBlocks(rc_data::tags().block(v["tag"].as_str().unwrap()), off)
            }
            "matching_fluids" => {
                let names: Vec<String> = match &v["fluids"] {
                    Value::String(s) => vec![s.clone()],
                    Value::Array(a) => a.iter().map(|x| x.as_str().unwrap().to_owned()).collect(),
                    _ => vec![],
                };
                let has = |n: &str| names.iter().any(|x| x == n);
                Self::MatchingFluids {
                    water: has("minecraft:water"),
                    flowing_water: has("minecraft:flowing_water"),
                    lava: has("minecraft:lava"),
                    flowing_lava: has("minecraft:flowing_lava"),
                    off,
                }
            }
            "has_sturdy_face" => Self::SturdyFace(direction(v["direction"].as_str().unwrap()), off),
            "solid" => Self::Solid(off),
            "replaceable" => Self::Replaceable(off),
            "would_survive" => Self::WouldSurvive(state(&v["state"]), off),
            "inside_world_bounds" => Self::InsideWorldBounds(off),
            "all_of" => Self::AllOf(
                v["predicates"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(Self::from_json)
                    .collect(),
            ),
            "any_of" => Self::AnyOf(
                v["predicates"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(Self::from_json)
                    .collect(),
            ),
            "not" => Self::Not(Box::new(Self::from_json(&v["predicate"]))),
            other => panic!("unknown block predicate {other}"),
        }
    }

    #[must_use]
    pub fn test(&self, r: &Region<'_>, p: BlockPos) -> bool {
        let at = |o: (i32, i32, i32)| BlockPos::new(p.x + o.0, p.y + o.1, p.z + o.2);
        let h = bh();
        match self {
            Self::True => true,
            Self::MatchingBlocks(set, o) => set.contains(h.block(r.get(at(*o)))),
            Self::MatchingFluids {
                water,
                flowing_water,
                lava,
                flowing_lava,
                off,
            } => {
                let f = r.fluid(at(*off));
                (f.is_water() && ((f.is_source() && *water) || (!f.is_source() && *flowing_water)))
                    || (f.is_lava()
                        && ((f.is_source() && *lava) || (!f.is_source() && *flowing_lava)))
            }
            Self::SturdyFace(d, o) => h.is_face_sturdy(r.get(at(*o)), *d),
            Self::Solid(o) => h.is_solid(r.get(at(*o))),
            Self::Replaceable(o) => h.can_be_replaced(r.get(at(*o))),
            Self::WouldSurvive(s, o) => can_survive(r, *s, at(*o)),
            Self::InsideWorldBounds(o) => !r.is_outside_build_height(at(*o).y),
            Self::AllOf(v) => v.iter().all(|x| x.test(r, p)),
            Self::AnyOf(v) => v.iter().any(|x| x.test(r, p)),
            Self::Not(x) => !x.test(r, p),
        }
    }
}

/// Ore rule tests (`RuleTest`).
#[derive(Clone, Debug)]
pub enum RuleTest {
    Always,
    Tag(BlockSet),
    Block(u16),
    State(BlockStateId),
    RandomBlock(u16, f32),
    RandomState(BlockStateId, f32),
}

impl RuleTest {
    #[must_use]
    pub fn from_json(v: &Value) -> Self {
        let b = rc_data::blocks();
        match v["predicate_type"]
            .as_str()
            .unwrap()
            .trim_start_matches("minecraft:")
        {
            "always_true" => Self::Always,
            "tag_match" => Self::Tag(rc_data::tags().block(v["tag"].as_str().unwrap())),
            "block_match" => Self::Block(b.index_of(v["block"].as_str().unwrap()).unwrap()),
            "blockstate_match" => Self::State(state(&v["block_state"])),
            "random_block_match" => Self::RandomBlock(
                b.index_of(v["block"].as_str().unwrap()).unwrap(),
                v["probability"].as_f64().unwrap() as f32,
            ),
            "random_blockstate_match" => Self::RandomState(
                state(&v["block_state"]),
                v["probability"].as_f64().unwrap() as f32,
            ),
            t => panic!("unknown rule test {t}"),
        }
    }

    pub fn test(&self, s: BlockStateId, r: &mut Random) -> bool {
        let h = bh();
        match self {
            Self::Always => true,
            Self::Tag(t) => t.contains(h.block(s)),
            Self::Block(b) => h.block(s) == *b,
            Self::State(st) => s == *st,
            Self::RandomBlock(b, p) => h.block(s) == *b && r.next_float() < *p,
            Self::RandomState(st, p) => s == *st && r.next_float() < *p,
        }
    }
}

#[derive(Clone, Debug)]
pub struct NoiseCfg {
    noise: NormalNoise,
    scale: f32,
}

impl NoiseCfg {
    fn new(v: &Value, key: &str, scale_key: &str) -> Self {
        let seed = v["seed"].as_i64().unwrap();
        let params = NoiseParams::from_json(&v[key]);
        Self {
            noise: NormalNoise::new(&mut Random::legacy(seed), &params),
            scale: v[scale_key].as_f64().unwrap() as f32,
        }
    }

    fn value(&self, p: BlockPos, scale: f32) -> f64 {
        let s = f64::from(scale);
        self.noise
            .get_value(f64::from(p.x) * s, f64::from(p.y) * s, f64::from(p.z) * s)
    }
}

fn pick_by_noise(list: &[BlockStateId], d: f64) -> BlockStateId {
    let e = mth::clamp((1.0 + d) / 2.0, 0.0, 0.9999);
    list[(e * list.len() as f64) as usize]
}

/// `BlockStateProvider`.
#[derive(Clone, Debug)]
pub enum StateProvider {
    Simple(BlockStateId),
    Weighted(Vec<(BlockStateId, i32)>, i32),
    Rotated(BlockStateId),
    RandomizedInt {
        source: Box<StateProvider>,
        property: String,
        values: IntProvider,
    },
    Noise {
        n: NoiseCfg,
        states: Vec<BlockStateId>,
    },
    NoiseThreshold {
        n: NoiseCfg,
        threshold: f32,
        high_chance: f32,
        default: BlockStateId,
        low: Vec<BlockStateId>,
        high: Vec<BlockStateId>,
    },
    DualNoise {
        n: NoiseCfg,
        slow: NoiseCfg,
        variety: (i32, i32),
        states: Vec<BlockStateId>,
    },
}

fn states(v: &Value) -> Vec<BlockStateId> {
    v.as_array().unwrap().iter().map(state).collect()
}

impl StateProvider {
    #[must_use]
    pub fn from_json(v: &Value) -> Self {
        match v["type"].as_str().unwrap().trim_start_matches("minecraft:") {
            "simple_state_provider" => Self::Simple(state(&v["state"])),
            "weighted_state_provider" => {
                let e: Vec<(BlockStateId, i32)> = v["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| (state(&e["data"]), e["weight"].as_i64().unwrap() as i32))
                    .collect();
                let total = e.iter().map(|x| x.1).sum();
                Self::Weighted(e, total)
            }
            "rotated_block_provider" => Self::Rotated(state(&v["state"])),
            "randomized_int_state_provider" => Self::RandomizedInt {
                source: Box::new(Self::from_json(&v["source"])),
                property: v["property"].as_str().unwrap().to_owned(),
                values: IntProvider::from_json(&v["values"]),
            },
            "noise_provider" => Self::Noise {
                n: NoiseCfg::new(v, "noise", "scale"),
                states: states(&v["states"]),
            },
            "noise_threshold_provider" => Self::NoiseThreshold {
                n: NoiseCfg::new(v, "noise", "scale"),
                threshold: v["threshold"].as_f64().unwrap() as f32,
                high_chance: v["high_chance"].as_f64().unwrap() as f32,
                default: state(&v["default_state"]),
                low: states(&v["low_states"]),
                high: states(&v["high_states"]),
            },
            "dual_noise_provider" => {
                let var = &v["variety"];
                let variety = match var {
                    Value::Array(a) => {
                        (a[0].as_i64().unwrap() as i32, a[1].as_i64().unwrap() as i32)
                    }
                    Value::Object(_) => (
                        var["min_inclusive"].as_i64().unwrap() as i32,
                        var["max_inclusive"].as_i64().unwrap() as i32,
                    ),
                    n => (n.as_i64().unwrap() as i32, n.as_i64().unwrap() as i32),
                };
                Self::DualNoise {
                    n: NoiseCfg::new(v, "noise", "scale"),
                    slow: NoiseCfg::new(v, "slow_noise", "slow_scale"),
                    variety,
                    states: states(&v["states"]),
                }
            }
            t => panic!("unknown state provider {t}"),
        }
    }

    pub fn get(&self, r: &mut Random, p: BlockPos) -> BlockStateId {
        let b = rc_data::blocks();
        match self {
            Self::Simple(s) => *s,
            Self::Weighted(list, total) => {
                let mut k = r.next_int_bound(*total);
                for (s, w) in list {
                    k -= w;
                    if k < 0 {
                        return *s;
                    }
                }
                unreachable!()
            }
            Self::Rotated(s) => {
                let axis = ["x", "y", "z"][r.next_int_bound(3) as usize];
                b.with(*s, "axis", axis)
            }
            Self::RandomizedInt {
                source,
                property,
                values,
            } => {
                let s = source.get(r, p);
                if b.prop_pos(s, property).is_none() {
                    return s;
                }
                let v = values.sample(r);
                b.with(s, property, &v.to_string())
            }
            Self::Noise { n, states } => pick_by_noise(states, n.value(p, n.scale)),
            Self::NoiseThreshold {
                n,
                threshold,
                high_chance,
                default,
                low,
                high,
            } => {
                let d = n.value(p, n.scale);
                if d < f64::from(*threshold) {
                    return low[r.next_int_bound(low.len() as i32) as usize];
                }
                if r.next_float() < *high_chance {
                    return high[r.next_int_bound(high.len() as i32) as usize];
                }
                *default
            }
            Self::DualNoise {
                n,
                slow,
                variety,
                states,
            } => {
                let d = slow.value(p, slow.scale);
                let i =
                    mth::clamped_map(d, -1.0, 1.0, f64::from(variety.0), f64::from(variety.1 + 1))
                        as i32;
                let list: Vec<BlockStateId> = (0..i)
                    .map(|j| {
                        let q = BlockPos::new(p.x + j * 54545, p.y, p.z + j * 34234);
                        pick_by_noise(states, slow.value(q, slow.scale))
                    })
                    .collect();
                pick_by_noise(&list, n.value(p, n.scale))
            }
        }
    }
}
