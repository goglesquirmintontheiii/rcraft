//! Carvers: caves, canyons and nether caves (`WorldCarver` family).

use std::sync::OnceLock;

use ahash::AHashMap;
use rc_core::{BlockStateId, ChunkPos};
use serde_json::Value;

use crate::aquifer::Aquifer;
use crate::density::{ChunkCache, Mode};
use crate::generator::{DimensionGen, PrelimSurface};
use crate::mth;
use crate::proto::{ProtoChunk, Status};
use crate::providers::{FloatProvider, HeightProvider};
use crate::rng::Random;
use crate::surface::{Anchor, BiomeLookup};

#[derive(Clone, Debug)]
pub enum CarverKind {
    Cave {
        horizontal: FloatProvider,
        vertical: FloatProvider,
        floor: FloatProvider,
        y_scale: FloatProvider,
        nether: bool,
    },
    Canyon {
        vertical_rotation: FloatProvider,
        y_scale: FloatProvider,
        distance_factor: FloatProvider,
        thickness: FloatProvider,
        width_smoothness: i32,
        horizontal_radius_factor: FloatProvider,
        vertical_radius_default_factor: f32,
        vertical_radius_center_factor: f32,
    },
}

#[derive(Clone, Debug)]
pub struct Carver {
    pub probability: f32,
    pub y: HeightProvider,
    pub lava_level: Anchor,
    pub replaceable: rc_data::tags::BlockSet,
    pub kind: CarverKind,
}

impl Carver {
    fn from_json(v: &Value) -> Self {
        let c = &v["config"];
        let t = v["type"].as_str().unwrap().trim_start_matches("minecraft:");
        let fp = |k: &str| FloatProvider::from_json(&c[k]);
        let kind = match t {
            "cave" | "nether_cave" => CarverKind::Cave {
                horizontal: fp("horizontal_radius_multiplier"),
                vertical: fp("vertical_radius_multiplier"),
                floor: fp("floor_level"),
                y_scale: fp("yScale"),
                nether: t == "nether_cave",
            },
            "canyon" => {
                let s = &c["shape"];
                CarverKind::Canyon {
                    vertical_rotation: fp("vertical_rotation"),
                    y_scale: fp("yScale"),
                    distance_factor: FloatProvider::from_json(&s["distance_factor"]),
                    thickness: FloatProvider::from_json(&s["thickness"]),
                    width_smoothness: s["width_smoothness"].as_i64().unwrap() as i32,
                    horizontal_radius_factor: FloatProvider::from_json(
                        &s["horizontal_radius_factor"],
                    ),
                    vertical_radius_default_factor: s["vertical_radius_default_factor"]
                        .as_f64()
                        .unwrap() as f32,
                    vertical_radius_center_factor: s["vertical_radius_center_factor"]
                        .as_f64()
                        .unwrap() as f32,
                }
            }
            other => panic!("unknown carver {other}"),
        };
        Self {
            probability: c["probability"].as_f64().unwrap() as f32,
            y: HeightProvider::from_json(&c["y"]),
            lava_level: Anchor::from_json(&c["lava_level"]),
            replaceable: rc_data::tags().block(c["replaceable"].as_str().unwrap()),
            kind,
        }
    }
}

/// All configured carvers by id.
pub fn carvers() -> &'static AHashMap<String, Carver> {
    static C: OnceLock<AHashMap<String, Carver>> = OnceLock::new();
    C.get_or_init(|| {
        let files = rc_data::files();
        files
            .list("worldgen/configured_carver")
            .into_iter()
            .map(|n| {
                let j = files
                    .json(&format!("worldgen/configured_carver/{n}.json"))
                    .unwrap();
                (format!("minecraft:{n}"), Carver::from_json(&j))
            })
            .collect()
    })
}

struct CarveCtx<'a, 'b> {
    dg: &'a DimensionGen,
    cc: &'a ChunkCache,
    chunk: &'b mut ProtoChunk,
    aquifer: &'b mut Aquifer,
    prelim: &'b mut PrelimSurface,
    biomes: BiomeLookup<'a>,
    min_y: i32,
    height: i32,
    states: CarveStates,
}

#[derive(Clone, Copy)]
struct CarveStates {
    lava: BlockStateId,
    cave_air: BlockStateId,
    grass: u16,
    mycelium: u16,
    dirt: u16,
}

#[derive(Clone, Copy)]
enum Skip<'a> {
    Cave(f64),
    Canyon(&'a [f32]),
}

impl CarveCtx<'_, '_> {
    #[allow(clippy::too_many_arguments)]
    fn carve_ellipsoid(
        &mut self,
        c: &Carver,
        x: f64,
        y: f64,
        z: f64,
        hr: f64,
        vr: f64,
        skip: Skip<'_>,
    ) -> bool {
        let cp = self.chunk.pos;
        let d = f64::from((cp.x << 4) + 8);
        let e = f64::from((cp.z << 4) + 8);
        let f = 16.0 + hr * 2.0;
        if (x - d).abs() > f || (z - e).abs() > f {
            return false;
        }
        let i = cp.x << 4;
        let j = cp.z << 4;
        let k = (mth::floor(x - hr) - i - 1).max(0);
        let l = (mth::floor(x + hr) - i).min(15);
        let m = (mth::floor(y - vr) - 1).max(self.min_y + 1);
        let n = 7;
        let o = (mth::floor(y + vr) + 1).min(self.min_y + self.height - 1 - n);
        let p = (mth::floor(z - hr) - j - 1).max(0);
        let q = (mth::floor(z + hr) - j).min(15);
        let mut bl = false;
        for r in k..=l {
            let s = i + r;
            let g = (f64::from(s) + 0.5 - x) / hr;
            for t in p..=q {
                let u = j + t;
                let h = (f64::from(u) + 0.5 - z) / hr;
                if g * g + h * h >= 1.0 {
                    continue;
                }
                let mut reached_surface = false;
                let mut v = o;
                while v > m {
                    let w = (f64::from(v) - 0.5 - y) / vr;
                    let skipped = match skip {
                        Skip::Cave(floor) => w <= floor || g * g + w * w + h * h >= 1.0,
                        Skip::Canyon(fs) => {
                            let jj = v - self.min_y;
                            (g * g + h * h) * f64::from(fs[(jj - 1) as usize]) + w * w / 6.0 >= 1.0
                        }
                    };
                    if skipped || self.chunk.mask_get(s, v, u) {
                        v -= 1;
                        continue;
                    }
                    self.chunk.mask_set(s, v, u);
                    bl |= self.carve_block(c, s, v, u, &mut reached_surface);
                    v -= 1;
                }
            }
        }
        bl
    }

    fn carve_block(
        &mut self,
        c: &Carver,
        x: i32,
        y: i32,
        z: i32,
        reached_surface: &mut bool,
    ) -> bool {
        let b = rc_data::blocks();
        let bs = self.chunk.get(x, y, z);
        let bi = b.block_index(bs);
        if let CarverKind::Cave { nether: true, .. } = c.kind {
            if c.replaceable.contains(bi) {
                let st = if y <= self.min_y + 31 {
                    self.states.lava
                } else {
                    self.states.cave_air
                };
                self.chunk.set(x, y, z, st);
                return true;
            }
            return false;
        }
        if bi == self.states.grass || bi == self.states.mycelium {
            *reached_surface = true;
        }
        if !c.replaceable.contains(bi) {
            return false;
        }
        let st = if y <= c.lava_level.resolve(self.min_y, self.height) {
            Some(self.states.lava)
        } else {
            let mut pb = self.prelim.bind(self.dg, self.cc);
            self.aquifer.compute_substance(
                &self.dg.graph,
                self.cc,
                Mode::ChunkPoint,
                &mut pb,
                x,
                y,
                z,
                0.0,
            )
        };
        let Some(st) = st else { return false };
        self.chunk.set(x, y, z, st);
        if self.aquifer.should_schedule_fluid_update && !b.fluid(st).is_empty() {
            self.chunk.post_process.push((x, y, z));
        }
        if *reached_surface && b.block_index(self.chunk.get(x, y - 1, z)) == self.states.dirt {
            let has_fluid = !b.fluid(st).is_empty();
            if let Some(top) = self.dg.top_material(
                self.chunk,
                self.cc,
                self.prelim,
                &mut self.biomes,
                x,
                y - 1,
                z,
                has_fluid,
            ) {
                self.chunk.set(x, y - 1, z, top);
                if !b.fluid(top).is_empty() {
                    self.chunk.post_process.push((x, y - 1, z));
                }
            }
        }
        true
    }

    // ---- Cave carver ------------------------------------------------------

    fn carve_cave(&mut self, c: &Carver, r: &mut Random, start: ChunkPos) {
        let CarverKind::Cave {
            horizontal,
            vertical,
            floor,
            y_scale,
            nether,
        } = &c.kind
        else {
            unreachable!()
        };
        let i = (4 * 2 - 1) * 16;
        let bound = if *nether { 10 } else { 15 };
        let j = {
            let a = r.next_int_bound(bound) + 1;
            let b2 = r.next_int_bound(a) + 1;
            r.next_int_bound(b2)
        };
        for _ in 0..j {
            let d = f64::from((start.x << 4) + r.next_int_bound(16));
            let e = f64::from(c.y.sample(r, self.min_y, self.height));
            let f = f64::from((start.z << 4) + r.next_int_bound(16));
            let g = f64::from(horizontal.sample(r));
            let h = f64::from(vertical.sample(r));
            let l = f64::from(floor.sample(r));
            let mut m = 1;
            if r.next_int_bound(4) == 0 {
                let n = f64::from(y_scale.sample(r));
                let o = 1.0 + r.next_float() * 6.0;
                let ii = 1.5 + f64::from(mth::sin(std::f32::consts::FRAC_PI_2) * o);
                let jj = ii * n;
                self.carve_ellipsoid(c, d + 1.0, e, f, ii, jj, Skip::Cave(l));
                m += r.next_int_bound(4);
            }
            for _ in 0..m {
                let q = r.next_float() * (std::f32::consts::PI * 2.0);
                let o = (r.next_float() - 0.5) / 4.0;
                let thickness = if *nether {
                    (r.next_float() * 2.0 + r.next_float()) * 2.0
                } else {
                    let mut ff = r.next_float() * 2.0 + r.next_float();
                    if r.next_int_bound(10) == 0 {
                        ff *= r.next_float() * r.next_float() * 3.0 + 1.0;
                    }
                    ff
                };
                let s = i - r.next_int_bound(i / 4);
                let y_mul = if *nether { 5.0 } else { 1.0 };
                let seed = r.next_long();
                self.create_tunnel(c, seed, d, e, f, g, h, thickness, q, o, 0, s, y_mul, l);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn create_tunnel(
        &mut self,
        c: &Carver,
        seed: i64,
        mut x: f64,
        mut y: f64,
        mut z: f64,
        hm: f64,
        vm: f64,
        thickness: f32,
        mut yaw: f32,
        mut pitch: f32,
        start: i32,
        end: i32,
        y_scale: f64,
        floor: f64,
    ) {
        let mut rs = Random::legacy(seed);
        let k = rs.next_int_bound(end / 2) + end / 4;
        let bl = rs.next_int_bound(6) == 0;
        let mut l = 0.0f32;
        let mut m = 0.0f32;
        for n in start..end {
            let o =
                1.5 + f64::from(mth::sin(std::f32::consts::PI * n as f32 / end as f32) * thickness);
            let p = o * y_scale;
            let q = mth::cos(pitch);
            x += f64::from(mth::cos(yaw) * q);
            y += f64::from(mth::sin(pitch));
            z += f64::from(mth::sin(yaw) * q);
            pitch *= if bl { 0.92 } else { 0.7 };
            pitch += m * 0.1;
            yaw += l * 0.1;
            m *= 0.9;
            l *= 0.75;
            m += (rs.next_float() - rs.next_float()) * rs.next_float() * 2.0;
            l += (rs.next_float() - rs.next_float()) * rs.next_float() * 4.0;
            if n == k && thickness > 1.0 {
                let s1 = rs.next_long();
                let t1 = rs.next_float() * 0.5 + 0.5;
                self.create_tunnel(
                    c,
                    s1,
                    x,
                    y,
                    z,
                    hm,
                    vm,
                    t1,
                    yaw - std::f32::consts::FRAC_PI_2,
                    pitch / 3.0,
                    n,
                    end,
                    1.0,
                    floor,
                );
                let s2 = rs.next_long();
                let t2 = rs.next_float() * 0.5 + 0.5;
                self.create_tunnel(
                    c,
                    s2,
                    x,
                    y,
                    z,
                    hm,
                    vm,
                    t2,
                    yaw + std::f32::consts::FRAC_PI_2,
                    pitch / 3.0,
                    n,
                    end,
                    1.0,
                    floor,
                );
                return;
            }
            if rs.next_int_bound(4) == 0 {
                continue;
            }
            if !can_reach(self.chunk.pos, x, z, n, end, thickness) {
                return;
            }
            self.carve_ellipsoid(c, x, y, z, o * hm, p * vm, Skip::Cave(floor));
        }
    }

    // ---- Canyon carver ----------------------------------------------------

    fn carve_canyon(&mut self, c: &Carver, r: &mut Random, start: ChunkPos) {
        let CarverKind::Canyon {
            vertical_rotation,
            y_scale,
            distance_factor,
            thickness,
            ..
        } = &c.kind
        else {
            unreachable!()
        };
        let i = (4 * 2 - 1) * 16;
        let d = f64::from((start.x << 4) + r.next_int_bound(16));
        let j = c.y.sample(r, self.min_y, self.height);
        let e = f64::from((start.z << 4) + r.next_int_bound(16));
        let f = r.next_float() * (std::f32::consts::PI * 2.0);
        let g = vertical_rotation.sample(r);
        let h = f64::from(y_scale.sample(r));
        let k = thickness.sample(r);
        let l = (i as f32 * distance_factor.sample(r)) as i32;
        let seed = r.next_long();
        self.do_canyon(c, seed, d, f64::from(j), e, k, f, g, 0, l, h);
    }

    #[allow(clippy::too_many_arguments)]
    fn do_canyon(
        &mut self,
        c: &Carver,
        seed: i64,
        mut x: f64,
        mut y: f64,
        mut z: f64,
        thickness: f32,
        mut yaw: f32,
        mut pitch: f32,
        start: i32,
        end: i32,
        y_scale: f64,
    ) {
        let CarverKind::Canyon {
            width_smoothness,
            horizontal_radius_factor,
            vertical_radius_default_factor,
            vertical_radius_center_factor,
            ..
        } = &c.kind
        else {
            unreachable!()
        };
        let mut rs = Random::legacy(seed);
        let depth = self.height as usize;
        let mut fs = vec![0f32; depth];
        let mut ff = 1.0f32;
        for (jj, slot) in fs.iter_mut().enumerate() {
            if jj == 0 || rs.next_int_bound(*width_smoothness) == 0 {
                ff = 1.0 + rs.next_float() * rs.next_float();
            }
            *slot = ff * ff;
        }
        let mut l = 0.0f32;
        let mut m = 0.0f32;
        for n in start..end {
            let mut o =
                1.5 + f64::from(mth::sin(n as f32 * std::f32::consts::PI / end as f32) * thickness);
            let mut p = o * y_scale;
            o *= f64::from(horizontal_radius_factor.sample(&mut rs));
            // updateVerticalRadius
            let fv = 1.0 - (0.5 - n as f32 / end as f32).abs() * 2.0;
            let gv = vertical_radius_default_factor + vertical_radius_center_factor * fv;
            let rb = rs.next_float() * (1.0 - 0.75) + 0.75;
            p = f64::from(gv) * p * f64::from(rb);
            let q = mth::cos(pitch);
            let rr = mth::sin(pitch);
            x += f64::from(mth::cos(yaw) * q);
            y += f64::from(rr);
            z += f64::from(mth::sin(yaw) * q);
            pitch *= 0.7;
            pitch += m * 0.05;
            yaw += l * 0.05;
            m *= 0.8;
            l *= 0.5;
            m += (rs.next_float() - rs.next_float()) * rs.next_float() * 2.0;
            l += (rs.next_float() - rs.next_float()) * rs.next_float() * 4.0;
            if rs.next_int_bound(4) == 0 {
                continue;
            }
            if !can_reach(self.chunk.pos, x, z, n, end, thickness) {
                return;
            }
            self.carve_ellipsoid(c, x, y, z, o, p, Skip::Canyon(&fs));
        }
    }
}

fn can_reach(cp: ChunkPos, x: f64, z: f64, n: i32, end: i32, thickness: f32) -> bool {
    let d = f64::from((cp.x << 4) + 8);
    let e = f64::from((cp.z << 4) + 8);
    let f = x - d;
    let g = z - e;
    let h = f64::from(end - n);
    let k = f64::from(thickness + 2.0 + 16.0);
    f * f + g * g - h * h <= k * k
}

impl DimensionGen {
    /// Carver stage (`NoiseBasedChunkGenerator.applyCarvers`, AIR step).
    pub fn apply_carvers(
        &self,
        chunk: &mut ProtoChunk,
        cc: &ChunkCache,
        aquifer: &mut Aquifer,
        prelim: &mut PrelimSurface,
    ) {
        let b = rc_data::blocks();
        let (grass, mycelium, dirt) = self.surface_dirt_states();
        let all = carvers();
        let pos = chunk.pos;
        let mut ctx = CarveCtx {
            dg: self,
            cc,
            chunk,
            aquifer,
            prelim,
            biomes: BiomeLookup::new(self, false),
            min_y: self.settings.min_y,
            height: self.settings.height,
            states: CarveStates {
                lava: self.states.lava,
                cave_air: b.default_state("cave_air"),
                grass,
                mycelium,
                dirt,
            },
        };
        let mut wr = Random::legacy(0);
        for j in -8..=8 {
            for k in -8..=8 {
                let cp2 = ChunkPos::new(pos.x + j, pos.z + k);
                let biome = self.noise_biome(cp2.x << 2, 0, cp2.z << 2);
                let list = &self.biomes.get(biome).carvers;
                for (l, name) in list.iter().enumerate() {
                    let Some(c) = all.get(name) else { continue };
                    wr.set_large_feature_seed(self.seed.wrapping_add(l as i64), cp2.x, cp2.z);
                    if wr.next_float() <= c.probability {
                        match c.kind {
                            CarverKind::Cave { .. } => ctx.carve_cave(c, &mut wr, cp2),
                            CarverKind::Canyon { .. } => ctx.carve_canyon(c, &mut wr, cp2),
                        }
                    }
                }
            }
        }
        ctx.chunk.status = Status::Carvers;
    }
}
