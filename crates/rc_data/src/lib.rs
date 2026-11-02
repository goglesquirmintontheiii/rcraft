//! Vanilla game data and the block registry.
//!
//! The data is the output of Mojang's official data generator for 1.21.1 plus
//! block property tables (see `data/vanilla/SOURCE.md`). It is embedded as one
//! zstd blob and decoded lazily on first use.

#![forbid(unsafe_code)]

pub mod blocks;
pub mod fluid;
pub mod tags;

use std::sync::OnceLock;

use ahash::AHashMap;

pub use blocks::{BlockDef, Blocks, StateInfo};
pub use tags::Tags;

static BLOB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/vanilla.bin.zst"));

/// Raw access to the embedded vanilla data files, keyed by path relative to
/// `data/vanilla/` (e.g. `worldgen/noise/temperature.json`).
pub struct VanillaFiles {
    files: AHashMap<String, Vec<u8>>,
}

impl std::fmt::Debug for VanillaFiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VanillaFiles({} files)", self.files.len())
    }
}

impl VanillaFiles {
    fn load() -> Self {
        let raw = zstd::decode_all(BLOB).expect("embedded vanilla data is valid zstd");
        let mut files = AHashMap::new();
        let mut p = 4usize;
        let n = u32::from_le_bytes(raw[0..4].try_into().unwrap()) as usize;
        for _ in 0..n {
            let nl = u16::from_le_bytes(raw[p..p + 2].try_into().unwrap()) as usize;
            p += 2;
            let name = std::str::from_utf8(&raw[p..p + nl]).unwrap().to_owned();
            p += nl;
            let dl = u32::from_le_bytes(raw[p..p + 4].try_into().unwrap()) as usize;
            p += 4;
            files.insert(name, raw[p..p + dl].to_vec());
            p += dl;
        }
        Self { files }
    }

    #[must_use]
    pub fn get(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    /// Parse a file as JSON.
    #[must_use]
    pub fn json(&self, path: &str) -> Option<serde_json::Value> {
        self.get(path)
            .map(|b| serde_json::from_slice(b).expect("vanilla json is valid"))
    }

    /// Resolve a namespaced id inside a registry directory, e.g.
    /// `("worldgen/noise", "minecraft:temperature")`.
    #[must_use]
    pub fn registry_json(&self, dir: &str, id: &str) -> Option<serde_json::Value> {
        let path = id.strip_prefix("minecraft:").unwrap_or(id);
        self.json(&format!("{dir}/{path}.json"))
    }

    /// All file paths under `dir/` (non-recursive names without `.json`,
    /// including sub-directories as `a/b`).
    pub fn list(&self, dir: &str) -> Vec<String> {
        let prefix = format!("{dir}/");
        let mut v: Vec<String> = self
            .files
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix))
            .filter_map(|k| k.strip_suffix(".json"))
            .map(str::to_owned)
            .collect();
        v.sort();
        v
    }
}

/// The embedded vanilla data files.
pub fn files() -> &'static VanillaFiles {
    static F: OnceLock<VanillaFiles> = OnceLock::new();
    F.get_or_init(VanillaFiles::load)
}

/// The global block registry.
pub fn blocks() -> &'static Blocks {
    static B: OnceLock<Blocks> = OnceLock::new();
    B.get_or_init(|| Blocks::load(files()))
}

/// Block / fluid / biome tags.
pub fn tags() -> &'static Tags {
    static T: OnceLock<Tags> = OnceLock::new();
    T.get_or_init(|| Tags::load(files(), blocks()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_files_present() {
        let f = super::files();
        assert!(f.get("worldgen/noise_settings/overworld.json").is_some());
        assert!(f.list("worldgen/biome").len() >= 64);
    }
}
