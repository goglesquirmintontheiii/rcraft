//! `cargo xtask <command>` — developer tooling that lives in the repo.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail};
use clap::{Parser, Subcommand};

mod parity;

#[derive(Parser)]
#[command(about = "RustCraft developer tasks")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Build the game and render a headless screenshot.
    Screenshot {
        #[arg(default_value = "docs/screenshots/latest.png")]
        out: PathBuf,
        #[arg(long, default_value = "1280x720")]
        size: String,
    },
    /// Diff generated chunks against a vanilla world (seed-exact parity).
    Parity {
        world: PathBuf,
        /// overworld | nether | end
        #[arg(long, default_value = "overworld")]
        dim: String,
        /// biomes | carvers | full
        #[arg(long, default_value = "carvers")]
        stage: String,
        #[arg(long, default_value_t = 64)]
        limit: usize,
        /// For --stage full: block size in chunks.
        #[arg(long, default_value_t = 3)]
        n: i32,
        /// For --stage full: chunk x,z to search near.
        #[arg(long, value_parser = parse_pair)]
        near: Option<(i32, i32)>,
    },
    /// Copy reference chunks from a vanilla world into tests/parity/<name>.
    ParityExtract { world: PathBuf, name: String },
    /// Write `latest.json` + `SHA256SUMS` for a set of release assets.
    Manifest {
        /// Release version, e.g. 0.1.0
        version: String,
        /// Directory containing the built assets.
        dir: PathBuf,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Screenshot { out, size } => screenshot(&out, &size),
        Cmd::Manifest { version, dir } => manifest(&version, &dir),
        Cmd::ParityExtract { world, name } => {
            parity::extract(&world, &name, &root().join("tests/parity"))
        }
        Cmd::Parity {
            world,
            dim,
            stage,
            limit,
            n,
            near,
        } => {
            let dim = match dim.as_str() {
                "nether" => rc_worldgen::Dimension::Nether,
                "end" => rc_worldgen::Dimension::End,
                _ => rc_worldgen::Dimension::Overworld,
            };
            if stage == "full" {
                parity::run_full(&world, dim, n, near)
            } else {
                parity::run(&world, dim, &stage, limit)
            }
        }
    }
}

fn parse_pair(s: &str) -> Result<(i32, i32), String> {
    let (a, b) = s.split_once(',').ok_or("expected X,Z")?;
    Ok((
        a.trim().parse().map_err(|_| "bad x")?,
        b.trim().parse().map_err(|_| "bad z")?,
    ))
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn screenshot(out: &Path, size: &str) -> Result<()> {
    let status = Command::new(env!("CARGO"))
        .current_dir(root())
        .args(["run", "-q", "-p", "rustcraft", "--", "--screenshot"])
        .arg(out)
        .args(["--size", size])
        .status()
        .context("running rustcraft")?;
    if !status.success() {
        bail!("rustcraft exited with {status}");
    }
    println!("wrote {}", out.display());
    Ok(())
}

fn manifest(version: &str, dir: &Path) -> Result<()> {
    use sha2::{Digest, Sha256};
    let mut assets = Vec::new();
    let mut sums = String::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let (platform, kind) = match name.as_str() {
            n if n.ends_with(".exe") => ("windows-x86_64", "exe"),
            n if n.ends_with(".AppImage") => ("linux-x86_64", "appimage"),
            _ => continue,
        };
        let bytes = std::fs::read(&path)?;
        let sha = format!("{:x}", Sha256::digest(&bytes));
        sums.push_str(&format!("{sha}  {name}\n"));
        assets.push(serde_json::json!({
            "platform": platform,
            "kind": kind,
            "url": format!("https://github.com/goglesquirmintontheiii/rcraft/releases/download/v{version}/{name}"),
            "size": bytes.len(),
            "sha256": sha,
        }));
    }
    if assets.is_empty() {
        bail!("no release assets found in {}", dir.display());
    }
    let manifest = serde_json::json!({
        "version": version,
        "published": chrono_now(),
        "notes_url": format!("https://github.com/goglesquirmintontheiii/rcraft/releases/tag/v{version}"),
        "assets": assets,
    });
    std::fs::write(
        dir.join("latest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    std::fs::write(dir.join("SHA256SUMS"), sums)?;
    println!(
        "wrote latest.json and SHA256SUMS for {} asset(s)",
        manifest["assets"].as_array().unwrap().len()
    );
    Ok(())
}

/// ISO-8601 UTC timestamp without pulling in a date crate.
fn chrono_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let days = secs / 86_400;
    let (h, m, s) = ((secs % 86_400) / 3600, (secs % 3600) / 60, secs % 60);
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}
