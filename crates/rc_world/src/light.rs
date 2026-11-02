//! Sky and block light.
//!
//! Light reaches at most 15 blocks, so the light of a chunk is a pure function
//! of the blocks in its 3×3 neighbourhood. [`compute`] floods a padded
//! 48×48×H volume and keeps the centre chunk, which makes lighting trivially
//! parallel (one job per chunk on the worker pool) and exact. After an edit,
//! the edited chunk and any neighbour within 15 blocks are recomputed.
//!
//! Rules (minecraft.wiki "Light"): sky light enters at 15 from the top and
//! travels straight down through fully transparent blocks without loss; every
//! other step costs `max(1, opacity)` of the block entered. Block light starts
//! at the emitter's luminance and follows the same cost rule.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::OnceLock;

use rc_core::{BlockStateId, SECTION_COUNT, WORLD_MIN_Y};

use crate::Chunk;

const W: usize = 48;
const H: usize = SECTION_COUNT * 16;
const VOL: usize = W * W * H;

#[inline]
const fn idx(x: usize, y: usize, z: usize) -> usize {
    (y * W + z) * W + x
}

/// Per-state opacity and emission.
struct Tables {
    opacity: Vec<u8>,
    emission: Vec<u8>,
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let b = rc_data::blocks();
        Tables {
            opacity: b.states.iter().map(|s| s.opacity.min(15)).collect(),
            emission: b.states.iter().map(|s| s.light.min(15)).collect(),
        }
    })
}

/// Opacity of a block state (0 transparent … 15 opaque).
#[must_use]
pub fn opacity(s: BlockStateId) -> u8 {
    tables().opacity.get(s.0 as usize).copied().unwrap_or(15)
}

/// Light emitted by a block state.
#[must_use]
pub fn emission(s: BlockStateId) -> u8 {
    tables().emission.get(s.0 as usize).copied().unwrap_or(0)
}

struct Scratch {
    opa: Vec<u8>,
    sky: Vec<u8>,
    blk: Vec<u8>,
    queue: VecDeque<u32>,
}

thread_local! {
    static SCRATCH: RefCell<Option<Scratch>> = const { RefCell::new(None) };
}

/// Neighbourhood for lighting: index `(dx + 1) * 3 + (dz + 1)`, centre is 4.
pub type Neighbourhood<'a> = [Option<&'a Chunk>; 9];

/// Light arrays for one chunk (packed nibbles per section).
pub struct ChunkLight {
    pub sky: Vec<Box<[u8; 2048]>>,
    pub block: Vec<Box<[u8; 2048]>>,
}

impl std::fmt::Debug for ChunkLight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ChunkLight")
    }
}

fn fill_volume(n: &Neighbourhood<'_>, s: &mut Scratch) {
    let t = tables();
    s.opa.fill(15);
    s.blk.fill(0);
    s.queue.clear();
    for cdx in 0..3 {
        for cdz in 0..3 {
            let Some(c) = n[cdx * 3 + cdz] else { continue };
            for (si, sec) in c.sections.iter().enumerate() {
                let y0 = si * 16;
                if sec.blocks.is_uniform() {
                    let st = sec.blocks.get(0) as usize;
                    let o = t.opacity.get(st).copied().unwrap_or(15);
                    let e = t.emission.get(st).copied().unwrap_or(0);
                    for y in 0..16 {
                        for z in 0..16 {
                            let base = idx(cdx * 16, y0 + y, cdz * 16 + z);
                            s.opa[base..base + 16].fill(o);
                            if e > 0 {
                                s.blk[base..base + 16].fill(e);
                            }
                        }
                    }
                    if e > 0 {
                        for y in 0..16 {
                            for z in 0..16 {
                                for x in 0..16 {
                                    s.queue
                                        .push_back(idx(cdx * 16 + x, y0 + y, cdz * 16 + z) as u32);
                                }
                            }
                        }
                    }
                    continue;
                }
                let mut i = 0;
                for y in 0..16 {
                    for z in 0..16 {
                        let base = idx(cdx * 16, y0 + y, cdz * 16 + z);
                        for x in 0..16 {
                            let st = sec.blocks.get(i) as usize;
                            i += 1;
                            s.opa[base + x] = t.opacity.get(st).copied().unwrap_or(15);
                            let e = t.emission.get(st).copied().unwrap_or(0);
                            if e > 0 {
                                s.blk[base + x] = e;
                                s.queue.push_back((base + x) as u32);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Breadth-first flood of `light` from the cells already in `queue`.
fn flood(light: &mut [u8], opa: &[u8], queue: &mut VecDeque<u32>) {
    while let Some(i) = queue.pop_front() {
        let i = i as usize;
        let l = light[i];
        if l <= 1 {
            continue;
        }
        let x = i % W;
        let z = (i / W) % W;
        let y = i / (W * W);
        let mut visit = |j: usize| {
            let cost = opa[j].max(1);
            if l > cost {
                let nl = l - cost;
                if light[j] < nl {
                    light[j] = nl;
                    queue.push_back(j as u32);
                }
            }
        };
        if x > 0 {
            visit(i - 1);
        }
        if x + 1 < W {
            visit(i + 1);
        }
        if z > 0 {
            visit(i - W);
        }
        if z + 1 < W {
            visit(i + W);
        }
        if y > 0 {
            visit(i - W * W);
        }
        if y + 1 < H {
            visit(i + W * W);
        }
    }
}

fn sky_light(s: &mut Scratch) {
    s.sky.fill(0);
    // Straight-down pass per column; remember the lowest full-15 cell.
    let mut top15 = vec![H; W * W];
    let mut bottom = vec![H; W * W];
    for z in 0..W {
        for x in 0..W {
            let mut l = 15u8;
            let mut full = true;
            for y in (0..H).rev() {
                let i = idx(x, y, z);
                let o = s.opa[i];
                if o == 0 && full {
                    s.sky[i] = 15;
                    top15[z * W + x] = y;
                    continue;
                }
                full = false;
                l = l.saturating_sub(o.max(1));
                if l == 0 {
                    break;
                }
                s.sky[i] = l;
                bottom[z * W + x] = y;
            }
            bottom[z * W + x] = bottom[z * W + x].min(top15[z * W + x]);
            if full {
                top15[z * W + x] = 0;
            }
        }
    }
    // Seed cells whose horizontal neighbours are darker than they could be:
    // only cells below the highest 15-boundary among the 4 neighbours matter.
    s.queue.clear();
    for z in 0..W {
        for x in 0..W {
            let own = top15[z * W + x];
            let mut hi = own;
            if x > 0 {
                hi = hi.max(top15[z * W + x - 1]);
            }
            if x + 1 < W {
                hi = hi.max(top15[z * W + x + 1]);
            }
            if z > 0 {
                hi = hi.max(top15[(z - 1) * W + x]);
            }
            if z + 1 < W {
                hi = hi.max(top15[(z + 1) * W + x]);
            }
            let from = bottom[z * W + x];
            for y in from..hi.max(own + 1).min(H) {
                let i = idx(x, y, z);
                if s.sky[i] > 1 {
                    s.queue.push_back(i as u32);
                }
            }
        }
    }
    let Scratch {
        sky, opa, queue, ..
    } = s;
    flood(sky, opa, queue);
}

fn pack(vol: &[u8], cx: usize, cz: usize) -> Vec<Box<[u8; 2048]>> {
    (0..SECTION_COUNT)
        .map(|si| {
            let mut out = Box::new([0u8; 2048]);
            let mut i = 0;
            for y in 0..16 {
                for z in 0..16 {
                    let base = idx(cx, si * 16 + y, cz + z);
                    for x in (0..16).step_by(2) {
                        out[i] = vol[base + x] | (vol[base + x + 1] << 4);
                        i += 1;
                    }
                }
            }
            out
        })
        .collect()
}

/// Compute light for the centre chunk of `n`. Missing neighbours are treated
/// as solid and dark.
#[must_use]
pub fn compute(n: &Neighbourhood<'_>) -> ChunkLight {
    SCRATCH.with(|cell| {
        let mut slot = cell.borrow_mut();
        let s = slot.get_or_insert_with(|| Scratch {
            opa: vec![0; VOL],
            sky: vec![0; VOL],
            blk: vec![0; VOL],
            queue: VecDeque::with_capacity(1 << 16),
        });
        fill_volume(n, s);
        {
            let Scratch {
                blk, opa, queue, ..
            } = &mut *s;
            flood(blk, opa, queue);
        }
        sky_light(s);
        ChunkLight {
            sky: pack(&s.sky, 16, 16),
            block: pack(&s.blk, 16, 16),
        }
    })
}

/// Store computed light into a chunk.
pub fn apply(c: &mut Chunk, l: ChunkLight) {
    for ((sec, sky), blk) in c.sections.iter_mut().zip(l.sky).zip(l.block) {
        sec.sky_light = Some(sky);
        sec.block_light = Some(blk);
    }
    c.light_valid = true;
}

/// Read a packed nibble.
#[inline]
#[must_use]
pub fn nibble(arr: &[u8; 2048], i: usize) -> u8 {
    (arr[i >> 1] >> ((i & 1) * 4)) & 15
}

/// Sky and block light at a world position within a chunk.
#[must_use]
pub fn light_at(c: &Chunk, x: i32, y: i32, z: i32) -> (u8, u8) {
    let sy = y - WORLD_MIN_Y;
    if sy < 0 {
        return (0, 0);
    }
    let si = (sy >> 4) as usize;
    if si >= SECTION_COUNT {
        return (15, 0);
    }
    let sec = &c.sections[si];
    let i = (((sy & 15) as usize) << 8) | (((z & 15) as usize) << 4) | (x & 15) as usize;
    (
        sec.sky_light.as_ref().map_or(15, |a| nibble(a, i)),
        sec.block_light.as_ref().map_or(0, |a| nibble(a, i)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rc_core::{BlockPos, ChunkPos};

    #[test]
    fn open_sky_and_torch() {
        let b = rc_data::blocks();
        let stone = b.default_state("stone");
        let torch = b.default_state("torch");
        let mut chunks: Vec<Chunk> = (0..9)
            .map(|i| {
                let mut c = Chunk::new_empty(ChunkPos::new(i / 3 - 1, i % 3 - 1));
                let (ox, oz) = (c.pos.x * 16, c.pos.z * 16);
                for x in 0..16 {
                    for z in 0..16 {
                        for y in -64..64 {
                            c.set_block(BlockPos::new(ox + x, y, oz + z), stone);
                        }
                    }
                }
                c
            })
            .collect();
        // A sealed cave in the centre chunk with a torch.
        let centre = &mut chunks[4];
        for x in 2..10 {
            for z in 2..10 {
                for y in 10..14 {
                    centre.set_block(BlockPos::new(x, y, z), BlockStateId::AIR);
                }
            }
        }
        centre.set_block(BlockPos::new(5, 10, 5), torch);
        let n: Neighbourhood<'_> = std::array::from_fn(|i| Some(&chunks[i]));
        let l = compute(&n);
        let mut c = chunks[4].clone();
        apply(&mut c, l);
        assert_eq!(light_at(&c, 3, 100, 3), (15, 0));
        assert_eq!(light_at(&c, 5, 10, 5), (0, 14));
        assert_eq!(light_at(&c, 6, 10, 5), (0, 13));
        assert_eq!(light_at(&c, 9, 13, 9), (0, 14 - 4 - 4 - 3));
        assert_eq!(light_at(&c, 3, 64, 3), (15, 0));
        assert_eq!(light_at(&c, 3, 63, 3), (0, 0));
    }
}
