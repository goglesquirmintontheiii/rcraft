//! Block, fluid and biome tags, resolved to bitsets / name sets.

use ahash::{AHashMap, AHashSet};
use rc_core::BlockStateId;

use crate::{Blocks, VanillaFiles};

/// A resolved block tag: one bit per block index.
#[derive(Clone, Debug, Default)]
pub struct BlockSet {
    bits: Vec<u64>,
}

impl BlockSet {
    fn with_len(n: usize) -> Self {
        Self {
            bits: vec![0; n.div_ceil(64)],
        }
    }

    pub fn insert(&mut self, block: u16) {
        self.bits[block as usize / 64] |= 1 << (block % 64);
    }

    #[inline]
    #[must_use]
    pub fn contains(&self, block: u16) -> bool {
        self.bits
            .get(block as usize / 64)
            .is_some_and(|w| w & (1 << (block % 64)) != 0)
    }

    #[inline]
    #[must_use]
    pub fn contains_state(&self, blocks: &Blocks, s: BlockStateId) -> bool {
        self.contains(blocks.block_index(s))
    }
}

pub struct Tags {
    block: AHashMap<String, BlockSet>,
    block_order: AHashMap<String, Vec<u16>>,
    biome: AHashMap<String, AHashSet<String>>,
    nblocks: usize,
}

impl std::fmt::Debug for Tags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Tags({} block, {} biome)",
            self.block.len(),
            self.biome.len()
        )
    }
}

fn norm(s: &str) -> String {
    let s = s.trim_start_matches('#');
    if s.contains(':') {
        s.to_owned()
    } else {
        format!("minecraft:{s}")
    }
}

fn resolve_names(
    files: &VanillaFiles,
    dir: &str,
    tag: &str,
    seen: &mut AHashSet<String>,
    out: &mut Vec<String>,
) {
    if !seen.insert(tag.to_owned()) {
        return;
    }
    let Some(j) = files.registry_json(dir, tag) else {
        return;
    };
    for v in j["values"].as_array().into_iter().flatten() {
        let (id, _required) = match v {
            serde_json::Value::String(s) => (s.clone(), true),
            o => (
                o["id"].as_str().unwrap_or_default().to_owned(),
                o["required"].as_bool().unwrap_or(true),
            ),
        };
        if let Some(t) = id.strip_prefix('#') {
            resolve_names(files, dir, &norm(t), seen, out);
        } else {
            out.push(norm(&id));
        }
    }
}

impl Tags {
    pub(crate) fn load(files: &VanillaFiles, blocks: &Blocks) -> Self {
        let mut block = AHashMap::new();
        let mut block_order = AHashMap::new();
        for name in files.list("tags/block") {
            let key = format!("minecraft:{name}");
            let mut names = Vec::new();
            resolve_names(files, "tags/block", &key, &mut AHashSet::new(), &mut names);
            let mut set = BlockSet::with_len(blocks.defs.len());
            let mut order = Vec::new();
            for n in names {
                if let Some(i) = blocks.index_of(&n) {
                    if !set.contains(i) {
                        order.push(i);
                    }
                    set.insert(i);
                }
            }
            block.insert(key.clone(), set);
            block_order.insert(key, order);
        }
        let mut biome = AHashMap::new();
        for name in files.list("tags/worldgen/biome") {
            let key = format!("minecraft:{name}");
            let mut names = Vec::new();
            resolve_names(
                files,
                "tags/worldgen/biome",
                &key,
                &mut AHashSet::new(),
                &mut names,
            );
            biome.insert(key, names.into_iter().collect());
        }
        Self {
            block,
            block_order,
            biome,
            nblocks: blocks.defs.len(),
        }
    }

    /// A block tag (`#minecraft:dirt`, `minecraft:dirt` or `dirt`). Unknown
    /// tags resolve to the empty set, like vanilla.
    #[must_use]
    pub fn block(&self, tag: &str) -> BlockSet {
        self.block
            .get(&norm(tag))
            .cloned()
            .unwrap_or_else(|| BlockSet::with_len(self.nblocks))
    }

    /// Tag contents in definition order (nested tags expanded in place,
    /// first occurrence kept), as `HolderSet` iteration gives them.
    #[must_use]
    pub fn block_list(&self, tag: &str) -> Vec<u16> {
        self.block_order
            .get(&norm(tag))
            .cloned()
            .unwrap_or_default()
    }

    #[must_use]
    pub fn block_ref(&self, tag: &str) -> Option<&BlockSet> {
        self.block.get(&norm(tag))
    }

    /// Biome ids (`minecraft:plains`) in a biome tag.
    #[must_use]
    pub fn biomes(&self, tag: &str) -> AHashSet<String> {
        self.biome.get(&norm(tag)).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn nested_tags_resolve() {
        let b = crate::blocks();
        let t = crate::tags();
        let dirt = t.block("#minecraft:dirt");
        assert!(dirt.contains_state(b, b.default_state("grass_block")));
        assert!(dirt.contains_state(b, b.default_state("rooted_dirt")));
        assert!(!dirt.contains_state(b, b.default_state("stone")));
        let logs = t.block("logs");
        assert!(logs.contains_state(b, b.default_state("oak_log")));
        assert!(t.biomes("is_ocean").contains("minecraft:deep_ocean"));
    }
}
