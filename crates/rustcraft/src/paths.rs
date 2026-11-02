//! Game directory resolution and logging setup.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

/// Resolve the game directory.
///
/// Priority: `--game-dir` → portable mode (`portable.txt` next to the exe) →
/// platform data dir (`%APPDATA%\.rustcraft`, `~/.local/share/rustcraft`).
pub fn resolve_game_dir(override_dir: Option<&Path>) -> Result<PathBuf> {
    let dir = if let Some(d) = override_dir {
        d.to_path_buf()
    } else if let Some(portable) = portable_dir() {
        portable
    } else {
        default_game_dir()?
    };
    for sub in [
        "saves",
        "screenshots",
        "logs",
        "crash-reports",
        "resourcepacks",
        "cache",
    ] {
        std::fs::create_dir_all(dir.join(sub))
            .with_context(|| format!("creating {}", dir.join(sub).display()))?;
    }
    Ok(dir)
}

fn portable_dir() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    exe_dir
        .join("portable.txt")
        .exists()
        .then(|| exe_dir.join("rustcraft-data"))
}

fn default_game_dir() -> Result<PathBuf> {
    if cfg!(windows) {
        let base = directories::BaseDirs::new().context("no home directory")?;
        Ok(base.data_dir().join(".rustcraft"))
    } else {
        let proj =
            directories::ProjectDirs::from("", "", "rustcraft").context("no home directory")?;
        Ok(proj.data_dir().to_path_buf())
    }
}

/// Log to stderr and `logs/latest.log`.
pub fn init_logging(game_dir: &Path) -> Result<()> {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};
    let file = std::fs::File::create(game_dir.join("logs/latest.log"))?;
    let filter = EnvFilter::try_from_env("RUSTCRAFT_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,wgpu=warn"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false))
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(file)),
        )
        .init();
    Ok(())
}
