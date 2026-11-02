//! Aquifers (`NoiseBasedAquifer`) and the global fluid picker.

use rc_core::BlockStateId;

use crate::density::{ChunkCache, Graph, Id, Mode};
use crate::mth::{self, floor_div};
use crate::rng::Positional;

pub const WAY_BELOW_MIN_Y: i32 = -2032 << 4;

/// `FluidStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FluidStatus {
    pub level: i32,
    pub fluid: BlockStateId,
}

impl FluidStatus {
    #[inline]
    #[must_use]
    pub fn at(&self, y: i32, air: BlockStateId) -> BlockStateId {
        if y < self.level { self.fluid } else { air }
    }
}

/// The router functions the aquifer samples.
#[derive(Clone, Copy, Debug)]
pub struct AquiferFns {
    pub barrier: Id,
    pub floodedness: Id,
    pub spread: Id,
    pub lava: Id,
    pub erosion: Id,
    pub depth: Id,
}

#[derive(Clone, Copy, Debug)]
pub struct GlobalFluid {
    pub sea_level: i32,
    pub sea_fluid: BlockStateId,
    pub lava: BlockStateId,
    pub water: BlockStateId,
}

impl GlobalFluid {
    #[inline]
    #[must_use]
    pub fn compute(&self, y: i32) -> FluidStatus {
        if y < (-54).min(self.sea_level) {
            FluidStatus {
                level: -54,
                fluid: self.lava,
            }
        } else {
            FluidStatus {
                level: self.sea_level,
                fluid: self.sea_fluid,
            }
        }
    }
}

const SURFACE_SAMPLING_OFFSETS: [(i32, i32); 13] = [
    (-2, -1),
    (-1, -1),
    (0, -1),
    (1, -1),
    (-3, 0),
    (-2, 0),
    (-1, 0),
    (0, 0),
    (1, 0),
    (-2, 1),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// Per-chunk aquifer state (`NoiseBasedAquifer`).
pub struct Aquifer {
    fns: AquiferFns,
    random: Positional,
    global: GlobalFluid,
    air: BlockStateId,
    min_grid_x: i32,
    min_grid_y: i32,
    min_grid_z: i32,
    grid_size_x: i32,
    grid_size_z: i32,
    status_cache: Vec<Option<FluidStatus>>,
    location_cache: Vec<i64>,
    pub should_schedule_fluid_update: bool,
    enabled: bool,
}

#[inline]
fn pack(x: i32, y: i32, z: i32) -> i64 {
    ((i64::from(x) & 0x3FF_FFFF) << 38)
        | ((i64::from(z) & 0x3FF_FFFF) << 12)
        | (i64::from(y) & 0xFFF)
}
#[inline]
fn unpack_x(l: i64) -> i32 {
    (l >> 38) as i32
}
#[inline]
fn unpack_y(l: i64) -> i32 {
    ((l << 52) >> 52) as i32
}
#[inline]
fn unpack_z(l: i64) -> i32 {
    ((l << 26) >> 38) as i32
}

#[inline]
fn similarity(i: i32, j: i32) -> f64 {
    1.0 - f64::from((j - i).abs()) / 25.0
}

/// Callback for `NoiseChunk.preliminarySurfaceLevel`.
pub trait PreliminarySurface {
    fn preliminary_surface_level(&mut self, x: i32, z: i32) -> i32;
}

impl Aquifer {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        enabled: bool,
        fns: AquiferFns,
        random: Positional,
        global: GlobalFluid,
        air: BlockStateId,
        chunk_min_x: i32,
        chunk_min_z: i32,
        min_y: i32,
        height: i32,
    ) -> Self {
        let min_grid_x = floor_div(chunk_min_x, 16) - 1;
        let max_grid_x = floor_div(chunk_min_x + 15, 16) + 1;
        let grid_size_x = max_grid_x - min_grid_x + 1;
        let min_grid_y = floor_div(min_y, 12) - 1;
        let max_grid_y = floor_div(min_y + height, 12) + 1;
        let grid_size_y = max_grid_y - min_grid_y + 1;
        let min_grid_z = floor_div(chunk_min_z, 16) - 1;
        let max_grid_z = floor_div(chunk_min_z + 15, 16) + 1;
        let grid_size_z = max_grid_z - min_grid_z + 1;
        let n = (grid_size_x * grid_size_y * grid_size_z) as usize;
        Self {
            fns,
            random,
            global,
            air,
            min_grid_x,
            min_grid_y,
            min_grid_z,
            grid_size_x,
            grid_size_z,
            status_cache: vec![None; n],
            location_cache: vec![i64::MAX; n],
            should_schedule_fluid_update: false,
            enabled,
        }
    }

    #[inline]
    fn index(&self, x: i32, y: i32, z: i32) -> usize {
        let l = x - self.min_grid_x;
        let m = y - self.min_grid_y;
        let n = z - self.min_grid_z;
        ((m * self.grid_size_z + n) * self.grid_size_x + l) as usize
    }

    /// `computeSubstance`. `None` means "use the default block / defer to the
    /// next material rule". `ctx` evaluates the barrier noise at the point.
    #[allow(clippy::too_many_arguments)]
    pub fn compute_substance(
        &mut self,
        g: &Graph,
        cc: &ChunkCache,
        mode: Mode,
        prelim: &mut dyn PreliminarySurface,
        x: i32,
        y: i32,
        z: i32,
        density: f64,
    ) -> Option<BlockStateId> {
        if !self.enabled {
            return if density > 0.0 {
                None
            } else {
                Some(self.global.compute(y).at(y, self.air))
            };
        }
        if density > 0.0 {
            self.should_schedule_fluid_update = false;
            return None;
        }
        let fs = self.global.compute(y);
        if fs.at(y, self.air) == self.global.lava {
            self.should_schedule_fluid_update = false;
            return Some(self.global.lava);
        }
        let l = floor_div(x - 5, 16);
        let m = floor_div(y + 1, 12);
        let n = floor_div(z - 5, 16);
        let (mut o, mut p, mut q) = (i32::MAX, i32::MAX, i32::MAX);
        let (mut s, mut t, mut u) = (0i64, 0i64, 0i64);
        for w in 0..=1 {
            for xx in -1..=1 {
                for yy in 0..=1 {
                    let gx = l + w;
                    let gy = m + xx;
                    let gz = n + yy;
                    let idx = self.index(gx, gy, gz);
                    let cached = self.location_cache[idx];
                    let ae = if cached != i64::MAX {
                        cached
                    } else {
                        let mut r = self.random.at(gx, gy, gz);
                        let a = gx * 16 + r.next_int_bound(10);
                        let b = gy * 12 + r.next_int_bound(9);
                        let c = gz * 16 + r.next_int_bound(10);
                        let v = pack(a, b, c);
                        self.location_cache[idx] = v;
                        v
                    };
                    let af = unpack_x(ae) - x;
                    let ag = unpack_y(ae) - y;
                    let ah = unpack_z(ae) - z;
                    let ai = af * af + ag * ag + ah * ah;
                    if o >= ai {
                        u = t;
                        t = s;
                        s = ae;
                        q = p;
                        p = o;
                        o = ai;
                    } else if p >= ai {
                        u = t;
                        t = ae;
                        q = p;
                        p = ai;
                    } else if q >= ai {
                        u = ae;
                        q = ai;
                    }
                }
            }
        }
        let fs2 = self.status(g, cc, prelim, s);
        let d = similarity(o, p);
        let bs = fs2.at(y, self.air);
        if d <= 0.0 {
            // FLOWING_UPDATE_SIMULARITY = similarity(10², 12²) = -0.76
            self.should_schedule_fluid_update = d >= -0.76 && fs2 != self.status(g, cc, prelim, t);
            return Some(bs);
        }
        if bs == self.global.water
            && self.global.compute(y - 1).at(y - 1, self.air) == self.global.lava
        {
            self.should_schedule_fluid_update = true;
            return Some(bs);
        }
        let mut barrier = f64::NAN;
        let fs3 = self.status(g, cc, prelim, t);
        let e = d * self.pressure(g, cc, mode, x, y, z, &mut barrier, fs2, fs3);
        if density + e > 0.0 {
            self.should_schedule_fluid_update = false;
            return None;
        }
        let fs4 = self.status(g, cc, prelim, u);
        let f = similarity(o, q);
        if f > 0.0 {
            let h = d * f * self.pressure(g, cc, mode, x, y, z, &mut barrier, fs2, fs4);
            if density + h > 0.0 {
                self.should_schedule_fluid_update = false;
                return None;
            }
        }
        let gg = similarity(p, q);
        if gg > 0.0 {
            let h = d * gg * self.pressure(g, cc, mode, x, y, z, &mut barrier, fs3, fs4);
            if density + h > 0.0 {
                self.should_schedule_fluid_update = false;
                return None;
            }
        }
        self.should_schedule_fluid_update = true;
        Some(bs)
    }

    #[allow(clippy::too_many_arguments)]
    fn pressure(
        &self,
        g: &Graph,
        cc: &ChunkCache,
        mode: Mode,
        x: i32,
        y: i32,
        z: i32,
        barrier: &mut f64,
        a: FluidStatus,
        b: FluidStatus,
    ) -> f64 {
        let bs = a.at(y, self.air);
        let bs2 = b.at(y, self.air);
        let lava = self.global.lava;
        let water = self.global.water;
        if (bs == lava && bs2 == water) || (bs == water && bs2 == lava) {
            return 2.0;
        }
        let j = (a.level - b.level).abs();
        if j == 0 {
            return 0.0;
        }
        let d = 0.5 * f64::from(a.level + b.level);
        let e = f64::from(y) + 0.5 - d;
        let f = f64::from(j) / 2.0;
        let o = f - e.abs();
        let q = if e > 0.0 {
            let pp = o;
            if pp > 0.0 { pp / 1.5 } else { pp / 2.5 }
        } else {
            let pp = 3.0 + o;
            if pp > 0.0 { pp / 3.0 } else { pp / 10.0 }
        };
        let r = if (-2.0..=2.0).contains(&q) {
            if barrier.is_nan() {
                *barrier = g.eval(self.fns.barrier, x, y, z, mode, Some(cc));
            }
            *barrier
        } else {
            0.0
        };
        2.0 * (r + q)
    }

    fn status(
        &mut self,
        g: &Graph,
        cc: &ChunkCache,
        prelim: &mut dyn PreliminarySurface,
        pos: i64,
    ) -> FluidStatus {
        let (x, y, z) = (unpack_x(pos), unpack_y(pos), unpack_z(pos));
        let idx = self.index(floor_div(x, 16), floor_div(y, 12), floor_div(z, 16));
        if let Some(s) = self.status_cache[idx] {
            return s;
        }
        let s = self.compute_fluid(g, cc, prelim, x, y, z);
        self.status_cache[idx] = Some(s);
        s
    }

    fn compute_fluid(
        &self,
        g: &Graph,
        cc: &ChunkCache,
        prelim: &mut dyn PreliminarySurface,
        x: i32,
        y: i32,
        z: i32,
    ) -> FluidStatus {
        let fs = self.global.compute(y);
        let mut l = i32::MAX;
        let m = y + 12;
        let n = y - 12;
        let mut bl = false;
        for (dx, dz) in SURFACE_SAMPLING_OFFSETS {
            let o = x + (dx << 4);
            let p = z + (dz << 4);
            let q = prelim.preliminary_surface_level(o, p);
            let r = q.wrapping_add(8);
            let center = dx == 0 && dz == 0;
            if center && n > r {
                return fs;
            }
            let above = m > r;
            if above || center {
                let fs2 = self.global.compute(r);
                if fs2.at(r, self.air) != self.air {
                    if center {
                        bl = true;
                    }
                    if above {
                        return fs2;
                    }
                }
            }
            l = l.min(q);
        }
        let s = self.surface_level(g, cc, x, y, z, fs, l, bl);
        FluidStatus {
            level: s,
            fluid: self.fluid_type(g, cc, x, y, z, fs, s),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn surface_level(
        &self,
        g: &Graph,
        cc: &ChunkCache,
        x: i32,
        y: i32,
        z: i32,
        fs: FluidStatus,
        min_surface: i32,
        bl: bool,
    ) -> i32 {
        let erosion = g.eval(self.fns.erosion, x, y, z, Mode::ChunkPoint, Some(cc));
        let depth = g.eval(self.fns.depth, x, y, z, Mode::ChunkPoint, Some(cc));
        let deep_dark = erosion < f64::from(-0.225f32) && depth > f64::from(0.9f32);
        let (d, e) = if deep_dark {
            (-1.0, -1.0)
        } else {
            let mm = min_surface + 8 - y;
            let f = if bl {
                mth::clamped_map(f64::from(mm), 0.0, 64.0, 1.0, 0.0)
            } else {
                0.0
            };
            let gg = mth::clamp(
                g.eval(self.fns.floodedness, x, y, z, Mode::ChunkPoint, Some(cc)),
                -1.0,
                1.0,
            );
            let h = mth::map(f, 1.0, 0.0, -0.3, 0.8);
            let nn = mth::map(f, 1.0, 0.0, -0.8, 0.4);
            (gg - nn, gg - h)
        };
        if e > 0.0 {
            fs.level
        } else if d > 0.0 {
            self.randomized_surface_level(g, cc, x, y, z, min_surface)
        } else {
            WAY_BELOW_MIN_Y
        }
    }

    fn randomized_surface_level(
        &self,
        g: &Graph,
        cc: &ChunkCache,
        x: i32,
        y: i32,
        z: i32,
        min_surface: i32,
    ) -> i32 {
        let l = floor_div(x, 16);
        let m = floor_div(y, 40);
        let n = floor_div(z, 16);
        let o = m * 40 + 20;
        let d = g.eval(self.fns.spread, l, m, n, Mode::ChunkPoint, Some(cc)) * 10.0;
        let q = mth::quantize(d, 3);
        min_surface.min(o + q)
    }

    #[allow(clippy::too_many_arguments)]
    fn fluid_type(
        &self,
        g: &Graph,
        cc: &ChunkCache,
        x: i32,
        y: i32,
        z: i32,
        fs: FluidStatus,
        level: i32,
    ) -> BlockStateId {
        let mut bs = fs.fluid;
        if level <= -10 && level != WAY_BELOW_MIN_Y && fs.fluid != self.global.lava {
            let m = floor_div(x, 64);
            let n = floor_div(y, 40);
            let o = floor_div(z, 64);
            let d = g.eval(self.fns.lava, m, n, o, Mode::ChunkPoint, Some(cc));
            if d.abs() > 0.3 {
                bs = self.global.lava;
            }
        }
        bs
    }
}
