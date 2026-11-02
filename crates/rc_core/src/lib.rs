//! Core types shared by every RustCraft crate.
//!
//! Nothing in here depends on another workspace crate. Keep it small and
//! allocation-free on hot paths.

#![forbid(unsafe_code)]

pub mod coords;
pub mod ids;
pub mod registry;
pub mod version;

pub use coords::{BlockPos, ChunkPos, Direction, SectionPos};
pub use ids::{BiomeId, BlockStateId, ItemId, ResourceKey};
pub use registry::Registry;

/// World vertical bounds for the 1.21.1 overworld.
pub const WORLD_MIN_Y: i32 = -64;
pub const WORLD_MAX_Y: i32 = 320; // exclusive
pub const SECTION_COUNT: usize = ((WORLD_MAX_Y - WORLD_MIN_Y) / 16) as usize;

/// Simulation tick rate, like vanilla.
pub const TICKS_PER_SECOND: u32 = 20;
