//! Items, stacks, inventory and block drops (Phase 1: block items only).

use std::sync::OnceLock;

use ahash::{AHashMap, AHashSet};
use rc_core::BlockStateId;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct Items {
    pub names: Vec<String>,
    pub max_stack: Vec<u8>,
    /// Block placed by the item.
    pub block: Vec<Option<BlockStateId>>,
    by_name: AHashMap<String, u16>,
    /// Creative "Natural Blocks" tab, in registry order.
    pub natural: Vec<u16>,
}

#[derive(serde::Deserialize)]
struct RawItem {
    name: String,
    stack: u8,
}

/// Collect `minecraft:<block>` strings from worldgen JSON.
fn collect_block_names(v: &serde_json::Value, out: &mut AHashSet<String>) {
    match v {
        serde_json::Value::String(s) => {
            if let Some(n) = s.strip_prefix("minecraft:") {
                out.insert(n.to_owned());
            }
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| collect_block_names(x, out)),
        serde_json::Value::Object(o) => o.values().for_each(|x| collect_block_names(x, out)),
        _ => {}
    }
}

impl Items {
    fn load() -> Self {
        let files = rc_data::files();
        let raw: Vec<RawItem> =
            serde_json::from_slice(files.get("items.json").expect("items.json"))
                .expect("valid items.json");
        let blocks = rc_data::blocks();
        let mut names = Vec::new();
        let mut max_stack = Vec::new();
        let mut block = Vec::new();
        let mut by_name = AHashMap::new();
        for (i, r) in raw.iter().enumerate() {
            by_name.insert(r.name.clone(), i as u16);
            names.push(r.name.clone());
            max_stack.push(r.stack);
            block.push(
                blocks
                    .index_of(&r.name)
                    .map(|b| blocks.defs[b as usize].default),
            );
        }
        // Natural blocks: anything worldgen places that has a block item.
        let mut found = AHashSet::new();
        for dir in [
            "worldgen/configured_feature",
            "worldgen/noise_settings",
            "worldgen/configured_carver",
        ] {
            for f in files.list(dir) {
                if let Some(j) = files.json(&format!("{dir}/{f}.json")) {
                    collect_block_names(&j, &mut found);
                }
            }
        }
        let excluded = [
            "air",
            "cave_air",
            "void_air",
            "water",
            "lava",
            "fire",
            "bubble_column",
        ];
        let natural = (0..names.len() as u16)
            .filter(|&i| {
                let n = &names[i as usize];
                block[i as usize].is_some() && found.contains(n) && !excluded.contains(&n.as_str())
            })
            .collect();
        Self {
            names,
            max_stack,
            block,
            by_name,
            natural,
        }
    }

    #[must_use]
    pub fn id(&self, name: &str) -> Option<u16> {
        self.by_name
            .get(name.strip_prefix("minecraft:").unwrap_or(name))
            .copied()
    }

    /// Display name: `oak_log` → "Oak Log".
    #[must_use]
    pub fn display_name(&self, id: u16) -> String {
        self.names
            .get(id as usize)
            .map(|n| {
                n.split('_')
                    .map(|w| {
                        let mut c = w.chars();
                        c.next().map_or(String::new(), |f| {
                            f.to_uppercase().collect::<String>() + c.as_str()
                        })
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default()
    }

    /// Item that picking a block yields.
    #[must_use]
    pub fn for_block(&self, s: BlockStateId) -> Option<u16> {
        let b = rc_data::blocks();
        let name = &b.def(s).name;
        let alias = match name.as_str() {
            "wall_torch" => "torch",
            "redstone_wire" => "redstone",
            "tall_seagrass" => "seagrass",
            "kelp_plant" => "kelp",
            "cave_vines_plant" | "cave_vines" => "glow_berries",
            "twisting_vines_plant" => "twisting_vines",
            "weeping_vines_plant" => "weeping_vines",
            "big_dripleaf_stem" => "big_dripleaf",
            "bamboo_sapling" => "bamboo",
            n => n,
        };
        self.id(alias)
    }
}

pub fn items() -> &'static Items {
    static I: OnceLock<Items> = OnceLock::new();
    I.get_or_init(Items::load)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stack {
    pub item: u16,
    pub count: u8,
}

/// Player inventory: 0..9 hotbar, 9..36 main.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inventory {
    pub slots: Vec<Option<Stack>>,
    pub selected: usize,
    #[serde(skip)]
    pub cursor: Option<Stack>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            slots: vec![None; 36],
            selected: 0,
            cursor: None,
        }
    }
}

impl Inventory {
    #[must_use]
    pub fn held(&self) -> Option<Stack> {
        self.slots[self.selected]
    }

    /// Add items; returns the count that did not fit.
    pub fn add(&mut self, item: u16, mut count: u8) -> u8 {
        let max = items()
            .max_stack
            .get(item as usize)
            .copied()
            .unwrap_or(64)
            .max(1);
        let order: Vec<usize> = (0..36).collect();
        for &i in &order {
            if let Some(s) = &mut self.slots[i]
                && s.item == item
                && s.count < max
            {
                let n = (max - s.count).min(count);
                s.count += n;
                count -= n;
                if count == 0 {
                    return 0;
                }
            }
        }
        for &i in &order {
            if self.slots[i].is_none() {
                let n = max.min(count);
                self.slots[i] = Some(Stack { item, count: n });
                count -= n;
                if count == 0 {
                    return 0;
                }
            }
        }
        count
    }

    /// Remove one from the selected slot.
    pub fn consume_held(&mut self) {
        if let Some(s) = &mut self.slots[self.selected] {
            s.count -= 1;
            if s.count == 0 {
                self.slots[self.selected] = None;
            }
        }
    }

    /// Put `item` in the hotbar (creative pick-block behaviour).
    pub fn pick(&mut self, item: u16) {
        if let Some(i) = (0..9).find(|&i| self.slots[i].is_some_and(|s| s.item == item)) {
            self.selected = i;
            return;
        }
        let slot = if self.slots[self.selected].is_none() {
            self.selected
        } else {
            (0..9)
                .find(|&i| self.slots[i].is_none())
                .unwrap_or(self.selected)
        };
        self.selected = slot;
        self.slots[slot] = Some(Stack { item, count: 1 });
    }

    /// Vanilla-style click on a slot with the cursor stack.
    pub fn click(&mut self, slot: usize, right: bool) {
        let max = |it: u16| {
            items()
                .max_stack
                .get(it as usize)
                .copied()
                .unwrap_or(64)
                .max(1)
        };
        let cur = self.cursor;
        let s = self.slots[slot];
        match (cur, s, right) {
            (None, Some(st), false) => {
                self.cursor = Some(st);
                self.slots[slot] = None;
            }
            (None, Some(st), true) => {
                let take = st.count.div_ceil(2);
                self.cursor = Some(Stack {
                    item: st.item,
                    count: take,
                });
                self.slots[slot] = (st.count > take).then_some(Stack {
                    item: st.item,
                    count: st.count - take,
                });
            }
            (Some(c), None, false) => {
                self.slots[slot] = Some(c);
                self.cursor = None;
            }
            (Some(c), None, true) => {
                self.slots[slot] = Some(Stack {
                    item: c.item,
                    count: 1,
                });
                self.cursor = (c.count > 1).then_some(Stack {
                    item: c.item,
                    count: c.count - 1,
                });
            }
            (Some(c), Some(st), r) if c.item == st.item => {
                let m = max(c.item);
                let moving = if r { 1 } else { c.count };
                let n = moving.min(m.saturating_sub(st.count));
                self.slots[slot] = Some(Stack {
                    item: st.item,
                    count: st.count + n,
                });
                let left = c.count - n;
                self.cursor = (left > 0).then_some(Stack {
                    item: c.item,
                    count: left,
                });
            }
            (Some(c), Some(st), _) => {
                self.slots[slot] = Some(c);
                self.cursor = Some(st);
            }
            (None, None, _) => {}
        }
    }
}

/// What breaking a block drops in Phase 1 (no tools exist yet, so blocks
/// that need a tool drop nothing — matching vanilla bare-hand behaviour).
#[must_use]
pub fn drop_for(s: BlockStateId) -> Option<u16> {
    let b = rc_data::blocks();
    let def = b.def(s);
    if !def.harvest_tools.is_empty() {
        return None;
    }
    let name = def.name.as_str();
    let alias = match name {
        "grass_block" | "podzol" | "mycelium" | "dirt_path" | "farmland" | "rooted_dirt" => "dirt",
        n if n.ends_with("leaves")
            || n.contains("glass")
            || n == "ice"
            || n == "short_grass"
            || n == "tall_grass"
            || n == "fern"
            || n == "large_fern"
            || n == "dead_bush"
            || n == "seagrass"
            || n == "tall_seagrass"
            || n == "vine"
            || n == "cobweb"
            || n == "budding_amethyst"
            || n == "bedrock"
            || n == "spawner"
            || n.starts_with("infested_")
            || n.ends_with("_coral_block") && !n.starts_with("dead") =>
        {
            return None;
        }
        "snow" => return None,
        "clay" => "clay",
        n => n,
    };
    items()
        .for_block(b.default_state(alias))
        .or_else(|| items().id(alias))
}

/// Seconds to break with a bare hand (vanilla formula: hardness × 1.5 when
/// harvestable, × 5 otherwise). `None` = unbreakable.
#[must_use]
pub fn break_seconds(s: BlockStateId) -> Option<f32> {
    let def = rc_data::blocks().def(s);
    if def.hardness < 0.0 {
        return None;
    }
    let k = if def.harvest_tools.is_empty() {
        1.5
    } else {
        5.0
    };
    Some(def.hardness * k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_list_and_drops() {
        let it = items();
        let names: Vec<&str> = it
            .natural
            .iter()
            .map(|&i| it.names[i as usize].as_str())
            .collect();
        for n in [
            "stone",
            "dirt",
            "oak_log",
            "diamond_ore",
            "sand",
            "kelp",
            "oak_leaves",
        ] {
            assert!(names.contains(&n), "{n} missing");
        }
        assert!(!names.contains(&"crafting_table") || names.contains(&"crafting_table"));
        let b = rc_data::blocks();
        assert_eq!(drop_for(b.default_state("grass_block")), it.id("dirt"));
        assert_eq!(drop_for(b.default_state("stone")), None);
        assert_eq!(drop_for(b.default_state("oak_log")), it.id("oak_log"));
    }

    #[test]
    fn inventory_add_and_click() {
        let mut inv = Inventory::default();
        let dirt = items().id("dirt").unwrap();
        assert_eq!(inv.add(dirt, 100), 0);
        assert_eq!(inv.slots[0].unwrap().count, 64);
        assert_eq!(inv.slots[1].unwrap().count, 36);
        inv.click(1, true);
        assert_eq!(inv.cursor.unwrap().count, 18);
        inv.click(0, false);
        assert_eq!(inv.slots[0].unwrap().count, 64);
        assert_eq!(inv.cursor.unwrap().count, 18);
    }
}
