//! RustCraft game binary.
//!
//! Phase 0: argument parsing, logging, game-directory resolution, a window
//! with a cleared wgpu surface, and headless `--screenshot` support.

#![forbid(unsafe_code)]

mod app;
mod paths;

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

    /// Render one frame headlessly to this PNG and exit.
    #[arg(long, value_name = "PNG")]
    screenshot: Option<PathBuf>,

    /// Window / screenshot size as WIDTHxHEIGHT.
    #[arg(long, default_value = "1280x720", value_parser = parse_size)]
    size: (u32, u32),

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
    info!(
        version = rc_core::version::VERSION,
        target = rc_core::version::TARGET_MINECRAFT_VERSION,
        game_dir = %game_dir.display(),
        "starting RustCraft"
    );

    if let Some(path) = cli.screenshot {
        let ctx = rc_render::GpuContext::new_headless()?;
        rc_render::screenshot::capture_png(&ctx, cli.size.0, cli.size.1, &path)?;
        info!(path = %path.display(), "screenshot written");
        return Ok(());
    }

    app::run(app::Options {
        size: cli.size,
        check_updates: !cli.no_update,
    })
}
