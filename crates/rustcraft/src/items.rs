//! Items, stacks, inventories, tools and creative tabs.
//!
//! Item properties (stack size, durability, food, tool rules, attack damage)
//! come from the data generator's item-component report (ADR-015).

use std::sync::OnceLock;

use ahash::{AHashMap, AHashSet};
use rc_core::BlockStateId;
use serde::{Deserialize, Serialize};

/// One tool rule: blocks (tag or id), mining speed, whether it yields drops.
#[derive(Clone, Debug)]
pub struct ToolRule {
    pub blocks: AHashSet<u16>,
    pub speed: Option<f32>,
    pub correct: Option<bool>,
}

/// Creative inventory tabs (vanilla order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    Building,
    Colored,
    Natural,
    Functional,
    Redstone,
    Tools,
    Combat,
    Food,
    Ingredients,
    SpawnEggs,
}

impl Tab {
    pub const TOP: [Tab; 5] = [
        Tab::Building,
        Tab::Colored,
        Tab::Natural,
        Tab::Functional,
        Tab::Redstone,
    ];
    pub const BOTTOM: [Tab; 5] = [
        Tab::Tools,
        Tab::Combat,
        Tab::Food,
        Tab::Ingredients,
        Tab::SpawnEggs,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Building => "Building Blocks",
            Self::Colored => "Colored Blocks",
            Self::Natural => "Natural Blocks",
            Self::Functional => "Functional Blocks",
            Self::Redstone => "Redstone Blocks",
            Self::Tools => "Tools & Utilities",
            Self::Combat => "Combat",
            Self::Food => "Food & Drinks",
            Self::Ingredients => "Ingredients",
            Self::SpawnEggs => "Spawn Eggs",
        }
    }

    #[must_use]
    pub fn icon(self) -> &'static str {
        match self {
            Self::Building => "bricks",
            Self::Colored => "cyan_wool",
            Self::Natural => "grass_block",
            Self::Functional => "crafting_table",
            Self::Redstone => "redstone",
            Self::Tools => "iron_pickaxe",
            Self::Combat => "iron_sword",
            Self::Food => "apple",
            Self::Ingredients => "iron_ingot",
            Self::SpawnEggs => "pig_spawn_egg",
        }
    }

    /// Redstone and spawn eggs arrive in v0.3.
    #[must_use]
    pub fn available(self) -> bool {
        !matches!(self, Self::Redstone | Self::SpawnEggs)
    }
}

#[derive(Debug)]
pub struct Items {
    pub names: Vec<String>,
    pub max_stack: Vec<u8>,
    pub durability: Vec<u16>,
    /// (nutrition, saturation)
    pub food: Vec<Option<(u8, f32)>>,
    pub tool: Vec<Vec<ToolRule>>,
    pub attack: Vec<f32>,
    /// Block placed by the item.
    pub block: Vec<Option<BlockStateId>>,
    by_name: AHashMap<String, u16>,
    pub tabs: AHashMap<Tab, Vec<u16>>,
    /// Everything shown in the Search tab.
    pub all: Vec<u16>,
}

#[derive(serde::Deserialize)]
struct RawItem {
    name: String,
    stack: u8,
    #[serde(default)]
    durability: Option<u16>,
    #[serde(default)]
    food: Option<(u8, f32)>,
    #[serde(default)]
    tool: Vec<(String, Option<f32>, Option<bool>)>,
    #[serde(default)]
    attack: Option<f32>,
}

/// Collect `minecraft:<name>` strings from worldgen JSON.
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

const COLORS: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

fn is_redstone(n: &str) -> bool {
    [
        "redstone",
        "repeater",
        "comparator",
        "piston",
        "observer",
        "hopper",
        "dropper",
        "dispenser",
        "lever",
        "button",
        "pressure_plate",
        "tripwire",
        "daylight_detector",
        "target",
        "sculk_sensor",
        "rail",
        "minecart",
        "tnt",
        "note_block",
        "lightning_rod",
        "trapped_chest",
        "crafter",
        "calibrated",
        "copper_bulb",
        "slime_block",
        "honey_block",
    ]
    .iter()
    .any(|k| n.contains(k))
}

fn category(n: &str, is_block: bool, natural: &AHashSet<String>) -> Option<Tab> {
    let has = |k: &str| n.contains(k);
    if n.ends_with("spawn_egg") {
        return Some(Tab::SpawnEggs);
    }
    if matches!(
        n,
        "air"
            | "debug_stick"
            | "knowledge_book"
            | "barrier"
            | "light"
            | "structure_void"
            | "structure_block"
            | "jigsaw"
            | "command_block"
            | "chain_command_block"
            | "repeating_command_block"
            | "command_block_minecart"
            | "spawner"
            | "trial_spawner"
            | "vault"
            | "petrified_oak_slab"
            | "player_head"
            | "bedrock"
            | "end_portal_frame"
            | "reinforced_deepslate"
            | "budding_amethyst"
            | "frogspawn"
            | "farmland"
            | "dirt_path"
    ) || n.starts_with("infested_")
    {
        return None;
    }
    if is_redstone(n) && !has("ore") && !n.ends_with("_block") || n == "redstone_block" {
        return Some(Tab::Redstone);
    }
    let colored = COLORS.iter().any(|c| n.starts_with(&format!("{c}_")))
        && [
            "wool",
            "carpet",
            "terracotta",
            "concrete",
            "stained_glass",
            "shulker_box",
            "bed",
            "candle",
            "banner",
        ]
        .iter()
        .any(|k| has(k));
    if colored
        || matches!(
            n,
            "terracotta" | "glass" | "tinted_glass" | "glass_pane" | "shulker_box" | "candle"
        )
    {
        return Some(Tab::Colored);
    }
    if n.ends_with("_sword")
        || n.ends_with("_helmet")
        || n.ends_with("_chestplate")
        || n.ends_with("_leggings")
        || n.ends_with("_boots")
        || n.ends_with("bow")
        || n == "trident"
        || n.ends_with("arrow")
        || n == "shield"
        || n.ends_with("horse_armor")
        || n == "totem_of_undying"
        || n == "mace"
        || n == "wind_charge"
        || n == "snowball"
        || n == "egg"
        || n == "wolf_armor"
        || n == "turtle_helmet"
    {
        return Some(Tab::Combat);
    }
    if n.ends_with("_pickaxe")
        || n.ends_with("_axe")
        || n.ends_with("_shovel")
        || n.ends_with("_hoe")
        || matches!(
            n,
            "shears"
                | "flint_and_steel"
                | "fishing_rod"
                | "bucket"
                | "water_bucket"
                | "lava_bucket"
                | "powder_snow_bucket"
                | "compass"
                | "recovery_compass"
                | "clock"
                | "spyglass"
                | "lead"
                | "name_tag"
                | "map"
                | "writable_book"
                | "saddle"
                | "carrot_on_a_stick"
                | "warped_fungus_on_a_stick"
                | "brush"
                | "bundle"
                | "elytra"
                | "firework_rocket"
                | "ender_pearl"
                | "ender_eye"
                | "bone_meal"
                | "book"
                | "milk_bucket"
                | "cod_bucket"
                | "salmon_bucket"
                | "tropical_fish_bucket"
                | "pufferfish_bucket"
                | "axolotl_bucket"
                | "tadpole_bucket"
        )
        || n.ends_with("_boat")
        || n.ends_with("_raft")
        || n.ends_with("music_disc")
        || n.starts_with("music_disc")
    {
        return Some(Tab::Tools);
    }
    if !is_block {
        let food = items_food_name(n);
        return Some(if food { Tab::Food } else { Tab::Ingredients });
    }
    let functional = [
        "crafting_table",
        "furnace",
        "smoker",
        "chest",
        "barrel",
        "anvil",
        "grindstone",
        "stonecutter",
        "loom",
        "cartography_table",
        "smithing_table",
        "fletching_table",
        "brewing_stand",
        "enchanting_table",
        "bookshelf",
        "lectern",
        "beacon",
        "campfire",
        "lantern",
        "torch",
        "sign",
        "_bed",
        "ladder",
        "scaffolding",
        "composter",
        "cauldron",
        "bell",
        "flower_pot",
        "decorated_pot",
        "jukebox",
        "respawn_anchor",
        "lodestone",
        "conduit",
        "item_frame",
        "painting",
        "armor_stand",
        "_head",
        "skull",
        "ender_chest",
        "end_rod",
        "chain",
        "bee_nest",
        "beehive",
        "suspicious",
        "end_crystal",
        "cake",
        "shulker_box",
        "heavy_core",
        "glow_lichen",
        "sculk_shrieker",
        "sculk_catalyst",
    ];
    if functional.iter().any(|k| has(k)) && !has("_wall_") && !has("pumpkin") {
        return Some(Tab::Functional);
    }
    let not_natural = [
        "glass",
        "planks",
        "fence",
        "slab",
        "stairs",
        "_wall",
        "brick",
        "polished",
        "smooth",
        "cut_",
        "chiseled",
        "door",
        "pillar",
        "tiles",
        "lamp",
        "_block",
        "cobblestone",
        "concrete",
        "pane",
        "bars",
        "carpet",
        "wool",
        "copper",
        "button",
        "pressure_plate",
        "rail",
        "stripped",
        "dried_kelp",
    ];
    let natural_extra = n.ends_with("_ore")
        || matches!(
            n,
            "coal_block"
                | "iron_block"
                | "gold_block"
                | "diamond_block"
                | "moss_block"
                | "snow_block"
                | "mushroom_stem"
                | "red_mushroom_block"
                | "brown_mushroom_block"
                | "bone_block"
                | "amethyst_block"
                | "raw_iron_block"
                | "raw_copper_block"
                | "raw_gold_block"
                | "dripstone_block"
                | "honeycomb_block"
                | "melon"
                | "pumpkin"
                | "hay_block"
                | "sculk"
                | "magma_block"
                | "mossy_cobblestone"
                | "packed_ice"
                | "blue_ice"
                | "nether_wart_block"
                | "warped_wart_block"
                | "shroomlight"
                | "obsidian"
                | "crying_obsidian"
        );
    if natural.contains(n)
        && (natural_extra || !not_natural.iter().any(|k| has(k)) || n.ends_with("_ore"))
    {
        return Some(Tab::Natural);
    }
    Some(Tab::Building)
}

fn items_food_name(n: &str) -> bool {
    // Refined by the food component after loading.
    n.contains("cooked") || n.contains("stew") || n.contains("soup") || n == "potion"
}

impl Items {
    fn load() -> Self {
        let files = rc_data::files();
        let raw: Vec<RawItem> =
            serde_json::from_slice(files.get("items.json").expect("items.json"))
                .expect("valid items.json");
        let blocks = rc_data::blocks();
        let tags = rc_data::tags();
        let resolve = |spec: &str| -> AHashSet<u16> {
            let mut set = AHashSet::new();
            for part in spec.split(',') {
                let part = part.trim();
                if let Some(t) = part.strip_prefix('#') {
                    let t = t.strip_prefix("minecraft:").unwrap_or(t);
                    for b in tags.block_list(t) {
                        set.insert(b);
                    }
                } else if let Some(b) =
                    blocks.index_of(part.strip_prefix("minecraft:").unwrap_or(part))
                {
                    set.insert(b);
                }
            }
            set
        };
        let mut s = Self {
            names: Vec::new(),
            max_stack: Vec::new(),
            durability: Vec::new(),
            food: Vec::new(),
            tool: Vec::new(),
            attack: Vec::new(),
            block: Vec::new(),
            by_name: AHashMap::new(),
            tabs: AHashMap::new(),
            all: Vec::new(),
        };
        for (i, r) in raw.iter().enumerate() {
            s.by_name.insert(r.name.clone(), i as u16);
            s.names.push(r.name.clone());
            s.max_stack.push(r.stack.max(1));
            s.durability.push(r.durability.unwrap_or(0));
            s.food.push(r.food);
            s.tool.push(
                r.tool
                    .iter()
                    .map(|(b, sp, c)| ToolRule {
                        blocks: resolve(b),
                        speed: *sp,
                        correct: *c,
                    })
                    .collect(),
            );
            s.attack.push(r.attack.unwrap_or(0.0));
            s.block.push(
                blocks
                    .index_of(&r.name)
                    .map(|b| blocks.defs[b as usize].default),
            );
        }
        // Naturally generated blocks: anything worldgen places.
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
        for i in 0..s.names.len() as u16 {
            let n = s.names[i as usize].clone();
            let is_block = s.block[i as usize].is_some();
            let Some(mut tab) = category(&n, is_block, &found) else {
                continue;
            };
            if !is_block && s.food[i as usize].is_some() && tab == Tab::Ingredients {
                tab = Tab::Food;
            }
            s.tabs.entry(tab).or_default().push(i);
            if tab.available() {
                s.all.push(i);
            }
        }
        s
    }

    #[must_use]
    pub fn id(&self, name: &str) -> Option<u16> {
        self.by_name
            .get(name.strip_prefix("minecraft:").unwrap_or(name))
            .copied()
    }

    #[must_use]
    pub fn name(&self, id: u16) -> &str {
        self.names.get(id as usize).map_or("air", String::as_str)
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
            "soul_wall_torch" => "soul_torch",
            "redstone_wire" => "redstone",
            "tall_seagrass" => "seagrass",
            "kelp_plant" => "kelp",
            "cave_vines_plant" | "cave_vines" => "glow_berries",
            "twisting_vines_plant" => "twisting_vines",
            "weeping_vines_plant" => "weeping_vines",
            "big_dripleaf_stem" => "big_dripleaf",
            "bamboo_sapling" => "bamboo",
            "water" => "water_bucket",
            "lava" => "lava_bucket",
            "wheat" => "wheat_seeds",
            "carrots" => "carrot",
            "potatoes" => "potato",
            "beetroots" => "beetroot_seeds",
            "melon_stem" | "attached_melon_stem" => "melon_seeds",
            "pumpkin_stem" | "attached_pumpkin_stem" => "pumpkin_seeds",
            "sweet_berry_bush" => "sweet_berries",
            "cocoa" => "cocoa_beans",
            n if n.ends_with("_wall_sign") => return self.id(&n.replace("_wall_sign", "_sign")),
            n if n.ends_with("_wall_hanging_sign") => {
                return self.id(&n.replace("_wall_hanging_sign", "_hanging_sign"));
            }
            n if n.ends_with("_wall_banner") => {
                return self.id(&n.replace("_wall_banner", "_banner"));
            }
            n if n.ends_with("_wall_head") => return self.id(&n.replace("_wall_head", "_head")),
            n if n.ends_with("_wall_skull") => return self.id(&n.replace("_wall_skull", "_skull")),
            n if n.starts_with("potted_") => return self.id(&n["potted_".len()..]),
            n => n,
        };
        self.id(alias)
    }

    /// (mining speed, yields drops) of `item` (None = bare hand) on `state`.
    #[must_use]
    pub fn mining(&self, item: Option<u16>, state: BlockStateId) -> (f32, bool) {
        let b = rc_data::blocks();
        let block = b.info(state).block;
        let needs_tool = !b.def(state).harvest_tools.is_empty();
        let mut speed = 1.0;
        let mut correct = !needs_tool;
        if let Some(it) = item {
            for r in self.tool.get(it as usize).into_iter().flatten() {
                if r.blocks.contains(&block) {
                    if let Some(sp) = r.speed {
                        speed = sp;
                    }
                    if let Some(c) = r.correct {
                        correct = c || !needs_tool;
                    }
                    break;
                }
            }
        }
        (speed, correct)
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
    /// Durability used up (tools/armour).
    #[serde(default)]
    pub damage: u16,
}

impl Stack {
    #[must_use]
    pub fn new(item: u16, count: u8) -> Self {
        Self {
            item,
            count,
            damage: 0,
        }
    }

    #[must_use]
    pub fn max(&self) -> u8 {
        items()
            .max_stack
            .get(self.item as usize)
            .copied()
            .unwrap_or(64)
            .max(1)
    }

    #[must_use]
    pub fn stacks_with(&self, o: &Stack) -> bool {
        self.item == o.item && self.damage == o.damage && self.max() > 1
    }
}

pub const MAIN: usize = 36;
/// 36..40 armour (feet, legs, chest, head), 40 offhand.
pub const SLOTS: usize = 41;

/// Player inventory: 0..9 hotbar, 9..36 main, 36..40 armour, 40 offhand.
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
            slots: vec![None; SLOTS],
            selected: 0,
            cursor: None,
        }
    }
}

impl Inventory {
    /// Older saves had 36 slots.
    pub fn normalize(&mut self) {
        self.slots.resize(SLOTS, None);
    }

    #[must_use]
    pub fn held(&self) -> Option<Stack> {
        self.slots[self.selected]
    }

    /// Add a stack; returns what did not fit.
    pub fn add_stack(&mut self, mut st: Stack) -> Option<Stack> {
        let max = st.max();
        for i in (0..MAIN).chain(std::iter::once(40)) {
            if let Some(s) = &mut self.slots[i]
                && s.stacks_with(&st)
                && s.count < max
            {
                let n = (max - s.count).min(st.count);
                s.count += n;
                st.count -= n;
                if st.count == 0 {
                    return None;
                }
            }
        }
        for i in 0..MAIN {
            if self.slots[i].is_none() {
                let n = max.min(st.count);
                self.slots[i] = Some(Stack { count: n, ..st });
                st.count -= n;
                if st.count == 0 {
                    return None;
                }
            }
        }
        Some(st)
    }

    /// Add items; returns the count that did not fit.
    pub fn add(&mut self, item: u16, count: u8) -> u8 {
        self.add_stack(Stack::new(item, count))
            .map_or(0, |s| s.count)
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

    /// Damage the held tool; returns true if it broke.
    pub fn damage_held(&mut self, amount: u16) -> bool {
        let Some(s) = &mut self.slots[self.selected] else {
            return false;
        };
        let max = items()
            .durability
            .get(s.item as usize)
            .copied()
            .unwrap_or(0);
        if max == 0 {
            return false;
        }
        s.damage += amount;
        if s.damage >= max {
            self.slots[self.selected] = None;
            return true;
        }
        false
    }

    /// Replace the held stack.
    pub fn set_held(&mut self, s: Option<Stack>) {
        self.slots[self.selected] = s;
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
        self.slots[slot] = Some(Stack::new(item, 1));
    }

    /// Vanilla-style click on a slot with the cursor stack.
    pub fn click(&mut self, slot: usize, right: bool) {
        click_slot(&mut self.slots[slot], &mut self.cursor, right);
    }

    /// Shift-click: move between hotbar and main inventory.
    pub fn quick_move(&mut self, slot: usize) {
        let Some(st) = self.slots[slot].take() else {
            return;
        };
        let range: Vec<usize> = if slot < 9 {
            (9..MAIN).collect()
        } else {
            (0..9).collect()
        };
        let mut st = st;
        for &i in &range {
            if let Some(s) = &mut self.slots[i]
                && s.stacks_with(&st)
                && s.count < s.max()
            {
                let n = (s.max() - s.count).min(st.count);
                s.count += n;
                st.count -= n;
            }
            if st.count == 0 {
                return;
            }
        }
        for &i in &range {
            if self.slots[i].is_none() {
                self.slots[i] = Some(st);
                return;
            }
        }
        self.slots[slot] = Some(st);
    }
}

/// Vanilla click semantics on any slot.
pub fn click_slot(slot: &mut Option<Stack>, cursor: &mut Option<Stack>, right: bool) {
    match (*cursor, *slot, right) {
        (None, Some(st), false) => {
            *cursor = Some(st);
            *slot = None;
        }
        (None, Some(st), true) => {
            let take = st.count.div_ceil(2);
            *cursor = Some(Stack { count: take, ..st });
            *slot = (st.count > take).then_some(Stack {
                count: st.count - take,
                ..st
            });
        }
        (Some(c), None, false) => {
            *slot = Some(c);
            *cursor = None;
        }
        (Some(c), None, true) => {
            *slot = Some(Stack { count: 1, ..c });
            *cursor = (c.count > 1).then_some(Stack {
                count: c.count - 1,
                ..c
            });
        }
        (Some(c), Some(st), r) if c.stacks_with(&st) => {
            let m = st.max();
            let moving = if r { 1 } else { c.count };
            let n = moving.min(m.saturating_sub(st.count));
            *slot = Some(Stack {
                count: st.count + n,
                ..st
            });
            let left = c.count - n;
            *cursor = (left > 0).then_some(Stack { count: left, ..c });
        }
        (Some(c), Some(st), _) => {
            *slot = Some(c);
            *cursor = Some(st);
        }
        (None, None, _) => {}
    }
}

/// Ticks to break `state` holding `item` (vanilla formula). `None` = unbreakable.
#[must_use]
pub fn break_ticks(
    state: BlockStateId,
    item: Option<u16>,
    in_water: bool,
    on_ground: bool,
) -> Option<u32> {
    let def = rc_data::blocks().def(state);
    if def.hardness < 0.0 {
        return None;
    }
    if def.hardness == 0.0 {
        return Some(0);
    }
    let (mut speed, correct) = items().mining(item, state);
    if in_water {
        speed /= 5.0;
    }
    if !on_ground {
        speed /= 5.0;
    }
    let dmg = speed / def.hardness / if correct { 30.0 } else { 100.0 };
    if dmg >= 1.0 {
        return Some(0);
    }
    Some((1.0 / dmg).ceil() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_and_tools() {
        let it = items();
        let nat: Vec<&str> = it.tabs[&Tab::Natural].iter().map(|&i| it.name(i)).collect();
        for n in [
            "stone",
            "dirt",
            "oak_log",
            "diamond_ore",
            "sand",
            "oak_leaves",
        ] {
            assert!(nat.contains(&n), "{n} missing from natural");
        }
        for n in ["glass", "oak_planks", "cobblestone"] {
            assert!(!nat.contains(&n), "{n} should not be natural");
        }
        let b = rc_data::blocks();
        let stone = b.default_state("stone");
        let pick = it.id("wooden_pickaxe");
        assert_eq!(it.mining(None, stone), (1.0, false));
        assert_eq!(it.mining(pick, stone), (2.0, true));
        // Vanilla: stone by hand 7.5 s = 150 ticks; wooden pickaxe 1.15 s.
        assert_eq!(break_ticks(stone, None, false, true), Some(150));
        assert_eq!(break_ticks(stone, pick, false, true), Some(23));
        assert_eq!(it.durability[it.id("iron_pickaxe").unwrap() as usize], 250);
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
