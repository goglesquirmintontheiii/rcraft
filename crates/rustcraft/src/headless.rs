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
        engine.screens.clear();
        match scene {
            "f3" => engine.f3 = true,
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
                tab: crate::containers::CreativeTab::Items(crate::items::Tab::Building),
                scroll: 0.0,
                search: String::new(),
            }),
            "survival" | "crafting" | "furnace" | "hand" => {
                let it = crate::items::items();
                let g = engine.game.as_mut().expect("game");
                g.player.mode = GameMode::Survival;
                g.player.flying = scene == "hand";
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
