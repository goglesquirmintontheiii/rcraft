//! Menus, HUD and container screens.

use rc_ui::{GRAY, Key, Ui, WHITE, YELLOW};

use crate::game::Game;
use crate::gfx::Gfx;
use crate::items::{Stack, items};
use crate::options::{Graphics, Options, Preset};
use crate::player::GameMode;

#[derive(Clone, Debug, PartialEq)]
pub enum Screen {
    Title,
    SelectWorld,
    CreateWorld {
        name: String,
        seed: String,
        creative: bool,
    },
    Options,
    Video,
    Performance,
    Controls,
    Updates,
    Pause,
    Loading,
    Death,
    Chat {
        text: String,
    },
    Inventory,
    Creative {
        scroll: f32,
        search: String,
    },
}

/// What the app should do after a screen frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Push(Screen),
    Pop,
    /// Close every screen (back to the game).
    CloseAll,
    PlayWorld,
    CreateWorld {
        name: String,
        seed: i64,
        creative: bool,
    },
    SaveAndQuit,
    Quit,
    Respawn,
    CheckUpdate,
    InstallUpdate,
    OptionsChanged {
        reload_world: bool,
        reload_assets: bool,
    },
    ToggleFullscreen,
}

/// Info about the single world slot for the world list.
#[derive(Clone, Debug)]
pub struct WorldSummary {
    pub name: String,
    pub seed: i64,
    pub last_played: String,
    pub mode: String,
}

/// Frame statistics shown on the debug screen.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub fps: f32,
    pub frame_ms: f32,
    pub adapter: String,
    pub backend: String,
    pub sections_drawn: usize,
    pub sections_total: usize,
    pub quads: usize,
    pub chunks: usize,
    pub pending_gen: usize,
    pub workers: usize,
    pub mem_mb: f32,
    pub window: (u32, u32),
}

pub struct Ctx<'a> {
    pub opts: &'a mut Options,
    pub gfx: &'a Gfx,
    pub game: Option<&'a mut Game>,
    pub world: Option<WorldSummary>,
    pub update: rc_update::Status,
    pub stats: &'a Stats,
    pub f3: bool,
    pub hud_hidden: bool,
    pub selected_changed_tick: u64,
}

impl std::fmt::Debug for Ctx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ctx")
    }
}

fn header(ui: &mut Ui<'_>, title: &str) {
    let w = ui.w;
    ui.text_centered(title, w / 2.0, 15.0, WHITE);
}

fn done_button(ui: &mut Ui<'_>, out: &mut Vec<Action>) {
    let (w, h) = (ui.w, ui.h);
    if ui.button("Done", w / 2.0 - 100.0, h - 27.0, 200.0, 20.0, true) || ui.input.key(Key::Escape)
    {
        out.push(Action::Pop);
    }
}

/// Two-column option layout helper: returns (x, y) of slot `i`.
fn slot(ui: &Ui<'_>, i: usize) -> (f32, f32) {
    let x = if i.is_multiple_of(2) {
        ui.w / 2.0 - 155.0
    } else {
        ui.w / 2.0 + 5.0
    };
    (x, 32.0 + (i / 2) as f32 * 24.0)
}

impl Screen {
    /// Whether the world keeps rendering (and ticking) behind this screen.
    #[must_use]
    pub fn pauses(&self) -> bool {
        matches!(
            self,
            Screen::Pause
                | Screen::Options
                | Screen::Video
                | Screen::Performance
                | Screen::Controls
                | Screen::Updates
        )
    }

    pub fn draw(&mut self, ui: &mut Ui<'_>, cx: &mut Ctx<'_>) -> Vec<Action> {
        let mut out = Vec::new();
        let in_world = cx.game.is_some();
        match self {
            Screen::Title => title(ui, cx, &mut out),
            Screen::SelectWorld => {
                ui.background(false);
                header(ui, "Select World");
                let (w, h) = (ui.w, ui.h);
                ui.list_background(32.0, h - 64.0, false);
                match &cx.world {
                    Some(s) => {
                        let (x, y) = (w / 2.0 - 110.0, 40.0);
                        let hover = ui.hover(x, y, 220.0, 36.0);
                        ui.fill(
                            x - 2.0,
                            y - 2.0,
                            224.0,
                            40.0,
                            if hover {
                                [1.0, 1.0, 1.0, 0.6]
                            } else {
                                [0.5, 0.5, 0.5, 0.5]
                            },
                        );
                        ui.fill(x - 1.0, y - 1.0, 222.0, 38.0, [0.0, 0.0, 0.0, 1.0]);
                        ui.text(&s.name, x + 4.0, y + 2.0, WHITE);
                        ui.text(
                            &format!("{} ({})", s.last_played, s.mode),
                            x + 4.0,
                            y + 13.0,
                            GRAY,
                        );
                        ui.text(&format!("Seed: {}", s.seed), x + 4.0, y + 24.0, GRAY);
                        if hover && ui.input.pressed {
                            out.push(Action::PlayWorld);
                        }
                    }
                    None => {
                        ui.text_centered(
                            "No world yet. Create one to start playing.",
                            w / 2.0,
                            60.0,
                            GRAY,
                        );
                    }
                }
                let by = h - 52.0;
                if ui.button(
                    "Play Selected World",
                    w / 2.0 - 154.0,
                    by,
                    150.0,
                    20.0,
                    cx.world.is_some(),
                ) {
                    out.push(Action::PlayWorld);
                }
                if ui.button(
                    "Create New World",
                    w / 2.0 + 4.0,
                    by,
                    150.0,
                    20.0,
                    cx.world.is_none(),
                ) {
                    out.push(Action::Push(Screen::CreateWorld {
                        name: "New World".into(),
                        seed: String::new(),
                        creative: false,
                    }));
                }
                if ui.button("Cancel", w / 2.0 + 4.0, by + 24.0, 150.0, 20.0, true)
                    || ui.input.key(Key::Escape)
                {
                    out.push(Action::Pop);
                }
                if cx.world.is_some() {
                    ui.text_centered(
                        "This version supports one world (more in v0.2).",
                        w / 2.0,
                        by + 30.0,
                        GRAY,
                    );
                }
            }
            Screen::CreateWorld {
                name,
                seed,
                creative,
            } => {
                ui.background(false);
                header(ui, "Create New World");
                let w = ui.w;
                ui.text("World Name", w / 2.0 - 100.0, 47.0, GRAY);
                ui.text_field(
                    "world-name",
                    w / 2.0 - 100.0,
                    60.0,
                    200.0,
                    20.0,
                    name,
                    32,
                    "",
                );
                ui.text("Seed for the world generator", w / 2.0 - 100.0, 87.0, GRAY);
                ui.text_field(
                    "world-seed",
                    w / 2.0 - 100.0,
                    100.0,
                    200.0,
                    20.0,
                    seed,
                    32,
                    "Leave blank for a random seed",
                );
                let label = format!(
                    "Game Mode: {}",
                    if *creative { "Creative" } else { "Survival" }
                );
                if ui.button(&label, w / 2.0 - 100.0, 130.0, 200.0, 20.0, true) {
                    *creative = !*creative;
                }
                let h = ui.h;
                if ui.button(
                    "Create New World",
                    w / 2.0 - 155.0,
                    h - 28.0,
                    150.0,
                    20.0,
                    !name.trim().is_empty(),
                ) {
                    out.push(Action::CreateWorld {
                        name: name.trim().to_owned(),
                        seed: parse_seed(seed),
                        creative: *creative,
                    });
                }
                if ui.button("Cancel", w / 2.0 + 5.0, h - 28.0, 150.0, 20.0, true)
                    || ui.input.key(Key::Escape)
                {
                    out.push(Action::Pop);
                }
            }
            Screen::Options => {
                ui.background(in_world);
                header(ui, "Options");
                let w = ui.w;
                let mut fov = (cx.opts.fov - 30.0) / 80.0;
                let label = if (cx.opts.fov - 70.0).abs() < 0.5 {
                    "FOV: Normal".to_owned()
                } else {
                    format!("FOV: {}", cx.opts.fov.round())
                };
                if ui.slider("fov", &label, w / 2.0 - 155.0, 32.0, 150.0, 20.0, &mut fov) {
                    cx.opts.fov = (30.0 + fov * 80.0).round();
                }
                let diff = cx
                    .game
                    .as_ref()
                    .map_or("Normal".to_owned(), |_| "Normal".to_owned());
                ui.button(
                    &format!("Difficulty: {diff}"),
                    w / 2.0 + 5.0,
                    32.0,
                    150.0,
                    20.0,
                    false,
                );
                let items = [
                    ("Video Settings...", Screen::Video),
                    ("Performance...", Screen::Performance),
                    ("Controls...", Screen::Controls),
                    ("Updates...", Screen::Updates),
                ];
                for (i, (l, s)) in items.into_iter().enumerate() {
                    let (x, y) = slot(ui, i + 2);
                    if ui.button(l, x, y + 24.0, 150.0, 20.0, true) {
                        out.push(Action::Push(s));
                    }
                }
                let (x, y) = slot(ui, 6);
                ui.button("Resource Packs... (v0.3)", x, y + 24.0, 150.0, 20.0, false);
                let (x, y) = slot(ui, 7);
                ui.button("Language... (English)", x, y + 24.0, 150.0, 20.0, false);
                done_button(ui, &mut out);
            }
            Screen::Video => video(ui, cx, &mut out, in_world),
            Screen::Performance => performance(ui, cx, &mut out, in_world),
            Screen::Controls => {
                ui.background(in_world);
                header(ui, "Controls");
                let w = ui.w;
                let mut s = cx.opts.mouse_sensitivity;
                let label = format!("Sensitivity: {}%", (s * 200.0).round());
                if ui.slider("sens", &label, w / 2.0 - 155.0, 32.0, 150.0, 20.0, &mut s) {
                    cx.opts.mouse_sensitivity = s;
                }
                let mut inv = cx.opts.invert_mouse;
                if ui.toggle("Invert Mouse", w / 2.0 + 5.0, 32.0, 150.0, &mut inv) {
                    cx.opts.invert_mouse = inv;
                    out.push(Action::OptionsChanged {
                        reload_world: false,
                        reload_assets: false,
                    });
                }
                let binds = [
                    ("Walk", "W A S D"),
                    ("Jump / Fly (double tap)", "Space"),
                    ("Sneak", "Left Shift"),
                    ("Sprint", "Left Ctrl / double tap W"),
                    ("Attack / Destroy", "Left Button"),
                    ("Use Item / Place Block", "Right Button"),
                    ("Pick Block", "Middle Button"),
                    ("Inventory", "E"),
                    ("Chat / Command", "T  /  /"),
                    ("Hotbar", "1-9, Mouse Wheel"),
                    ("Debug Screen", "F3"),
                    ("Hide HUD / Screenshot", "F1 / F2"),
                    ("Fullscreen", "F11"),
                ];
                for (i, (a, k)) in binds.iter().enumerate() {
                    let y = 64.0 + i as f32 * 11.0;
                    ui.text(a, w / 2.0 - 150.0, y, WHITE);
                    ui.text_right(k, w / 2.0 + 150.0, y, GRAY);
                }
                done_button(ui, &mut out);
            }
            Screen::Updates => {
                ui.background(in_world);
                header(ui, "Updates");
                let w = ui.w;
                let pol = cx.opts.update_policy;
                let label = match pol {
                    rc_update::UpdatePolicy::Automatic => "Updates: Install Automatically",
                    rc_update::UpdatePolicy::Ask => "Updates: Ask Before Installing",
                    rc_update::UpdatePolicy::Off => "Updates: Never Check",
                };
                if ui.button(label, w / 2.0 - 155.0, 32.0, 310.0, 20.0, true) {
                    cx.opts.update_policy = match pol {
                        rc_update::UpdatePolicy::Automatic => rc_update::UpdatePolicy::Ask,
                        rc_update::UpdatePolicy::Ask => rc_update::UpdatePolicy::Off,
                        rc_update::UpdatePolicy::Off => rc_update::UpdatePolicy::Automatic,
                    };
                    out.push(Action::OptionsChanged {
                        reload_world: false,
                        reload_assets: false,
                    });
                }
                let busy = matches!(
                    cx.update,
                    rc_update::Status::Checking | rc_update::Status::Downloading { .. }
                );
                if ui.button("Check Now", w / 2.0 - 155.0, 56.0, 150.0, 20.0, !busy) {
                    out.push(Action::CheckUpdate);
                }
                let can_install = matches!(cx.update, rc_update::Status::Available(_));
                if ui.button(
                    "Install Update",
                    w / 2.0 + 5.0,
                    56.0,
                    150.0,
                    20.0,
                    can_install,
                ) {
                    out.push(Action::InstallUpdate);
                }
                ui.text_centered(
                    &format!("Installed: RustCraft {}", rc_core::version::VERSION),
                    w / 2.0,
                    90.0,
                    GRAY,
                );
                ui.text_centered(&update_text(&cx.update), w / 2.0, 104.0, WHITE);
                done_button(ui, &mut out);
            }
            Screen::Pause => {
                ui.background(true);
                let (w, h) = (ui.w, ui.h);
                ui.text_centered("Game Menu", w / 2.0, 40.0, WHITE);
                let y = h / 4.0 + 8.0;
                if ui.button("Back to Game", w / 2.0 - 102.0, y, 204.0, 20.0, true)
                    || ui.input.key(Key::Escape)
                {
                    out.push(Action::CloseAll);
                }
                ui.button("Advancements", w / 2.0 - 102.0, y + 24.0, 98.0, 20.0, false);
                ui.button("Statistics", w / 2.0 + 4.0, y + 24.0, 98.0, 20.0, false);
                ui.button(
                    "Give Feedback",
                    w / 2.0 - 102.0,
                    y + 48.0,
                    98.0,
                    20.0,
                    false,
                );
                ui.button("Report Bugs", w / 2.0 + 4.0, y + 48.0, 98.0, 20.0, false);
                if ui.button("Options...", w / 2.0 - 102.0, y + 72.0, 98.0, 20.0, true) {
                    out.push(Action::Push(Screen::Options));
                }
                ui.button("Open to LAN", w / 2.0 + 4.0, y + 72.0, 98.0, 20.0, false);
                if ui.button(
                    "Save and Quit to Title",
                    w / 2.0 - 102.0,
                    y + 96.0,
                    204.0,
                    20.0,
                    true,
                ) {
                    out.push(Action::SaveAndQuit);
                }
            }
            Screen::Loading => {
                ui.background(false);
                let (w, h) = (ui.w, ui.h);
                let (ready, total) = cx
                    .game
                    .as_ref()
                    .map_or((0, 1), |g| g.world.spawn_progress(g.player_chunk(), 2));
                let pct = ready as f32 / total.max(1) as f32;
                ui.text_centered("Loading terrain...", w / 2.0, h / 2.0 - 30.0, WHITE);
                ui.text_centered(
                    &format!("{}%", (pct * 100.0).round()),
                    w / 2.0,
                    h / 2.0 - 15.0,
                    GRAY,
                );
                ui.fill(w / 2.0 - 100.0, h / 2.0, 200.0, 4.0, [0.3, 0.3, 0.3, 1.0]);
                ui.fill(
                    w / 2.0 - 100.0,
                    h / 2.0,
                    200.0 * pct,
                    4.0,
                    [0.5, 0.9, 0.5, 1.0],
                );
            }
            Screen::Death => {
                let (w, h) = (ui.w, ui.h);
                ui.gradient(
                    0.0,
                    0.0,
                    w,
                    h,
                    [0.31, 0.0, 0.0, 0.38],
                    [0.63, 0.19, 0.19, 0.63],
                );
                // Large title at 2×.
                let s = ui.scale;
                ui.font.draw(
                    ui.dl,
                    "You Died!",
                    (w / 2.0 - ui.font.width("You Died!")) / 2.0,
                    30.0,
                    s * 2.0,
                    WHITE,
                    true,
                );
                let cause = cx.game.as_ref().map_or(String::new(), |g| {
                    format!("Player {}", g.player.last_damage_cause)
                });
                ui.text_centered(&cause, w / 2.0, 85.0, WHITE);
                if ui.button(
                    "Respawn",
                    w / 2.0 - 100.0,
                    h / 4.0 + 72.0,
                    200.0,
                    20.0,
                    true,
                ) {
                    out.push(Action::Respawn);
                }
                if ui.button(
                    "Title Screen",
                    w / 2.0 - 100.0,
                    h / 4.0 + 96.0,
                    200.0,
                    20.0,
                    true,
                ) {
                    out.push(Action::SaveAndQuit);
                }
            }
            Screen::Chat { text } => {
                let (w, h) = (ui.w, ui.h);
                if let Some(g) = cx.game.as_mut() {
                    draw_chat(ui, g, true);
                }
                ui.fill(2.0, h - 14.0, w - 4.0, 12.0, [0.0, 0.0, 0.0, 0.5]);
                ui.input.focus = Some(rc_ui::id("chat"));
                let enter = ui.text_field("chat", 2.0, h - 14.0, w - 4.0, 12.0, text, 256, "");
                if enter {
                    if let Some(g) = cx.game.as_mut() {
                        g.submit_chat(text);
                    }
                    out.push(Action::CloseAll);
                } else if ui.input.key(Key::Escape) {
                    out.push(Action::CloseAll);
                }
            }
            Screen::Inventory => {
                if let Some(g) = cx.game.as_mut() {
                    survival_inventory(ui, cx.gfx, g, &mut out);
                }
            }
            Screen::Creative { scroll, search } => {
                if let Some(g) = cx.game.as_mut() {
                    creative_inventory(ui, cx.gfx, g, scroll, search, &mut out);
                }
            }
        }
        out
    }
}

fn parse_seed(s: &str) -> i64 {
    let s = s.trim();
    if s.is_empty() {
        return rand::random::<i64>();
    }
    if let Ok(v) = s.parse::<i64>() {
        return v;
    }
    // Java String.hashCode for text seeds.
    i64::from(
        s.chars()
            .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32)),
    )
}

fn update_text(s: &rc_update::Status) -> String {
    match s {
        rc_update::Status::Idle => "Not checked yet".into(),
        rc_update::Status::Checking => "Checking for updates...".into(),
        rc_update::Status::UpToDate => "You have the latest version".into(),
        rc_update::Status::Available(m) => format!("Version {} is available", m.version),
        rc_update::Status::Downloading { done, total } => {
            format!("Downloading... {}%", done * 100 / (*total).max(1))
        }
        rc_update::Status::Installed(v) => format!("Updated to {v} - restart to apply"),
        rc_update::Status::Failed(e) => format!("§cUpdate failed: {e}"),
    }
}

fn title(ui: &mut Ui<'_>, cx: &mut Ctx<'_>, out: &mut Vec<Action>) {
    ui.background(false);
    let (w, h) = (ui.w, ui.h);
    // Logo: the project name in large shadowed type.
    let s = ui.scale;
    let name = "RUSTCRAFT";
    let k = 4.0;
    let tw = ui.font.width(name) * k;
    let x = (w - tw) / 2.0;
    ui.font.draw(
        ui.dl,
        name,
        x / k,
        30.0 / k,
        s * k,
        [0.95, 0.55, 0.25, 1.0],
        true,
    );
    let sub = "a clean-room voxel game";
    ui.text_centered(sub, w / 2.0, 30.0 + 8.0 * k + 6.0, GRAY);
    // Splash.
    let t = ui.time;
    let pulse = 1.8 - ((t * 0.12).sin().abs()) * 0.1;
    let splash = "Written in Rust!";
    let sw = ui.font.width(splash);
    ui.font.draw(
        ui.dl,
        splash,
        (w / 2.0 + tw / 2.0 - sw * pulse / 2.0 - 10.0) / pulse,
        (30.0 + 9.0 * k - 4.0) / pulse,
        s * pulse,
        [1.0, 1.0, 0.0, 1.0],
        true,
    );
    let y = h / 4.0 + 48.0;
    if ui.button("Singleplayer", w / 2.0 - 100.0, y, 200.0, 20.0, true) {
        out.push(Action::Push(Screen::SelectWorld));
    }
    ui.button(
        "Multiplayer (v0.4)",
        w / 2.0 - 100.0,
        y + 24.0,
        200.0,
        20.0,
        false,
    );
    ui.button("Mods (v0.5)", w / 2.0 - 100.0, y + 48.0, 200.0, 20.0, false);
    if ui.button("Options...", w / 2.0 - 100.0, y + 84.0, 98.0, 20.0, true) {
        out.push(Action::Push(Screen::Options));
    }
    if ui.button("Quit Game", w / 2.0 + 2.0, y + 84.0, 98.0, 20.0, true) {
        out.push(Action::Quit);
    }
    ui.text(
        &format!("RustCraft {}", rc_core::version::VERSION),
        2.0,
        h - 10.0,
        WHITE,
    );
    let note = "Not affiliated with Mojang or Microsoft";
    ui.text_right(note, w - 2.0, h - 10.0, WHITE);
    match &cx.update {
        rc_update::Status::Available(m) => {
            let text = format!("§eUpdate {} available - click to install", m.version);
            let tw = ui.font.width(&text);
            let (bx, by) = (w - tw - 4.0, 4.0);
            ui.text(&text, bx, by, WHITE);
            if ui.hover(bx, by, tw, 9.0) && ui.input.pressed {
                out.push(Action::InstallUpdate);
            }
        }
        rc_update::Status::Downloading { .. } | rc_update::Status::Installed(_) => {
            let text = update_text(&cx.update);
            ui.text_right(&text, w - 2.0, 4.0, YELLOW);
        }
        _ => {}
    }
    if !cx.gfx.assets.is_vanilla() {
        let msg = "No Minecraft install found: using the built-in textures. Install Minecraft Java 1.21.1 for the vanilla look.";
        for (i, line) in ui.font.wrap(msg, w - 20.0).iter().enumerate() {
            ui.text_centered(line, w / 2.0, h - 34.0 + i as f32 * 10.0, GRAY);
        }
    }
}

fn video(ui: &mut Ui<'_>, cx: &mut Ctx<'_>, out: &mut Vec<Action>, in_world: bool) {
    ui.background(in_world);
    header(ui, "Video Settings");
    let o = &mut *cx.opts;
    let mut changed = false;
    let mut reload_world = false;
    let mut reload_assets = false;
    // 0: graphics
    let (x, y) = slot(ui, 0);
    let g = match o.graphics {
        Graphics::Fancy => "Fancy",
        Graphics::Fast => "Fast",
    };
    if ui.button(&format!("Graphics: {g}"), x, y, 150.0, 20.0, true) {
        o.graphics = if o.graphics == Graphics::Fancy {
            Graphics::Fast
        } else {
            Graphics::Fancy
        };
        changed = true;
        reload_world = true;
    }
    let (x, y) = slot(ui, 1);
    let mut rd = (o.render_distance - 2) as f32 / 30.0;
    if ui.slider(
        "rd",
        &format!("Render Distance: {} chunks", o.render_distance),
        x,
        y,
        150.0,
        20.0,
        &mut rd,
    ) {
        o.render_distance = 2 + (rd * 30.0).round() as i32;
        changed = true;
    }
    let (x, y) = slot(ui, 2);
    let mut sm = o.smooth_lighting;
    if ui.toggle("Smooth Lighting", x, y, 150.0, &mut sm) {
        o.smooth_lighting = sm;
        changed = true;
        reload_world = true;
    }
    let (x, y) = slot(ui, 3);
    let mut fps = if o.max_fps == 0 {
        1.0
    } else {
        (o.max_fps as f32 - 10.0) / 250.0
    };
    let fl = if o.max_fps == 0 {
        "Max Framerate: Unlimited".to_owned()
    } else {
        format!("Max Framerate: {} fps", o.max_fps)
    };
    if ui.slider("fps", &fl, x, y, 150.0, 20.0, &mut fps) {
        let v = 10 + (fps * 250.0 / 10.0).round() as u32 * 10;
        o.max_fps = if v >= 260 { 0 } else { v };
        changed = true;
    }
    let (x, y) = slot(ui, 4);
    let mut vs = o.vsync;
    if ui.toggle("VSync", x, y, 150.0, &mut vs) {
        o.vsync = vs;
        changed = true;
    }
    let (x, y) = slot(ui, 5);
    let gs = if o.gui_scale == 0 {
        "Auto".to_owned()
    } else {
        o.gui_scale.to_string()
    };
    if ui.button(&format!("GUI Scale: {gs}"), x, y, 150.0, 20.0, true) {
        o.gui_scale = (o.gui_scale + 1) % 5;
        changed = true;
    }
    let (x, y) = slot(ui, 6);
    let mut br = o.brightness;
    let bl = match (br * 100.0).round() as i32 {
        0 => "Brightness: Moody".to_owned(),
        100 => "Brightness: Bright".to_owned(),
        v => format!("Brightness: {v}%"),
    };
    if ui.slider("bright", &bl, x, y, 150.0, 20.0, &mut br) {
        o.brightness = br;
        changed = true;
    }
    let (x, y) = slot(ui, 7);
    let mut cl = o.clouds;
    if ui.toggle("Clouds", x, y, 150.0, &mut cl) {
        o.clouds = cl;
        changed = true;
    }
    let (x, y) = slot(ui, 8);
    let mut vb = o.view_bobbing;
    if ui.toggle("View Bobbing", x, y, 150.0, &mut vb) {
        o.view_bobbing = vb;
        changed = true;
    }
    let (x, y) = slot(ui, 9);
    let mut fs = o.fullscreen;
    if ui.toggle("Fullscreen", x, y, 150.0, &mut fs) {
        o.fullscreen = fs;
        out.push(Action::ToggleFullscreen);
        changed = true;
    }
    let (x, y) = slot(ui, 10);
    let mut mm = o.mipmap_levels as f32 / 4.0;
    let ml = if o.mipmap_levels == 0 {
        "Mipmap Levels: OFF".to_owned()
    } else {
        format!("Mipmap Levels: {}", o.mipmap_levels)
    };
    if ui.slider("mip", &ml, x, y, 150.0, 20.0, &mut mm) {
        let v = (mm * 4.0).round() as u32;
        if v != o.mipmap_levels {
            o.mipmap_levels = v;
            changed = true;
            reload_assets = true;
        }
    }
    let (x, y) = slot(ui, 11);
    let mut fpsd = o.show_fps;
    if ui.toggle("Show FPS", x, y, 150.0, &mut fpsd) {
        o.show_fps = fpsd;
        changed = true;
    }
    if changed {
        out.push(Action::OptionsChanged {
            reload_world,
            reload_assets,
        });
    }
    done_button(ui, out);
}

fn performance(ui: &mut Ui<'_>, cx: &mut Ctx<'_>, out: &mut Vec<Action>, in_world: bool) {
    ui.background(in_world);
    header(ui, "Performance");
    let o = &mut *cx.opts;
    let mut changed = false;
    let w = ui.w;
    let (x, y) = slot(ui, 0);
    if ui.button(
        &format!("Preset: {}", o.preset.label()),
        x,
        y,
        150.0,
        20.0,
        true,
    ) {
        let i = Preset::ALL.iter().position(|p| *p == o.preset).unwrap_or(0);
        o.apply_preset(Preset::ALL[(i + 1) % Preset::ALL.len()]);
        changed = true;
    }
    let total_mb = crate::sysinfo::total_memory_mb().max(1024);
    let (x, y) = slot(ui, 1);
    let mut m = (o.max_memory_mb as f32 - 256.0) / (total_mb as f32 - 256.0);
    let ml = format!("Max Memory: {} MB", o.max_memory_mb);
    if ui.slider("mem", &ml, x, y, 150.0, 20.0, &mut m) {
        let v = 256.0 + m.clamp(0.0, 1.0) * (total_mb as f32 - 256.0);
        o.max_memory_mb = ((v / 128.0).round() * 128.0) as u32;
        changed = true;
    }
    let cpus = crate::options::cpu_count() as f32;
    let (x, y) = slot(ui, 2);
    let mut c = o.cpu_threads as f32 / cpus;
    let cl = if o.cpu_threads == 0 {
        format!("CPU Threads: Auto ({})", o.worker_threads())
    } else {
        format!("CPU Threads: {}", o.cpu_threads)
    };
    if ui.slider("cpu", &cl, x, y, 150.0, 20.0, &mut c) {
        o.cpu_threads = (c * cpus).round() as u32;
        changed = true;
    }
    let (x, y) = slot(ui, 3);
    let mut g = (o.gpu_limit_percent as f32 - 10.0) / 90.0;
    let gl = format!("GPU Usage Limit: {}%", o.gpu_limit_percent);
    if ui.slider("gpu", &gl, x, y, 150.0, 20.0, &mut g) {
        o.gpu_limit_percent = 10 + (g * 9.0).round() as u32 * 10;
        changed = true;
    }
    let info = [
        format!(
            "Effective render distance: {} chunks (limited by memory)",
            o.effective_render_distance()
        ),
        format!(
            "Worker threads: {} of {} logical CPUs (applies on world load)",
            o.worker_threads(),
            cpus
        ),
        format!(
            "Frame cap: {}",
            match o.frame_cap() {
                0 => "none".to_owned(),
                v => format!("{v} fps"),
            }
        ),
        "Generation, lighting and meshing run on the worker pool;".to_owned(),
        "the render thread is never blocked by chunk work.".to_owned(),
    ];
    for (i, l) in info.iter().enumerate() {
        ui.text_centered(l, w / 2.0, 92.0 + i as f32 * 11.0, GRAY);
    }
    if changed {
        out.push(Action::OptionsChanged {
            reload_world: false,
            reload_assets: false,
        });
    }
    done_button(ui, out);
}

// ---------------------------------------------------------------------------
// HUD
// ---------------------------------------------------------------------------

pub fn draw_chat(ui: &mut Ui<'_>, g: &Game, open: bool) {
    let h = ui.h;
    let lines: Vec<&crate::game::ChatLine> = g
        .chat
        .iter()
        .rev()
        .filter(|l| open || g.tick.saturating_sub(l.tick) < 200)
        .take(if open { 20 } else { 10 })
        .collect();
    let base = h - 48.0;
    for (i, l) in lines.iter().enumerate() {
        let age = g.tick.saturating_sub(l.tick) as f32;
        let a = if open {
            1.0
        } else {
            (1.0 - (age - 180.0).max(0.0) / 20.0).clamp(0.0, 1.0)
        };
        let y = base - i as f32 * 9.0;
        ui.fill(2.0, y - 1.0, 320.0, 9.0, [0.0, 0.0, 0.0, 0.5 * a]);
        ui.text(&l.text, 4.0, y, [1.0, 1.0, 1.0, a]);
    }
}

fn draw_stack(ui: &mut Ui<'_>, gfx: &Gfx, s: Stack, x: f32, y: f32) {
    let scale = ui.scale;
    gfx.draw_item(ui.dl, scale, s.item, x, y, 16.0);
    if s.count > 1 {
        let t = s.count.to_string();
        let tw = ui.font.width(&t);
        ui.text(&t, x + 17.0 - tw, y + 9.0, WHITE);
    }
}

pub fn hud(ui: &mut Ui<'_>, cx: &mut Ctx<'_>) {
    let Some(g) = cx.game.as_deref() else { return };
    let (w, h) = (ui.w, ui.h);
    let t = &cx.gfx.theme;
    if g.player.eyes_in_water {
        ui.fill(0.0, 0.0, w, h, [0.02, 0.05, 0.25, 0.35]);
    }
    if g.player.hurt_time > 0 {
        ui.fill(
            0.0,
            0.0,
            w,
            h,
            [0.6, 0.0, 0.0, g.player.hurt_time as f32 / 40.0],
        );
    }
    if cx.hud_hidden {
        return;
    }
    // Crosshair.
    match t.crosshair {
        Some(s) => ui.sprite(
            &s,
            (w - 15.0) / 2.0,
            (h - 15.0) / 2.0,
            15.0,
            15.0,
            [1.0, 1.0, 1.0, 0.9],
        ),
        None => {
            ui.fill(w / 2.0 - 0.5, h / 2.0 - 5.0, 1.0, 10.0, WHITE);
            ui.fill(w / 2.0 - 5.0, h / 2.0 - 0.5, 10.0, 1.0, WHITE);
        }
    }
    // Hotbar.
    let hx = w / 2.0 - 91.0;
    let hy = h - 22.0;
    match t.hotbar {
        Some(s) => ui.sprite(&s, hx, hy, 182.0, 22.0, WHITE),
        None => {
            ui.fill(hx, hy, 182.0, 22.0, [0.0, 0.0, 0.0, 0.5]);
        }
    }
    let sel = g.inv.selected as f32;
    match t.hotbar_selection {
        Some(s) => ui.sprite(&s, hx - 1.0 + sel * 20.0, hy - 1.0, 24.0, 23.0, WHITE),
        None => ui.fill(hx - 1.0 + sel * 20.0, hy - 1.0, 24.0, 2.0, WHITE),
    }
    for i in 0..9 {
        if let Some(s) = g.inv.slots[i] {
            draw_stack(ui, cx.gfx, s, hx + 3.0 + i as f32 * 20.0, hy + 3.0);
        }
    }
    // Selected item name.
    let since = g.tick.saturating_sub(cx.selected_changed_tick);
    if since < 40
        && let Some(s) = g.inv.held()
    {
        let a = (1.0 - (since as f32 - 30.0).max(0.0) / 10.0).clamp(0.0, 1.0);
        let name = items().display_name(s.item);
        let y = h - 59.0
            + if g.player.mode == GameMode::Survival {
                0.0
            } else {
                14.0
            };
        ui.text_centered(&name, w / 2.0, y, [1.0, 1.0, 1.0, a]);
    }
    if g.player.mode == GameMode::Survival {
        // XP bar (empty in this version).
        if let Some(s) = t.xp_background {
            ui.sprite(&s, hx, h - 29.0, 182.0, 5.0, WHITE);
        }
        let hp = g.player.health.ceil() as i32;
        let blink = g.player.hurt_time > 0 && (g.player.hurt_time / 3) % 2 == 1;
        for i in 0..10 {
            let x = hx + i as f32 * 8.0;
            let y = h - 39.0
                + if hp <= 4 {
                    ((g.tick as f32 * 1.7 + i as f32 * 2.3).sin() * 1.0).round()
                } else {
                    0.0
                };
            if let Some(c) = t.heart_container {
                ui.sprite(
                    &c,
                    x,
                    y,
                    9.0,
                    9.0,
                    if blink { [1.0, 0.6, 0.6, 1.0] } else { WHITE },
                );
            }
            let v = hp - i * 2;
            let s = if v >= 2 {
                t.heart_full
            } else if v == 1 {
                t.heart_half
            } else {
                None
            };
            match s {
                Some(s) => ui.sprite(&s, x, y, 9.0, 9.0, WHITE),
                None if v >= 1 && t.heart_full.is_none() => ui.fill(
                    x + 1.0,
                    y + 1.0,
                    if v >= 2 { 7.0 } else { 3.5 },
                    7.0,
                    [0.85, 0.1, 0.1, 1.0],
                ),
                None => {}
            }
        }
        let food = g.player.food as i32;
        for i in 0..10 {
            let x = hx + 182.0 - 9.0 - i as f32 * 8.0;
            let y = h - 39.0;
            if let Some(c) = t.food_empty {
                ui.sprite(&c, x, y, 9.0, 9.0, WHITE);
            }
            let v = food - i * 2;
            let s = if v >= 2 {
                t.food_full
            } else if v == 1 {
                t.food_half
            } else {
                None
            };
            match s {
                Some(s) => ui.sprite(&s, x, y, 9.0, 9.0, WHITE),
                None if v >= 1 && t.food_full.is_none() => {
                    ui.fill(x + 1.0, y + 1.0, 7.0, 7.0, [0.6, 0.4, 0.2, 1.0])
                }
                None => {}
            }
        }
        if g.player.eyes_in_water || g.player.air < 300 {
            let air = g.player.air.max(0);
            let full = (air - 2) * 10 / 300;
            let pop = (air * 10 + 299) / 300 - full;
            for i in 0..10 {
                let x = hx + 182.0 - 9.0 - i as f32 * 8.0;
                let y = h - 49.0;
                let s = if i < full {
                    t.air
                } else if i < full + pop {
                    t.air_bursting
                } else {
                    None
                };
                if let Some(s) = s {
                    ui.sprite(&s, x, y, 9.0, 9.0, WHITE);
                } else if i < full {
                    ui.fill(x + 1.0, y + 1.0, 7.0, 7.0, [0.6, 0.8, 1.0, 1.0]);
                }
            }
        }
    }
    draw_chat(ui, g, false);
    if cx.opts.show_fps && !cx.f3 {
        ui.text(&format!("{:.0} fps", cx.stats.fps), 2.0, 2.0, WHITE);
    }
    if cx.f3 {
        debug_screen(ui, cx, g);
    }
}

fn debug_screen(ui: &mut Ui<'_>, cx: &Ctx<'_>, g: &Game) {
    let st = cx.stats;
    let p = g.player.pos;
    let bp = rc_core::BlockPos::new(
        p[0].floor() as i32,
        p[1].floor() as i32,
        p[2].floor() as i32,
    );
    let yaw = g.player.yaw.rem_euclid(360.0);
    let facing = match ((yaw + 45.0) / 90.0) as i32 % 4 {
        0 => "south (Towards positive Z)",
        1 => "west (Towards negative X)",
        2 => "north (Towards negative Z)",
        _ => "east (Towards positive X)",
    };
    let biome = rc_worldgen::biome::registry()
        .get(g.world.biome(bp))
        .name
        .clone();
    let (sky, blk) = g.world.light(bp);
    let left = vec![
        format!(
            "RustCraft {} (1.21.1 compatible)",
            rc_core::version::VERSION
        ),
        format!(
            "{:.0} fps ({:.1} ms)  T: {}",
            st.fps,
            st.frame_ms,
            match cx.opts.frame_cap() {
                0 => "inf".to_owned(),
                v => v.to_string(),
            }
        ),
        format!(
            "C: {}/{} sections, {} quads",
            st.sections_drawn, st.sections_total, st.quads
        ),
        format!(
            "Chunks: {} loaded, {} generating, {} workers",
            st.chunks, st.pending_gen, st.workers
        ),
        String::new(),
        format!("XYZ: {:.3} / {:.5} / {:.3}", p[0], p[1], p[2]),
        format!("Block: {} {} {}", bp.x, bp.y, bp.z),
        format!(
            "Chunk: {} {} {} in {} {} {}",
            bp.x & 15,
            bp.y & 15,
            bp.z & 15,
            bp.x >> 4,
            bp.y >> 4,
            bp.z >> 4
        ),
        format!(
            "Facing: {facing} ({:.1} / {:.1})",
            g.player.yaw.rem_euclid(360.0) - if yaw > 180.0 { 360.0 } else { 0.0 },
            g.player.pitch
        ),
        format!(
            "Client Light: {} ({} sky, {} block)",
            sky.max(blk),
            sky,
            blk
        ),
        format!("Biome: {biome}"),
        format!("Seed: {}", g.world.meta.seed),
        format!(
            "Mode: {}  Flying: {}",
            g.player.mode.name(),
            g.player.flying
        ),
    ];
    for (i, l) in left.iter().enumerate() {
        if l.is_empty() {
            continue;
        }
        let y = 2.0 + i as f32 * 9.0;
        let tw = ui.font.width(l);
        ui.fill(1.0, y - 1.0, tw + 2.0, 9.0, [0.31, 0.31, 0.31, 0.56]);
        ui.text_plain(l, 2.0, y, [0.88, 0.88, 0.88, 1.0]);
    }
    let mut right = vec![
        format!(
            "Mem: {:.0} MB (limit {} MB)",
            st.mem_mb, cx.opts.max_memory_mb
        ),
        format!(
            "CPU: {} logical, {} workers",
            crate::options::cpu_count(),
            st.workers
        ),
        String::new(),
        format!("Display: {}x{}", st.window.0, st.window.1),
        st.adapter.clone(),
        st.backend.clone(),
    ];
    if let Some(h) = g.target {
        let s = g.world.get_block(h.pos);
        let b = rc_data::blocks();
        right.push(String::new());
        right.push(format!(
            "§nTargeted Block: {}, {}, {}",
            h.pos.x, h.pos.y, h.pos.z
        ));
        right.push(format!("minecraft:{}", b.def(s).name));
        let full = b.to_string(s);
        if let Some(props) = full
            .split_once('[')
            .map(|x| x.1.trim_end_matches(']').to_owned())
        {
            for kv in props.split(',') {
                right.push(kv.replace('=', ": "));
            }
        }
    }
    let w = ui.w;
    for (i, l) in right.iter().enumerate() {
        if l.is_empty() {
            continue;
        }
        let y = 2.0 + i as f32 * 9.0;
        let tw = ui.font.width(l);
        ui.fill(
            w - tw - 3.0,
            y - 1.0,
            tw + 2.0,
            9.0,
            [0.31, 0.31, 0.31, 0.56],
        );
        ui.text_plain(l, w - tw - 2.0, y, [0.88, 0.88, 0.88, 1.0]);
    }
}

// ---------------------------------------------------------------------------
// Containers
// ---------------------------------------------------------------------------

fn slot_box(ui: &mut Ui<'_>, x: f32, y: f32) {
    ui.fill(x - 1.0, y - 1.0, 18.0, 18.0, [0.22, 0.22, 0.22, 1.0]);
    ui.fill(x, y, 17.0, 17.0, [1.0, 1.0, 1.0, 1.0]);
    ui.fill(x, y, 16.0, 16.0, [0.55, 0.55, 0.55, 1.0]);
}

/// Draw one inventory slot; returns true when hovered.
fn inv_slot(ui: &mut Ui<'_>, gfx: &Gfx, s: Option<Stack>, x: f32, y: f32, draw_box: bool) -> bool {
    if draw_box {
        slot_box(ui, x, y);
    }
    if let Some(s) = s {
        draw_stack(ui, gfx, s, x, y);
    }
    let hover = ui.hover(x, y, 16.0, 16.0);
    if hover {
        ui.fill(x, y, 16.0, 16.0, [1.0, 1.0, 1.0, 0.5]);
    }
    hover
}

fn panel(
    ui: &mut Ui<'_>,
    sprite: Option<rc_ui::Sprite>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    tex_size: (f32, f32),
) {
    match sprite {
        Some(mut s) => {
            s.uv = [0.0, 0.0, w / tex_size.0, h / tex_size.1];
            s.border = 0.0;
            ui.sprite(&s, x, y, w, h, WHITE);
        }
        None => {
            ui.fill(x, y, w, h, [0.78, 0.78, 0.78, 1.0]);
            ui.fill(x, y, w, 1.0, WHITE);
            ui.fill(x, y + h - 1.0, w, 1.0, [0.33, 0.33, 0.33, 1.0]);
        }
    }
}

fn survival_inventory(ui: &mut Ui<'_>, gfx: &Gfx, g: &mut Game, out: &mut Vec<Action>) {
    let (w, h) = (ui.w, ui.h);
    ui.fill(0.0, 0.0, w, h, [0.06, 0.06, 0.06, 0.75]);
    let (x0, y0) = ((w - 176.0) / 2.0, (h - 166.0) / 2.0);
    let has_tex = gfx.theme.inventory.is_some();
    panel(
        ui,
        gfx.theme.inventory,
        x0,
        y0,
        176.0,
        166.0,
        (256.0, 256.0),
    );
    ui.text_plain("Crafting", x0 + 97.0, y0 + 8.0, [0.25, 0.25, 0.25, 1.0]);
    let mut hovered = None;
    for i in 0..36 {
        let (sx, sy) = if i < 9 {
            (x0 + 8.0 + i as f32 * 18.0, y0 + 142.0)
        } else {
            let k = i - 9;
            (
                x0 + 8.0 + (k % 9) as f32 * 18.0,
                y0 + 84.0 + (k / 9) as f32 * 18.0,
            )
        };
        if inv_slot(ui, gfx, g.inv.slots[i], sx, sy, !has_tex) {
            hovered = Some(i);
        }
    }
    if let Some(i) = hovered {
        if ui.input.pressed {
            g.inv.click(i, false);
            ui.input.pressed = false;
        } else if ui.input.right_pressed {
            g.inv.click(i, true);
        }
        if g.inv.cursor.is_none()
            && let Some(s) = g.inv.slots[i]
        {
            ui.tooltip(&[items().display_name(s.item)]);
        }
    }
    cursor_stack(ui, gfx, g);
    if ui.input.key(Key::Escape) {
        if let Some(c) = g.inv.cursor.take() {
            g.inv.add(c.item, c.count);
        }
        out.push(Action::CloseAll);
    }
}

fn cursor_stack(ui: &mut Ui<'_>, gfx: &Gfx, g: &Game) {
    if let Some(c) = g.inv.cursor {
        let (mx, my) = ui.mouse();
        draw_stack(ui, gfx, c, mx - 8.0, my - 8.0);
    }
}

fn creative_inventory(
    ui: &mut Ui<'_>,
    gfx: &Gfx,
    g: &mut Game,
    scroll: &mut f32,
    search: &mut String,
    out: &mut Vec<Action>,
) {
    let (w, h) = (ui.w, ui.h);
    ui.fill(0.0, 0.0, w, h, [0.06, 0.06, 0.06, 0.75]);
    let (x0, y0) = ((w - 195.0) / 2.0, (h - 136.0) / 2.0);
    let it = items();
    let q = search.to_lowercase();
    let list: Vec<u16> = it
        .natural
        .iter()
        .copied()
        .filter(|&i| {
            q.is_empty()
                || it.names[i as usize].replace('_', " ").contains(&q)
                || it.display_name(i).to_lowercase().contains(&q)
        })
        .collect();
    let rows = list.len().div_ceil(9);
    let max_scroll = rows.saturating_sub(5) as f32;
    if ui.input.scroll != 0.0 {
        *scroll = (*scroll - ui.input.scroll.signum()).clamp(0.0, max_scroll);
    }
    let bg = if search.is_empty() {
        gfx.theme.creative_items
    } else {
        gfx.theme.creative_search.or(gfx.theme.creative_items)
    };
    let has_tex = bg.is_some();
    panel(ui, bg, x0, y0, 195.0, 136.0, (256.0, 256.0));
    // Tab label.
    if let Some(tab) = gfx.theme.creative_tab_top_selected.first() {
        ui.sprite(tab, x0, y0 - 28.0, 26.0, 32.0, WHITE);
    }
    if let Some(stone) = it.id("grass_block") {
        gfx.draw_item(ui.dl, ui.scale, stone, x0 + 5.0, y0 - 20.0, 16.0);
    }
    ui.text_plain(
        if search.is_empty() {
            "Natural Blocks"
        } else {
            "Search Items"
        },
        x0 + 8.0,
        y0 + 6.0,
        [0.25, 0.25, 0.25, 1.0],
    );
    // Search typed directly.
    for c in ui.input.typed.chars() {
        if !c.is_control() && search.len() < 32 && !(search.is_empty() && c == 'e') {
            search.push(c);
            *scroll = 0.0;
        }
    }
    if ui.input.key(Key::Backspace) {
        search.pop();
    }
    if !search.is_empty() {
        ui.text_plain(search, x0 + 82.0, y0 + 6.0, WHITE);
    }
    let start = (*scroll as usize) * 9;
    let mut hovered_palette = None;
    for r in 0..5 {
        for c in 0..9 {
            let i = start + r * 9 + c;
            let (sx, sy) = (x0 + 9.0 + c as f32 * 18.0, y0 + 18.0 + r as f32 * 18.0);
            let s = list.get(i).map(|&item| Stack { item, count: 1 });
            if inv_slot(ui, gfx, s, sx, sy, !has_tex) && s.is_some() {
                hovered_palette = s;
            }
        }
    }
    let mut hovered_hotbar = None;
    for i in 0..9 {
        let (sx, sy) = (x0 + 9.0 + i as f32 * 18.0, y0 + 112.0);
        if inv_slot(ui, gfx, g.inv.slots[i], sx, sy, !has_tex) {
            hovered_hotbar = Some(i);
        }
    }
    // Scrollbar.
    let track = 112.0 - 17.0;
    let sy = y0
        + 18.0
        + if max_scroll > 0.0 {
            *scroll / max_scroll * track
        } else {
            0.0
        };
    match gfx.theme.scroller {
        Some(s) => ui.sprite(&s, x0 + 175.0, sy, 12.0, 15.0, WHITE),
        None => ui.fill(x0 + 175.0, sy, 12.0, 15.0, [0.8, 0.8, 0.8, 1.0]),
    }
    if ui.input.down && ui.hover(x0 + 174.0, y0 + 18.0, 14.0, 112.0) && max_scroll > 0.0 {
        let (_, my) = ui.mouse();
        *scroll = (((my - y0 - 18.0 - 7.0) / track) * max_scroll)
            .round()
            .clamp(0.0, max_scroll);
    }
    if let Some(s) = hovered_palette {
        if ui.input.pressed {
            ui.input.pressed = false;
            if g.inv.cursor.is_some() {
                g.inv.cursor = None;
            } else if ui.input.shift {
                g.inv.add(s.item, it.max_stack[s.item as usize]);
            } else {
                g.inv.cursor = Some(Stack {
                    item: s.item,
                    count: it.max_stack[s.item as usize],
                });
            }
        } else if ui.input.right_pressed && g.inv.cursor.is_none() {
            g.inv.cursor = Some(Stack {
                item: s.item,
                count: 1,
            });
        }
        if g.inv.cursor.is_none() {
            ui.tooltip(&[
                it.display_name(s.item),
                format!("§9minecraft:{}", it.names[s.item as usize]),
            ]);
        }
    }
    if let Some(i) = hovered_hotbar {
        if ui.input.pressed {
            ui.input.pressed = false;
            if ui.input.shift {
                g.inv.slots[i] = None;
            } else {
                g.inv.click(i, false);
            }
        } else if ui.input.right_pressed {
            g.inv.click(i, true);
        }
        if g.inv.cursor.is_none()
            && let Some(s) = g.inv.slots[i]
        {
            ui.tooltip(&[it.display_name(s.item)]);
        }
    }
    // Clicking outside drops (deletes) the cursor stack.
    if ui.input.pressed && !ui.hover(x0, y0, 195.0, 136.0) {
        g.inv.cursor = None;
    }
    cursor_stack(ui, gfx, g);
    if ui.input.key(Key::Escape) {
        g.inv.cursor = None;
        out.push(Action::CloseAll);
    }
}
