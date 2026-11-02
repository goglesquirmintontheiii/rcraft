//! End features.

use rc_core::BlockPos;
use serde_json::Value;

use super::{Ctx, Registry};
use crate::rng::Random;

#[derive(Clone, Debug)]
pub enum EndKind {
    Todo(&'static str),
}

impl EndKind {
    pub fn from_json(_reg: &mut Registry, t: &str, _c: &Value) -> Option<Self> {
        match t {
            "end_island" => Some(Self::Todo("end_island")),
            "chorus_plant" => Some(Self::Todo("chorus_plant")),
            "end_spike" => Some(Self::Todo("end_spike")),
            "end_gateway" => Some(Self::Todo("end_gateway")),
            _ => None,
        }
    }

    pub fn place(&self, _ctx: &mut Ctx<'_, '_>, _r: &mut Random, _pos: BlockPos) -> bool {
        false
    }
}
