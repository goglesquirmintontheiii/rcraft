//! Block, chunk and section coordinates.

use serde::{Deserialize, Serialize};

/// Integer block position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl BlockPos {
    pub const ZERO: Self = Self::new(0, 0, 0);

    #[inline]
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    #[inline]
    #[must_use]
    pub const fn offset(self, d: Direction) -> Self {
        let (dx, dy, dz) = d.delta();
        Self::new(self.x + dx, self.y + dy, self.z + dz)
    }

    #[inline]
    #[must_use]
    pub const fn chunk(self) -> ChunkPos {
        ChunkPos::new(self.x >> 4, self.z >> 4)
    }

    /// Index inside a 16×16×16 section, in vanilla `y<<8 | z<<4 | x` order.
    #[inline]
    #[must_use]
    pub const fn section_index(self) -> usize {
        (((self.y & 15) << 8) | ((self.z & 15) << 4) | (self.x & 15)) as usize
    }

    /// Vanilla's 64-bit packed form (26/12/26 bits), useful for hashing and
    /// for Anvil interop.
    #[inline]
    #[must_use]
    pub const fn as_long(self) -> i64 {
        ((self.x as i64 & 0x3FF_FFFF) << 38)
            | ((self.z as i64 & 0x3FF_FFFF) << 12)
            | (self.y as i64 & 0xFFF)
    }
}

/// Chunk-column position (16×16 blocks, full height).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

impl ChunkPos {
    #[inline]
    #[must_use]
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// Region (32×32 chunks) containing this chunk.
    #[inline]
    #[must_use]
    pub const fn region(self) -> (i32, i32) {
        (self.x >> 5, self.z >> 5)
    }

    /// Index of this chunk within its region's 1024-entry table.
    #[inline]
    #[must_use]
    pub const fn region_index(self) -> usize {
        (((self.z & 31) << 5) | (self.x & 31)) as usize
    }

    #[inline]
    #[must_use]
    pub const fn min_block(self) -> BlockPos {
        BlockPos::new(self.x << 4, 0, self.z << 4)
    }
}

/// 16×16×16 section position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SectionPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl SectionPos {
    #[inline]
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    #[inline]
    #[must_use]
    pub const fn of_block(p: BlockPos) -> Self {
        Self::new(p.x >> 4, p.y >> 4, p.z >> 4)
    }
}

/// The six block faces, in vanilla's enum order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Direction {
    Down = 0,
    Up = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

impl Direction {
    pub const ALL: [Self; 6] = [
        Self::Down,
        Self::Up,
        Self::North,
        Self::South,
        Self::West,
        Self::East,
    ];

    #[inline]
    #[must_use]
    pub const fn delta(self) -> (i32, i32, i32) {
        match self {
            Self::Down => (0, -1, 0),
            Self::Up => (0, 1, 0),
            Self::North => (0, 0, -1),
            Self::South => (0, 0, 1),
            Self::West => (-1, 0, 0),
            Self::East => (1, 0, 0),
        }
    }

    #[inline]
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_coords_floor_correctly() {
        let p = BlockPos::new(-1, -64, -17);
        assert_eq!(p.chunk(), ChunkPos::new(-1, -2));
        assert_eq!(SectionPos::of_block(p), SectionPos::new(-1, -4, -2));
        assert_eq!(p.section_index(), (15 << 4) | 15); // y=0 (−64 & 15), z=15, x=15
    }

    #[test]
    fn region_mapping() {
        assert_eq!(ChunkPos::new(-1, 31).region(), (-1, 0));
        assert_eq!(ChunkPos::new(-1, 31).region_index(), (31 << 5) | 31);
    }

    #[test]
    fn packed_long_round_trips_sign() {
        // Matches the vanilla packing documented on the wiki.
        let p = BlockPos::new(-5, -60, 7);
        let l = p.as_long();
        let x = (l >> 38) as i32;
        let y = ((l << 52) >> 52) as i32;
        let z = ((l << 26) >> 38) as i32;
        assert_eq!((x, y, z), (-5, -60, 7));
    }

    #[test]
    fn opposite_is_involution() {
        for d in Direction::ALL {
            assert_eq!(d.opposite().opposite(), d);
            assert_eq!(
                BlockPos::ZERO.offset(d).offset(d.opposite()),
                BlockPos::ZERO
            );
        }
    }
}
