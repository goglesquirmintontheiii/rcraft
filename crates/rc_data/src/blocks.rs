//! Block registry with dense state ids in vanilla global-palette order.
//!
//! State id layout matches vanilla's: blocks in registry order, each block's
//! states as the cartesian product of its properties with the *last* property
//! varying fastest. This makes our ids identical to the protocol ids, which
//! Phase 4 relies on.

use ahash::AHashMap;
use rc_core::BlockStateId;
use serde::Deserialize;

use crate::VanillaFiles;
use crate::fluid::{FluidKind, FluidState};

#[derive(Debug, Clone)]
pub struct Property {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BlockDef {
    /// Path without namespace, e.g. `grass_block`.
    pub name: String,
    pub props: Vec<Property>,
    pub first: u32,
    pub count: u32,
    pub default: BlockStateId,
    pub hardness: f32,
    pub resistance: f32,
    pub light: u8,
    pub filter_light: u8,
    pub transparent: bool,
    /// `mineable/pickaxe` etc.
    pub tool: String,
    pub harvest_tools: Vec<String>,
}

/// Per-state cached information used on hot paths.
#[derive(Debug, Clone, Copy)]
pub struct StateInfo {
    pub block: u16,
    /// Collision shape index into [`Blocks::shapes`].
    pub shape: u16,
    pub fluid: FluidState,
    pub flags: u8,
    pub light: u8,
    pub opacity: u8,
}

pub const FLAG_AIR: u8 = 1;
pub const FLAG_FULL_CUBE: u8 = 2;
pub const FLAG_NO_COLLISION: u8 = 4;
pub const FLAG_LEGACY_SOLID: u8 = 8;

#[derive(Deserialize)]
struct RawBlock {
    name: String,
    props: Vec<(String, Vec<String>)>,
    default: u32,
    first_id: u32,
    hardness: f32,
    resistance: f32,
    light: u8,
    filter_light: u8,
    transparent: bool,
    tool: String,
    harvest_tools: Vec<String>,
    shapes: Vec<u16>,
}

#[derive(Deserialize)]
struct RawBlocks {
    blocks: Vec<RawBlock>,
    shapes: std::collections::HashMap<String, Vec<[f64; 6]>>,
}

/// The block registry.
pub struct Blocks {
    pub defs: Vec<BlockDef>,
    pub states: Vec<StateInfo>,
    /// Collision boxes, `[min_x, min_y, min_z, max_x, max_y, max_z]`.
    pub shapes: Vec<Vec<[f64; 6]>>,
    by_name: AHashMap<String, u16>,
}

impl std::fmt::Debug for Blocks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Blocks({} blocks, {} states)",
            self.defs.len(),
            self.states.len()
        )
    }
}

/// A block-state lookup failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownBlock(pub String);

impl Blocks {
    pub(crate) fn load(files: &VanillaFiles) -> Self {
        let raw: RawBlocks = serde_json::from_slice(files.get("blocks.json").expect("blocks.json"))
            .expect("blocks.json valid");
        let mut shape_ids: Vec<u16> = raw.shapes.keys().map(|k| k.parse().unwrap()).collect();
        shape_ids.sort_unstable();
        let max_shape = *shape_ids.last().unwrap_or(&0) as usize;
        let mut shapes = vec![Vec::new(); max_shape + 1];
        for (k, v) in raw.shapes {
            shapes[k.parse::<usize>().unwrap()] = v;
        }
        let mut defs = Vec::with_capacity(raw.blocks.len());
        let mut states = Vec::new();
        let mut by_name = AHashMap::new();
        for (bi, b) in raw.blocks.into_iter().enumerate() {
            let count: u32 = b.props.iter().map(|p| p.1.len() as u32).product();
            assert_eq!(
                b.first_id as usize,
                states.len(),
                "state ids must be contiguous ({})",
                b.name
            );
            let props: Vec<Property> = b
                .props
                .into_iter()
                .map(|(name, values)| Property { name, values })
                .collect();
            let def = BlockDef {
                name: b.name.clone(),
                props,
                first: b.first_id,
                count,
                default: BlockStateId(b.first_id + b.default),
                hardness: b.hardness,
                resistance: b.resistance,
                light: b.light,
                filter_light: b.filter_light,
                transparent: b.transparent,
                tool: b.tool,
                harvest_tools: b.harvest_tools,
            };
            let is_air = matches!(b.name.as_str(), "air" | "cave_air" | "void_air");
            for i in 0..count {
                let shape = if b.shapes.len() == 1 {
                    b.shapes[0]
                } else {
                    b.shapes[i as usize]
                };
                let boxes = &shapes[shape as usize];
                let full = boxes.len() == 1 && boxes[0] == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
                let mut flags = 0;
                if is_air {
                    flags |= FLAG_AIR;
                }
                if full {
                    flags |= FLAG_FULL_CUBE;
                }
                if boxes.is_empty() {
                    flags |= FLAG_NO_COLLISION;
                }
                if legacy_solid(&b.name, boxes) {
                    flags |= FLAG_LEGACY_SOLID;
                }
                let fluid = FluidState::for_state(&def, i);
                states.push(StateInfo {
                    block: bi as u16,
                    shape,
                    fluid,
                    flags,
                    light: b.light,
                    opacity: b.filter_light,
                });
            }
            by_name.insert(b.name, bi as u16);
            defs.push(def);
        }
        Self {
            defs,
            states,
            shapes,
            by_name,
        }
    }

    #[inline]
    #[must_use]
    pub fn info(&self, s: BlockStateId) -> &StateInfo {
        &self.states[s.index()]
    }

    #[inline]
    #[must_use]
    pub fn def(&self, s: BlockStateId) -> &BlockDef {
        &self.defs[self.states[s.index()].block as usize]
    }

    #[inline]
    #[must_use]
    pub fn block_index(&self, s: BlockStateId) -> u16 {
        self.states[s.index()].block
    }

    /// Block index by path (`stone`) or full key (`minecraft:stone`).
    #[must_use]
    pub fn index_of(&self, name: &str) -> Option<u16> {
        let n = name.strip_prefix("minecraft:").unwrap_or(name);
        self.by_name.get(n).copied()
    }

    /// Default state of a block by name. Panics on unknown names: callers use
    /// this for vanilla constants.
    #[must_use]
    pub fn default_state(&self, name: &str) -> BlockStateId {
        let i = self
            .index_of(name)
            .unwrap_or_else(|| panic!("unknown block {name}"));
        self.defs[i as usize].default
    }

    #[inline]
    #[must_use]
    pub fn is_air(&self, s: BlockStateId) -> bool {
        self.states[s.index()].flags & FLAG_AIR != 0
    }

    #[inline]
    #[must_use]
    pub fn is_block(&self, s: BlockStateId, block: u16) -> bool {
        self.states[s.index()].block == block
    }

    #[inline]
    #[must_use]
    pub fn fluid(&self, s: BlockStateId) -> FluidState {
        self.states[s.index()].fluid
    }

    /// Value index of property `p` (by position) for state `s`.
    #[must_use]
    pub fn prop_index(&self, s: BlockStateId, p: usize) -> usize {
        let d = self.def(s);
        let local = s.0 - d.first;
        let stride: u32 = d.props[p + 1..]
            .iter()
            .map(|q| q.values.len() as u32)
            .product();
        ((local / stride) % d.props[p].values.len() as u32) as usize
    }

    /// Property position by name.
    #[must_use]
    pub fn prop_pos(&self, s: BlockStateId, name: &str) -> Option<usize> {
        self.def(s).props.iter().position(|p| p.name == name)
    }

    /// Property value by name, e.g. `get(s, "axis") == Some("y")`.
    #[must_use]
    pub fn get(&self, s: BlockStateId, name: &str) -> Option<&str> {
        let p = self.prop_pos(s, name)?;
        Some(self.def(s).props[p].values[self.prop_index(s, p)].as_str())
    }

    /// Return `s` with property `name` set to `value`. Unknown property or
    /// value returns `s` unchanged.
    #[must_use]
    pub fn with(&self, s: BlockStateId, name: &str, value: &str) -> BlockStateId {
        let d = self.def(s);
        let Some(p) = d.props.iter().position(|q| q.name == name) else {
            return s;
        };
        let Some(vi) = d.props[p].values.iter().position(|v| v == value) else {
            return s;
        };
        let stride: u32 = d.props[p + 1..]
            .iter()
            .map(|q| q.values.len() as u32)
            .product();
        let local = s.0 - d.first;
        let cur = (local / stride) % d.props[p].values.len() as u32;
        BlockStateId(d.first + local - cur * stride + vi as u32 * stride)
    }

    /// Parse `minecraft:oak_log[axis=y]` / `oak_log`.
    pub fn parse_state(&self, s: &str) -> Result<BlockStateId, UnknownBlock> {
        let (name, props) = match s.find('[') {
            Some(i) => (&s[..i], Some(s[i + 1..].trim_end_matches(']'))),
            None => (s, None),
        };
        let idx = self
            .index_of(name)
            .ok_or_else(|| UnknownBlock(s.to_owned()))?;
        let mut st = self.defs[idx as usize].default;
        if let Some(props) = props {
            for kv in props.split(',').filter(|x| !x.is_empty()) {
                let (k, v) = kv
                    .split_once('=')
                    .ok_or_else(|| UnknownBlock(s.to_owned()))?;
                st = self.with(st, k.trim(), v.trim());
            }
        }
        Ok(st)
    }

    /// Parse the codec form `{"Name": "...", "Properties": {...}}`.
    pub fn from_json(&self, v: &serde_json::Value) -> Result<BlockStateId, UnknownBlock> {
        let name = v["Name"]
            .as_str()
            .ok_or_else(|| UnknownBlock(v.to_string()))?;
        let idx = self
            .index_of(name)
            .ok_or_else(|| UnknownBlock(name.to_owned()))?;
        let mut st = self.defs[idx as usize].default;
        if let Some(props) = v.get("Properties").and_then(|p| p.as_object()) {
            for (k, val) in props {
                if let Some(val) = val.as_str() {
                    st = self.with(st, k, val);
                }
            }
        }
        Ok(st)
    }

    /// Format as `minecraft:name[k=v,...]` (properties in definition order).
    #[must_use]
    pub fn to_string(&self, s: BlockStateId) -> String {
        let d = self.def(s);
        if d.props.is_empty() {
            return format!("minecraft:{}", d.name);
        }
        let props: Vec<String> = (0..d.props.len())
            .map(|p| {
                format!(
                    "{}={}",
                    d.props[p].name,
                    d.props[p].values[self.prop_index(s, p)]
                )
            })
            .collect();
        format!("minecraft:{}[{}]", d.name, props.join(","))
    }

    /// Collision boxes of a state.
    #[inline]
    #[must_use]
    pub fn collision(&self, s: BlockStateId) -> &[[f64; 6]] {
        &self.shapes[self.states[s.index()].shape as usize]
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.states.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.states.is_empty()
    }
}

/// Vanilla's `legacySolid` (`BlockStateBase.calculateSolid`): forced on/off
/// for the blocks whose pre-1.20 material solidity disagrees with their shape,
/// otherwise derived from the collision bounds.
fn legacy_solid(name: &str, boxes: &[[f64; 6]]) -> bool {
    const FORCE_ON: &[&str] = &[
        "pointed_dripstone",
        "small_amethyst_bud",
        "medium_amethyst_bud",
        "large_amethyst_bud",
        "amethyst_cluster",
        "cobweb",
        "bamboo_sapling",
        "chain",
        "lantern",
        "soul_lantern",
        "turtle_egg",
        "sniffer_egg",
        "decorated_pot",
        "heavy_core",
    ];
    const FORCE_OFF: &[&str] = &["snow", "scaffolding", "powder_snow"];
    if FORCE_ON.contains(&name) {
        return true;
    }
    if FORCE_OFF.contains(&name) {
        return false;
    }
    if boxes.is_empty() {
        return false;
    }
    let (mut min, mut max) = ([1.0f64; 3], [0.0f64; 3]);
    for b in boxes {
        for i in 0..3 {
            min[i] = min[i].min(b[i]);
            max[i] = max[i].max(b[i + 3]);
        }
    }
    let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    (size[0] + size[1] + size[2]) / 3.0 >= 0.729_166_666_666_666_6 || size[1] >= 1.0
}

impl FluidState {
    fn for_state(def: &BlockDef, local: u32) -> Self {
        let prop = |name: &str| -> Option<&str> {
            let p = def.props.iter().position(|q| q.name == name)?;
            let stride: u32 = def.props[p + 1..]
                .iter()
                .map(|q| q.values.len() as u32)
                .product();
            Some(
                def.props[p].values[((local / stride) % def.props[p].values.len() as u32) as usize]
                    .as_str(),
            )
        };
        match def.name.as_str() {
            "water" | "lava" => {
                let level: u8 = prop("level").unwrap().parse().unwrap();
                let kind = if def.name == "water" {
                    FluidKind::Water
                } else {
                    FluidKind::Lava
                };
                FluidState::from_level(kind, level)
            }
            "kelp" | "kelp_plant" | "seagrass" | "tall_seagrass" | "bubble_column" => {
                FluidState::source(FluidKind::Water)
            }
            _ => {
                if prop("waterlogged") == Some("true") {
                    FluidState::source(FluidKind::Water)
                } else {
                    FluidState::EMPTY
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_state_ids() {
        let b = crate::blocks();
        assert_eq!(b.default_state("air"), BlockStateId(0));
        assert_eq!(b.default_state("stone"), BlockStateId(1));
        // grass_block[snowy=false] is 9 in vanilla 1.21.1.
        assert_eq!(b.default_state("grass_block"), BlockStateId(9));
        assert_eq!(
            b.parse_state("minecraft:grass_block[snowy=true]").unwrap(),
            BlockStateId(8)
        );
        assert_eq!(b.len(), 26_684);
    }

    #[test]
    fn properties_round_trip() {
        let b = crate::blocks();
        let log = b.parse_state("oak_log[axis=x]").unwrap();
        assert_eq!(b.get(log, "axis"), Some("x"));
        let z = b.with(log, "axis", "z");
        assert_eq!(b.get(z, "axis"), Some("z"));
        assert_eq!(b.to_string(z), "minecraft:oak_log[axis=z]");
        let leaves = b
            .parse_state("oak_leaves[distance=3,persistent=true,waterlogged=true]")
            .unwrap();
        assert_eq!(b.get(leaves, "distance"), Some("3"));
        assert!(b.fluid(leaves).is_source());
    }

    #[test]
    fn flags() {
        let b = crate::blocks();
        assert!(b.is_air(b.default_state("cave_air")));
        assert!(b.info(b.default_state("stone")).flags & FLAG_FULL_CUBE != 0);
        assert!(b.info(b.default_state("short_grass")).flags & FLAG_NO_COLLISION != 0);
        assert_eq!(b.fluid(b.default_state("water")).kind, FluidKind::Water);
        assert_eq!(b.info(b.default_state("glowstone")).light, 15);
    }
}
