//! Nether features.

use rc_core::BlockPos;
use serde_json::Value;

use super::{Ctx, Registry};
use crate::rng::Random;

#[derive(Clone, Debug)]
pub enum NetherKind {
    Todo(&'static str),
}

impl NetherKind {
    pub fn from_json(_reg: &mut Registry, t: &str, _c: &Value) -> Option<Self> {
        match t {
            "huge_fungus" => Some(Self::Todo("huge_fungus")),
            "nether_forest_vegetation" => Some(Self::Todo("nether_forest_vegetation")),
            "twisting_vines" => Some(Self::Todo("twisting_vines")),
            "weeping_vines" => Some(Self::Todo("weeping_vines")),
            "basalt_columns" => Some(Self::Todo("basalt_columns")),
            "basalt_pillar" => Some(Self::Todo("basalt_pillar")),
            "delta_feature" => Some(Self::Todo("delta_feature")),
            "netherrack_replace_blobs" => Some(Self::Todo("netherrack_replace_blobs")),
            "glowstone_blob" => Some(Self::Todo("glowstone_blob")),
            _ => None,
        }
    }

    pub fn place(&self, _ctx: &mut Ctx<'_, '_>, _r: &mut Random, _pos: BlockPos) -> bool {
        false
    }
}
