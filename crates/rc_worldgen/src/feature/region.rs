//! `WorldGenRegion`: the 3×3 chunk window a chunk's decoration reads and
//! writes.

use rc_core::{BiomeId, BlockPos, BlockStateId, ChunkPos, Direction};
use rc_data::fluid::FluidState;

use crate::generator::DimensionGen;
use crate::proto::{Heightmap, ProtoChunk};

thread_local! {
    /// Debug: position to trace writes at, and the feature currently placing.
    pub static TRACE: std::cell::RefCell<(Option<BlockPos>, String)> = const { std::cell::RefCell::new((None, String::new())) };
}

fn trace(p: BlockPos, s: BlockStateId) {
    TRACE.with(|t| {
        let t = t.borrow();
        if t.0 == Some(p) {
            eprintln!(
                "TRACE {p:?} <- {} by {}",
                rc_data::blocks().to_string(s),
                t.1
            );
        }
    });
}

pub struct Region<'a> {
    pub dg: &'a DimensionGen,
    pub center: ChunkPos,
    /// `(dx + 1) * 3 + (dz + 1)`
    pub chunks: [Option<ProtoChunk>; 9],
    pub min_y: i32,
    pub max_y: i32,
    air: BlockStateId,
}

impl std::fmt::Debug for Region<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Region({:?})", self.center)
    }
}

impl<'a> Region<'a> {
    #[must_use]
    pub fn new(dg: &'a DimensionGen, center: ChunkPos, chunks: [Option<ProtoChunk>; 9]) -> Self {
        Self {
            dg,
            center,
            chunks,
            min_y: dg.settings.min_y,
            max_y: dg.settings.min_y + dg.settings.height,
            air: BlockStateId::AIR,
        }
    }

    #[inline]
    fn slot(&self, x: i32, z: i32) -> Option<usize> {
        let dx = (x >> 4) - self.center.x;
        let dz = (z >> 4) - self.center.z;
        if (-1..=1).contains(&dx) && (-1..=1).contains(&dz) {
            Some(((dx + 1) * 3 + (dz + 1)) as usize)
        } else {
            None
        }
    }

    #[inline]
    #[must_use]
    pub fn chunk_at(&self, x: i32, z: i32) -> Option<&ProtoChunk> {
        self.slot(x, z).and_then(|i| self.chunks[i].as_ref())
    }

    #[inline]
    pub fn chunk_at_mut(&mut self, x: i32, z: i32) -> Option<&mut ProtoChunk> {
        let i = self.slot(x, z)?;
        self.chunks[i].as_mut()
    }

    #[inline]
    #[must_use]
    pub fn get(&self, p: BlockPos) -> BlockStateId {
        self.chunk_at(p.x, p.z)
            .map_or(self.air, |c| c.get(p.x, p.y, p.z))
    }

    #[inline]
    #[must_use]
    pub fn get_xyz(&self, x: i32, y: i32, z: i32) -> BlockStateId {
        self.get(BlockPos::new(x, y, z))
    }

    #[inline]
    #[must_use]
    pub fn fluid(&self, p: BlockPos) -> FluidState {
        rc_data::blocks().fluid(self.get(p))
    }

    #[inline]
    #[must_use]
    pub fn is_air(&self, p: BlockPos) -> bool {
        rc_data::blocks().is_air(self.get(p))
    }

    #[inline]
    #[must_use]
    pub fn is_outside_build_height(&self, y: i32) -> bool {
        y < self.min_y || y >= self.max_y
    }

    /// `ensureCanWrite`: inside the 3×3 window.
    #[inline]
    #[must_use]
    pub fn can_write(&self, p: BlockPos) -> bool {
        self.slot(p.x, p.z)
            .is_some_and(|i| self.chunks[i].is_some())
    }

    /// `WorldGenRegion.setBlock`.
    pub fn set(&mut self, p: BlockPos, s: BlockStateId) -> bool {
        trace(p, s);
        match self.chunk_at_mut(p.x, p.z) {
            Some(c) => {
                c.set(p.x, p.y, p.z, s);
                true
            }
            None => false,
        }
    }

    /// Direct section write without heightmap maintenance (ore features use
    /// `BulkSectionAccess`).
    pub fn set_raw(&mut self, p: BlockPos, s: BlockStateId) {
        trace(p, s);
        if let Some(c) = self.chunk_at_mut(p.x, p.z) {
            c.set_raw(p.x, p.y, p.z, s);
        }
    }

    pub fn mark_post_process(&mut self, p: BlockPos) {
        if let Some(c) = self.chunk_at_mut(p.x, p.z) {
            c.post_process.push((p.x, p.y, p.z));
        }
    }

    /// `getHeight(type, x, z)` — first free y above the heightmap's top block.
    pub fn height(&mut self, h: Heightmap, x: i32, z: i32) -> i32 {
        let min = self.min_y;
        self.chunk_at_mut(x, z)
            .map_or(min, |c| c.first_available(h, x, z))
    }

    /// `getHeightmapPos`.
    pub fn heightmap_pos(&mut self, h: Heightmap, p: BlockPos) -> BlockPos {
        BlockPos::new(p.x, self.height(h, p.x, p.z), p.z)
    }

    /// Stored quart biome, y clamped (`WorldGenRegion.getNoiseBiome`).
    pub fn noise_biome(&self, qx: i32, qy: i32, qz: i32) -> BiomeId {
        match self.chunk_at(qx << 2, qz << 2) {
            Some(c) => c.biome(qx, qy, qz),
            None => self
                .dg
                .noise_biome(qx, qy.clamp(self.min_y >> 2, (self.max_y >> 2) - 1), qz),
        }
    }

    /// `getBiome(pos)` (fuzzy zoom over stored biomes).
    pub fn biome(&self, p: BlockPos) -> BiomeId {
        self.dg
            .zoomed_biome(p.x, p.y, p.z, &mut |a, b, c| self.noise_biome(a, b, c))
    }

    #[must_use]
    pub fn neighbor(p: BlockPos, d: Direction) -> BlockPos {
        p.offset(d)
    }

    /// Take the chunks back out.
    #[must_use]
    pub fn into_chunks(self) -> [Option<ProtoChunk>; 9] {
        self.chunks
    }
}
