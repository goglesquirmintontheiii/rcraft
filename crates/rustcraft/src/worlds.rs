//! World list management (multiple saves, rename, delete, backup) and the
//! world-select / create / edit / import / sound screens.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use rc_ui::{GRAY, Key, Ui, WHITE};

use crate::screens::{Action, Ctx, Screen, WorldSummary};

/// Boolean game rules offered on the create screen (vanilla names/defaults).
pub const DEFAULT_RULES: [(&str, bool); 10] = [
    ("doDaylightCycle", true),
    ("doWeatherCycle", true),
    ("doMobSpawning", true),
    ("doMobLoot", true),
    ("doTileDrops", true),
    ("keepInventory", false),
    ("naturalRegeneration", true),
    ("mobGriefing", true),
    ("doFireTick", true),
    ("doImmediateRespawn", false),
];

/// The `saves` folder.
#[must_use]
pub fn saves_dir(game_dir: &Path) -> PathBuf {
    game_dir.join("saves")
}

/// The folder of the world named by directory `dir`.
#[must_use]
pub fn world_path(game_dir: &Path, dir: &str) -> PathBuf {
    saves_dir(game_dir).join(dir)
}

fn civil(ms: u64) -> String {
    if ms == 0 {
        return "Never played".to_owned();
    }
    let secs = ms / 1000;
    let days = secs / 86_400;
    // Civil date from days since epoch (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{mo:02}-{d:02} {:02}:{:02} UTC",
        (secs / 3600) % 24,
        (secs / 60) % 60
    )
}

/// All worlds in `saves`, most recently played first.
#[must_use]
pub fn list(game_dir: &Path) -> Vec<WorldSummary> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(saves_dir(game_dir)) else {
        return out;
    };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(m) = rc_world::rcw::WorldMeta::read(&p.join("world.rcw")) else {
            continue;
        };
        out.push(WorldSummary {
            dir: e.file_name().to_string_lossy().into_owned(),
            name: m.name,
            seed: m.seed,
            last_played: civil(m.last_played_ms),
            mode: if m.game_mode.is_empty() {
                "survival".into()
            } else {
                m.game_mode
            },
            last_played_ms: m.last_played_ms,
        });
    }
    out.sort_by(|a, b| {
        b.last_played_ms
            .cmp(&a.last_played_ms)
            .then(a.name.cmp(&b.name))
    });
    out
}

/// A fresh folder name derived from the world name (like vanilla: illegal
/// characters become `_`, duplicates get ` (n)`).
#[must_use]
pub fn new_dir_name(game_dir: &Path, name: &str) -> String {
    let mut base: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .to_owned();
    if base.is_empty() || base.chars().all(|c| c == '_') {
        base = "World".into();
    }
    const RESERVED: [&str; 8] = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "LPT1", "LPT2"];
    if RESERVED.contains(&base.to_uppercase().as_str()) {
        base = format!("_{base}_");
    }
    let saves = saves_dir(game_dir);
    let mut cand = base.clone();
    let mut n = 1;
    while saves.join(&cand).exists() {
        cand = format!("{base} ({n})");
        n += 1;
    }
    cand
}

/// Change a world's display name.
pub fn rename(game_dir: &Path, dir: &str, name: &str) -> Result<()> {
    let f = world_path(game_dir, dir).join("world.rcw");
    let mut m = rc_world::rcw::WorldMeta::read(&f)?;
    m.name = name.to_owned();
    m.write(&f)
}

/// Delete a world folder.
pub fn delete(game_dir: &Path, dir: &str) -> Result<()> {
    let p = world_path(game_dir, dir);
    anyhow::ensure!(
        p.starts_with(saves_dir(game_dir)) && !dir.is_empty() && !dir.contains(".."),
        "bad world folder"
    );
    std::fs::remove_dir_all(&p).with_context(|| format!("deleting {}", p.display()))
}

fn copy_dir(src: &Path, dst: &Path) -> Result<u64> {
    std::fs::create_dir_all(dst)?;
    let mut n = 0;
    for e in std::fs::read_dir(src)?.flatten() {
        let (s, d) = (e.path(), dst.join(e.file_name()));
        if e.file_type()?.is_dir() {
            n += copy_dir(&s, &d)?;
        } else {
            n += std::fs::copy(&s, &d)?;
        }
    }
    Ok(n)
}

/// Copy a world into `backups/<dir>_<timestamp>`; returns the backup path.
pub fn backup(game_dir: &Path, dir: &str) -> Result<PathBuf> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let dst = game_dir.join("backups").join(format!("{dir}_{ts}"));
    copy_dir(&world_path(game_dir, dir), &dst)?;
    Ok(dst)
}

// ---------------------------------------------------------------------------
// Screens
// ---------------------------------------------------------------------------

const ROW_H: f32 = 36.0;

pub fn select(
    ui: &mut Ui<'_>,
    cx: &mut Ctx<'_>,
    selected: &mut Option<usize>,
    scroll: &mut f32,
    out: &mut Vec<Action>,
) {
    ui.background(false);
    let (w, h) = (ui.w, ui.h);
    ui.text_centered("Select World", w / 2.0, 15.0, WHITE);
    let (top, bottom) = (32.0, h - 64.0);
    ui.list_background(top, bottom, false);
    let worlds = &cx.worlds;
    let visible = bottom - top - 8.0;
    let total = worlds.len() as f32 * ROW_H;
    if ui.hover(0.0, top, w, bottom - top) && ui.input.scroll != 0.0 {
        *scroll -= ui.input.scroll * 18.0;
    }
    *scroll = scroll.clamp(0.0, (total - visible).max(0.0));
    if let Some(s) = *selected
        && s >= worlds.len()
    {
        *selected = None;
    }
    if worlds.is_empty() {
        ui.text_centered(
            "No worlds yet. Create one to start playing.",
            w / 2.0,
            60.0,
            GRAY,
        );
    }
    let x = w / 2.0 - 110.0;
    for (i, s) in worlds.iter().enumerate() {
        let y = top + 4.0 + i as f32 * ROW_H - *scroll;
        if y + ROW_H - 4.0 < top || y > bottom - 2.0 {
            continue;
        }
        let hover = ui.hover(x, y, 220.0, ROW_H - 4.0) && ui.hover(0.0, top, w, bottom - top);
        let sel = *selected == Some(i);
        if sel || hover {
            ui.fill(
                x - 2.0,
                y - 2.0,
                224.0,
                ROW_H,
                if sel {
                    [1.0, 1.0, 1.0, 0.8]
                } else {
                    [0.5, 0.5, 0.5, 0.5]
                },
            );
            ui.fill(x - 1.0, y - 1.0, 222.0, ROW_H - 2.0, [0.0, 0.0, 0.0, 1.0]);
        }
        ui.text(&s.name, x + 4.0, y + 2.0, WHITE);
        ui.text(
            &format!("{} ({})", s.dir, s.last_played),
            x + 4.0,
            y + 13.0,
            GRAY,
        );
        let mode = s.mode[..1].to_uppercase() + &s.mode[1..];
        ui.text(
            &format!("{mode} Mode, seed {}", s.seed),
            x + 4.0,
            y + 23.0,
            GRAY,
        );
        if hover && ui.input.pressed {
            if sel {
                // Second click on the selected row plays it (vanilla double-click).
                out.push(Action::PlayWorld(s.dir.clone()));
            }
            *selected = Some(i);
        }
    }
    let cur = selected.and_then(|i| worlds.get(i));
    let has = cur.is_some();
    let (by, bx) = (h - 52.0, w / 2.0 - 154.0);
    if ui.button("Play Selected World", bx, by, 150.0, 20.0, has)
        || (has && ui.input.key(Key::Enter))
    {
        if let Some(s) = cur {
            out.push(Action::PlayWorld(s.dir.clone()));
        }
    }
    if ui.button("Create New World", bx + 158.0, by, 150.0, 20.0, true) {
        out.push(Action::Push(Screen::CreateWorld {
            name: "New World".into(),
            seed: String::new(),
            mode: 0,
            difficulty: 2,
            more: false,
            rules: DEFAULT_RULES
                .iter()
                .map(|(k, v)| ((*k).to_owned(), *v))
                .collect(),
        }));
    }
    let bw = 72.0;
    if ui.button("Edit", bx, by + 24.0, bw, 20.0, has)
        && let Some(s) = cur
    {
        out.push(Action::Push(Screen::EditWorld {
            dir: s.dir.clone(),
            name: s.name.clone(),
        }));
    }
    if ui.button("Delete", bx + 78.0, by + 24.0, bw, 20.0, has)
        && let Some(s) = cur
    {
        out.push(Action::Push(Screen::ConfirmDelete {
            dir: s.dir.clone(),
            name: s.name.clone(),
        }));
    }
    if ui.button("Import", bx + 158.0, by + 24.0, bw, 20.0, true) {
        out.push(Action::Push(Screen::ImportWorld {
            selected: None,
            scroll: 0.0,
            path: String::new(),
        }));
    }
    if ui.button("Cancel", bx + 236.0, by + 24.0, bw, 20.0, true) || ui.input.key(Key::Escape) {
        out.push(Action::Pop);
    }
}

fn cycle_label(names: &[&str], v: u8) -> String {
    names.get(v as usize).copied().unwrap_or("?").to_owned()
}

pub fn create(
    ui: &mut Ui<'_>,
    name: &mut String,
    seed: &mut String,
    mode: &mut u8,
    difficulty: &mut u8,
    more: &mut bool,
    rules: &mut [(String, bool)],
    out: &mut Vec<Action>,
) {
    ui.background(false);
    let (w, h) = (ui.w, ui.h);
    ui.text_centered("Create New World", w / 2.0, 15.0, WHITE);
    let x = w / 2.0 - 155.0;
    if !*more {
        ui.text("World Name", x, 37.0, GRAY);
        ui.text_field("world-name", x, 48.0, 310.0, 20.0, name, 32, "");
        ui.text("Seed for the world generator", x, 75.0, GRAY);
        ui.text_field(
            "world-seed",
            x,
            86.0,
            310.0,
            20.0,
            seed,
            32,
            "Leave blank for a random seed",
        );
        const MODES: [&str; 3] = ["Survival", "Creative", "Spectator"];
        if ui.button(
            &format!("Game Mode: {}", cycle_label(&MODES, *mode)),
            x,
            116.0,
            150.0,
            20.0,
            true,
        ) {
            *mode = (*mode + 1) % 3;
        }
        const DIFF: [&str; 4] = ["Peaceful", "Easy", "Normal", "Hard"];
        if ui.button(
            &format!("Difficulty: {}", cycle_label(&DIFF, *difficulty)),
            x + 160.0,
            116.0,
            150.0,
            20.0,
            true,
        ) {
            *difficulty = (*difficulty + 1) % 4;
        }
        let desc = match *mode {
            0 => "Search for resources, craft, gain levels, health and hunger",
            1 => "Unlimited resources, free flying and destroy blocks instantly",
            _ => "Fly through the world without touching it",
        };
        ui.text_centered(desc, w / 2.0, 140.0, GRAY);
        if ui.button("Game Rules...", x, 156.0, 310.0, 20.0, true) {
            *more = true;
        }
    } else {
        ui.text_centered("Game Rules", w / 2.0, 30.0, GRAY);
        for (i, (k, v)) in rules.iter_mut().enumerate() {
            let (cx, cy) = (x + (i % 2) as f32 * 160.0, 44.0 + (i / 2) as f32 * 24.0);
            if cy > h - 60.0 {
                break;
            }
            ui.toggle(k, cx, cy, 150.0, v);
        }
        if ui.button("Back", w / 2.0 - 100.0, h - 52.0, 200.0, 20.0, true) {
            *more = false;
        }
    }
    let ok = !name.trim().is_empty();
    if ui.button("Create New World", x, h - 28.0, 150.0, 20.0, ok) {
        out.push(Action::CreateWorld {
            name: name.trim().to_owned(),
            seed: crate::screens::parse_seed(seed),
            mode: *mode,
            difficulty: *difficulty,
            rules: rules.to_vec(),
        });
    }
    if ui.button("Cancel", x + 160.0, h - 28.0, 150.0, 20.0, true) || ui.input.key(Key::Escape) {
        if *more {
            *more = false;
        } else {
            out.push(Action::Pop);
        }
    }
}

pub fn edit(ui: &mut Ui<'_>, dir: &mut String, name: &mut String, out: &mut Vec<Action>) {
    ui.background(false);
    let (w, h) = (ui.w, ui.h);
    ui.text_centered("Edit World", w / 2.0, 15.0, WHITE);
    let x = w / 2.0 - 100.0;
    ui.text("World Name", x, 37.0, GRAY);
    ui.text_field("edit-name", x, 48.0, 200.0, 20.0, name, 32, "");
    ui.text(&format!("Folder: saves/{dir}"), x, 72.0, GRAY);
    let mut y = 88.0;
    if ui.button("Make Backup", x, y, 200.0, 20.0, true) {
        out.push(Action::BackupWorld(dir.clone()));
    }
    y += 24.0;
    if ui.button("Export to Java Edition", x, y, 200.0, 20.0, true) {
        out.push(Action::ExportJava(dir.clone()));
    }
    y += 24.0;
    if ui.button("Open Data Folder", x, y, 200.0, 20.0, true) {
        out.push(Action::OpenDataFolder);
    }
    let ok = !name.trim().is_empty();
    if ui.button("Save", x - 55.0, h - 28.0, 150.0, 20.0, ok) {
        out.push(Action::RenameWorld(dir.clone(), name.trim().to_owned()));
        out.push(Action::Pop);
    }
    if ui.button("Cancel", x + 105.0, h - 28.0, 150.0, 20.0, true) || ui.input.key(Key::Escape) {
        out.push(Action::Pop);
    }
}

pub fn confirm_delete(ui: &mut Ui<'_>, dir: &str, name: &str, out: &mut Vec<Action>) {
    ui.background(false);
    let (w, h) = (ui.w, ui.h);
    ui.text_centered(
        "Are you sure you want to delete this world?",
        w / 2.0,
        h / 2.0 - 40.0,
        WHITE,
    );
    ui.text_centered(
        &format!("'{name}' will be lost forever! (A long time!)"),
        w / 2.0,
        h / 2.0 - 25.0,
        GRAY,
    );
    if ui.button("Delete", w / 2.0 - 155.0, h / 2.0 + 10.0, 150.0, 20.0, true) {
        out.push(Action::DeleteWorld(dir.to_owned()));
        out.push(Action::Pop);
    }
    if ui.button("Cancel", w / 2.0 + 5.0, h / 2.0 + 10.0, 150.0, 20.0, true)
        || ui.input.key(Key::Escape)
    {
        out.push(Action::Pop);
    }
}

pub fn import(
    ui: &mut Ui<'_>,
    cx: &mut Ctx<'_>,
    selected: &mut Option<usize>,
    scroll: &mut f32,
    path: &mut String,
    out: &mut Vec<Action>,
) {
    ui.background(false);
    let (w, h) = (ui.w, ui.h);
    ui.text_centered("Import Java Edition World", w / 2.0, 15.0, WHITE);
    let (top, bottom) = (32.0, h - 88.0);
    ui.list_background(top, bottom, false);
    let list = &cx.java_worlds;
    let total = list.len() as f32 * 24.0;
    if ui.hover(0.0, top, w, bottom - top) && ui.input.scroll != 0.0 {
        *scroll -= ui.input.scroll * 18.0;
    }
    *scroll = scroll.clamp(0.0, (total - (bottom - top - 8.0)).max(0.0));
    if list.is_empty() {
        ui.text_centered(
            "No Java worlds found in .minecraft/saves.",
            w / 2.0,
            50.0,
            GRAY,
        );
        ui.text_centered(
            "Type the path of a world folder below.",
            w / 2.0,
            62.0,
            GRAY,
        );
    }
    let x = w / 2.0 - 150.0;
    for (i, (name, p)) in list.iter().enumerate() {
        let y = top + 4.0 + i as f32 * 24.0 - *scroll;
        if y < top || y > bottom - 20.0 {
            continue;
        }
        let hover = ui.hover(x, y, 300.0, 22.0);
        if *selected == Some(i) || hover {
            ui.fill(
                x - 1.0,
                y - 1.0,
                302.0,
                24.0,
                if *selected == Some(i) {
                    [1.0, 1.0, 1.0, 0.8]
                } else {
                    [0.5, 0.5, 0.5, 0.5]
                },
            );
            ui.fill(x, y, 300.0, 22.0, [0.0, 0.0, 0.0, 1.0]);
        }
        ui.text(name, x + 4.0, y + 2.0, WHITE);
        let ps = p.display().to_string();
        let ps = if ps.len() > 60 {
            format!("...{}", &ps[ps.len() - 57..])
        } else {
            ps
        };
        ui.text(&ps, x + 4.0, y + 12.0, GRAY);
        if hover && ui.input.pressed {
            *selected = Some(i);
            *path = p.display().to_string();
        }
    }
    ui.text_field(
        "import-path",
        x,
        h - 80.0,
        300.0,
        20.0,
        path,
        260,
        "Path to a Java world folder",
    );
    let ok = !path.trim().is_empty();
    if ui.button("Import", x, h - 52.0, 145.0, 20.0, ok) {
        out.push(Action::ImportJava(path.trim().to_owned()));
    }
    if ui.button("Cancel", x + 155.0, h - 52.0, 145.0, 20.0, true) || ui.input.key(Key::Escape) {
        out.push(Action::Pop);
    }
    ui.text_centered(
        "Imports blocks, biomes, player and inventory (1.18 - 1.21.1).",
        w / 2.0,
        h - 26.0,
        GRAY,
    );
}

pub fn sounds(ui: &mut Ui<'_>, cx: &mut Ctx<'_>, in_world: bool, out: &mut Vec<Action>) {
    ui.background(in_world);
    let (w, h) = (ui.w, ui.h);
    ui.text_centered("Music & Sounds", w / 2.0, 15.0, WHITE);
    let mut changed = false;
    for (i, cat) in rc_audio::Category::ALL.iter().enumerate() {
        let (x, y) = if i == 0 {
            (w / 2.0 - 155.0, 32.0)
        } else {
            let j = i - 1;
            (
                if j % 2 == 0 {
                    w / 2.0 - 155.0
                } else {
                    w / 2.0 + 5.0
                },
                56.0 + (j / 2) as f32 * 24.0,
            )
        };
        let width = if i == 0 { 310.0 } else { 150.0 };
        let v = &mut cx.opts.volumes[i];
        let pct = (*v * 100.0).round() as i32;
        let label = if pct == 0 {
            format!("{}: OFF", cat.label())
        } else {
            format!("{}: {pct}%", cat.label())
        };
        if ui.slider(&format!("vol{i}"), &label, x, y, width, 20.0, v) {
            changed = true;
        }
    }
    if cx.audio.is_none_or(|a| !a.enabled()) {
        ui.text_centered(
            "No sounds loaded: install Minecraft Java 1.21.1 for sound effects.",
            w / 2.0,
            h - 50.0,
            GRAY,
        );
    }
    if changed {
        out.push(Action::VolumesChanged);
    }
    if ui.button("Done", w / 2.0 - 100.0, h - 27.0, 200.0, 20.0, true) || ui.input.key(Key::Escape)
    {
        out.push(Action::Pop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_names_are_safe_and_unique() {
        let g = std::env::temp_dir().join(format!("rc-worlds-{}", std::process::id()));
        std::fs::create_dir_all(g.join("saves/My World")).unwrap();
        assert_eq!(new_dir_name(&g, "My World"), "My World (1)");
        assert_eq!(new_dir_name(&g, "a/b:c"), "a_b_c");
        assert_eq!(new_dir_name(&g, "con"), "_con_");
        assert_eq!(new_dir_name(&g, "///"), "World");
        let _ = std::fs::remove_dir_all(&g);
    }
}
