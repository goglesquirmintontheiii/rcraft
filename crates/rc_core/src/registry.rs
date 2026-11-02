//! Generic string-key → dense-id registry.

use ahash::AHashMap;

use crate::ResourceKey;

/// Append-only registry mapping [`ResourceKey`]s to dense `u32` ids and values.
///
/// Ids are never reused; the order of insertion defines the id, which is what
/// lets a world save carry its own key table and stay stable across versions.
#[derive(Debug, Clone)]
pub struct Registry<T> {
    entries: Vec<(ResourceKey, T)>,
    by_key: AHashMap<ResourceKey, u32>,
}

impl<T> Default for Registry<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            by_key: AHashMap::new(),
        }
    }
}

impl<T> Registry<T> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `value` under `key`, returning its id. Re-registering an
    /// existing key replaces the value and returns the existing id.
    pub fn register(&mut self, key: ResourceKey, value: T) -> u32 {
        if let Some(&id) = self.by_key.get(&key) {
            self.entries[id as usize].1 = value;
            return id;
        }
        let id = u32::try_from(self.entries.len()).expect("registry overflow");
        self.by_key.insert(key.clone(), id);
        self.entries.push((key, value));
        id
    }

    #[must_use]
    pub fn id(&self, key: &ResourceKey) -> Option<u32> {
        self.by_key.get(key).copied()
    }

    #[must_use]
    pub fn get(&self, id: u32) -> Option<&T> {
        self.entries.get(id as usize).map(|(_, v)| v)
    }

    #[must_use]
    pub fn key(&self, id: u32) -> Option<&ResourceKey> {
        self.entries.get(id as usize).map(|(k, _)| k)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (u32, &ResourceKey, &T)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(i, (k, v))| (i as u32, k, v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_dense_and_stable() {
        let mut r = Registry::new();
        let air = r.register(ResourceKey::minecraft("air"), 0u8);
        let stone = r.register(ResourceKey::minecraft("stone"), 1u8);
        assert_eq!((air, stone), (0, 1));
        assert_eq!(r.register(ResourceKey::minecraft("air"), 9), 0);
        assert_eq!(r.get(0), Some(&9));
        assert_eq!(r.key(1).map(ResourceKey::path), Some("stone"));
        assert_eq!(r.id(&ResourceKey::minecraft("dirt")), None);
    }
}
