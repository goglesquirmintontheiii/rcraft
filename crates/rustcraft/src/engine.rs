//! The engine: simulation, streaming, rendering and UI for one frame.
//! Shared by the windowed app and headless screenshot runs.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use parking_lot::Mutex;
use rc_core::BlockPos;
use rc_render::mesh::MeshTables;
use rc_render::terrain::{FrameParams, QuadVertex};
use rc_ui::Input;
use tracing::{info, warn};

use crate::game::{Game, TickInput};
use crate::gfx::Gfx;
use crate::options::{Graphics, Options};
use crate::player::{GameMode, MoveInput};
use crate::screens::{Action, Ctx, Screen, Stats, WorldSummary};
use crate::world::{MeshShared, World, biome_colors};

/// Keys/buttons currently held, mapped to game actions.
#[derive(Clone, Copy, Debug, Default)]
pub struct Held {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
    pub attack: bool,
    pub use_item: bool,
}

/// One-shot inputs since the last tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct Edges {
    pub use_pressed: bool,
    pub pick: bool,
    pub jump_pressed: bool,
    pub forward_pressed: bool,
}

pub struct Engine {
    pub opts: Options,
    pub game_dir: PathBuf,
    pub gfx: Gfx,
    pub screens: Vec<Screen>,
    pub game: Option<Game>,
    pub input: Input,
    pub held: Held,
    pub edges: Edges,
    pub look_delta: (f64, f64),
    pub scroll_ticks: i32,
    pub stats: Stats,
    pub update: Arc<Mutex<rc_update::Status>>,
    pub f3: bool,
    pub hud_hidden: bool,
    pub time: f32,
    tick_acc: f64,
    selected_changed_tick: u64,
    last_selected: usize,
    last_forward_tick: Option<u64>,
    sprint_toggle: bool,
    pub quit: bool,
    pub wants_fullscreen: Option<bool>,
    frame_times: Vec<f32>,
    pub adapter: String,
    pub backend: String,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Engine({} screens)", self.screens.len())
    }
}

/// The single world slot.
pub fn world_dir(game_dir: &Path) -> PathBuf {
    game_dir.join("saves/world")
}

fn summary(game_dir: &Path) -> Option<WorldSummary> {
    let m = rc_world::rcw::WorldMeta::read(&world_dir(game_dir).join("world.rcw")).ok()?;
    let secs = m.last_played_ms / 1000;
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
    let last = if m.last_played_ms == 0 {
        "Never played".to_owned()
    } else {
        format!(
            "{y:04}-{mo:02}-{d:02} {:02}:{:02}",
            (secs / 3600) % 24,
            (secs / 60) % 60
        )
    };
    Some(WorldSummary {
        name: m.name,
        seed: m.seed,
        last_played: last,
        mode: m.game_mode,
    })
}

fn mesh_shared(gfx: &Gfx, opts: &Options) -> Arc<MeshShared> {
    let fancy = opts.graphics == Graphics::Fancy;
    Arc::new(MeshShared {
        tables: MeshTables::new(&gfx.models, fancy),
        models: gfx.models.clone(),
        colors: biome_colors(gfx.grass_map.as_ref(), gfx.foliage_map.as_ref()),
        smooth: opts.smooth_lighting,
    })
}

fn rgb(c: u32) -> [f32; 3] {
    [
        ((c >> 16) & 255) as f32 / 255.0,
        ((c >> 8) & 255) as f32 / 255.0,
        (c & 255) as f32 / 255.0,
    ]
}

impl Engine {
    pub fn new(
        opts: Options,
        game_dir: PathBuf,
        gfx: Gfx,
        adapter: String,
        backend: String,
    ) -> Self {
        Self {
            opts,
            game_dir,
            gfx,
            screens: vec![Screen::Title],
            game: None,
            input: Input::default(),
            held: Held::default(),
            edges: Edges::default(),
            look_delta: (0.0, 0.0),
            scroll_ticks: 0,
            stats: Stats::default(),
            update: Arc::new(Mutex::new(rc_update::Status::Idle)),
            f3: false,
            hud_hidden: false,
            time: 0.0,
            tick_acc: 0.0,
            selected_changed_tick: 0,
            last_selected: 0,
            last_forward_tick: None,
            sprint_toggle: false,
            quit: false,
            wants_fullscreen: None,
            frame_times: Vec::with_capacity(64),
            adapter,
            backend,
        }
    }

    /// Whether the mouse should be captured for looking around.
    #[must_use]
    pub fn wants_grab(&self) -> bool {
        self.game.is_some() && self.screens.is_empty()
    }

    pub fn save_options(&self) {
        self.opts.save(&self.game_dir.join("options.json"));
    }

    /// Start a background update check (and install, if automatic).
    pub fn check_updates(&self, install: bool) {
        let st = self.update.clone();
        *st.lock() = rc_update::Status::Checking;
        let auto = install || self.opts.update_policy == rc_update::UpdatePolicy::Automatic;
        std::thread::Builder::new()
            .name("rc-updater".into())
            .spawn(move || {
                let cur = semver::Version::parse(rc_core::version::VERSION)
                    .unwrap_or(semver::Version::new(0, 0, 0));
                match rc_update::check(&cur) {
                    Ok(None) => *st.lock() = rc_update::Status::UpToDate,
                    Ok(Some(m)) => {
                        info!(version = %m.version, "update available");
                        *st.lock() = rc_update::Status::Available(Box::new(m.clone()));
                        if auto {
                            install_update(&st, &m);
                        }
                    }
                    Err(e) => {
                        warn!("update check failed: {e:#}");
                        *st.lock() = rc_update::Status::Failed(e.to_string());
                    }
                }
            })
            .ok();
    }

    fn install(&self) {
        let st = self.update.clone();
        let m = match &*st.lock() {
            rc_update::Status::Available(m) => (**m).clone(),
            _ => return,
        };
        std::thread::spawn(move || install_update(&st, &m));
    }

    /// Open (or create) the single world.
    pub fn open_world(&mut self, create: Option<(String, i64, bool)>) -> Result<()> {
        let dir = world_dir(&self.game_dir);
        let (name, seed) = create
            .as_ref()
            .map_or(("World".to_owned(), 0), |c| (c.0.clone(), c.1));
        if create.is_some() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        let mut world = World::open(
            &dir,
            &name,
            seed,
            self.opts.worker_threads(),
            mesh_shared(&self.gfx, &self.opts),
        )?;
        if let Some((_, _, creative)) = create {
            world.meta.game_mode = if creative { "creative" } else { "survival" }.into();
        }
        self.gfx.terrain.clear();
        let mut g = Game::new(world);
        g.message(format!(
            "§7RustCraft {} - press T for chat, /help for commands",
            rc_core::version::VERSION
        ));
        self.game = Some(g);
        self.screens = vec![Screen::Loading];
        Ok(())
    }

    pub fn close_world(&mut self) {
        if let Some(mut g) = self.game.take() {
            g.save();
            drop(g);
        }
        self.gfx.terrain.clear();
        self.screens = vec![Screen::Title];
    }

    /// Fixed-rate simulation.
    fn simulate(&mut self, dt: f64) {
        let paused = self.screens.last().is_some_and(Screen::pauses);
        let Some(g) = self.game.as_mut() else { return };
        // Mouse look every frame.
        if self.screens.is_empty() {
            let s = f64::from(self.opts.mouse_sensitivity) * 0.6 + 0.2;
            let k = s * s * s * 8.0 * 0.15;
            let (dx, dy) = self.look_delta;
            g.player.yaw += (dx * k) as f32;
            let inv = if self.opts.invert_mouse { -1.0 } else { 1.0 };
            g.player.pitch = (g.player.pitch + (dy * k * inv) as f32).clamp(-90.0, 90.0);
        }
        self.look_delta = (0.0, 0.0);
        if self.scroll_ticks != 0 && self.screens.is_empty() {
            let n = (g.inv.selected as i32 - self.scroll_ticks).rem_euclid(9);
            g.inv.selected = n as usize;
        }
        self.scroll_ticks = 0;
        if g.inv.selected != self.last_selected {
            self.last_selected = g.inv.selected;
            self.selected_changed_tick = g.tick;
        }
        if paused {
            return;
        }
        self.tick_acc += dt;
        if self.tick_acc > 0.25 {
            self.tick_acc = 0.25;
        }
        let in_ui = !self.screens.is_empty();
        while self.tick_acc >= 0.05 {
            self.tick_acc -= 0.05;
            let h = if in_ui { Held::default() } else { self.held };
            // Double-tap W sprints.
            if self.edges.forward_pressed {
                if let Some(t) = self.last_forward_tick
                    && g.tick - t < 7
                {
                    self.sprint_toggle = true;
                }
                self.last_forward_tick = Some(g.tick);
            }
            if !h.forward {
                self.sprint_toggle = false;
            }
            let ti = TickInput {
                mv: MoveInput {
                    forward: f32::from(u8::from(h.forward)) - f32::from(u8::from(h.back)),
                    strafe: f32::from(u8::from(h.left)) - f32::from(u8::from(h.right)),
                    jump: h.jump,
                    sneak: h.sneak,
                    sprint: h.sprint || self.sprint_toggle,
                },
                attack: h.attack,
                use_held: h.use_item,
                use_pressed: self.edges.use_pressed && !in_ui,
                pick: self.edges.pick && !in_ui,
                jump_pressed: self.edges.jump_pressed && !in_ui,
            };
            self.edges = Edges::default();
            g.tick(&ti);
        }
    }

    fn frame_params(&self, size: (u32, u32), partial: f64) -> Option<FrameParams> {
        let g = self.game.as_ref()?;
        let p = &g.player;
        let mut eye = p.eye(partial);
        let mut pitch = p.pitch;
        if self.opts.view_bobbing {
            let wd = p.prev_walk_dist + (p.walk_dist - p.prev_walk_dist) * partial as f32;
            let bob = p.prev_bob + (p.bob - p.prev_bob) * partial as f32;
            let ph = wd * std::f32::consts::PI;
            eye[1] += f64::from((ph.cos() * bob).abs() * -0.5 * 0.0);
            pitch += (ph.cos() * bob).abs() * 5.0 * 0.6;
            let yaw = f64::from(p.yaw).to_radians();
            let side = f64::from(ph.sin() * bob * 0.5);
            eye[0] += yaw.cos() * side;
            eye[2] += yaw.sin() * side;
            eye[1] -= f64::from((ph.cos() * bob).abs());
        }
        let bp = BlockPos::new(
            eye[0].floor() as i32,
            eye[1].floor() as i32,
            eye[2].floor() as i32,
        );
        let reg = rc_worldgen::biome::registry();
        let biome = reg.get(g.world.biome(bp));
        let day = g.world.meta.day_time.rem_euclid(24000) as f32 / 24000.0;
        // Vanilla's time-of-day curve: 0 = noon at 6000 ticks.
        let tod = {
            let t = (day - 0.25).rem_euclid(1.0);
            let k = 1.0 - ((t * std::f32::consts::PI).cos() + 1.0) / 2.0;
            t + (k - t) / 3.0
        };
        let sun_h = (tod * std::f32::consts::TAU).cos();
        let daylight = (sun_h * 2.0 + 0.5).clamp(0.0, 1.0);
        let mut sky = rgb(biome.sky_color);
        let mut fog = rgb(biome.fog_color);
        for c in sky.iter_mut().chain(fog.iter_mut()) {
            *c *= daylight * 0.94 + 0.06;
        }
        let underwater = p.eyes_in_water;
        if underwater {
            fog = rgb(biome.water_fog_color);
        }
        let rd = self.opts.effective_render_distance() as f32 * 16.0;
        let fov = self.opts.fov
            + if p.sprinting { 8.0 } else { 0.0 }
            + if p.flying && p.sprinting { 4.0 } else { 0.0 };
        Some(FrameParams {
            cam_pos: eye,
            yaw: p.yaw,
            pitch,
            fov_y_deg: fov,
            aspect: size.0 as f32 / size.1.max(1) as f32,
            render_distance_blocks: rd,
            sky_color: sky,
            fog_color: fog,
            time_of_day: (0.25 - tod).rem_euclid(1.0),
            underwater,
            cloud_offset: (g.tick as f32 + partial as f32) * 0.03,
            clouds: self.opts.clouds,
            outline: None,
        })
    }

    /// Stream and upload without rendering (headless warm-up).
    pub fn stream_only(&mut self, device: &wgpu::Device) {
        if let Some(g) = self.game.as_mut() {
            let rd = self.opts.effective_render_distance();
            g.world.update(g.player.pos, rd, Duration::from_millis(20));
            for p in g.world.unloaded.drain(..) {
                self.gfx.terrain.remove_chunk(p.x, p.z);
            }
            while let Some((origin, mesh)) = g.world.mesh_out.pop() {
                self.gfx.terrain.upload(device, origin, &mesh);
            }
        }
    }

    /// Simulate, stream, render and draw UI into `target`.
    pub fn frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        size: (u32, u32),
        dt: f64,
    ) -> Vec<Action> {
        let t0 = Instant::now();
        self.time += (dt * 20.0) as f32;
        self.simulate(dt);
        // Stream chunks and upload finished meshes.
        if let Some(g) = self.game.as_mut() {
            let rd = self.opts.effective_render_distance();
            let budget = if matches!(self.screens.last(), Some(Screen::Loading)) {
                Duration::from_millis(12)
            } else {
                Duration::from_millis(3)
            };
            g.world.update(g.player.pos, rd, budget);
            for p in g.world.unloaded.drain(..) {
                self.gfx.terrain.remove_chunk(p.x, p.z);
            }
            let upload_start = Instant::now();
            let mut n = 0;
            while let Some((origin, mesh)) = g.world.mesh_out.pop() {
                self.gfx.terrain.upload(device, origin, &mesh);
                n += 1;
                if n > 64 && upload_start.elapsed() > Duration::from_millis(4) {
                    break;
                }
            }
            if g.player.dead() && !matches!(self.screens.last(), Some(Screen::Death)) {
                self.screens = vec![Screen::Death];
            }
            if matches!(self.screens.last(), Some(Screen::Loading)) {
                let (r, t) = g.world.spawn_progress(g.player_chunk(), 2);
                if g.spawned && r * 10 >= t * 9 {
                    self.screens.clear();
                }
            }
        }
        let partial = (self.tick_acc / 0.05).clamp(0.0, 1.0);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("frame"),
        });
        let world_drawn = if let Some(params) = self.frame_params(size, partial)
            && !matches!(self.screens.last(), Some(Screen::Loading))
        {
            let g = self.game.as_ref().expect("game");
            let day = g.world.meta.day_time.rem_euclid(24000) as f32 / 24000.0;
            let daylight = ((day - 0.25) * std::f32::consts::TAU)
                .cos()
                .mul_add(2.0, 0.5)
                .clamp(0.2, 1.0);
            self.gfx
                .terrain
                .update_lightmap(queue, daylight, self.opts.brightness, false);
            self.gfx
                .terrain
                .update_animations(queue, &self.gfx.models.atlas, g.tick);
            // Breaking overlay.
            self.gfx.terrain.overlay.clear();
            if let Some((p, prog)) = g.breaking {
                let stage = ((prog * 10.0) as usize).min(9);
                let s = self.gfx.models.destroy[stage];
                for bb in crate::player::outline_boxes(g.world.get_block(p)) {
                    push_box_overlay(&mut self.gfx.terrain.overlay, p, bb, s);
                }
            }
            let outline = g.target_outline();
            self.gfx.terrain.render(
                device,
                queue,
                &mut encoder,
                target,
                size,
                &params,
                outline.as_ref(),
            );
            true
        } else {
            false
        };
        // UI.
        let scale = if self.opts.gui_scale == 0 {
            rc_ui::auto_scale(size.0, size.1, 6)
        } else {
            self.opts
                .gui_scale
                .min(rc_ui::auto_scale(size.0, size.1, 99))
        } as f32;
        self.gfx.dl.clear();
        let mut dl = std::mem::take(&mut self.gfx.dl);
        let update = self.update.lock().clone();
        let world_summary = summary(&self.game_dir);
        let mut actions = Vec::new();
        {
            let mut cx = Ctx {
                opts: &mut self.opts,
                gfx: &self.gfx,
                game: self.game.as_mut(),
                world: world_summary,
                update,
                stats: &self.stats,
                f3: self.f3,
                hud_hidden: self.hud_hidden,
                selected_changed_tick: self.selected_changed_tick,
            };
            let mut ui = rc_ui::Ui {
                dl: &mut dl,
                font: &self.gfx.font,
                theme: &self.gfx.theme,
                input: &mut self.input,
                scale,
                w: size.0 as f32 / scale,
                h: size.1 as f32 / scale,
                time: self.time,
            };
            if world_drawn && !matches!(self.screens.last(), Some(Screen::Death)) {
                crate::screens::hud(&mut ui, &mut cx);
            }
            if let Some(top) = self.screens.last_mut() {
                actions = top.draw(&mut ui, &mut cx);
            }
        }
        self.input.end_frame();
        let clear = (!world_drawn).then_some(wgpu::Color::BLACK);
        self.gfx
            .gui
            .render(device, queue, &mut encoder, target, size, &dl, clear);
        self.gfx.dl = dl;
        queue.submit([encoder.finish()]);
        // Stats.
        let ft = t0.elapsed().as_secs_f32() * 1000.0;
        self.frame_times.push(dt as f32);
        if self.frame_times.len() > 30 {
            self.frame_times.remove(0);
        }
        let avg = self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32;
        let rs = self.gfx.terrain.stats;
        self.stats = Stats {
            fps: if avg > 0.0 { 1.0 / avg } else { 0.0 },
            frame_ms: ft,
            adapter: self.adapter.clone(),
            backend: self.backend.clone(),
            sections_drawn: rs.sections_drawn,
            sections_total: rs.sections_total,
            quads: rs.quads_drawn,
            chunks: self.game.as_ref().map_or(0, |g| g.world.chunks.len()),
            pending_gen: self
                .game
                .as_ref()
                .map_or(0, |g| g.world.pending_generation()),
            workers: self
                .game
                .as_ref()
                .map_or(self.opts.worker_threads(), |g| g.world.threads()),
            mem_mb: if (self.time as u64).is_multiple_of(20) {
                crate::sysinfo::process_memory_mb()
            } else {
                self.stats.mem_mb
            },
            window: size,
        };
        actions
    }

    /// Apply screen actions. Returns true if the GPU resources need rebuilding.
    pub fn handle(&mut self, actions: Vec<Action>) -> bool {
        let mut rebuild = false;
        for a in actions {
            match a {
                Action::Push(s) => self.screens.push(s),
                Action::Pop => {
                    self.screens.pop();
                    if self.screens.is_empty() && self.game.is_none() {
                        self.screens.push(Screen::Title);
                    }
                    self.save_options();
                }
                Action::CloseAll => {
                    self.screens.clear();
                    if self.game.is_none() {
                        self.screens.push(Screen::Title);
                    }
                }
                Action::PlayWorld => {
                    if let Err(e) = self.open_world(None) {
                        warn!("opening world: {e:#}");
                    }
                }
                Action::CreateWorld {
                    name,
                    seed,
                    creative,
                } => {
                    if let Err(e) = self.open_world(Some((name, seed, creative))) {
                        warn!("creating world: {e:#}");
                    }
                }
                Action::SaveAndQuit => self.close_world(),
                Action::Quit => self.quit = true,
                Action::Respawn => {
                    if let Some(g) = self.game.as_mut() {
                        g.respawn();
                    }
                    self.screens.clear();
                }
                Action::CheckUpdate => self.check_updates(false),
                Action::InstallUpdate => self.install(),
                Action::OptionsChanged {
                    reload_world,
                    reload_assets,
                } => {
                    self.save_options();
                    if reload_assets {
                        rebuild = true;
                    } else if reload_world && let Some(g) = self.game.as_mut() {
                        g.world.set_mesh_shared(mesh_shared(&self.gfx, &self.opts));
                    }
                }
                Action::ToggleFullscreen => self.wants_fullscreen = Some(self.opts.fullscreen),
            }
        }
        rebuild
    }

    /// Replace GPU-side resources (after mipmap/asset changes).
    pub fn replace_gfx(&mut self, gfx: Gfx) {
        self.gfx = gfx;
        if let Some(g) = self.game.as_mut() {
            g.world.set_mesh_shared(mesh_shared(&self.gfx, &self.opts));
        }
    }

    /// Keyboard shortcuts that work in game (not in text fields).
    pub fn game_key(&mut self, key: GameKey) {
        // The key that opens a screen must not also act inside it.
        if matches!(key, GameKey::Escape | GameKey::Inventory | GameKey::Chat(_))
            && self.screens.is_empty()
        {
            self.input.keys.clear();
            self.input.typed.clear();
        }
        let in_game = self.game.is_some();
        let top = self.screens.last().cloned();
        match key {
            GameKey::Escape => {
                if top.is_none() && in_game {
                    self.screens.push(Screen::Pause);
                }
            }
            GameKey::Inventory => match top {
                None if in_game => {
                    let creative = self
                        .game
                        .as_ref()
                        .is_some_and(|g| g.player.mode == GameMode::Creative);
                    self.screens.push(if creative {
                        Screen::Creative {
                            scroll: 0.0,
                            search: String::new(),
                        }
                    } else {
                        Screen::Inventory
                    });
                }
                Some(Screen::Inventory) => {
                    if let Some(g) = self.game.as_mut()
                        && let Some(c) = g.inv.cursor.take()
                    {
                        g.inv.add(c.item, c.count);
                    }
                    self.screens.clear();
                }
                Some(Screen::Creative { ref search, .. }) if search.is_empty() => {
                    if let Some(g) = self.game.as_mut() {
                        g.inv.cursor = None;
                    }
                    self.screens.clear();
                }
                _ => {}
            },
            GameKey::Chat(slash) => {
                if top.is_none() && in_game {
                    self.screens.push(Screen::Chat {
                        text: if slash { "/".into() } else { String::new() },
                    });
                    self.input.caret = usize::from(slash);
                    self.input.typed.clear();
                }
            }
            GameKey::Hotbar(i) => {
                if top.is_none()
                    && let Some(g) = self.game.as_mut()
                {
                    g.inv.selected = i;
                }
            }
            GameKey::F3 => self.f3 = !self.f3,
            GameKey::F1 => self.hud_hidden = !self.hud_hidden,
            GameKey::Drop => {
                if top.is_none()
                    && let Some(g) = self.game.as_mut()
                    && g.player.mode == GameMode::Survival
                {
                    g.inv.consume_held();
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum GameKey {
    Escape,
    Inventory,
    Chat(bool),
    Hotbar(usize),
    F3,
    F1,
    Drop,
}

fn install_update(st: &Arc<Mutex<rc_update::Status>>, m: &rc_update::Manifest) {
    let Some(asset) = m.asset_for_current_platform().cloned() else {
        *st.lock() = rc_update::Status::Failed("no build for this platform".into());
        return;
    };
    *st.lock() = rc_update::Status::Downloading {
        done: 0,
        total: asset.size,
    };
    let st2 = st.clone();
    match rc_update::download_and_install(&asset, |d, t| {
        *st2.lock() = rc_update::Status::Downloading { done: d, total: t };
    }) {
        Ok(()) => {
            info!(version = %m.version, "update installed; restart to apply");
            *st.lock() = rc_update::Status::Installed(m.version.clone());
        }
        Err(e) => {
            warn!("update failed: {e:#}");
            *st.lock() = rc_update::Status::Failed(e.to_string());
        }
    }
}

fn push_box_overlay(out: &mut Vec<QuadVertex>, p: BlockPos, b: [f64; 6], s: rc_assets::Sprite) {
    let e = 0.002;
    let (x0, y0, z0) = (
        f64::from(p.x) + b[0] - e,
        f64::from(p.y) + b[1] - e,
        f64::from(p.z) + b[2] - e,
    );
    let (x1, y1, z1) = (
        f64::from(p.x) + b[3] + e,
        f64::from(p.y) + b[4] + e,
        f64::from(p.z) + b[5] + e,
    );
    let faces = [
        [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
        [[x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1]],
        [[x1, y1, z0], [x1, y0, z0], [x0, y0, z0], [x0, y1, z0]],
        [[x0, y1, z1], [x0, y0, z1], [x1, y0, z1], [x1, y1, z1]],
        [[x0, y1, z0], [x0, y0, z0], [x0, y0, z1], [x0, y1, z1]],
        [[x1, y1, z1], [x1, y0, z1], [x1, y0, z0], [x1, y1, z0]],
    ];
    let uv = [
        s.map(0.0, 0.0),
        s.map(0.0, 16.0),
        s.map(16.0, 16.0),
        s.map(16.0, 0.0),
    ];
    for f in faces {
        for i in [0, 1, 2, 0, 2, 3] {
            out.push(QuadVertex {
                pos: [f[i][0] as f32, f[i][1] as f32, f[i][2] as f32],
                uv: uv[i],
                color: [0.0, 0.0, 0.0, 0.7],
            });
        }
    }
}
