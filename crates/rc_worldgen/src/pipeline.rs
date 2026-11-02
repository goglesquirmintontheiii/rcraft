//! Multi-chunk generation: terrain in parallel, decoration over 3×3 windows.

use ahash::AHashMap;
use rayon::prelude::*;
use rc_core::ChunkPos;

use crate::feature::region::Region;
use crate::generator::DimensionGen;
use crate::proto::{ProtoChunk, Status};

/// A set of chunks under generation.
pub struct ChunkStore<'a> {
    pub dg: &'a DimensionGen,
    pub chunks: AHashMap<ChunkPos, ProtoChunk>,
}

impl std::fmt::Debug for ChunkStore<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ChunkStore({} chunks)", self.chunks.len())
    }
}

impl<'a> ChunkStore<'a> {
    #[must_use]
    pub fn new(dg: &'a DimensionGen) -> Self {
        Self {
            dg,
            chunks: AHashMap::new(),
        }
    }

    /// Generate any missing chunks up to the carvers stage (in parallel).
    pub fn ensure_carved(&mut self, positions: &[ChunkPos]) {
        let missing: Vec<ChunkPos> = positions
            .iter()
            .copied()
            .filter(|p| !self.chunks.contains_key(p))
            .collect();
        let dg = self.dg;
        let made: Vec<ProtoChunk> = missing
            .par_iter()
            .map(|&p| dg.generate_to_carvers(p))
            .collect();
        for c in made {
            self.chunks.insert(c.pos, c);
        }
    }

    /// Decorate one chunk; its 3×3 neighbourhood must be carved.
    pub fn decorate(&mut self, pos: ChunkPos) {
        let mut window: [Option<ProtoChunk>; 9] = Default::default();
        for dx in -1..=1 {
            for dz in -1..=1 {
                let p = ChunkPos::new(pos.x + dx, pos.z + dz);
                window[((dx + 1) * 3 + (dz + 1)) as usize] = self.chunks.remove(&p);
            }
        }
        let mut region = Region::new(self.dg, pos, window);
        self.dg.decorate(&mut region);
        for (i, c) in region.into_chunks().into_iter().enumerate() {
            if let Some(mut c) = c {
                if i == 4 {
                    c.status = Status::Features;
                }
                self.chunks.insert(c.pos, c);
            }
        }
    }

    /// Carve a block and decorate the given chunks in order.
    pub fn generate(&mut self, decorate_order: &[ChunkPos]) {
        let mut need = Vec::new();
        for p in decorate_order {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    need.push(ChunkPos::new(p.x + dx, p.z + dz));
                }
            }
        }
        need.sort_by_key(|p| (p.x, p.z));
        need.dedup();
        self.ensure_carved(&need);
        for &p in decorate_order {
            if self
                .chunks
                .get(&p)
                .is_some_and(|c| c.status < Status::Features)
            {
                self.decorate(p);
            }
        }
    }
}
