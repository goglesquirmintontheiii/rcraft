//! Entity framework, player, mobs
//!
//! See `docs/ARCHITECTURE.md` and `docs/ROADMAP.md` for where this crate
//! fits and which phase fills it in.

#![forbid(unsafe_code)]

/// Crate marker used by smoke tests until real functionality lands.
pub const CRATE_NAME: &str = "rc_entity";

#[cfg(test)]
mod tests {
    #[test]
    fn links() {
        assert_eq!(super::CRATE_NAME, "rc_entity");
    }
}
