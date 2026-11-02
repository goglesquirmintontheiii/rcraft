//! Loot tables (vanilla JSON format from the data generator): block drops
//! and mob drops.

use ahash::AHashMap;
use rand::Rng as _;
use rc_core::BlockStateId;
use serde_json::Value;

use crate::items::{Stack, items};

/// Context for evaluating conditions.
pub struct LootCtx<'a> {
    pub tool: Option<Stack>,
    pub state: Option<BlockStateId>,
    pub silk_touch: bool,
    pub rng: &'a mut rand::rngs::ThreadRng,
}

impl std::fmt::Debug for LootCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LootCtx")
    }
}

fn number(v: &Value, rng: &mut rand::rngs::ThreadRng) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(1.0),
        Value::Object(o) => match o
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("minecraft:uniform")
        {
            "minecraft:binomial" => {
                let n = o.get("n").map_or(1.0, |x| number(x, rng)) as u32;
                let p = o.get("p").map_or(0.5, |x| number(x, rng));
                (0..n).filter(|_| rng.random::<f64>() < p).count() as f64
            }
            "minecraft:constant" => o.get("value").map_or(1.0, |x| number(x, rng)),
            _ => {
                let lo = o.get("min").map_or(0.0, |x| number(x, rng));
                let hi = o.get("max").map_or(lo, |x| number(x, rng));
                if hi <= lo {
                    lo
                } else {
                    rng.random_range(lo..=hi)
                }
            }
        },
        _ => 1.0,
    }
}

fn tool_matches(pred: &Value, cx: &LootCtx<'_>) -> bool {
    let it = items();
    if let Some(list) = pred.get("items") {
        let Some(tool) = cx.tool else { return false };
        let name = it.name(tool.item);
        let ok = match list {
            Value::String(s) if s.starts_with('#') => {
                let tag = s.trim_start_matches('#').trim_start_matches("minecraft:");
                rc_data::tags().item_list(tag).iter().any(|n| n == name)
            }
            Value::String(s) => s.trim_start_matches("minecraft:") == name,
            Value::Array(a) => a
                .iter()
                .filter_map(Value::as_str)
                .any(|s| s.trim_start_matches("minecraft:") == name),
            _ => false,
        };
        if !ok {
            return false;
        }
    }
    if pred.pointer("/predicates/minecraft:enchantments").is_some() {
        return cx.silk_touch;
    }
    true
}

fn condition(c: &Value, cx: &mut LootCtx<'_>) -> bool {
    let kind = c.get("condition").and_then(Value::as_str).unwrap_or("");
    match kind {
        "minecraft:survives_explosion" | "minecraft:killed_by_player" => true,
        "minecraft:inverted" => !c.get("term").is_some_and(|t| condition(t, cx)),
        "minecraft:any_of" | "minecraft:alternative" => c
            .get("terms")
            .and_then(Value::as_array)
            .is_some_and(|t| t.iter().any(|x| condition(x, cx))),
        "minecraft:all_of" => c
            .get("terms")
            .and_then(Value::as_array)
            .is_none_or(|t| t.iter().all(|x| condition(x, cx))),
        "minecraft:match_tool" => c.get("predicate").is_some_and(|p| tool_matches(p, cx)),
        "minecraft:block_state_property" => {
            let Some(s) = cx.state else { return false };
            let b = rc_data::blocks();
            c.get("properties")
                .and_then(Value::as_object)
                .is_none_or(|props| {
                    props.iter().all(|(k, v)| {
                        let have = b.get(s, k).unwrap_or("");
                        match v {
                            Value::String(want) => have == want,
                            Value::Object(r) => {
                                let h: i64 = have.parse().unwrap_or(0);
                                let lo = r
                                    .get("min")
                                    .and_then(|x| {
                                        x.as_str().and_then(|s| s.parse().ok()).or(x.as_i64())
                                    })
                                    .unwrap_or(i64::MIN);
                                let hi = r
                                    .get("max")
                                    .and_then(|x| {
                                        x.as_str().and_then(|s| s.parse().ok()).or(x.as_i64())
                                    })
                                    .unwrap_or(i64::MAX);
                                h >= lo && h <= hi
                            }
                            _ => true,
                        }
                    })
                })
        }
        "minecraft:random_chance" => {
            let p = c.get("chance").map_or(1.0, |v| number(v, cx.rng));
            cx.rng.random::<f64>() < p
        }
        "minecraft:random_chance_with_enchanted_bonus" => {
            let p = c
                .get("unenchanted_chance")
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            cx.rng.random::<f64>() < p
        }
        "minecraft:table_bonus" => {
            let p = c
                .pointer("/chances/0")
                .and_then(Value::as_f64)
                .unwrap_or(1.0);
            cx.rng.random::<f64>() < p
        }
        "minecraft:entity_properties"
        | "minecraft:location_check"
        | "minecraft:damage_source_properties" => false,
        _ => true,
    }
}

fn conditions(e: &Value, cx: &mut LootCtx<'_>) -> bool {
    e.get("conditions")
        .and_then(Value::as_array)
        .is_none_or(|cs| cs.iter().all(|c| condition(c, cx)))
}

fn apply_functions(e: &Value, mut count: f64, cx: &mut LootCtx<'_>) -> f64 {
    for f in e
        .get("functions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if !conditions(f, cx) {
            continue;
        }
        match f.get("function").and_then(Value::as_str).unwrap_or("") {
            "minecraft:set_count" => {
                let n = f.get("count").map_or(1.0, |v| number(v, cx.rng));
                if f.get("add").and_then(Value::as_bool).unwrap_or(false) {
                    count += n;
                } else {
                    count = n;
                }
            }
            "minecraft:limit_count" => {
                if let Some(l) = f.get("limit") {
                    if let Some(lo) = l.get("min") {
                        count = count.max(number(lo, cx.rng));
                    }
                    if let Some(hi) = l.get("max") {
                        count = count.min(number(hi, cx.rng));
                    }
                }
            }
            _ => {}
        }
    }
    count
}

fn entry(e: &Value, cx: &mut LootCtx<'_>, out: &mut Vec<Stack>, depth: u32) -> bool {
    if depth > 8 || !conditions(e, cx) {
        return false;
    }
    match e.get("type").and_then(Value::as_str).unwrap_or("") {
        "minecraft:item" => {
            let Some(name) = e.get("name").and_then(Value::as_str) else {
                return false;
            };
            let Some(id) = items().id(name) else {
                return true;
            };
            let n = apply_functions(e, 1.0, cx).floor().max(0.0) as u32;
            let mut n = n;
            while n > 0 {
                let max = u32::from(Stack::new(id, 1).max());
                let k = n.min(max);
                out.push(Stack::new(id, k as u8));
                n -= k;
            }
            true
        }
        "minecraft:alternatives" => e
            .get("children")
            .and_then(Value::as_array)
            .is_some_and(|c| c.iter().any(|x| entry(x, cx, out, depth + 1))),
        "minecraft:group" => {
            for x in e
                .get("children")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                entry(x, cx, out, depth + 1);
            }
            true
        }
        "minecraft:sequence" => {
            for x in e
                .get("children")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if !entry(x, cx, out, depth + 1) {
                    break;
                }
            }
            true
        }
        "minecraft:loot_table" => {
            if let Some(t) = e.get("value").and_then(Value::as_str) {
                let t = t.trim_start_matches("minecraft:");
                if let Some(j) = rc_data::files().json(&format!("loot_table/{t}.json")) {
                    out.extend(roll_table(&j, cx));
                }
            }
            true
        }
        "minecraft:empty" => true,
        _ => false,
    }
}

/// Roll a full loot table.
pub fn roll_table(table: &Value, cx: &mut LootCtx<'_>) -> Vec<Stack> {
    let mut out = Vec::new();
    for pool in table
        .get("pools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if !conditions(pool, cx) {
            continue;
        }
        let rolls = pool.get("rolls").map_or(1.0, |v| number(v, cx.rng)).round() as u32;
        let entries: Vec<&Value> = pool
            .get("entries")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .collect();
        for _ in 0..rolls {
            // Weighted choice among entries whose conditions pass.
            let total: i64 = entries
                .iter()
                .map(|e| e.get("weight").and_then(Value::as_i64).unwrap_or(1))
                .sum();
            if entries.len() == 1 || total <= 1 {
                for e in &entries {
                    entry(e, cx, &mut out, 0);
                }
                continue;
            }
            let mut r = cx.rng.random_range(0..total);
            for e in &entries {
                let w = e.get("weight").and_then(Value::as_i64).unwrap_or(1);
                if r < w {
                    entry(e, cx, &mut out, 0);
                    break;
                }
                r -= w;
            }
        }
    }
    out
}

thread_local! {
    static CACHE: std::cell::RefCell<AHashMap<String, Option<Value>>> = std::cell::RefCell::new(AHashMap::new());
}

fn table(path: &str) -> Option<Value> {
    CACHE.with(|c| {
        c.borrow_mut()
            .entry(path.to_owned())
            .or_insert_with(|| rc_data::files().json(&format!("loot_table/{path}.json")))
            .clone()
    })
}

/// What breaking `state` with `tool` drops.
#[must_use]
pub fn block_drops(state: BlockStateId, tool: Option<Stack>) -> Vec<Stack> {
    let name = &rc_data::blocks().def(state).name;
    let Some(t) = table(&format!("blocks/{name}")) else {
        return Vec::new();
    };
    let mut rng = rand::rng();
    let mut cx = LootCtx {
        tool,
        state: Some(state),
        silk_touch: false,
        rng: &mut rng,
    };
    roll_table(&t, &mut cx)
}

/// What killing an entity of `kind` drops.
#[must_use]
pub fn entity_drops(kind: &str) -> Vec<Stack> {
    let Some(t) = table(&format!("entities/{kind}")) else {
        return Vec::new();
    };
    let mut rng = rand::rng();
    let mut cx = LootCtx {
        tool: None,
        state: None,
        silk_touch: false,
        rng: &mut rng,
    };
    roll_table(&t, &mut cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_block_drops() {
        let b = rc_data::blocks();
        let it = items();
        let d = block_drops(b.default_state("grass_block"), None);
        assert_eq!(d, vec![Stack::new(it.id("dirt").unwrap(), 1)]);
        let d = block_drops(
            b.default_state("stone"),
            it.id("wooden_pickaxe").map(|i| Stack::new(i, 1)),
        );
        assert_eq!(d, vec![Stack::new(it.id("cobblestone").unwrap(), 1)]);
        let d = block_drops(b.default_state("oak_log"), None);
        assert_eq!(d.len(), 1);
        // Gravel: flint 10% else gravel.
        let d = block_drops(b.default_state("gravel"), None);
        assert_eq!(d.len(), 1);
        let n = block_drops(b.default_state("glass"), None);
        assert!(n.is_empty());
    }
}
