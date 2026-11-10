//! RustCraft game binary.

// No console window for release builds on Windows; logs go to logs/latest.log.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]
// Renderer/UI entry points take many plain parameters by design.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod app;
mod blockentity;
mod blocklogic;
mod commands;
mod containers;
mod engine;
mod entity;
mod entity_icons;
mod fallback_tex;
mod game;
mod gfx;
mod headless;
mod item_tints;
mod items;
mod java;
mod loot;
mod mobs;
mod models;
mod options;
mod particles;
mod paths;
mod player;
mod recipes;
mod scene;
mod screens;
mod sysinfo;
mod world;
mod worlds;

thread_local! {
    /// Block particle sprites (atlas UVs per block state), set when models load.
    pub static PARTICLE_SPRITES: std::cell::RefCell<Vec<[f32; 4]>> = const { std::cell::RefCell::new(Vec::new()) };
}

static AUDIO_EVENTS: std::sync::RwLock<Option<std::sync::Arc<rc_audio::Audio>>> =
    std::sync::RwLock::new(None);

/// Whether the loaded sound set defines `event`.
pub fn audio_has(event: &str) -> bool {
    AUDIO_EVENTS
        .read()
        .ok()
        .and_then(|a| a.as_ref().map(|a| a.has_event(event)))
        .unwrap_or(false)
}

/// Register the active audio system for `audio_has`.
pub fn set_audio(a: std::sync::Arc<rc_audio::Audio>) {
    if let Ok(mut g) = AUDIO_EVENTS.write() {
        *g = Some(a);
    }
}

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use tracing::info;

#[derive(Parser, Debug)]
#[command(
    name = "rustcraft",
    version,
    about = "RustCraft — clean-room Minecraft 1.21.1 in Rust"
)]
struct Cli {
    /// Override the game directory (saves, options, screenshots…).
    #[arg(long, value_name = "DIR")]
    game_dir: Option<PathBuf>,

    /// Use assets from this Minecraft jar or extracted folder.
    #[arg(long, value_name = "PATH")]
    assets: Option<PathBuf>,

    /// Render headlessly to this PNG and exit (see --scene).
    #[arg(long, value_name = "PNG")]
    screenshot: Option<PathBuf>,

    /// Headless scene: title, options, video, performance, world, f3, inventory, creative, pause.
    #[arg(long, default_value = "title")]
    scene: String,

    /// Seed for headless world scenes.
    #[arg(long, default_value_t = 0)]
    seed: i64,

    /// Camera for headless world scenes: x,y,z,yaw,pitch (y may be "surface").
    #[arg(long)]
    camera: Option<String>,

    /// Window / screenshot size as WIDTHxHEIGHT.
    #[arg(long, default_value = "1280x720", value_parser = parse_size)]
    size: (u32, u32),

    /// Render distance override for headless runs.
    #[arg(long)]
    render_distance: Option<i32>,

    /// Skip the update check for this launch.
    #[arg(long)]
    no_update: bool,
}

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s.split_once('x').ok_or("expected WIDTHxHEIGHT")?;
    let w = w.parse().map_err(|_| "bad width")?;
    let h = h.parse().map_err(|_| "bad height")?;
    Ok((w, h))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let game_dir = paths::resolve_game_dir(cli.game_dir.as_deref())?;
    paths::init_logging(&game_dir)?;
    paths::install_crash_reporter(&game_dir);
    info!(
        version = rc_core::version::VERSION,
        target = rc_core::version::TARGET_MINECRAFT_VERSION,
        game_dir = %game_dir.display(),
        cpus = options::cpu_count(),
        "starting RustCraft"
    );

    if let Some(path) = cli.screenshot {
        return headless::run(&headless::Request {
            path,
            scene: cli.scene,
            seed: cli.seed,
            camera: cli.camera,
            size: cli.size,
            game_dir,
            assets: cli.assets,
            render_distance: cli.render_distance,
        });
    }

    app::run(app::LaunchOptions {
        size: cli.size,
        check_updates: !cli.no_update,
        game_dir,
        assets: cli.assets,
    })
}
