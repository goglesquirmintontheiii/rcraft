//! Seed-exact vanilla 1.21.1 world generation.
//!
//! Pipeline per chunk: biomes → noise (terrain, aquifers, ore veins) →
//! surface → carvers → features. Every stage reproduces the game's
//! arithmetic and random-draw order; `tests/parity.rs` checks each stage
//! against chunks saved by the real game (ADR-012).

#![forbid(unsafe_code)]
// Expressions mirror the game's own arithmetic (operand order, comparison
// form, loop shape) on purpose: "simplifying" them can change results.
#![allow(
    clippy::too_many_arguments,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::int_plus_one,
    clippy::needless_range_loop,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::needless_lifetimes,
    clippy::type_complexity
)]

pub mod aquifer;
pub mod biome;
pub mod carver;
pub mod climate;
pub mod density;
pub mod feature;
pub mod generator;
pub mod mth;
pub mod noise;
pub mod pipeline;
pub mod proto;
pub mod providers;
pub mod rng;
pub mod surface;

pub use generator::{Dimension, DimensionGen};
pub use proto::{ProtoChunk, Status};
