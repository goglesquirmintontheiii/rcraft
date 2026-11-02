//! Recipes in the vanilla data-pack JSON format: shaped/shapeless crafting,
//! smelting family and stonecutting.

use std::sync::OnceLock;

use ahash::{AHashMap, AHashSet};
use serde_json::Value;

use crate::items::{Stack, items};

/// Set of item ids accepted by one ingredient slot.
pub type Ingredient = AHashSet<u16>;

#[derive(Clone, Debug)]
pub enum Recipe {
    Shaped {
        w: usize,
        h: usize,
        /// Row-major, `None` = empty.
        cells: Vec<Option<Ingredient>>,
        result: Stack,
    },
    Shapeless {
        ingredients: Vec<Ingredient>,
        result: Stack,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Cooker {
    Furnace,
    Blast,
    Smoker,
    Campfire,
}

#[derive(Clone, Debug)]
pub struct Cooking {
    pub input: Ingredient,
    pub result: Stack,
    pub xp: f32,
    pub ticks: u32,
}

#[derive(Debug, Default)]
pub struct Recipes {
    pub crafting: Vec<Recipe>,
    pub cooking: AHashMap<Cooker, Vec<Cooking>>,
    pub stonecutting: Vec<(Ingredient, Stack)>,
}

fn ingredient(v: &Value) -> Ingredient {
    let it = items();
    let mut set = AHashSet::new();
    let mut one = |o: &Value| {
        if let Some(i) = o.get("item").and_then(Value::as_str) {
            if let Some(id) = it.id(i) {
                set.insert(id);
            }
        } else if let Some(t) = o.get("tag").and_then(Value::as_str) {
            for n in rc_data::tags().item_list(t) {
                if let Some(id) = it.id(n) {
                    set.insert(id);
                }
            }
        }
    };
    match v {
        Value::Array(a) => a.iter().for_each(&mut one),
        o => one(o),
    }
    set
}

fn result(v: &Value) -> Option<Stack> {
    let id = v
        .get("id")
        .or_else(|| v.get("item"))
        .and_then(Value::as_str)?;
    let id = items().id(id)?;
    let count = v.get("count").and_then(Value::as_u64).unwrap_or(1) as u8;
    Some(Stack::new(id, count.max(1)))
}

impl Recipes {
    fn load() -> Self {
        let files = rc_data::files();
        let mut r = Self::default();
        for name in files.list("recipe") {
            let Some(j) = files.json(&format!("recipe/{name}.json")) else {
                continue;
            };
            let kind = j["type"].as_str().unwrap_or("");
            match kind {
                "minecraft:crafting_shaped" => {
                    let Some(res) = result(&j["result"]) else {
                        continue;
                    };
                    let pattern: Vec<Vec<char>> = j["pattern"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|p| p.as_str().map(|s| s.chars().collect()))
                        .collect();
                    let h = pattern.len();
                    let w = pattern.iter().map(Vec::len).max().unwrap_or(0);
                    let key: AHashMap<char, Ingredient> = j["key"]
                        .as_object()
                        .into_iter()
                        .flatten()
                        .filter_map(|(k, v)| Some((k.chars().next()?, ingredient(v))))
                        .collect();
                    let mut cells = Vec::with_capacity(w * h);
                    for row in &pattern {
                        for x in 0..w {
                            let c = row.get(x).copied().unwrap_or(' ');
                            cells.push(if c == ' ' { None } else { key.get(&c).cloned() });
                        }
                    }
                    r.crafting.push(Recipe::Shaped {
                        w,
                        h,
                        cells,
                        result: res,
                    });
                }
                "minecraft:crafting_shapeless" => {
                    let Some(res) = result(&j["result"]) else {
                        continue;
                    };
                    let ingredients = j["ingredients"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(ingredient)
                        .collect();
                    r.crafting.push(Recipe::Shapeless {
                        ingredients,
                        result: res,
                    });
                }
                "minecraft:smelting"
                | "minecraft:blasting"
                | "minecraft:smoking"
                | "minecraft:campfire_cooking" => {
                    let Some(res) = result(&j["result"]) else {
                        continue;
                    };
                    let c = match kind {
                        "minecraft:blasting" => Cooker::Blast,
                        "minecraft:smoking" => Cooker::Smoker,
                        "minecraft:campfire_cooking" => Cooker::Campfire,
                        _ => Cooker::Furnace,
                    };
                    r.cooking.entry(c).or_default().push(Cooking {
                        input: ingredient(&j["ingredient"]),
                        result: res,
                        xp: j["experience"].as_f64().unwrap_or(0.0) as f32,
                        ticks: j["cookingtime"].as_u64().unwrap_or(200) as u32,
                    });
                }
                "minecraft:stonecutting" => {
                    if let Some(res) = result(&j["result"]) {
                        r.stonecutting.push((ingredient(&j["ingredient"]), res));
                    }
                }
                _ => {}
            }
        }
        r
    }

    /// Match a crafting grid (`size`×`size`, row-major).
    #[must_use]
    pub fn craft(&self, grid: &[Option<Stack>], size: usize) -> Option<Stack> {
        let filled: Vec<(usize, usize)> = (0..size * size)
            .filter(|&i| grid[i].is_some())
            .map(|i| (i % size, i / size))
            .collect();
        if filled.is_empty() {
            return None;
        }
        let x0 = filled.iter().map(|p| p.0).min()?;
        let x1 = filled.iter().map(|p| p.0).max()?;
        let y0 = filled.iter().map(|p| p.1).min()?;
        let y1 = filled.iter().map(|p| p.1).max()?;
        let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
        let at = |x: usize, y: usize| grid[(y0 + y) * size + x0 + x].map(|s| s.item);
        for r in &self.crafting {
            match r {
                Recipe::Shaped {
                    w,
                    h,
                    cells,
                    result,
                } => {
                    if *w != bw || *h != bh {
                        continue;
                    }
                    for mirror in [false, true] {
                        let ok = (0..*h).all(|y| {
                            (0..*w).all(|x| {
                                let cx = if mirror { w - 1 - x } else { x };
                                match (&cells[y * w + cx], at(x, y)) {
                                    (None, None) => true,
                                    (Some(ing), Some(it)) => ing.contains(&it),
                                    _ => false,
                                }
                            })
                        });
                        if ok {
                            return Some(*result);
                        }
                    }
                }
                Recipe::Shapeless {
                    ingredients,
                    result,
                } => {
                    if ingredients.len() != filled.len() {
                        continue;
                    }
                    let mut used = vec![false; ingredients.len()];
                    let ok = filled.iter().all(|&(x, y)| {
                        let it = grid[y * size + x].map(|s| s.item).unwrap_or(0);
                        if let Some(k) = (0..ingredients.len())
                            .find(|&k| !used[k] && ingredients[k].contains(&it))
                        {
                            used[k] = true;
                            true
                        } else {
                            false
                        }
                    });
                    if ok {
                        return Some(*result);
                    }
                }
            }
        }
        None
    }

    #[must_use]
    pub fn cook(&self, c: Cooker, input: u16) -> Option<&Cooking> {
        self.cooking
            .get(&c)?
            .iter()
            .find(|r| r.input.contains(&input))
    }
}

pub fn recipes() -> &'static Recipes {
    static R: OnceLock<Recipes> = OnceLock::new();
    R.get_or_init(Recipes::load)
}

/// Furnace fuel burn time in ticks (vanilla values, minecraft.wiki "Fuel").
#[must_use]
pub fn fuel_ticks(item: u16) -> u32 {
    let n = items().name(item);
    let in_tag = |t: &str| rc_data::tags().item_list(t).iter().any(|x| x == n);
    match n {
        "lava_bucket" => 20000,
        "coal_block" => 16000,
        "dried_kelp_block" => 4001,
        "blaze_rod" => 2400,
        "coal" | "charcoal" => 1600,
        "bamboo" | "scaffolding" => 50,
        "stick" => 100,
        _ if in_tag("logs") || in_tag("planks") || in_tag("wooden_slabs") && false => 300,
        _ if n.ends_with("_slab") && in_tag("wooden_slabs") => 150,
        _ if n.contains("_boat") => 1200,
        _ if in_tag("wooden_stairs") || in_tag("wooden_fences") || n.ends_with("fence_gate") => 300,
        _ if n == "crafting_table"
            || n == "chest"
            || n == "bookshelf"
            || n == "barrel"
            || n == "jukebox" =>
        {
            300
        }
        _ if n.ends_with("_sapling") || in_tag("wool") || n.ends_with("_carpet") => 100,
        _ if n.starts_with("wooden_") => 200,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crafting_basics() {
        let it = items();
        let r = recipes();
        let log = Stack::new(it.id("oak_log").unwrap(), 1);
        let mut g = vec![None; 4];
        g[3] = Some(log);
        assert_eq!(
            r.craft(&g, 2),
            Some(Stack::new(it.id("oak_planks").unwrap(), 4))
        );
        let p = Some(Stack::new(it.id("oak_planks").unwrap(), 1));
        let g = vec![p, p, p, p];
        assert_eq!(
            r.craft(&g, 2).map(|s| it.name(s.item).to_owned()),
            Some("crafting_table".to_owned())
        );
        let c = Some(Stack::new(it.id("cobblestone").unwrap(), 1));
        let s = Some(Stack::new(it.id("stick").unwrap(), 1));
        let g = vec![c, c, c, None, s, None, None, s, None];
        assert_eq!(
            r.craft(&g, 3).map(|s| it.name(s.item).to_owned()),
            Some("stone_pickaxe".to_owned())
        );
        let raw = it.id("raw_iron").unwrap();
        assert_eq!(
            r.cook(Cooker::Furnace, raw).map(|c| it.name(c.result.item)),
            Some("iron_ingot")
        );
        assert_eq!(fuel_ticks(it.id("coal").unwrap()), 1600);
        assert_eq!(fuel_ticks(it.id("oak_planks").unwrap()), 300);
    }
}
