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

#[cfg(test)]
mod tests {
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
