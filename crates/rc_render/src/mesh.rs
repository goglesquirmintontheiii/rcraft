//! CPU chunk-section meshing with smooth lighting and ambient occlusion.
//!
//! Meshing runs on the worker pool; it reads an 18³ snapshot (the section
//! plus a one-block border) so no locks are held while building.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use rc_assets::{BakedModel, BlockModels, Dir, Layer, Tint};
use rc_core::{BiomeId, BlockStateId, SECTION_COUNT, WORLD_MIN_Y};
use rc_data::fluid::FluidKind;
use rc_world::Chunk;
use rc_world::light::nibble;

/// Terrain vertex (28 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    /// Position relative to the section origin.
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    /// Tint × face shade × AO, alpha for translucency.
    pub color: [u8; 4],
    /// Sky light ×16, block light ×16, unused, unused.
    pub light: [u8; 4],
}

/// Meshes for the three draw passes of one section.
#[derive(Debug, Default)]
pub struct SectionMesh {
    pub solid: Vec<Vertex>,
    pub cutout: Vec<Vertex>,
    pub translucent: Vec<Vertex>,
}

impl SectionMesh {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.solid.is_empty() && self.cutout.is_empty() && self.translucent.is_empty()
    }
}

/// Per-state flags the mesher needs, precomputed once.
#[derive(Debug)]
pub struct MeshTables {
    /// Full opaque cube: hides neighbour faces and darkens AO.
    pub occludes: Vec<bool>,
    pub fluid: Vec<u8>,
    /// Fluid height ×1/9 encoding: 0 = none, else own height in 1/255ths.
    pub fluid_height: Vec<u8>,
    pub block_of: Vec<u16>,
    pub emission: Vec<u8>,
    pub full: Vec<bool>,
    pub fancy_leaves: bool,
}

const FLUID_NONE: u8 = 0;
const FLUID_WATER: u8 = 1;
const FLUID_LAVA: u8 = 2;

impl MeshTables {
    #[must_use]
    pub fn new(models: &BlockModels, fancy_leaves: bool) -> Self {
        let b = rc_data::blocks();
        let n = b.len();
        let mut occludes = vec![false; n];
        let mut fluid = vec![FLUID_NONE; n];
        let mut fluid_height = vec![0u8; n];
        let mut block_of = vec![0u16; n];
        let mut emission = vec![0u8; n];
        let mut fullv = vec![false; n];
        for (i, st) in b.states.iter().enumerate() {
            let def = &b.defs[st.block as usize];
            block_of[i] = st.block;
            emission[i] = st.light;
            let full = st.flags & rc_data::blocks::FLAG_FULL_CUBE != 0;
            fullv[i] = full;
            let leaves = def.name.ends_with("leaves");
            occludes[i] = full
                && models.layers[i] != Layer::Invisible
                && models.layers[i] != Layer::Translucent
                && (!def.transparent || (leaves && !fancy_leaves));
            let f = st.fluid;
            fluid[i] = match f.kind {
                FluidKind::Water => FLUID_WATER,
                FluidKind::Lava => FLUID_LAVA,
                _ => FLUID_NONE,
            };
            if fluid[i] != FLUID_NONE {
                fluid_height[i] = (f.own_height() * 255.0).round().clamp(1.0, 255.0) as u8;
            }
        }
        Self {
            occludes,
            fluid,
            fluid_height,
            block_of,
            emission,
            full: fullv,
            fancy_leaves,
        }
    }
}

/// Biome tint colours, indexed by [`BiomeId`].
#[derive(Clone, Debug, Default)]
pub struct BiomeColors {
    pub grass: Vec<u32>,
    pub foliage: Vec<u32>,
    pub water: Vec<u32>,
}

const S: usize = 18;

#[inline]
const fn si(x: usize, y: usize, z: usize) -> usize {
    (y * S + z) * S + x
}

/// 18³ snapshot around a section.
pub struct Snapshot {
    pub blocks: Vec<u32>,
    pub sky: Vec<u8>,
    pub blk: Vec<u8>,
    /// Biome per column (18×18) at the section's middle height.
    pub biome: Vec<BiomeId>,
    pub origin: [i32; 3],
}

impl Snapshot {
    /// Copy the section `sy` of the centre chunk (index 4 of `n`) plus border.
    #[must_use]
    pub fn capture(n: &[Option<Arc<Chunk>>; 9], sy: usize) -> Self {
        let centre = n[4].as_ref().expect("centre chunk");
        let origin = [
            centre.pos.x * 16,
            WORLD_MIN_Y + sy as i32 * 16,
            centre.pos.z * 16,
        ];
        let mut blocks = vec![0u32; S * S * S];
        let mut sky = vec![15u8; S * S * S];
        let mut blk = vec![0u8; S * S * S];
        let mut biome = vec![BiomeId(0); S * S];
        for lz in 0..S {
            for lx in 0..S {
                let wx = lx as i32 - 1;
                let wz = lz as i32 - 1;
                let cx = (wx.div_euclid(16) + 1) as usize;
                let cz = (wz.div_euclid(16) + 1) as usize;
                let Some(c) = &n[cx * 3 + cz] else {
                    for ly in 0..S {
                        blocks[si(lx, ly, lz)] = 0;
                        sky[si(lx, ly, lz)] = 0;
                    }
                    continue;
                };
                let bx = wx.rem_euclid(16) as usize;
                let bz = wz.rem_euclid(16) as usize;
                {
                    let sec = &c.sections[sy];
                    biome[lz * S + lx] = sec.biome(bx, 8, bz);
                }
                for ly in 0..S {
                    let wy = sy as i32 * 16 + ly as i32 - 1;
                    let i = si(lx, ly, lz);
                    if wy < 0 {
                        blocks[i] = 0;
                        sky[i] = 0;
                        continue;
                    }
                    let s = (wy >> 4) as usize;
                    if s >= SECTION_COUNT {
                        continue;
                    }
                    let sec = &c.sections[s];
                    let by = (wy & 15) as usize;
                    let li = (by << 8) | (bz << 4) | bx;
                    blocks[i] = sec.blocks.get(li);
                    sky[i] = sec.sky_light.as_ref().map_or(15, |a| nibble(a, li));
                    blk[i] = sec.block_light.as_ref().map_or(0, |a| nibble(a, li));
                }
            }
        }
        Self {
            blocks,
            sky,
            blk,
            biome,
            origin,
        }
    }
}

fn shade(d: Dir) -> f32 {
    match d {
        Dir::Up => 1.0,
        Dir::Down => 0.5,
        Dir::North | Dir::South => 0.8,
        Dir::West | Dir::East => 0.6,
    }
}

fn rgb(c: u32) -> [f32; 3] {
    [
        ((c >> 16) & 255) as f32 / 255.0,
        ((c >> 8) & 255) as f32 / 255.0,
        (c & 255) as f32 / 255.0,
    ]
}

const AO_CURVE: [f32; 4] = [0.4, 0.6, 0.8, 1.0];

struct Ctx<'a> {
    snap: &'a Snapshot,
    t: &'a MeshTables,
    models: &'a BlockModels,
    colors: &'a BiomeColors,
    smooth: bool,
}

impl Ctx<'_> {
    #[inline]
    fn occ(&self, i: usize) -> bool {
        self.t.occludes[self.snap.blocks[i] as usize]
    }

    fn tint(&self, tint: Tint, lx: usize, lz: usize) -> [f32; 3] {
        let table = match tint {
            Tint::None => return [1.0; 3],
            Tint::Fixed(c) => return rgb(c),
            Tint::Grass => &self.colors.grass,
            Tint::Foliage => &self.colors.foliage,
            Tint::Water => &self.colors.water,
        };
        // 3×3 blend in the snapshot (vanilla blends over a configurable radius).
        let mut acc = [0.0f32; 3];
        let mut n = 0.0;
        for dz in 0..3 {
            for dx in 0..3 {
                let b = self.snap.biome[(lz + dz - 1) * S + (lx + dx - 1)];
                let c = rgb(table.get(b.0 as usize).copied().unwrap_or(0x7f_b2_38));
                acc[0] += c[0];
                acc[1] += c[1];
                acc[2] += c[2];
                n += 1.0;
            }
        }
        [acc[0] / n, acc[1] / n, acc[2] / n]
    }

    /// Light (sky, block) and AO factor at a vertex.
    fn vertex_light(&self, front: [usize; 3], d: Dir, p: [f32; 3], ao: bool) -> (f32, f32, f32) {
        let fi = si(front[0], front[1], front[2]);
        let (s0, b0) = (f32::from(self.snap.sky[fi]), f32::from(self.snap.blk[fi]));
        if !self.smooth {
            return (s0, b0, 1.0);
        }
        let (ta, tb) = match d {
            Dir::Up | Dir::Down => (0usize, 2usize),
            Dir::North | Dir::South => (0, 1),
            Dir::West | Dir::East => (2, 1),
        };
        let step = |_axis: usize, v: f32| -> isize { if v > 0.5 { 1 } else { -1 } };
        let mut a = front;
        let sa = step(ta, p[ta]);
        a[ta] = (a[ta] as isize + sa) as usize;
        let mut b = front;
        let sb = step(tb, p[tb]);
        b[tb] = (b[tb] as isize + sb) as usize;
        let mut c = a;
        c[tb] = (c[tb] as isize + sb) as usize;
        let ia = si(a[0], a[1], a[2]);
        let ib = si(b[0], b[1], b[2]);
        let ic = si(c[0], c[1], c[2]);
        let (oa, ob) = (self.occ(ia), self.occ(ib));
        let oc = if oa && ob { true } else { self.occ(ic) };
        let sample = |i: usize, o: bool| -> (f32, f32) {
            if o {
                (s0, b0)
            } else {
                (f32::from(self.snap.sky[i]), f32::from(self.snap.blk[i]))
            }
        };
        let (sa_, ba_) = sample(ia, oa);
        let (sb_, bb_) = sample(ib, ob);
        let (sc_, bc_) = sample(ic, oc);
        let sky = (s0 + sa_ + sb_ + sc_) * 0.25;
        let blk = (b0 + ba_ + bb_ + bc_) * 0.25;
        let aof = if ao {
            let level = if oa && ob {
                0
            } else {
                3 - (usize::from(oa) + usize::from(ob) + usize::from(oc))
            };
            AO_CURVE[level]
        } else {
            1.0
        };
        (sky, blk, aof)
    }
}

#[inline]
fn on_face(d: Dir, p: &[[f32; 3]; 4]) -> bool {
    let eps = 1e-4;
    match d {
        Dir::Down => p.iter().all(|v| v[1] <= eps),
        Dir::Up => p.iter().all(|v| v[1] >= 1.0 - eps),
        Dir::North => p.iter().all(|v| v[2] <= eps),
        Dir::South => p.iter().all(|v| v[2] >= 1.0 - eps),
        Dir::West => p.iter().all(|v| v[0] <= eps),
        Dir::East => p.iter().all(|v| v[0] >= 1.0 - eps),
    }
}

fn neighbour(x: usize, y: usize, z: usize, d: Dir) -> [usize; 3] {
    let n = d.normal();
    [
        (x as i32 + n[0]) as usize,
        (y as i32 + n[1]) as usize,
        (z as i32 + n[2]) as usize,
    ]
}

/// Mesh one section.
#[must_use]
pub fn mesh_section(
    snap: &Snapshot,
    tables: &MeshTables,
    models: &BlockModels,
    colors: &BiomeColors,
    smooth: bool,
) -> SectionMesh {
    let ctx = Ctx {
        snap,
        t: tables,
        models,
        colors,
        smooth,
    };
    let mut out = SectionMesh::default();
    let mut sel: Vec<&BakedModel> = Vec::with_capacity(4);
    for y in 1..17 {
        for z in 1..17 {
            for x in 1..17 {
                let i = si(x, y, z);
                let st = snap.blocks[i];
                if st == 0 {
                    continue;
                }
                let state = BlockStateId(st);
                let fluid = tables.fluid[st as usize];
                if fluid != FLUID_NONE {
                    mesh_fluid(&ctx, x, y, z, fluid, &mut out);
                }
                let layer = models.layers[st as usize];
                if layer == Layer::Invisible {
                    continue;
                }
                let wx = snap.origin[0] + x as i32 - 1;
                let wy = snap.origin[1] + y as i32 - 1;
                let wz = snap.origin[2] + z as i32 - 1;
                models.select(state, wx, wy, wz, &mut sel);
                let own_block = tables.block_of[st as usize];
                let tint_kind = models.tints[st as usize];
                let tint = ctx.tint(tint_kind, x, z);
                let emissive = tables.emission[st as usize];
                let target = match layer {
                    Layer::Solid => &mut out.solid,
                    Layer::Cutout => {
                        let leaves = tint_kind == Tint::Foliage
                            || matches!(tint_kind, Tint::Fixed(0x80a755 | 0x619961));
                        if leaves && !tables.fancy_leaves {
                            &mut out.solid
                        } else {
                            &mut out.cutout
                        }
                    }
                    _ => &mut out.translucent,
                };
                for m in &sel {
                    for q in &m.quads {
                        if let Some(c) = q.cull {
                            let [nx, ny, nz] = neighbour(x, y, z, c);
                            let ni = si(nx, ny, nz);
                            let ns = snap.blocks[ni];
                            if tables.occludes[ns as usize] {
                                continue;
                            }
                            // Same translucent/cutout full block (glass, ice) hides shared faces.
                            if layer != Layer::Solid
                                && tables.block_of[ns as usize] == own_block
                                && tables.full[ns as usize]
                                && !(tables.fancy_leaves && tint_kind == Tint::Foliage)
                            {
                                continue;
                            }
                        }
                        let boundary = on_face(q.dir, &q.pos);
                        let front = if boundary {
                            neighbour(x, y, z, q.dir)
                        } else {
                            [x, y, z]
                        };
                        let sh = if q.shade { shade(q.dir) } else { 1.0 };
                        let col = if q.tint >= 0 { tint } else { [1.0; 3] };
                        let mut verts = [Vertex::zeroed(); 4];
                        let mut aos = [1.0f32; 4];
                        for k in 0..4 {
                            let p = q.pos[k];
                            let (sky, mut blk, ao) =
                                ctx.vertex_light(front, q.dir, p, m.ao && q.shade);
                            if emissive > 0 {
                                blk = blk.max(f32::from(emissive));
                            }
                            aos[k] = ao;
                            let f = sh * ao;
                            verts[k] = Vertex {
                                pos: [
                                    p[0] + (x - 1) as f32,
                                    p[1] + (y - 1) as f32,
                                    p[2] + (z - 1) as f32,
                                ],
                                uv: q.uv[k],
                                color: [
                                    (col[0] * f * 255.0) as u8,
                                    (col[1] * f * 255.0) as u8,
                                    (col[2] * f * 255.0) as u8,
                                    255,
                                ],
                                light: [(sky * 16.0) as u8, (blk * 16.0) as u8, 0, 0],
                            };
                        }
                        // Flip the diagonal to avoid AO anisotropy.
                        if aos[0] + aos[2] < aos[1] + aos[3] {
                            target.extend_from_slice(&[verts[1], verts[2], verts[3], verts[0]]);
                        } else {
                            target.extend_from_slice(&verts);
                        }
                    }
                }
            }
        }
    }
    out
}

fn fluid_level(ctx: &Ctx<'_>, i: usize, kind: u8) -> Option<f32> {
    let s = ctx.snap.blocks[i] as usize;
    (ctx.t.fluid[s] == kind).then(|| f32::from(ctx.t.fluid_height[s]) / 255.0)
}

fn mesh_fluid(ctx: &Ctx<'_>, x: usize, y: usize, z: usize, kind: u8, out: &mut SectionMesh) {
    let i = si(x, y, z);
    let above = si(x, y + 1, z);
    let full_above = fluid_level(ctx, above, kind).is_some();
    // Corner heights: average of the up-to-4 fluid columns sharing the corner.
    let corner = |dx: isize, dz: isize| -> f32 {
        if full_above {
            return 1.0;
        }
        let mut sum = 0.0;
        let mut n = 0.0;
        for (ox, oz) in [(0isize, 0isize), (dx, 0), (0, dz), (dx, dz)] {
            let cx = (x as isize + ox) as usize;
            let cz = (z as isize + oz) as usize;
            if fluid_level(ctx, si(cx, y + 1, cz), kind).is_some() {
                return 1.0;
            }
            if let Some(h) = fluid_level(ctx, si(cx, y, cz), kind) {
                sum += h;
                n += 1.0;
            }
        }
        if n > 0.0 { sum / n } else { 0.89 }
    };
    let h00 = corner(-1, -1);
    let h10 = corner(1, -1);
    let h01 = corner(-1, 1);
    let h11 = corner(1, 1);
    let water = kind == FLUID_WATER;
    let sprite = if water {
        ctx.models.fluid[0]
    } else {
        ctx.models.fluid[2]
    };
    let tint = if water {
        ctx.tint(Tint::Water, x, z)
    } else {
        [1.0; 3]
    };
    let (alpha, target) = if water {
        (255u8, &mut out.translucent)
    } else {
        (255u8, &mut out.solid)
    };
    let uv = |u: f32, v: f32| sprite.map(u * 16.0, v * 16.0);
    let base = [(x - 1) as f32, (y - 1) as f32, (z - 1) as f32];
    let mut push = |pos: [[f32; 3]; 4], uvs: [[f32; 2]; 4], d: Dir, front: [usize; 3]| {
        let fi = si(front[0], front[1], front[2]);
        let sky = f32::from(ctx.snap.sky[fi].max(ctx.snap.sky[i]));
        let mut blk = f32::from(ctx.snap.blk[fi].max(ctx.snap.blk[i]));
        if !water {
            blk = 15.0;
        }
        let f = shade(d);
        let c = [
            (tint[0] * f * 255.0) as u8,
            (tint[1] * f * 255.0) as u8,
            (tint[2] * f * 255.0) as u8,
            alpha,
        ];
        for k in 0..4 {
            target.push(Vertex {
                pos: [
                    base[0] + pos[k][0],
                    base[1] + pos[k][1],
                    base[2] + pos[k][2],
                ],
                uv: uvs[k],
                color: c,
                light: [(sky * 16.0) as u8, (blk * 16.0) as u8, 0, 0],
            });
        }
    };
    // Top.
    if !full_above && !ctx.occ(above) {
        let top = [
            [0.0, h00, 0.0],
            [0.0, h01, 1.0],
            [1.0, h11, 1.0],
            [1.0, h10, 0.0],
        ];
        let uvs = [uv(0.0, 0.0), uv(0.0, 1.0), uv(1.0, 1.0), uv(1.0, 0.0)];
        push(top, uvs, Dir::Up, [x, y + 1, z]);
        // Underside of the surface, visible from below the water.
        let under = [top[3], top[2], top[1], top[0]];
        push(under, [uvs[3], uvs[2], uvs[1], uvs[0]], Dir::Up, [x, y, z]);
    }
    // Bottom.
    let below = si(x, y - 1, z);
    if fluid_level(ctx, below, kind).is_none() && !ctx.occ(below) {
        let p = [
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
        ];
        push(
            p,
            [uv(0.0, 0.0), uv(0.0, 1.0), uv(1.0, 1.0), uv(1.0, 0.0)],
            Dir::Down,
            [x, y - 1, z],
        );
    }
    // Sides.
    let sides: [(Dir, [usize; 3], [[f32; 3]; 4]); 4] = [
        (
            Dir::North,
            [x, y, z - 1],
            [
                [1.0, h10, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, h00, 0.0],
            ],
        ),
        (
            Dir::South,
            [x, y, z + 1],
            [
                [0.0, h01, 1.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 1.0],
                [1.0, h11, 1.0],
            ],
        ),
        (
            Dir::West,
            [x - 1, y, z],
            [
                [0.0, h00, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, h01, 1.0],
            ],
        ),
        (
            Dir::East,
            [x + 1, y, z],
            [
                [1.0, h11, 1.0],
                [1.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
                [1.0, h10, 0.0],
            ],
        ),
    ];
    for (d, n, p) in sides {
        let ni = si(n[0], n[1], n[2]);
        if fluid_level(ctx, ni, kind).is_some() || ctx.occ(ni) {
            continue;
        }
        let uvs = [
            uv(0.0, 1.0 - p[0][1]),
            uv(0.0, 1.0),
            uv(0.5, 1.0),
            uv(0.5, 1.0 - p[3][1]),
        ];
        push(p, uvs, d, n);
        // Back face so the side is visible from inside the water.
        push(
            [p[3], p[2], p[1], p[0]],
            [uvs[3], uvs[2], uvs[1], uvs[0]],
            d,
            [x, y, z],
        );
    }
}
