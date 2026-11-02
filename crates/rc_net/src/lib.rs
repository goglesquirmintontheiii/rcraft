//! Java Edition protocol client (Phase 4).
//!
//! Offline-mode servers must work with no Microsoft account (ADR-013);
//! Microsoft/Mojang authentication is an optional layer on top.
//!
//! See `docs/ARCHITECTURE.md` and `docs/ROADMAP.md` for where this crate
//! fits and which phase fills it in.

#![forbid(unsafe_code)]

/// Crate marker used by smoke tests until real functionality lands.
pub const CRATE_NAME: &str = "rc_net";

#[cfg(test)]
mod tests {
    #[test]
    fn links() {
        assert_eq!(super::CRATE_NAME, "rc_net");
    }
}
