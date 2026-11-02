//! Chunks under generation (`ProtoChunk`) with vanilla heightmap semantics.

use std::sync::OnceLock;

use rc_core::{BiomeId, BlockStateId, ChunkPos};

/// Heightmap kinds tracked during generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Heightmap {
    WorldSurfaceWg = 0,
    OceanFloorWg = 1,
    WorldSurface = 2,
    OceanFloor = 3,
    MotionBlocking = 4,
    MotionBlockingNoLeaves = 5,
}

impl Heightmap {
    #[must_use]
    pub fn from_name(s: &str) -> Option<Self> {
        Some(match s {
            "WORLD_SURFACE_WG" => Self::WorldSurfaceWg,
            "OCEAN_FLOOR_WG" => Self::OceanFloorWg,
            "WORLD_SURFACE" => Self::WorldSurface,
            "OCEAN_FLOOR" => Self::OceanFloor,
            "MOTION_BLOCKING" => Self::MotionBlocking,
            "MOTION_BLOCKING_NO_LEAVES" => Self::MotionBlockingNoLeaves,
            _ => return None,
        })
    }

    #[inline]
    fn bit(self) -> u8 {
        match self {
            Self::WorldSurfaceWg | Self::WorldSurface => HM_NOT_AIR,
            Self::OceanFloorWg | Self::OceanFloor => HM_BLOCKS_MOTION,
            Self::MotionBlocking => HM_MOTION_BLOCKING,
            Self::MotionBlockingNoLeaves => HM_MB_NO_LEAVES,
        }
    }
}

const HM_NOT_AIR: u8 = 1;
const HM_BLOCKS_MOTION: u8 = 2;
const HM_MOTION_BLOCKING: u8 = 4;
const HM_MB_NO_LEAVES: u8 = 8;

/// Per-state heightmap predicate bits.
pub fn hm_flags() -> &'static [u8] {
    static F: OnceLock<Vec<u8>> = OnceLock::new();
    F.get_or_init(|| {
        let b = rc_data::blocks();
        let leaves = rc_data::tags().block("leaves");
        let cobweb = b.index_of("cobweb").unwrap();
        let bamboo_sapling = b.index_of("bamboo_sapling").unwrap();
        (0..b.len())
            .map(|i| {
                let s = BlockStateId(i as u32);
                let info = b.info(s);
                let mut f = 0;
                if !b.is_air(s) {
                    f |= HM_NOT_AIR;
                }
                let motion = info.flags & rc_data::blocks::FLAG_LEGACY_SOLID != 0
                    && info.block != cobweb
                    && info.block != bamboo_sapling;
                if motion {
                    f |= HM_BLOCKS_MOTION;
                }
                if motion || !info.fluid.is_empty() {
                    f |= HM_MOTION_BLOCKING;
                    if !leaves.contains(info.block) {
                        f |= HM_MB_NO_LEAVES;
                    }
                }
                f
            })
            .collect()
    })
}

/// Generation status reached by a [`ProtoChunk`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Empty,
    Biomes,
    Noise,
    Surface,
    Carvers,
    Features,
}

/// A chunk under generation.
pub struct ProtoChunk {
    pub pos: ChunkPos,
    pub min_y: i32,
    pub height: i32,
    /// `((y - min_y) * 16 + z) * 16 + x`
    pub blocks: Vec<BlockStateId>,
    /// Quart biomes, `((qy - min_qy) * 4 + qz) * 4 + qx`.
    pub biomes: Vec<BiomeId>,
    heightmaps: [Option<Box<[i32; 256]>>; 6],
    pub carving_mask: Vec<u64>,
    pub status: Status,
    /// Positions (x, y, z world) marked for post-processing (fluid updates).
    pub post_process: Vec<(i32, i32, i32)>,
}

impl std::fmt::Debug for ProtoChunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ProtoChunk({:?}, {:?})", self.pos, self.status)
    }
}

impl ProtoChunk {
    #[must_use]
    pub fn new(pos: ChunkPos, min_y: i32, height: i32) -> Self {
        let n = (16 * 16 * height) as usize;
        Self {
            pos,
            min_y,
            height,
            blocks: vec![BlockStateId::AIR; n],
            biomes: vec![BiomeId(0); (16 * height / 4) as usize],
            heightmaps: Default::default(),
            carving_mask: vec![0; n.div_ceil(64)],
            status: Status::Empty,
            post_process: Vec::new(),
        }
    }

    #[inline]
    #[must_use]
    pub fn max_y(&self) -> i32 {
        self.min_y + self.height
    }

    #[inline]
    fn idx(&self, x: i32, y: i32, z: i32) -> usize {
        ((((y - self.min_y) << 4) | (z & 15)) << 4 | (x & 15)) as usize
    }

    /// Block at world coordinates (x/z may be any value inside this chunk).
    #[inline]
    #[must_use]
    pub fn get(&self, x: i32, y: i32, z: i32) -> BlockStateId {
        if y < self.min_y || y >= self.max_y() {
            return void_air();
        }
        self.blocks[self.idx(x, y, z)]
    }

    /// Set a block (no heightmap update).
    #[inline]
    pub fn set_raw(&mut self, x: i32, y: i32, z: i32, s: BlockStateId) {
        if y >= self.min_y && y < self.max_y() {
            let i = self.idx(x, y, z);
            self.blocks[i] = s;
        }
    }

    /// Which heightmaps `setBlockState` keeps updated at the current status.
    fn live_heightmaps(&self) -> &'static [Heightmap] {
        if self.status >= Status::Carvers {
            &[
                Heightmap::WorldSurface,
                Heightmap::OceanFloor,
                Heightmap::MotionBlocking,
                Heightmap::MotionBlockingNoLeaves,
            ]
        } else {
            &[Heightmap::WorldSurfaceWg, Heightmap::OceanFloorWg]
        }
    }

    /// `ProtoChunk.setBlockState` with heightmap maintenance.
    pub fn set(&mut self, x: i32, y: i32, z: i32, s: BlockStateId) {
        if y < self.min_y || y >= self.max_y() {
            return;
        }
        let i = self.idx(x, y, z);
        self.blocks[i] = s;
        for &h in self.live_heightmaps() {
            if self.heightmaps[h as usize].is_none() {
                self.prime(h);
            }
            self.update_heightmap(h, x & 15, y, z & 15, s);
        }
    }

    fn update_heightmap(&mut self, h: Heightmap, x: i32, y: i32, z: i32, s: BlockStateId) {
        let flags = hm_flags();
        let bit = h.bit();
        let col = ((z << 4) | x) as usize;
        let cur = self.heightmaps[h as usize].as_ref().unwrap()[col];
        if y <= cur - 2 {
            return;
        }
        if flags[s.index()] & bit != 0 {
            if y >= cur {
                self.heightmaps[h as usize].as_mut().unwrap()[col] = y + 1;
            }
        } else if cur - 1 == y {
            let mut v = self.min_y;
            for l in (self.min_y..y).rev() {
                if flags[self.blocks[self.idx(x, l, z)].index()] & bit != 0 {
                    v = l + 1;
                    break;
                }
            }
            self.heightmaps[h as usize].as_mut().unwrap()[col] = v;
        }
    }

    /// `Heightmap.primeHeightmaps` for one type.
    pub fn prime(&mut self, h: Heightmap) {
        let flags = hm_flags();
        let bit = h.bit();
        let mut m = Box::new([self.min_y; 256]);
        for z in 0..16 {
            for x in 0..16 {
                for y in (self.min_y..self.max_y()).rev() {
                    if flags[self.blocks[self.idx(x, y, z)].index()] & bit != 0 {
                        m[((z << 4) | x) as usize] = y + 1;
                        break;
                    }
                }
            }
        }
        self.heightmaps[h as usize] = Some(m);
    }

    /// `getFirstAvailable` (topmost matching block + 1), priming lazily.
    pub fn first_available(&mut self, h: Heightmap, x: i32, z: i32) -> i32 {
        if self.heightmaps[h as usize].is_none() {
            self.prime(h);
        }
        self.heightmaps[h as usize].as_ref().unwrap()[(((z & 15) << 4) | (x & 15)) as usize]
    }

    /// Read-only height (requires the heightmap to be primed).
    #[must_use]
    pub fn height_primed(&self, h: Heightmap, x: i32, z: i32) -> Option<i32> {
        self.heightmaps[h as usize]
            .as_ref()
            .map(|m| m[(((z & 15) << 4) | (x & 15)) as usize])
    }

    #[inline]
    fn mask_idx(&self, x: i32, y: i32, z: i32) -> usize {
        ((((y - self.min_y) << 4) | (z & 15)) << 4 | (x & 15)) as usize
    }

    #[inline]
    #[must_use]
    pub fn mask_get(&self, x: i32, y: i32, z: i32) -> bool {
        let i = self.mask_idx(x, y, z);
        self.carving_mask[i / 64] & (1 << (i % 64)) != 0
    }

    #[inline]
    pub fn mask_set(&mut self, x: i32, y: i32, z: i32) {
        let i = self.mask_idx(x, y, z);
        self.carving_mask[i / 64] |= 1 << (i % 64);
    }

    /// Stored quart biome; `qy` is clamped to the chunk like vanilla.
    #[inline]
    #[must_use]
    pub fn biome(&self, qx: i32, qy: i32, qz: i32) -> BiomeId {
        let min_q = self.min_y >> 2;
        let max_q = min_q + (self.height >> 2) - 1;
        let qy = qy.clamp(min_q, max_q);
        self.biomes[(((qy - min_q) << 4) | ((qz & 3) << 2) | (qx & 3)) as usize]
    }
}

fn void_air() -> BlockStateId {
    static V: OnceLock<BlockStateId> = OnceLock::new();
    *V.get_or_init(|| rc_data::blocks().default_state("void_air"))
}
