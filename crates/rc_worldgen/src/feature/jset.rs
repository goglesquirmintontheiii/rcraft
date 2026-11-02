//! Emulation of `java.util.HashSet<BlockPos>` iteration order.
//!
//! Tree decorators iterate position sets whose order comes from Java's
//! `HashMap`: bucket index `spread(hashCode) & (capacity - 1)`, insertion order
//! within a bucket (preserved across resizes and treeification). Seed parity
//! for vines / cocoa / beehives depends on reproducing it.

use ahash::AHashMap;
use rc_core::BlockPos;

/// Insertion-ordered set of positions that can be iterated in Java order.
#[derive(Clone, Debug, Default)]
pub struct JavaPosSet {
    items: Vec<BlockPos>,
    index: AHashMap<BlockPos, usize>,
}

#[inline]
fn java_hash(p: BlockPos) -> i32 {
    // Vec3i.hashCode
    (p.y.wrapping_add(p.z.wrapping_mul(31)))
        .wrapping_mul(31)
        .wrapping_add(p.x)
}

#[inline]
fn spread(h: i32) -> u32 {
    (h ^ ((h as u32) >> 16) as i32) as u32
}

impl JavaPosSet {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true if newly inserted.
    pub fn insert(&mut self, p: BlockPos) -> bool {
        if self.index.contains_key(&p) {
            return false;
        }
        self.index.insert(p, self.items.len());
        self.items.push(p);
        true
    }

    #[must_use]
    pub fn contains(&self, p: &BlockPos) -> bool {
        self.index.contains_key(p)
    }

    #[must_use]
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Insertion order.
    pub fn iter_inserted(&self) -> impl Iterator<Item = &BlockPos> {
        self.items.iter()
    }

    /// Final table capacity after all insertions (default 16, load 0.75).
    fn capacity(&self) -> u32 {
        let mut cap = 16u32;
        while self.items.len() as u64 > u64::from(cap) * 3 / 4 {
            cap *= 2;
        }
        cap
    }

    /// Positions in Java `HashSet` iteration order.
    #[must_use]
    pub fn java_order(&self) -> Vec<BlockPos> {
        let mask = self.capacity() - 1;
        let mut v: Vec<(u32, usize, BlockPos)> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, &p)| (spread(java_hash(p)) & mask, i, p))
            .collect();
        v.sort_unstable_by_key(|e| (e.0, e.1));
        v.into_iter().map(|e| e.2).collect()
    }

    /// `new ObjectArrayList<>(set)` then stable-sorted by y, as
    /// `TreeDecorator.Context` does.
    #[must_use]
    pub fn sorted_by_y(&self) -> Vec<BlockPos> {
        let mut v = self.java_order();
        v.sort_by_key(|p| p.y);
        v
    }
}
