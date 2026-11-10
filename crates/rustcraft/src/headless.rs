//! Headless rendering of menus and world views to PNG (docs screenshots,
//! CI smoke tests).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use tracing::info;

use crate::engine::Engine;
use crate::gfx::Gfx;
use crate::options::Options;
use crate::player::GameMode;
use crate::screens::Screen;

#[derive(Debug)]
pub struct Request {
    pub path: PathBuf,
    pub scene: String,
    pub seed: i64,
    pub camera: Option<String>,
    pub size: (u32, u32),
    pub game_dir: PathBuf,
    pub assets: Option<PathBuf>,
    pub render_distance: Option<i32>,
}

pub fn run(req: &Request) -> Result<()> {
    let ctx = rc_render::GpuContext::new_headless()?;
    let info = ctx.adapter.get_info();
    let mut opts = Options::load(&req.game_dir.join("options.json"));
    if let Some(rd) = req.render_distance {
        opts.render_distance = rd;
    }
    opts.max_memory_mb = opts.max_memory_mb.max(8192);
    opts.apply_minecraft_dir();
    let assets = rc_assets::Assets::discover(
        req.assets
            .as_deref()
            .or(opts.assets_path.as_deref().map(std::path::Path::new)),
    );
    let format = rc_render::OFFSCREEN_FORMAT;
    let gfx = Gfx::new(&ctx.device, &ctx.queue, format, assets, &opts)?;
    let mut engine = Engine::new(
        opts,
        req.game_dir.clone(),
        gfx,
        info.name.clone(),
        format!("{:?}", info.backend),
    );
    let (w, h) = req.size;
    let tex = ctx.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("headless"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let frame = |e: &mut Engine| {
        let a = e.frame(&ctx.device, &ctx.queue, &view, (w, h), 1.0 / 60.0);
        let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
        a
    };
    let scene = req.scene.as_str();
    if scene == "bench" {
        return bench(&mut engine, &ctx, req);
    }
    let world_scene = matches!(
        scene,
        "world"
            | "f3"
            | "inventory"
            | "creative"
            | "pause"
            | "chat"
            | "survival"
            | "crafting"
            | "furnace"
            | "mobs"
            | "hand"
            | "rain"
            | "nether"
            | "station"
    );
    if world_scene {
        let dir = "Screenshot World";
        let path = crate::worlds::world_path(&req.game_dir, dir);
        let existing = rc_world::rcw::WorldMeta::read(&path.join("world.rcw"))
            .is_ok_and(|m| m.seed == req.seed);
        if existing {
            engine.open_world(Some(dir), None)?;
        } else {
            let _ = std::fs::remove_dir_all(&path);
            engine.open_world(
                None,
                Some(crate::engine::NewWorld {
                    name: dir.into(),
                    seed: req.seed,
                    mode: 1,
                    difficulty: 2,
                    rules: Vec::new(),
                }),
            )?;
        }
        let g = engine.game.as_mut().context("world")?;
        g.player.mode = GameMode::Creative;
        g.player.flying = true;
        let cam: Vec<String> = req
            .camera
            .clone()
            .unwrap_or_else(|| "0,surface,0,0,20".into())
            .split(',')
            .map(str::to_owned)
            .collect();
        let num = |i: usize, d: f64| cam.get(i).and_then(|s| s.parse::<f64>().ok()).unwrap_or(d);
        let (x, z) = (num(0, 0.0), num(2, 0.0));
        let surface = cam.get(1).is_some_and(|s| s.starts_with("surface"));
        let above: f64 = cam
            .get(1)
            .and_then(|s| s.strip_prefix("surface+"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(2.0);
        g.player.pos = [x, if surface { 200.0 } else { num(1, 100.0) }, z];
        g.player.prev_pos = g.player.pos;
        g.player.yaw = num(3, 0.0) as f32;
        g.player.pitch = num(4, 20.0) as f32;
        g.spawned = true;
        let start = Instant::now();
        let rd = engine.opts.effective_render_distance();
        let mut placed = !surface;
        loop {
            engine.stream_only(&ctx.device);
            let g = engine.game.as_mut().expect("game");
            g.player.vel = [0.0; 3];
            if !placed && let Some(y) = g.world.surface_y(x.floor() as i32, z.floor() as i32) {
                g.player.pos[1] = f64::from(y) + 1.0 + above - 1.62;
                g.player.prev_pos = g.player.pos;
                placed = true;
            }
            let (r, t) = g.world.spawn_progress(g.player_chunk(), rd);
            if placed && r == t && g.world.mesh_out.is_empty() && g.world.pending_generation() == 0
            {
                break;
            }
            if start.elapsed() > Duration::from_secs(900) {
                info!("timed out waiting for chunks ({r}/{t})");
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        info!(elapsed = ?start.elapsed(), "world ready");
        if scene == "nether" {
            // Through a portal: change dimension, then wait for the arrival
            // (which finds or builds a portal) and the chunks around it.
            let pos = engine.game.as_ref().context("world")?.player.pos;
            let (dim, to) = crate::portal::destination(rc_worldgen::Dimension::Overworld, pos)
                .context("destination")?;
            engine.change_dimension(dim, to)?;
            let start = Instant::now();
            loop {
                let _ = frame(&mut engine);
                let g = engine.game.as_mut().expect("game");
                let (r, t) = g.world.spawn_progress(g.player_chunk(), rd);
                if !g.arriving
                    && r == t
                    && g.world.mesh_out.is_empty()
                    && g.world.pending_generation() == 0
                {
                    break;
                }
                if start.elapsed() > Duration::from_secs(300) {
                    info!("timed out in the nether ({r}/{t})");
                    break;
                }
            }
            let g = engine.game.as_mut().expect("game");
            g.player.yaw = num(3, 0.0) as f32;
            g.player.pitch = num(4, 20.0) as f32;
            info!(pos = ?g.player.pos, dim = ?g.world.dimension, "arrived");
            // Step out of the portal to look back at it.
            g.player.pos[2] -= 3.0;
            g.player.prev_pos = g.player.pos;
            g.portal_cooldown = 1000;
        }
        engine.screens.clear();
        match scene {
            "f3" => engine.f3 = true,
            "station" => {
                // RC_STATION: anvil, grindstone, stonecutter, smithing_table.
                let it = crate::items::items();
                let g = engine.game.as_mut().expect("game");
                let name = std::env::var("RC_STATION").unwrap_or_else(|_| "anvil".into());
                let kind = crate::stations::Station::for_block(&name).context("station")?;
                g.player.mode = GameMode::Survival;
                g.player.xp_level = 5;
                let s = |n: &str, c: u8, d: u16| {
                    it.id(n).map(|item| crate::items::Stack {
                        item,
                        count: c,
                        damage: d,
                    })
                };
                let inputs = match kind {
                    crate::stations::Station::Anvil => {
                        vec![s("iron_pickaxe", 1, 180), s("iron_ingot", 4, 0)]
                    }
                    crate::stations::Station::Grindstone => {
                        vec![s("diamond_sword", 1, 900), s("diamond_sword", 1, 1200)]
                    }
                    crate::stations::Station::Stonecutter => vec![s("stone", 12, 0)],
                    crate::stations::Station::Smithing => vec![
                        s("netherite_upgrade_smithing_template", 1, 0),
                        s("diamond_chestplate", 1, 40),
                        s("netherite_ingot", 1, 0),
                    ],
                };
                for (i, st) in inputs.into_iter().enumerate() {
                    g.craft[i] = st;
                }
                g.container = Some(crate::game::Container::Station {
                    pos: rc_core::BlockPos::new(0, 0, 0),
                    kind,
                    selected: (kind == crate::stations::Station::Stonecutter).then_some(2),
                    scroll: 0,
                });
                engine.screens.push(Screen::Inventory);
            }
            "inventory" => {
                let it = crate::items::items();
                let g = engine.game.as_mut().expect("game");
                for (i, n) in [
                    "grass_block",
                    "oak_log",
                    "stone",
                    "dirt",
                    "oak_leaves",
                    "sand",
                    "diamond_ore",
                    "poppy",
                    "deepslate",
                ]
                .iter()
                .enumerate()
                {
                    if let Some(id) = it.id(n) {
                        g.inv.slots[i] = Some(crate::items::Stack {
                            damage: 0,
                            item: id,
                            count: if i == 0 { 64 } else { 1 + i as u8 * 7 },
                        });
                    }
                }
                engine.screens.push(Screen::Creative {
                    tab: crate::containers::CreativeTab::Inventory,
                    scroll: 0.0,
                    search: String::new(),
                });
            }
            "creative" => engine.screens.push(Screen::Creative {
                // RC_TAB picks the tab: "search", "inventory" or an index.
                tab: match std::env::var("RC_TAB").as_deref() {
                    Ok("search") => crate::containers::CreativeTab::Search,
                    Ok("inventory") => crate::containers::CreativeTab::Inventory,
                    Ok(n) => crate::containers::CreativeTab::Items(
                        crate::items::Tab::ALL
                            [n.parse::<usize>().unwrap_or(0) % crate::items::Tab::ALL.len()],
                    ),
                    Err(_) => crate::containers::CreativeTab::Items(crate::items::Tab::Building),
                },
                scroll: 0.0,
                search: String::new(),
            }),
            "survival" | "crafting" | "furnace" | "hand" => {
                let it = crate::items::items();
                let g = engine.game.as_mut().expect("game");
                g.player.mode = GameMode::Survival;
                g.player.flying = scene == "hand";
                g.player.xp_level = 7;
                g.player.xp_progress = 0.4;
                g.inv.slots.iter_mut().for_each(|s| *s = None);
                for (i, (n, c)) in [
                    ("oak_planks", 12u8),
                    ("stick", 8),
                    ("cobblestone", 30),
                    ("iron_pickaxe", 1),
                    ("bread", 5),
                    ("coal", 9),
                    ("raw_iron", 6),
                    ("oak_log", 3),
                ]
                .iter()
                .enumerate()
                {
                    if let Some(id) = it.id(n) {
                        g.inv.slots[i] = Some(crate::items::Stack {
                            damage: if *n == "iron_pickaxe" { 90 } else { 0 },
                            item: id,
                            count: *c,
                        });
                    }
                }
                let p = rc_core::BlockPos::new(
                    g.player.pos[0].floor() as i32 + 2,
                    g.player.pos[1].floor() as i32,
                    g.player.pos[2].floor() as i32,
                );
                match scene {
                    "crafting" => {
                        g.craft[0] = Some(crate::items::Stack::new(
                            it.id("oak_planks").unwrap_or(0),
                            1,
                        ));
                        g.craft[1] = Some(crate::items::Stack::new(
                            it.id("oak_planks").unwrap_or(0),
                            1,
                        ));
                        g.craft[3] = Some(crate::items::Stack::new(
                            it.id("oak_planks").unwrap_or(0),
                            1,
                        ));
                        g.craft[4] = Some(crate::items::Stack::new(
                            it.id("oak_planks").unwrap_or(0),
                            1,
                        ));
                        g.container = Some(crate::game::Container::Crafting { pos: p });
                        engine.screens.push(Screen::Inventory);
                    }
                    "furnace" => {
                        let st = rc_data::blocks().default_state("furnace");
                        g.set_block(p, st);
                        if let Some(crate::blockentity::BlockEntity::Furnace {
                            slots,
                            burn,
                            burn_total,
                            cook,
                            cook_total,
                            ..
                        }) = g.world.block_entities.get_mut(&p)
                        {
                            slots[0] =
                                Some(crate::items::Stack::new(it.id("raw_iron").unwrap_or(0), 4));
                            slots[1] =
                                Some(crate::items::Stack::new(it.id("coal").unwrap_or(0), 3));
                            slots[2] = Some(crate::items::Stack::new(
                                it.id("iron_ingot").unwrap_or(0),
                                2,
                            ));
                            (*burn, *burn_total, *cook, *cook_total) = (900, 1600, 120, 200);
                        }
                        g.container = Some(crate::game::Container::Furnace { pos: p });
                        engine.screens.push(Screen::Inventory);
                    }
                    "survival" => engine.screens.push(Screen::Inventory),
                    _ => {
                        g.inv.selected = std::env::var("RC_SLOT")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(3)
                    }
                }
            }
            "mobs" | "rain" => {
                let g = engine.game.as_mut().expect("game");
                let (yaw, pos) = (g.player.yaw, g.player.pos);
                let (sy, cy) = f64::from(yaw).to_radians().sin_cos();
                let kinds = crate::mobs::MobKind::ALL;
                for (i, k) in kinds.iter().enumerate() {
                    for baby in [false, true] {
                        let side = (i as f64 - 1.5) * 2.2 + if baby { 0.9 } else { 0.0 };
                        let fwd = 6.0 + if baby { 1.5 } else { 0.0 };
                        let x = pos[0] - sy * fwd - cy * side;
                        let z = pos[2] + cy * fwd - sy * side;
                        let y = g
                            .world
                            .surface_y(x.floor() as i32, z.floor() as i32)
                            .map_or(pos[1], |y| f64::from(y) + 1.0);
                        let mut e = crate::entity::Entity::new(
                            crate::entity::Kind::Mob(Box::new(crate::mobs::Mob::new(*k, baby))),
                            [x, y, z],
                        );
                        e.yaw = yaw + 180.0 + i as f32 * 25.0;
                        e.prev_yaw = e.yaw;
                        e.prev_pos = e.pos;
                        if let crate::entity::Kind::Mob(m) = &mut e.kind {
                            m.head_yaw = e.yaw;
                        }
                        g.entities.spawn(e);
                    }
                }
                let it = crate::items::items();
                for (j, n) in ["diamond", "oak_log", "apple", "torch"].iter().enumerate() {
                    let x = pos[0] - sy * 4.0 - cy * (j as f64 - 1.5);
                    let z = pos[2] + cy * 4.0 - sy * (j as f64 - 1.5);
                    let y = g
                        .world
                        .surface_y(x.floor() as i32, z.floor() as i32)
                        .map_or(pos[1], |y| f64::from(y) + 1.0);
                    let mut e = crate::entity::Entity::item(
                        crate::items::Stack::new(it.id(n).unwrap_or(0), 1),
                        [x, y, z],
                        40,
                    );
                    e.prev_pos = e.pos;
                    g.entities.spawn(e);
                }
                if scene == "rain" {
                    g.rain = 1.0;
                    g.prev_rain = 1.0;
                    g.world.meta.raining = true;
                }
                let p = rc_core::BlockPos::new(
                    (pos[0] - sy * 3.0).floor() as i32,
                    pos[1].floor() as i32 - 1,
                    (pos[2] + cy * 3.0).floor() as i32,
                );
                let st = g.world.get_block(p);
                let uv = crate::PARTICLE_SPRITES
                    .with(|s| s.borrow().get(st.0 as usize).copied().unwrap_or([0.0; 4]));
                g.particles.block_break(p, uv, [1.0; 3]);
            }
            "pause" => engine.screens.push(Screen::Pause),
            "chat" => {
                let g = engine.game.as_mut().expect("game");
                g.submit_chat("/seed");
                g.submit_chat("Hello from RustCraft!");
                engine.screens.push(Screen::Chat {
                    text: "/gamemode creative".into(),
                });
            }
            _ => {}
        }
        if matches!(
            scene,
            "inventory" | "creative" | "survival" | "crafting" | "furnace"
        ) {
            engine.input.mouse_px = (w as f32 * 0.42, h as f32 * 0.42);
        }
    } else {
        engine.screens = match scene {
            "options" => vec![Screen::Title, Screen::Options],
            "video" => vec![Screen::Title, Screen::Video],
            "performance" => vec![Screen::Title, Screen::Performance],
            "select" => vec![
                Screen::Title,
                Screen::SelectWorld {
                    selected: None,
                    scroll: 0.0,
                },
            ],
            "create" => vec![
                Screen::Title,
                Screen::CreateWorld {
                    name: "New World".into(),
                    seed: String::new(),
                    mode: 0,
                    difficulty: 2,
                    more: false,
                    rules: crate::worlds::DEFAULT_RULES
                        .iter()
                        .map(|(k, v)| ((*k).to_owned(), *v))
                        .collect(),
                },
            ],
            _ => vec![Screen::Title],
        };
        engine.input.mouse_px = (w as f32 * 0.5, h as f32 * 0.47);
    }
    for _ in 0..3 {
        frame(&mut engine);
    }
    let px = rc_render::screenshot::readback(&ctx.device, &ctx.queue, &tex, w, h)?;
    rc_render::screenshot::save_png(&px, w, h, &req.path)?;
    info!(path = %req.path.display(), "screenshot written");
    if engine.game.is_some() {
        engine.close_world();
    }
    Ok(())
}

/// `--scene bench`: stream a fresh world while flying, and report chunk
/// throughput, main-thread streaming cost and memory. Environment:
/// `RC_BENCH_SECS` (default 30), `RC_BENCH_SPEED` blocks/s (default 0 =
/// stand still), `RC_BENCH_FRAMES` (default 1 = full frames, 0 = streaming
/// only), `RC_BENCH_SPIN` degrees/s of turning, `RC_BENCH_Y` eye height,
/// `RC_BENCH_MEM` memory limit in MB.
fn bench(engine: &mut Engine, ctx: &rc_render::GpuContext, req: &Request) -> Result<()> {
    let env = |k: &str, d: f64| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    let secs = env("RC_BENCH_SECS", 30.0);
    let speed = env("RC_BENCH_SPEED", 0.0);
    let frames = env("RC_BENCH_FRAMES", 1.0) > 0.0;
    let spin = env("RC_BENCH_SPIN", 0.0) as f32;
    let y0 = env("RC_BENCH_Y", 140.0);
    if let Ok(v) = std::env::var("RC_BENCH_MEM")
        && let Ok(mb) = v.parse()
    {
        engine.opts.max_memory_mb = mb;
    }
    info!(
        mem_mb = crate::sysinfo::process_memory_mb(),
        "bench before world"
    );
    let dir = "Bench World";
    let _ = std::fs::remove_dir_all(crate::worlds::world_path(&req.game_dir, dir));
    engine.open_world(
        None,
        Some(crate::engine::NewWorld {
            name: dir.into(),
            seed: req.seed,
            mode: 1,
            difficulty: 2,
            rules: Vec::new(),
        }),
    )?;
    engine.screens.clear();
    info!(
        mem_mb = crate::sysinfo::process_memory_mb(),
        "bench world open"
    );
    let (w, h) = req.size;
    let tex = ctx.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bench"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: rc_render::OFFSCREEN_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    {
        let g = engine.game.as_mut().context("world")?;
        g.player.mode = GameMode::Creative;
        g.player.flying = true;
        g.player.pos = [0.0, y0, 0.0];
        g.player.prev_pos = g.player.pos;
        g.player.pitch = 10.0;
        g.spawned = true;
    }
    let start = Instant::now();
    let mut times: Vec<f64> = Vec::new();
    let mut last = Instant::now();
    let mut peak_mb = 0.0f32;
    let mut report = Instant::now();
    while start.elapsed().as_secs_f64() < secs {
        let dt = last.elapsed().as_secs_f64().clamp(0.001, 0.25);
        last = Instant::now();
        {
            let g = engine.game.as_mut().context("world")?;
            g.player.pos[0] += speed * dt;
            g.player.yaw = (g.player.yaw + spin * dt as f32).rem_euclid(360.0);
            g.player.prev_pos = g.player.pos;
            g.player.vel = [0.0; 3];
        }
        let t = Instant::now();
        if frames {
            let _ = engine.frame(&ctx.device, &ctx.queue, &view, (w, h), dt);
            let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
        } else {
            engine.stream_only(&ctx.device);
        }
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        let mb = crate::sysinfo::process_memory_mb();
        peak_mb = peak_mb.max(mb);
        if report.elapsed() > Duration::from_secs(5) {
            report = Instant::now();
            let g = engine.game.as_ref().context("world")?;
            info!(
                t = start.elapsed().as_secs(),
                generated = g.world.generated_count,
                loaded = g.world.chunks.len(),
                pending = g.world.pending_generation(),
                rd = engine.render_distance(),
                mem_mb = mb,
                "bench"
            );
        }
        if !frames {
            std::thread::sleep(Duration::from_millis(4));
        }
    }
    let g = engine.game.as_ref().context("world")?;
    let el = start.elapsed().as_secs_f64();
    times.sort_by(f64::total_cmp);
    let pct = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
    info!(
        chunks_per_sec = format!("{:.1}", g.world.generated_count as f64 / el),
        generated = g.world.generated_count,
        loaded = g.world.chunks.len(),
        frames = times.len(),
        frame_ms_p50 = format!("{:.2}", pct(0.5)),
        frame_ms_p99 = format!("{:.2}", pct(0.99)),
        frame_ms_max = format!("{:.2}", pct(1.0)),
        peak_mem_mb = peak_mb,
        "bench result"
    );
    info!(
        carved = crate::world::prof::CARVED.load(std::sync::atomic::Ordering::Relaxed),
        "bench carved chunks"
    );
    for (stage, secs) in crate::world::prof::report() {
        info!(
            stage,
            cpu_secs = format!("{secs:.2}"),
            ms_per_chunk = format!(
                "{:.2}",
                secs * 1000.0 / g.world.generated_count.max(1) as f64
            ),
            "bench stage"
        );
    }
    Ok(())
}
