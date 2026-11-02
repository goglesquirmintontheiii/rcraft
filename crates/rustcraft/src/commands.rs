//! Chat commands with target selectors and Brigadier-style suggestions.

use rc_core::BlockPos;

use crate::entity::{Entity, Kind};
use crate::game::{Difficulty, Game};
use crate::items::{Inventory, items};
use crate::mobs::{self, MobKind};
use crate::player::GameMode;

/// Argument kinds used by the suggestion engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arg {
    Lit(&'static str),
    Choice(&'static [&'static str]),
    Target,
    Pos,
    Item,
    Entity,
    Int,
    Rule,
    Text,
}

/// The command tree: (command, alternative argument lists).
fn tree() -> Vec<(&'static str, Vec<Vec<Arg>>)> {
    use Arg::*;
    const MODES: &[&str] = &["survival", "creative", "adventure", "spectator"];
    const TIMES: &[&str] = &["day", "noon", "night", "midnight"];
    vec![
        ("help", vec![vec![]]),
        ("gamemode", vec![vec![Choice(MODES), Target]]),
        (
            "tp",
            vec![
                vec![Pos],
                vec![Target, Pos],
                vec![Target, Target],
                vec![Target],
            ],
        ),
        (
            "teleport",
            vec![vec![Pos], vec![Target, Pos], vec![Target, Target]],
        ),
        ("give", vec![vec![Target, Item, Int]]),
        ("clear", vec![vec![Target]]),
        ("kill", vec![vec![Target]]),
        (
            "time",
            vec![
                vec![Lit("set"), Choice(TIMES)],
                vec![Lit("add"), Int],
                vec![Lit("query"), Choice(&["daytime", "gametime", "day"])],
            ],
        ),
        (
            "weather",
            vec![vec![Choice(&["clear", "rain", "thunder"]), Int]],
        ),
        ("summon", vec![vec![Entity, Pos]]),
        ("seed", vec![vec![]]),
        ("spawnpoint", vec![vec![Target, Pos]]),
        ("setworldspawn", vec![vec![Pos]]),
        (
            "difficulty",
            vec![vec![Choice(&["peaceful", "easy", "normal", "hard"])]],
        ),
        ("gamerule", vec![vec![Rule, Choice(&["true", "false"])]]),
        (
            "xp",
            vec![vec![Lit("add"), Target, Int, Choice(&["points", "levels"])]],
        ),
        ("say", vec![vec![Text]]),
        ("me", vec![vec![Text]]),
    ]
}

const RULES: &[&str] = &[
    "doDaylightCycle",
    "doWeatherCycle",
    "keepInventory",
    "doMobLoot",
    "doTileDrops",
    "randomTickSpeed",
    "doMobSpawning",
    "naturalRegeneration",
    "doImmediateRespawn",
];

const SELECTORS: &[&str] = &["@s", "@p", "@a", "@r", "@e", "Player"];

/// Suggestions for the partially typed command line (without `/`), and an
/// argument hint like `<x> <y> <z>`.
#[must_use]
pub fn suggest(line: &str) -> (Vec<String>, String) {
    let ends_space = line.ends_with(' ');
    let mut toks: Vec<&str> = line.split_whitespace().collect();
    let current = if ends_space || toks.is_empty() {
        ""
    } else {
        toks.pop().unwrap_or("")
    };
    let t = tree();
    if toks.is_empty() {
        let mut v: Vec<String> = t
            .iter()
            .map(|c| c.0.to_owned())
            .filter(|c| c.starts_with(current))
            .collect();
        v.sort();
        return (v, String::new());
    }
    let Some((_, alts)) = t.iter().find(|c| c.0 == toks[0]) else {
        return (Vec::new(), String::new());
    };
    let idx = toks.len() - 1;
    let mut out: Vec<String> = Vec::new();
    let mut hint = String::new();
    for alt in alts {
        // Each position arg consumes three tokens.
        let mut flat: Vec<Arg> = Vec::new();
        for a in alt {
            if *a == Arg::Pos {
                flat.extend([Arg::Pos, Arg::Pos, Arg::Pos]);
            } else {
                flat.push(*a);
            }
        }
        let prev_ok = toks[1..].iter().zip(&flat).all(|(tok, a)| match a {
            Arg::Lit(l) => tok == l,
            Arg::Choice(c) => c.contains(tok),
            _ => true,
        });
        if !prev_ok {
            continue;
        }
        let Some(a) = flat.get(idx) else { continue };
        let cands: Vec<String> = match a {
            Arg::Lit(l) => vec![(*l).to_owned()],
            Arg::Choice(c) => c.iter().map(|s| (*s).to_owned()).collect(),
            Arg::Target => SELECTORS.iter().map(|s| (*s).to_owned()).collect(),
            Arg::Pos => vec!["~".to_owned(), "~ ~ ~".to_owned()],
            Arg::Item => {
                let it = items();
                it.all
                    .iter()
                    .map(|&i| format!("minecraft:{}", it.name(i)))
                    .collect()
            }
            Arg::Entity => MobKind::ALL
                .iter()
                .map(|k| format!("minecraft:{}", k.name()))
                .collect(),
            Arg::Rule => RULES.iter().map(|s| (*s).to_owned()).collect(),
            Arg::Int | Arg::Text => Vec::new(),
        };
        if hint.is_empty() {
            hint = match a {
                Arg::Pos => {
                    let k = idx - flat.iter().take(idx).filter(|x| **x != Arg::Pos).count();
                    ["<x>", "<y>", "<z>"][k % 3].to_owned()
                }
                Arg::Target => "<targets>".into(),
                Arg::Item => "<item>".into(),
                Arg::Entity => "<entity>".into(),
                Arg::Int => "<count>".into(),
                Arg::Rule => "<rule>".into(),
                Arg::Text => "<message>".into(),
                _ => String::new(),
            };
        }
        for c in cands {
            let bare = c.trim_start_matches("minecraft:");
            if (c.starts_with(current) || bare.starts_with(current)) && !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out.truncate(200);
    (out, hint)
}

fn coord(s: &str, cur: f64) -> Option<f64> {
    if let Some(r) = s.strip_prefix('~') {
        Some(cur + if r.is_empty() { 0.0 } else { r.parse().ok()? })
    } else {
        let v: f64 = s.parse().ok()?;
        // Whole block coordinates target the block centre, like vanilla.
        Some(if s.contains('.') { v } else { v + 0.5 })
    }
}

fn coord_y(s: &str, cur: f64) -> Option<f64> {
    if let Some(r) = s.strip_prefix('~') {
        Some(cur + if r.is_empty() { 0.0 } else { r.parse().ok()? })
    } else {
        s.parse().ok()
    }
}

/// What a selector resolves to in single player.
enum Targets {
    Player,
    /// Indices into the entity list (mobs).
    Mobs(Vec<usize>),
    Both(Vec<usize>),
    None,
}

fn select(g: &Game, sel: &str) -> Targets {
    let (base, filter) = sel
        .split_once('[')
        .map_or((sel, ""), |(b, f)| (b, f.trim_end_matches(']')));
    let mut type_filter: Option<String> = None;
    let mut limit: Option<usize> = None;
    let mut not_player = false;
    for kv in filter.split(',').filter(|s| !s.is_empty()) {
        if let Some((k, v)) = kv.split_once('=') {
            match k.trim() {
                "type" => {
                    let v = v.trim();
                    if let Some(neg) = v.strip_prefix('!') {
                        if neg.trim_start_matches("minecraft:") == "player" {
                            not_player = true;
                        }
                    } else {
                        type_filter = Some(v.trim_start_matches("minecraft:").to_owned());
                    }
                }
                "limit" => limit = v.trim().parse().ok(),
                _ => {}
            }
        }
    }
    let mobs: Vec<usize> = g
        .entities
        .list
        .iter()
        .enumerate()
        .filter(|(_, e)| match &e.kind {
            Kind::Mob(m) => type_filter.as_deref().is_none_or(|t| t == m.kind.name()),
            Kind::Item { .. } => type_filter.as_deref() == Some("item"),
            _ => false,
        })
        .map(|(i, _)| i)
        .take(limit.unwrap_or(usize::MAX))
        .collect();
    match base {
        "@s" | "@p" | "@a" | "@r" | "Player" => Targets::Player,
        "@e" => {
            let player_ok = !not_player && type_filter.as_deref().is_none_or(|t| t == "player");
            if player_ok {
                Targets::Both(mobs)
            } else {
                Targets::Mobs(mobs)
            }
        }
        _ => Targets::None,
    }
}

/// Run a command (without the leading `/`); returns feedback lines.
pub fn execute(g: &mut Game, cmd: &str) -> String {
    let args: Vec<&str> = cmd.split_whitespace().collect();
    let p = g.player.pos;
    match args.as_slice() {
        ["help"] => "§eCommands: /gamemode, /tp, /give, /clear, /kill, /time, /weather, /summon, /seed, /spawnpoint, /setworldspawn, /difficulty, /gamerule, /xp, /say. Press Tab to complete.".into(),
        ["gamemode", m] | ["gamemode", m, _] => match GameMode::parse(m) {
            Some(mode) => {
                g.player.mode = mode;
                if mode == GameMode::Survival {
                    g.player.flying = false;
                }
                format!("Set own game mode to {}", mode.name())
            }
            None => format!("§cUnknown game mode: {m}"),
        },
        ["tp" | "teleport", x, y, z] => teleport(g, x, y, z),
        ["tp" | "teleport", t, x, y, z] => match select(g, t) {
            Targets::Player | Targets::Both(_) => teleport(g, x, y, z),
            Targets::Mobs(list) => {
                let (Some(x), Some(y), Some(z)) = (coord(x, p[0]), coord_y(y, p[1]), coord(z, p[2])) else {
                    return "§cInvalid position".into();
                };
                let n = list.len();
                for i in list {
                    g.entities.list[i].pos = [x, y, z];
                    g.entities.list[i].prev_pos = [x, y, z];
                }
                format!("Teleported {n} entities to {x:.2}, {y:.2}, {z:.2}")
            }
            Targets::None => "§cNo entity was found".into(),
        },
        ["tp" | "teleport", a, b] => {
            // Teleport targets `a` to the first entity of `b`.
            let dest = match select(g, b) {
                Targets::Player | Targets::Both(_) => Some(g.player.pos),
                Targets::Mobs(l) => l.first().map(|&i| g.entities.list[i].pos),
                Targets::None => None,
            };
            let Some(d) = dest else { return "§cNo entity was found".into() };
            match select(g, a) {
                Targets::Player | Targets::Both(_) => {
                    g.player.pos = d;
                    g.player.prev_pos = d;
                    "Teleported Player".into()
                }
                Targets::Mobs(l) => {
                    let n = l.len();
                    for i in l {
                        g.entities.list[i].pos = d;
                    }
                    format!("Teleported {n} entities")
                }
                Targets::None => "§cNo entity was found".into(),
            }
        }
        ["give", t, item] => give(g, t, item, 1),
        ["give", t, item, n] => give(g, t, item, n.parse().unwrap_or(1)),
        ["clear"] | ["clear", _] => {
            g.inv = Inventory::default();
            "Removed all items from Player".into()
        }
        ["kill"] => kill_player(g),
        ["kill", t] => match select(g, t) {
            Targets::Player => kill_player(g),
            Targets::Mobs(l) => kill_mobs(g, &l),
            Targets::Both(l) => {
                let s = kill_mobs(g, &l);
                kill_player(g);
                s
            }
            Targets::None => "§cNo entity was found".into(),
        },
        ["time", "set", t] => {
            let v = match *t {
                "day" => 1000,
                "noon" => 6000,
                "night" => 13000,
                "midnight" => 18000,
                n => match n.trim_end_matches('t').parse::<i64>() {
                    Ok(v) => v,
                    Err(_) => return "§cInvalid time".into(),
                },
            };
            let day = g.world.meta.day_time.div_euclid(24000);
            g.world.meta.day_time = day * 24000 + v;
            format!("Set the time to {v}")
        }
        ["time", "add", n] => match n.parse::<i64>() {
            Ok(v) => {
                g.world.meta.day_time += v;
                format!("Set the time to {}", g.world.meta.day_time.rem_euclid(24000))
            }
            Err(_) => "§cInvalid time".into(),
        },
        ["time", "query", q] => match *q {
            "daytime" => format!("The time is {}", g.world.meta.day_time.rem_euclid(24000)),
            "gametime" => format!("The time is {}", g.world.meta.time),
            _ => format!("The time is {}", g.world.meta.day_time.div_euclid(24000)),
        },
        ["weather", w] | ["weather", w, _] => {
            let dur: i64 = args.get(2).and_then(|d| d.parse().ok()).unwrap_or(6000);
            let m = &mut g.world.meta;
            match *w {
                "clear" => {
                    m.raining = false;
                    m.thundering = false;
                    m.clear_weather_time = dur;
                    "Set the weather to clear".into()
                }
                "rain" => {
                    m.raining = true;
                    m.thundering = false;
                    m.clear_weather_time = 0;
                    m.rain_time = dur;
                    "Set the weather to rain".into()
                }
                "thunder" => {
                    m.raining = true;
                    m.thundering = true;
                    m.clear_weather_time = 0;
                    m.rain_time = dur;
                    m.thunder_time = dur;
                    "Set the weather to rain & thunder".into()
                }
                _ => "§cUnknown weather".into(),
            }
        }
        ["summon", e] => summon(g, e, p),
        ["summon", e, x, y, z] => match (coord(x, p[0]), coord_y(y, p[1]), coord(z, p[2])) {
            (Some(x), Some(y), Some(z)) => summon(g, e, [x, y, z]),
            _ => "§cInvalid position".into(),
        },
        ["seed"] => format!("Seed: [{}]", g.world.meta.seed),
        ["spawnpoint"] | ["spawnpoint", _] | ["setworldspawn"] => {
            g.spawn = BlockPos::new(p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32);
            g.world.meta.spawn = [g.spawn.x, g.spawn.y, g.spawn.z];
            format!("Set spawn point to {}, {}, {}", g.spawn.x, g.spawn.y, g.spawn.z)
        }
        ["difficulty", d] => {
            g.difficulty = Difficulty::parse(d);
            format!("The difficulty has been set to {}", g.difficulty.name())
        }
        ["difficulty"] => format!("The difficulty is {}", g.difficulty.name()),
        ["gamerule", r] => format!("Gamerule {r} is currently set to: {}", g.world.meta.rules.get(*r).cloned().unwrap_or_else(|| {
            if *r == "randomTickSpeed" { "3".into() } else { crate::game::rule_bool(&g.world.meta, r).to_string() }
        })),
        ["gamerule", r, v] => {
            g.world.meta.rules.insert((*r).to_owned(), (*v).to_owned());
            format!("Gamerule {r} is now set to: {v}")
        }
        ["xp", "add", _, n] | ["xp", "add", _, n, "points"] => match n.parse() {
            Ok(v) => {
                g.player.add_xp(v);
                format!("Gave {v} experience points to Player")
            }
            Err(_) => "§cInvalid number".into(),
        },
        ["xp", "add", _, n, "levels"] => match n.parse::<u32>() {
            Ok(v) => {
                g.player.xp_level += v;
                format!("Gave {v} experience levels to Player")
            }
            Err(_) => "§cInvalid number".into(),
        },
        ["say", rest @ ..] => format!("[Player] {}", rest.join(" ")),
        ["me", rest @ ..] => format!("* Player {}", rest.join(" ")),
        _ => format!("§cUnknown or incomplete command: /{cmd}. Type /help for help."),
    }
}

fn teleport(g: &mut Game, x: &str, y: &str, z: &str) -> String {
    let p = g.player.pos;
    match (coord(x, p[0]), coord_y(y, p[1]), coord(z, p[2])) {
        (Some(x), Some(y), Some(z)) => {
            g.player.pos = [x, y, z];
            g.player.prev_pos = g.player.pos;
            g.player.vel = [0.0; 3];
            g.player.fall_distance = 0.0;
            format!("Teleported Player to {x:.2}, {y:.2}, {z:.2}")
        }
        _ => "§cInvalid position".into(),
    }
}

fn give(g: &mut Game, t: &str, item: &str, n: u32) -> String {
    if !matches!(select(g, t), Targets::Player | Targets::Both(_)) {
        return "§cNo player was found".into();
    }
    let Some(id) = items().id(item) else {
        return format!("§cUnknown item '{item}'");
    };
    let mut left = n.min(6400);
    let max = u32::from(items().max_stack[id as usize]);
    while left > 0 {
        let k = left.min(max) as u8;
        if g.inv.add(id, k) > 0 {
            break;
        }
        left -= u32::from(k);
    }
    format!(
        "Gave {} [{}] to Player",
        n.min(6400) - left,
        items().display_name(id)
    )
}

fn kill_player(g: &mut Game) -> String {
    g.player.health = 0.0;
    g.player.last_damage_cause = "fell out of the world".into();
    "Killed Player".into()
}

fn kill_mobs(g: &mut Game, list: &[usize]) -> String {
    let mut ev = Vec::new();
    for &i in list {
        let e = &mut g.entities.list[i];
        match &mut e.kind {
            Kind::Mob(m) => {
                m.hurt_time = 0;
                mobs::hurt_mob(m, 1000.0, e.pos, &mut ev);
            }
            _ => e.dead = true,
        }
    }
    format!("Killed {} entities", list.len())
}

fn summon(g: &mut Game, e: &str, pos: [f64; 3]) -> String {
    match MobKind::parse(e) {
        Some(k) => {
            g.entities.spawn(mobs::spawn(k, pos, false));
            format!("Summoned new {}", k.name())
        }
        None if e.trim_start_matches("minecraft:") == "lightning_bolt" => {
            g.entities.spawn(Entity::new(Kind::Lightning, pos));
            "Summoned new Lightning Bolt".into()
        }
        None => format!("§cUnable to summon entity '{e}' in this version"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn suggestions() {
        let (s, _) = super::suggest("ga");
        assert!(s.contains(&"gamemode".to_owned()) && s.contains(&"gamerule".to_owned()));
        let (s, _) = super::suggest("gamemode c");
        assert_eq!(s, vec!["creative".to_owned()]);
        let (s, h) = super::suggest("tp ");
        assert!(s.contains(&"@s".to_owned()) && s.contains(&"~".to_owned()));
        assert!(!h.is_empty());
        let (s, _) = super::suggest("give @s diamond_o");
        assert!(s.contains(&"minecraft:diamond_ore".to_owned()));
        let (_, h) = super::suggest("tp 1 ");
        assert_eq!(h, "<y>");
    }
}
