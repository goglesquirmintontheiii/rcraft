//! Build/version information, used by the title screen, F3 overlay, crash
//! reports and the self-updater.

/// Crate version of the workspace (`CARGO_PKG_VERSION` of the game binary).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The Minecraft: Java Edition version this build targets for parity.
pub const TARGET_MINECRAFT_VERSION: &str = "1.21.1";

/// Java Edition protocol number for the target version (used from Phase 4).
pub const TARGET_PROTOCOL_VERSION: i32 = 767;

/// Vanilla data version for 1.21.1 (used in Anvil import/export).
pub const TARGET_DATA_VERSION: i32 = 3955;

/// Resource-pack format for 1.21.1.
pub const TARGET_PACK_FORMAT: u32 = 34;

/// Parsed semver of this build.
#[must_use]
pub fn semver() -> semver::Version {
    semver::Version::parse(VERSION).expect("workspace version is valid semver")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_parses() {
        assert!(super::semver().major < 100);
    }
}
