//! Chunk columns and 16³ sections.

use rc_core::{BiomeId, BlockPos, BlockStateId, ChunkPos, SECTION_COUNT, WORLD_MIN_Y};

use crate::PalettedContainer;

/// One 16×16×16 section of a chunk column.
#[derive(Clone, Debug, Default)]
pub struct Section {
    pub blocks: PalettedContainer<4096>,
    /// Biomes at 4×4×4 resolution.
    pub biomes: PalettedContainer<64>,
    /// Packed 4-bit sky light, 2 per byte; `None` when not yet computed.
    pub sky_light: Option<Box<[u8; 2048]>>,
    pub block_light: Option<Box<[u8; 2048]>>,
}

impl Section {
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blocks.is_uniform() && self.blocks.get(0) == BlockStateId::AIR.0
    }

    #[inline]
    #[must_use]
    pub fn get(&self, x: usize, y: usize, z: usize) -> BlockStateId {
        BlockStateId(self.blocks.get((y << 8) | (z << 4) | x))
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, s: BlockStateId) {
        self.blocks.set((y << 8) | (z << 4) | x, s.0);
    }

    #[inline]
    #[must_use]
    pub fn biome(&self, x: usize, y: usize, z: usize) -> BiomeId {
        BiomeId(
            self.biomes
                .get(((y >> 2) << 4) | ((z >> 2) << 2) | (x >> 2)),
        )
    }
}

/// A full-height chunk column.
#[derive(Clone, Debug)]
pub struct Chunk {
    pub pos: ChunkPos,
    pub sections: Box<[Section; SECTION_COUNT]>,
    /// Highest non-air block per column (`WORLD_SURFACE`), `WORLD_MIN_Y - 1` if none.
    pub height_surface: [i16; 256],
    pub dirty: bool,
    /// Sky/block light arrays are up to date.
    pub light_valid: bool,
    /// Serialized entities owned by this chunk (opaque to the storage layer).
    pub entities: Vec<Vec<u8>>,
    /// Serialized block entities: (local index y<<8|z<<4|x relative to min y, data).
    pub block_entities: Vec<(u32, Vec<u8>)>,
}

impl Chunk {
    #[must_use]
    pub fn new_empty(pos: ChunkPos) -> Self {
        Self {
            pos,
            sections: Box::new(std::array::from_fn(|_| Section::default())),
            height_surface: [(WORLD_MIN_Y - 1) as i16; 256],
            dirty: false,
            light_valid: false,
            entities: Vec::new(),
            block_entities: Vec::new(),
        }
    }

    #[inline]
    fn section_of(y: i32) -> Option<usize> {
        let i = (y - WORLD_MIN_Y) >> 4;
        (0..SECTION_COUNT as i32).contains(&i).then_some(i as usize)
    }

    /// Block at world position `p`; air outside the vertical bounds.
    #[must_use]
    pub fn get_block(&self, p: BlockPos) -> BlockStateId {
        match Self::section_of(p.y) {
            Some(s) => self.sections[s].get(
                (p.x & 15) as usize,
                (p.y & 15) as usize,
                (p.z & 15) as usize,
            ),
            None => BlockStateId::AIR,
        }
    }

    /// Set a block; returns `false` if `p.y` is out of bounds.
    pub fn set_block(&mut self, p: BlockPos, s: BlockStateId) -> bool {
        let Some(sec) = Self::section_of(p.y) else {
            return false;
        };
        let (x, y, z) = (
            (p.x & 15) as usize,
            (p.y & 15) as usize,
            (p.z & 15) as usize,
        );
        self.sections[sec].set(x, y, z, s);
        self.dirty = true;
        let col = (z << 4) | x;
        let h = i32::from(self.height_surface[col]);
        if s != BlockStateId::AIR {
            if p.y > h {
                self.height_surface[col] = p.y as i16;
            }
        } else if p.y == h {
            // Scan down for the next solid block.
            let next = (WORLD_MIN_Y..p.y)
                .rev()
                .find(|&y| self.get_block(BlockPos::new(p.x, y, p.z)) != BlockStateId::AIR)
                .unwrap_or(WORLD_MIN_Y - 1);
            self.height_surface[col] = next as i16;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_round_trip_and_heightmap() {
        let mut c = Chunk::new_empty(ChunkPos::new(-3, 7));
        let stone = BlockStateId(1);
        assert!(c.set_block(BlockPos::new(-40, -64, 115), stone));
        assert!(c.set_block(BlockPos::new(-40, 100, 115), stone));
        assert_eq!(c.get_block(BlockPos::new(-40, 100, 115)), stone);
        assert_eq!(c.get_block(BlockPos::new(-40, 99, 115)), BlockStateId::AIR);
        let col = ((115 & 15) << 4) | (-40i32 & 15) as usize;
        assert_eq!(c.height_surface[col], 100);
        c.set_block(BlockPos::new(-40, 100, 115), BlockStateId::AIR);
        assert_eq!(c.height_surface[col], -64);
        assert!(!c.set_block(BlockPos::new(0, 320, 0), stone));
        assert!(!c.set_block(BlockPos::new(0, -65, 0), stone));
    }

    #[test]
    fn empty_sections_are_detected() {
        let c = Chunk::new_empty(ChunkPos::new(0, 0));
        assert!(c.sections.iter().all(Section::is_empty));
    }
}
