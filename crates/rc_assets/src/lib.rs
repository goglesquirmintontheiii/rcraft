//! Assets: the player's own vanilla assets (read from their Minecraft
//! install), resource packs (Phase 3) and the procedural CC0 fallback pack.

#![forbid(unsafe_code)]
// Renderer/UI entry points take many plain parameters by design.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod atlas;
pub mod fallback;
pub mod model;
pub mod source;

use std::path::{Path, PathBuf};

use tracing::info;

pub use atlas::{Atlas, Image, Sprite};
pub use model::{BakedModel, BakedQuad, BlockModels, Dir, Layer, StateModel, Tint};
pub use source::{AssetSource, AssetStack};

/// Where the active assets came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Vanilla(PathBuf),
    Fallback,
}

/// Loaded asset set.
#[derive(Debug)]
pub struct Assets {
    pub stack: AssetStack,
    pub origin: Origin,
}

impl Assets {
    /// Use `explicit` if given, else auto-detect the player's vanilla jar, else
    /// the fallback pack.
    #[must_use]
    pub fn discover(explicit: Option<&Path>) -> Self {
        let candidate = explicit
            .map(Path::to_owned)
            .or_else(|| std::env::var_os("RUSTCRAFT_ASSETS").map(PathBuf::from))
            .or_else(source::find_vanilla_jar);
        if let Some(p) = candidate {
            match AssetSource::open(&p) {
                Ok(src)
                    if src
                        .read("assets/minecraft/blockstates/stone.json")
                        .is_some() =>
                {
                    info!(path = %p.display(), "using vanilla assets from the player's install");
                    let mut sources = vec![src];
                    // Sounds and sounds.json live in the launcher's object store.
                    if let Some(o) = AssetSource::objects_for_jar(&p) {
                        info!(source = ?o, "using the launcher asset index for sounds");
                        sources.push(o);
                    }
                    return Self {
                        stack: AssetStack { sources },
                        origin: Origin::Vanilla(p),
                    };
                }
                Ok(_) => tracing::warn!(path = %p.display(), "no vanilla assets in this location"),
                Err(e) => tracing::warn!(path = %p.display(), "cannot open assets: {e:#}"),
            }
        }
        info!("no Minecraft install found; using the built-in fallback pack");
        Self {
            stack: AssetStack::default(),
            origin: Origin::Fallback,
        }
    }

    #[must_use]
    pub fn is_vanilla(&self) -> bool {
        matches!(self.origin, Origin::Vanilla(_))
    }

    /// Bake block models from whichever source is active.
    #[must_use]
    pub fn block_models(&self, mip_levels: u32) -> BlockModels {
        let blocks = rc_data::blocks();
        if self.is_vanilla() {
            BlockModels::load(&self.stack, blocks, mip_levels)
        } else {
            fallback::block_models(blocks, mip_levels)
        }
    }

    /// Read and decode a PNG by resource id and kind directory.
    #[must_use]
    pub fn png(&self, id: &str) -> Option<Image> {
        self.stack
            .read_id("textures", id, ".png")
            .and_then(|b| Image::decode_png(&b))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fallback_models_cover_common_blocks() {
        let blocks = rc_data::blocks();
        let m = super::fallback::block_models(blocks, 4);
        let stone = blocks.default_state("stone");
        assert_eq!(m.layers[stone.0 as usize], super::Layer::Solid);
        assert_eq!(
            m.models[m.states[stone.0 as usize].parts[0][0].0 as usize]
                .quads
                .len(),
            6
        );
        assert_eq!(m.atlas.mips.len(), 4);
    }

    #[test]
    fn vanilla_models_if_available() {
        let Some(dir) = std::env::var_os("RUSTCRAFT_TEST_ASSETS") else {
            return;
        };
        let a = super::Assets::discover(Some(std::path::Path::new(&dir)));
        assert!(a.is_vanilla());
        let t = std::time::Instant::now();
        let m = a.block_models(5);
        eprintln!(
            "baked {} models in {:?}, atlas {}x{}",
            m.models.len(),
            t.elapsed(),
            m.atlas.mips[0].w,
            m.atlas.mips[0].h
        );
        let blocks = rc_data::blocks();
        let grass = blocks.default_state("grass_block");
        assert_eq!(m.states[grass.0 as usize].parts[0].len(), 4);
        let leaves = blocks.default_state("oak_leaves");
        assert_eq!(m.layers[leaves.0 as usize], super::Layer::Cutout);
        let glass = blocks.default_state("white_stained_glass");
        assert_eq!(m.layers[glass.0 as usize], super::Layer::Translucent);
    }
}
