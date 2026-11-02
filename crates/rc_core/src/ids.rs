//! Dense ids and namespaced resource keys.

use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! dense_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
                 bytemuck::Pod, bytemuck::Zeroable)]
        #[repr(transparent)]
        pub struct $name(pub u32);

        impl $name {
            #[inline]
            #[must_use]
            pub const fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

dense_id!(
    /// A block *state* (block + property values). `0` is always `minecraft:air`.
    BlockStateId
);
dense_id!(
    /// An item type.
    ItemId
);
dense_id!(
    /// A biome.
    BiomeId
);

impl BlockStateId {
    pub const AIR: Self = Self(0);
}

/// A namespaced key such as `minecraft:stone`.
///
/// Stored as a single string with the separator position cached so that both
/// halves can be borrowed without allocation.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ResourceKey {
    full: String,
    sep: usize,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ResourceKeyError {
    #[error("invalid character in resource key `{0}`")]
    InvalidChar(String),
    #[error("empty path in resource key `{0}`")]
    EmptyPath(String),
}

impl ResourceKey {
    pub const DEFAULT_NAMESPACE: &'static str = "minecraft";

    /// Parse `namespace:path` or bare `path` (namespace defaults to `minecraft`).
    pub fn parse(s: &str) -> Result<Self, ResourceKeyError> {
        let (ns, path) = match s.split_once(':') {
            Some((ns, path)) => (ns, path),
            None => (Self::DEFAULT_NAMESPACE, s),
        };
        if path.is_empty() {
            return Err(ResourceKeyError::EmptyPath(s.to_owned()));
        }
        let ns_ok = ns
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.'));
        let path_ok = path
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'/'));
        if !ns_ok || !path_ok {
            return Err(ResourceKeyError::InvalidChar(s.to_owned()));
        }
        let full = format!("{ns}:{path}");
        Ok(Self {
            sep: ns.len(),
            full,
        })
    }

    /// Build a key in the `minecraft` namespace.
    #[must_use]
    pub fn minecraft(path: &str) -> Self {
        Self::parse(path).expect("static vanilla key")
    }

    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.full[..self.sep]
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.full[self.sep + 1..]
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.full
    }
}

impl fmt::Debug for ResourceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.full)
    }
}

impl fmt::Display for ResourceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.full)
    }
}

impl TryFrom<String> for ResourceKey {
    type Error = ResourceKeyError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s)
    }
}

impl From<ResourceKey> for String {
    fn from(k: ResourceKey) -> Self {
        k.full
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_with_default_namespace() {
        let k = ResourceKey::parse("stone").unwrap();
        assert_eq!(k.namespace(), "minecraft");
        assert_eq!(k.path(), "stone");
        assert_eq!(k.to_string(), "minecraft:stone");
    }

    #[test]
    fn parses_custom_namespace_and_paths() {
        let k = ResourceKey::parse("rustcraft:block/oak_log_top").unwrap();
        assert_eq!(k.namespace(), "rustcraft");
        assert_eq!(k.path(), "block/oak_log_top");
    }

    #[test]
    fn rejects_bad_keys() {
        assert!(ResourceKey::parse("Minecraft:stone").is_err());
        assert!(ResourceKey::parse("minecraft:").is_err());
        assert!(ResourceKey::parse("a b").is_err());
    }
}
