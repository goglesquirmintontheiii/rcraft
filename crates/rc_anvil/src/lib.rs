//! Java Edition (Anvil) world import and export.
//!
//! The *reader* (region files, NBT, 1.21.1 chunk format) lands in Phase 1 as
//! the reference source for seed-exact worldgen parity tests (ADR-012); the
//! writer and the user-facing import/export UI land in Phase 2.
//!
//! See `docs/ARCHITECTURE.md` and `docs/ROADMAP.md` for where this crate
//! fits and which phase fills it in.

#![forbid(unsafe_code)]

/// Crate marker used by smoke tests until real functionality lands.
pub const CRATE_NAME: &str = "rc_anvil";

#[cfg(test)]
mod tests {
    #[test]
    fn links() {
        assert_eq!(super::CRATE_NAME, "rc_anvil");
    }
}
