//! Java Edition (Anvil) world import and export.
//!
//! The *reader* (region files, NBT, 1.21.1 chunk format) lands in Phase 1 as
//! the reference source for seed-exact worldgen parity tests (ADR-012); the
//! writer and the user-facing import/export UI land in Phase 2.
//!
//! Format sources: minecraft.wiki "NBT format", "Region file format",
//! "Chunk format" (see docs/CLEAN_ROOM.md).

#![forbid(unsafe_code)]

pub mod chunk;
pub mod nbt;
pub mod region;

pub use chunk::{AnvilChunk, AnvilSection};
pub use nbt::Tag;
pub use region::RegionFile;
