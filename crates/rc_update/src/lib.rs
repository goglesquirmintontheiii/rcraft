//! Self-update.
//!
//! Flow (Phase 1 completes the network and file-swap steps):
//! 1. Fetch `latest.json` from the GitHub Releases "latest" download URL.
//! 2. Compare its `version` with [`rc_core::version::semver`].
//! 3. Download the asset for this platform, check size + SHA-256, verify the
//!    minisign signature of `SHA256SUMS` with the embedded public key.
//! 4. Replace the running executable (`self-replace`) and relaunch.
//!
//! The manifest format is fixed here so `release.yml` and the game agree.

#![forbid(unsafe_code)]

use semver::Version;
use serde::{Deserialize, Serialize};

/// Repository the updater reads releases from.
pub const REPO: &str = "goglesquirmintontheiii/rcraft";
pub const MANIFEST_URL: &str =
    "https://github.com/goglesquirmintontheiii/rcraft/releases/latest/download/latest.json";

/// User-facing update policy, persisted in `options.toml`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePolicy {
    /// Download and install silently, apply on next launch.
    Automatic,
    /// Check and prompt in the title screen.
    #[default]
    Ask,
    Off,
}

/// `latest.json` published with every release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: Version,
    /// ISO-8601 publication time.
    pub published: String,
    pub notes_url: String,
    pub assets: Vec<Asset>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    /// `windows-x86_64` | `linux-x86_64`
    pub platform: String,
    /// `exe` | `appimage`
    pub kind: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

/// Platform string of this build, matching [`Asset::platform`].
#[must_use]
pub fn current_platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x86_64",
        ("linux", "x86_64") => "linux-x86_64",
        _ => "unsupported",
    }
}

impl Manifest {
    #[must_use]
    pub fn asset_for_current_platform(&self) -> Option<&Asset> {
        let p = current_platform();
        self.assets.iter().find(|a| a.platform == p)
    }

    /// True when the manifest describes a newer build than `current`.
    #[must_use]
    pub fn is_newer_than(&self, current: &Version) -> bool {
        self.version > *current
    }
}

/// Progress of a background update, shared with the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    Available(Box<Manifest>),
    Downloading {
        done: u64,
        total: u64,
    },
    /// Installed; takes effect on restart.
    Installed(Version),
    Failed(String),
}

/// Fetch `latest.json`.
pub fn fetch_manifest() -> anyhow::Result<Manifest> {
    let mut resp = ureq::get(MANIFEST_URL)
        .header(
            "User-Agent",
            concat!("RustCraft-Updater/", env!("CARGO_PKG_VERSION")),
        )
        .call()?;
    let body = resp.body_mut().with_config().limit(1 << 20).read_to_vec()?;
    Ok(serde_json::from_slice(&body)?)
}

/// Check for a newer release than `current`.
pub fn check(current: &Version) -> anyhow::Result<Option<Manifest>> {
    let m = fetch_manifest()?;
    Ok(m.is_newer_than(current).then_some(m))
}

/// Hex SHA-256 of `bytes`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(bytes);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// Verify downloaded bytes against the manifest entry.
pub fn verify(asset: &Asset, bytes: &[u8]) -> anyhow::Result<()> {
    anyhow::ensure!(
        bytes.len() as u64 == asset.size,
        "size mismatch: got {} expected {}",
        bytes.len(),
        asset.size
    );
    let got = sha256_hex(bytes);
    anyhow::ensure!(
        got.eq_ignore_ascii_case(&asset.sha256),
        "checksum mismatch: got {got}"
    );
    Ok(())
}

/// Download `asset`, verify it and replace the running executable. The new
/// version runs on next launch. `progress(done, total)` is called as bytes
/// arrive.
pub fn download_and_install(
    asset: &Asset,
    mut progress: impl FnMut(u64, u64),
) -> anyhow::Result<()> {
    use std::io::Read as _;
    anyhow::ensure!(
        asset.url.starts_with("https://github.com/")
            || asset
                .url
                .starts_with("https://objects.githubusercontent.com/"),
        "refusing to download from an unexpected host"
    );
    let mut resp = ureq::get(&asset.url)
        .header(
            "User-Agent",
            concat!("RustCraft-Updater/", env!("CARGO_PKG_VERSION")),
        )
        .call()?;
    let mut reader = resp.body_mut().with_config().limit(asset.size + 1).reader();
    let mut bytes = Vec::with_capacity(asset.size as usize);
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buf[..n]);
        progress(bytes.len() as u64, asset.size);
    }
    verify(asset, &bytes)?;
    let exe = std::env::current_exe()?;
    let tmp = exe.with_extension("update.tmp");
    std::fs::write(&tmp, &bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    // AppImages are replaced at their own path (APPIMAGE), not the mount.
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        std::fs::rename(&tmp, appimage)?;
    } else {
        self_replace::self_replace(&tmp)?;
        let _ = std::fs::remove_file(&tmp);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn sha_and_verify() {
        let a = super::Asset {
            platform: "x".into(),
            kind: "exe".into(),
            url: String::new(),
            size: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
        };
        assert!(super::verify(&a, b"abc").is_ok());
        assert!(super::verify(&a, b"abd").is_err());
    }

    use super::*;

    #[test]
    fn manifest_round_trip_and_compare() {
        let json = r#"{
            "version": "0.1.0",
            "published": "2026-11-02T00:00:00Z",
            "notes_url": "https://github.com/goglesquirmintontheiii/rcraft/releases/tag/v0.1.0",
            "assets": [{
                "platform": "windows-x86_64", "kind": "exe",
                "url": "https://example.invalid/rustcraft.exe", "size": 1, "sha256": "00"
            }]
        }"#;
        let m: Manifest = serde_json::from_str(json).unwrap();
        assert!(m.is_newer_than(&Version::new(0, 0, 1)));
        assert!(!m.is_newer_than(&Version::new(0, 1, 0)));
        let back = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Manifest>(&back).unwrap(), m);
    }
}
