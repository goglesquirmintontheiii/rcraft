//! World storage: chunk columns, sections, light, ticking, and the RCW format.
//!
//! Phase 0 ships the storage primitives ([`PalettedContainer`], [`Section`],
//! [`Chunk`]). The light engine, ticking and the on-disk format follow in Phase 1
//! (see `docs/WORLD_FORMAT.md`).

#![forbid(unsafe_code)]

pub mod chunk;
pub mod palette;
pub mod rcw;

pub use chunk::{Chunk, Section};
pub use palette::PalettedContainer;
