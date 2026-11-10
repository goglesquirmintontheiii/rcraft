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
use crate::screens::{Action, Ctx, Screen, Stats};
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
    pub attack_pressed: bool,
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
    /// Smoothed FOV multiplier (sprinting/flying), eased each frame like vanilla.
    fov_mod: f32,
    scroll_acc: f32,
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
    pub audio: Arc<rc_audio::Audio>,
    /// Cached world list (refreshed when the list screens open).
    pub worlds: Vec<crate::screens::WorldSummary>,
    pub java_worlds: Vec<(String, PathBuf)>,
    /// Folder name (under `saves`) of the open world.
    pub world_name: Option<String>,
    scene_scratch: Vec<crate::models::ModelQuad>,
    music_cooldown: u32,
    ambient_cooldown: u32,
    /// Render-distance cap from measured process memory (see `memory_guard`).
    mem_rd_cap: i32,
    mem_check: Instant,
    /// Process memory before the world opened (driver, assets, …): the part
    /// the render distance cannot reduce.
    base_mem_mb: f32,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Engine({} screens)", self.screens.len())
    }
}

/// Parameters for a brand-new world.
#[derive(Clone, Debug)]
pub struct NewWorld {
    pub name: String,
    pub seed: i64,
    pub mode: u8,
    pub difficulty: u8,
    pub rules: Vec<(String, bool)>,
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
        let audio = Arc::new(rc_audio::Audio::new(Arc::new(gfx.assets.stack.reopen())));
        crate::set_audio(audio.clone());
        let worlds = crate::worlds::list(&game_dir);
        let e = Self {
            audio,
            worlds,
            java_worlds: Vec::new(),
            world_name: None,
            scene_scratch: Vec::with_capacity(512),
            music_cooldown: 600,
            mem_rd_cap: crate::options::MAX_RENDER_DISTANCE,
            mem_check: Instant::now(),
            base_mem_mb: 0.0,
            ambient_cooldown: 0,
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
            fov_mod: 1.0,
            scroll_acc: 0.0,
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
        };
        e.apply_volumes();
        e.set_particle_sprites();
        e
    }

    /// Render distance in use: the option, limited by the memory estimate
    /// and by what the process actually uses.
    #[must_use]
    pub fn render_distance(&self) -> i32 {
        self.opts
            .effective_render_distance()
            .min(self.mem_rd_cap)
            .max(2)
    }

    /// Enforce the memory limit against real usage. Loaded chunks cost
    /// roughly (2r+7)² × per-chunk, so when over the limit the view distance
    /// is scaled by the square root of how far over the chunk part is,
    /// instead of one step every two seconds (issue #2).
    fn memory_guard(&mut self) {
        if Instant::now() < self.mem_check + Duration::from_secs(1) || self.game.is_none() {
            return;
        }
        self.mem_check = Instant::now();
        let used = crate::sysinfo::process_memory_mb();
        let limit = self.opts.max_memory_mb as f32;
        let cur = self.render_distance();
        let base = self.base_mem_mb.max(1.0);
        if used > limit * 1.03 && cur > 2 {
            let chunk_used = (used - base).max(1.0);
            let chunk_budget = (limit - base).max(0.0) * 0.9;
            let side = (2 * cur + 7) as f32 * (chunk_budget / chunk_used).sqrt();
            let rd = (((side - 7.0) / 2.0).floor() as i32).clamp(2, cur - 1);
            self.mem_rd_cap = rd;
            // Let the unloads land (and the allocator settle) before
            // measuring again, so one overshoot is not counted twice.
            self.mem_check = Instant::now() + Duration::from_secs(6);
            warn!(
                used,
                limit, base, rd, "over the memory limit; reducing render distance"
            );
        } else if used < limit * 0.8 && self.mem_rd_cap < self.opts.effective_render_distance() {
            self.mem_rd_cap += 1;
        }
    }

    /// Push option volumes into the audio system.
    pub fn apply_volumes(&self) {
        let mut v = self.audio.volumes.lock();
        for (i, c) in rc_audio::Category::ALL.iter().enumerate() {
            v.insert(*c, self.opts.volumes[i].clamp(0.0, 1.0));
        }
    }

    fn set_particle_sprites(&self) {
        let uv: Vec<[f32; 4]> = self.gfx.models.particle.iter().map(|s| s.uv).collect();
        crate::PARTICLE_SPRITES.with(|p| *p.borrow_mut() = uv);
    }

    fn refresh_worlds(&mut self) {
        self.worlds = crate::worlds::list(&self.game_dir);
    }

    fn message(&mut self, title: &str, text: impl Into<String>) {
        self.screens.push(Screen::Message {
            title: title.into(),
            text: text.into(),
        });
    }

    /// Feed raw wheel movement; emits whole notches only.
    pub fn add_scroll(&mut self, d: f32) {
        self.scroll_acc += d;
        let steps = self.scroll_acc.trunc();
        if steps != 0.0 {
            self.scroll_acc -= steps;
            self.input.scroll += steps;
            if self.screens.is_empty() {
                self.scroll_ticks += steps as i32;
            }
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

    /// Open the world in `saves/<dir>`, or create it from `create`.
    pub fn open_world(&mut self, dir: Option<&str>, create: Option<NewWorld>) -> Result<()> {
        self.base_mem_mb = crate::sysinfo::process_memory_mb();
        self.mem_rd_cap = crate::options::MAX_RENDER_DISTANCE;
        let dir_name = match (&create, dir) {
            (Some(c), _) => crate::worlds::new_dir_name(&self.game_dir, &c.name),
            (None, Some(d)) => d.to_owned(),
            (None, None) => anyhow::bail!("no world selected"),
        };
        let path = crate::worlds::world_path(&self.game_dir, &dir_name);
        let (name, seed) = create
            .as_ref()
            .map_or((dir_name.clone(), 0), |c| (c.name.clone(), c.seed));
        let mut world = World::open(
            &path,
            &name,
            seed,
            self.opts.worker_threads(),
            mesh_shared(&self.gfx, &self.opts),
        )?;
        if let Some(c) = &create {
            world.meta.game_mode =
                ["survival", "creative", "spectator"][usize::from(c.mode.min(2))].into();
            world.meta.difficulty =
                ["peaceful", "easy", "normal", "hard"][usize::from(c.difficulty.min(3))].into();
            for (k, v) in &c.rules {
                world.meta.rules.insert(k.clone(), v.to_string());
            }
        }
        self.gfx.terrain.clear();
        let mut g = Game::new(world);
        g.message(format!(
            "§7RustCraft {} - press T for chat, /help for commands",
            rc_core::version::VERSION
        ));
        self.game = Some(g);
        self.world_name = Some(dir_name);
        self.screens = vec![Screen::Loading];
        Ok(())
    }

    /// Move the player to another dimension of the same world: save, then
    /// reopen the world streaming that dimension.
    pub fn change_dimension(&mut self, dim: &str, pos: [f64; 3]) -> Result<()> {
        let Some(dir_name) = self.world_name.clone() else {
            return Ok(());
        };
        let Some(mut g) = self.game.take() else {
            return Ok(());
        };
        let respawning = !g.spawned;
        g.player.pos = pos;
        g.player.prev_pos = pos;
        g.player.vel = [0.0; 3];
        g.world.meta.dimension = if dim == "overworld" {
            String::new()
        } else {
            dim.to_owned()
        };
        g.save();
        drop(g);
        self.gfx.terrain.clear();
        self.gfx.terrain.clear_batches();
        let path = crate::worlds::world_path(&self.game_dir, &dir_name);
        let world = World::open(
            &path,
            &dir_name,
            0,
            self.opts.worker_threads(),
            mesh_shared(&self.gfx, &self.opts),
        )?;
        let mut g = Game::new(world);
        if respawning {
            // Died elsewhere: find the spawn point again.
            g.spawned = false;
        } else {
            g.arriving = true;
        }
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
        self.gfx.terrain.clear_batches();
        self.world_name = None;
        self.refresh_worlds();
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
        // Vanilla eases the FOV modifier toward its target by half the gap per tick.
        let target = if g.player.sprinting {
            if g.player.flying { 1.15 } else { 1.12 }
        } else {
            1.0
        };
        let k = 1.0 - 0.5f32.powf((dt * 20.0) as f32);
        self.fov_mod += (target - self.fov_mod) * k;
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
                attack_pressed: self.edges.attack_pressed && !in_ui,
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
        let overworld = g.world.dimension == rc_worldgen::Dimension::Overworld;
        let mut sky = rgb(biome.sky_color);
        let mut fog = rgb(biome.fog_color);
        if overworld {
            for c in sky.iter_mut().chain(fog.iter_mut()) {
                *c *= daylight * 0.94 + 0.06;
            }
        } else {
            // No sky in the Nether or the End: everything fades to fog.
            sky = fog;
        }
        let underwater = p.eyes_in_water;
        if underwater {
            fog = rgb(biome.water_fog_color);
        }
        let rd = self.render_distance() as f32 * 16.0;
        let fov = self.opts.fov * self.fov_mod;
        Some(FrameParams {
            cam_pos: eye,
            yaw: p.yaw,
            pitch,
            fov_y_deg: fov,
            aspect: size.0 as f32 / size.1.max(1) as f32,
            render_distance_blocks: rd,
            sky_color: sky,
            fog_color: fog,
            time_of_day: if overworld {
                (0.25 - tod).rem_euclid(1.0)
            } else {
                0.25
            },
            underwater,
            cloud_offset: (g.tick as f32 + partial as f32) * 0.03,
            clouds: self.opts.clouds,
            celestial: overworld,
            outline: None,
        })
    }

    /// Stream and upload without rendering (headless warm-up).
    pub fn stream_only(&mut self, device: &wgpu::Device) {
        let rd = self.render_distance();
        if let Some(g) = self.game.as_mut() {
            g.stream(rd, Duration::from_millis(20));
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
        self.memory_guard();
        self.time += (dt * 20.0) as f32;
        self.simulate(dt);
        if let Some((dim, pos)) = self.game.as_mut().and_then(|g| g.wants_dimension.take())
            && let Err(e) = self.change_dimension(&dim, pos)
        {
            warn!("changing dimension: {e:#}");
            self.close_world();
        }
        let t_sim = t0.elapsed();
        let mut t_stream = Duration::ZERO;
        let mut t_upload = Duration::ZERO;
        // Stream chunks and upload finished meshes.
        let rd = self.render_distance();
        if let Some(g) = self.game.as_mut() {
            let budget = if matches!(self.screens.last(), Some(Screen::Loading)) {
                Duration::from_millis(12)
            } else if !g.player_chunk_ready() {
                // Just teleported: spend more of the frame on arrivals.
                Duration::from_millis(8)
            } else {
                Duration::from_millis(3)
            };
            let ts = Instant::now();
            g.stream(rd, budget);
            for p in g.world.unloaded.drain(..) {
                self.gfx.terrain.remove_chunk(p.x, p.z);
            }
            t_stream = ts.elapsed();
            let upload_start = Instant::now();
            let mut n = 0;
            while let Some((origin, mesh)) = g.world.mesh_out.pop() {
                self.gfx.terrain.upload(device, origin, &mesh);
                n += 1;
                if n > 64 && upload_start.elapsed() > Duration::from_millis(4) {
                    break;
                }
            }
            t_upload = upload_start.elapsed();
            // A block GUI opened this tick.
            if g.container.is_some() && self.screens.is_empty() {
                self.screens.push(Screen::Inventory);
            }
            // Sounds.
            self.audio.set_listener(g.player.eye(1.0), g.player.yaw);
            for snd in g.sounds.drain(..) {
                self.audio
                    .play(&snd.event, snd.pos, snd.volume, snd.pitch, snd.cat);
            }
            if self.audio.enabled() {
                self.music_cooldown = self.music_cooldown.saturating_sub(1);
                if self.music_cooldown == 0 {
                    let ev = if g.player.mode == GameMode::Creative {
                        "music.creative"
                    } else {
                        "music.game"
                    };
                    self.audio.play_music(ev);
                    self.music_cooldown = 20 * 60 * 6;
                }
                // Rain ambience.
                self.ambient_cooldown = self.ambient_cooldown.saturating_sub(1);
                if g.rain > 0.2 && self.ambient_cooldown == 0 {
                    let p = g.player.pos;
                    let under = g
                        .world
                        .surface_y(p[0].floor() as i32, p[2].floor() as i32)
                        .is_some_and(|y| f64::from(y) > p[1] + 1.0);
                    let ev = if under {
                        "weather.rain.above"
                    } else {
                        "weather.rain"
                    };
                    self.audio.play(
                        ev,
                        None,
                        0.2 * g.rain,
                        if under { 0.5 } else { 1.0 },
                        rc_audio::Category::Weather,
                    );
                    self.ambient_cooldown = 25;
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
            let daylight = crate::scene::daylight(g, partial as f32).max(0.2);
            let flash = g.flash > 0;
            self.gfx.terrain.update_lightmap(
                queue,
                if flash { 1.0 } else { daylight },
                self.opts.brightness,
                false,
            );
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
            let cam = crate::scene::Cam::new(params.cam_pos, params.yaw, params.pitch);
            let first_person =
                !self.hud_hidden && !matches!(self.screens.last(), Some(Screen::Death));
            crate::scene::fill(
                &mut self.gfx,
                g,
                &cam,
                partial,
                first_person,
                &mut self.scene_scratch,
            );
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
        let mut actions = Vec::new();
        let worlds = std::mem::take(&mut self.worlds);
        let java_worlds = std::mem::take(&mut self.java_worlds);
        {
            let mut cx = Ctx {
                opts: &mut self.opts,
                gfx: &self.gfx,
                game: self.game.as_mut(),
                worlds,
                java_worlds,
                audio: Some(&self.audio),
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
            self.worlds = std::mem::take(&mut cx.worlds);
            self.java_worlds = std::mem::take(&mut cx.java_worlds);
        }
        if self.input.clicked_sound {
            self.audio.play(
                "ui.button.click",
                None,
                0.25,
                1.0,
                rc_audio::Category::Master,
            );
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
        if ft > 50.0 {
            warn!(
                frame_ms = ft,
                simulate_ms = t_sim.as_secs_f32() * 1000.0,
                stream_ms = t_stream.as_secs_f32() * 1000.0,
                upload_ms = t_upload.as_secs_f32() * 1000.0,
                "long frame"
            );
        }
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
                Action::Push(s) => {
                    match &s {
                        Screen::SelectWorld { .. } => self.refresh_worlds(),
                        Screen::ImportWorld { .. } => {
                            self.java_worlds = crate::java::find_java_worlds()
                        }
                        _ => {}
                    }
                    self.screens.push(s);
                }
                Action::Pop => {
                    self.screens.pop();
                    if self.screens.is_empty() && self.game.is_none() {
                        self.screens.push(Screen::Title);
                    }
                    self.save_options();
                }
                Action::CloseAll => {
                    if let Some(g) = self.game.as_mut() {
                        g.close_container();
                    }
                    self.screens.clear();
                    if self.game.is_none() {
                        self.screens.push(Screen::Title);
                    }
                }
                Action::PlayWorld(dir) => {
                    if let Err(e) = self.open_world(Some(&dir), None) {
                        warn!("opening world: {e:#}");
                        self.message("Could not open world", format!("{e:#}"));
                    }
                }
                Action::CreateWorld {
                    name,
                    seed,
                    mode,
                    difficulty,
                    rules,
                } => {
                    if let Err(e) = self.open_world(
                        None,
                        Some(NewWorld {
                            name,
                            seed,
                            mode,
                            difficulty,
                            rules,
                        }),
                    ) {
                        warn!("creating world: {e:#}");
                        self.message("Could not create world", format!("{e:#}"));
                    }
                }
                Action::RenameWorld(dir, name) => {
                    if let Err(e) = crate::worlds::rename(&self.game_dir, &dir, &name) {
                        self.message("Rename failed", format!("{e:#}"));
                    }
                    self.refresh_worlds();
                }
                Action::DeleteWorld(dir) => {
                    if let Err(e) = crate::worlds::delete(&self.game_dir, &dir) {
                        self.message("Delete failed", format!("{e:#}"));
                    }
                    self.refresh_worlds();
                }
                Action::BackupWorld(dir) => match crate::worlds::backup(&self.game_dir, &dir) {
                    Ok(p) => self.message("Backup created", p.display().to_string()),
                    Err(e) => self.message("Backup failed", format!("{e:#}")),
                },
                Action::ImportJava(path) => {
                    let src = PathBuf::from(&path);
                    let name = src
                        .file_name()
                        .map_or("Imported".into(), |n| n.to_string_lossy().into_owned());
                    let dir = crate::worlds::new_dir_name(&self.game_dir, &name);
                    let dest = crate::worlds::world_path(&self.game_dir, &dir);
                    match crate::java::import(&src, &dest) {
                        Ok(msg) => {
                            self.screens.pop();
                            self.refresh_worlds();
                            self.message("World imported", msg);
                        }
                        Err(e) => {
                            let _ = std::fs::remove_dir_all(&dest);
                            self.message("Import failed", format!("{e:#}"));
                        }
                    }
                }
                Action::ExportJava(dir) => {
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs());
                    let dest = self
                        .game_dir
                        .join("exports")
                        .join(format!("{dir}_java_{ts}"));
                    if self.world_name.as_deref() == Some(dir.as_str())
                        && let Some(g) = self.game.as_mut()
                    {
                        g.save();
                    }
                    match crate::java::export(
                        &crate::worlds::world_path(&self.game_dir, &dir),
                        &dest,
                    ) {
                        Ok(msg) => self.message(
                            "Exported for Java Edition",
                            format!("{msg}. Copy the folder into .minecraft/saves to play it."),
                        ),
                        Err(e) => self.message("Export failed", format!("{e:#}")),
                    }
                }
                Action::VolumesChanged => {
                    self.apply_volumes();
                    self.save_options();
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
                Action::OpenDataFolder => open_folder(&self.game_dir),
            }
        }
        rebuild
    }

    /// Replace GPU-side resources (after mipmap/asset changes).
    pub fn replace_gfx(&mut self, gfx: Gfx) {
        self.gfx = gfx;
        self.set_particle_sprites();
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
                            tab: crate::containers::CreativeTab::Items(crate::items::Tab::Building),
                            scroll: 0.0,
                            search: String::new(),
                        }
                    } else {
                        Screen::Inventory
                    });
                }
                Some(Screen::Inventory) => {
                    if let Some(g) = self.game.as_mut() {
                        g.close_container();
                    }
                    self.screens.clear();
                }
                Some(Screen::Creative {
                    ref search,
                    ref tab,
                    ..
                }) if search.is_empty() || *tab != crate::containers::CreativeTab::Search => {
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
            GameKey::Drop(whole) => {
                if top.is_none()
                    && let Some(g) = self.game.as_mut()
                    && g.player.mode != GameMode::Spectator
                {
                    g.drop_held(whole);
                    g.swing_arm(true);
                }
            }
            GameKey::SwapHands => {
                if top.is_none()
                    && let Some(g) = self.game.as_mut()
                    && g.player.mode != GameMode::Spectator
                {
                    let sel = g.inv.selected;
                    g.inv.slots.swap(sel, crate::items::OFFHAND_SLOT);
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
    /// Q (true with Ctrl: whole stack).
    Drop(bool),
    /// F: swap the held item with the offhand.
    SwapHands,
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

/// Show a folder in the platform file manager.
fn open_folder(dir: &Path) {
    let cmd = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(cmd).arg(dir).spawn() {
        warn!("could not open {}: {e}", dir.display());
    }
}
