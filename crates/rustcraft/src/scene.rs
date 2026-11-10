//! Fills the renderer's textured batches each frame: mobs, dropped items,
//! falling blocks, XP orbs, lightning, particles, rain/snow and the
//! first-person hand. All positions are camera-relative.

use rc_assets::Dir;
use rc_assets::model::{JavaRandom, pick};
use rc_core::{BlockPos, BlockStateId};
use rc_render::terrain::QuadVertex;

use crate::entity::Kind;
use crate::game::Game;
use crate::gfx::{Gfx, WorldIcon};
use crate::mobs::MobKind;
use crate::models::{ModelQuad, build};

type V3 = [f32; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// Push a quad (4 corners, CCW or CW: culling is off) as two triangles.
fn quad(out: &mut Vec<QuadVertex>, p: [V3; 4], uv: [[f32; 2]; 4], color: [f32; 4]) {
    for i in [0, 1, 2, 0, 2, 3] {
        out.push(QuadVertex {
            pos: p[i],
            uv: uv[i],
            color,
        });
    }
}

/// Camera basis for the frame.
#[derive(Clone, Copy, Debug)]
pub struct Cam {
    pub eye: [f64; 3],
    pub fwd: V3,
    pub right: V3,
    pub up: V3,
}

impl Cam {
    #[must_use]
    pub fn new(eye: [f64; 3], yaw: f32, pitch: f32) -> Self {
        let (sy, cy) = yaw.to_radians().sin_cos();
        let (sp, cp) = pitch.to_radians().sin_cos();
        let fwd = [-sy * cp, -sp, cy * cp];
        let right = [-cy, 0.0, -sy];
        let up = [
            right[1] * fwd[2] - right[2] * fwd[1],
            right[2] * fwd[0] - right[0] * fwd[2],
            right[0] * fwd[1] - right[1] * fwd[0],
        ];
        Self {
            eye,
            fwd,
            right,
            up,
        }
    }

    fn rel(&self, p: [f64; 3]) -> V3 {
        [
            (p[0] - self.eye[0]) as f32,
            (p[1] - self.eye[1]) as f32,
            (p[2] - self.eye[2]) as f32,
        ]
    }
}

/// Brightness (0..1) at a block from sky/block light and daylight.
fn light_at(g: &Game, p: BlockPos, daylight: f32) -> f32 {
    let (sky, block) = g.world.light(p);
    let l = (f32::from(sky) * daylight).max(f32::from(block)) / 15.0;
    (l / (4.0 - 3.0 * l)).max(0.04) * 0.95 + 0.05
}

fn shade_for(n: V3) -> f32 {
    // Vanilla-like directional shading for entity faces.
    let up = n[1];
    if up > 0.5 {
        1.0
    } else if up < -0.5 {
        0.5
    } else if n[2].abs() > n[0].abs() {
        0.8
    } else {
        0.6
    }
}

/// Emit a block model (unit cube space 0..1) through `xf` (to camera space).
fn emit_block(
    gfx: &Gfx,
    s: BlockStateId,
    light: f32,
    out: &mut Vec<QuadVertex>,
    xf: impl Fn(V3) -> V3,
) {
    let models = &gfx.models;
    let Some(sm) = models.states.get(s.0 as usize) else {
        return;
    };
    let mut rng = JavaRandom::new(0);
    let tint = crate::gfx::tint_rgb(models.tints[s.0 as usize]);
    for part in &sm.parts {
        let m = &models.models[pick(part, &mut rng) as usize];
        for q in &m.quads {
            let shade = match q.dir {
                Dir::Up => 1.0,
                Dir::Down => 0.5,
                Dir::North | Dir::South => 0.8,
                Dir::West | Dir::East => 0.6,
            } * light;
            let c = if q.tint >= 0 { tint } else { [1.0; 3] };
            quad(
                out,
                q.pos.map(&xf),
                q.uv,
                [c[0] * shade, c[1] * shade, c[2] * shade, 1.0],
            );
        }
    }
}

/// Emit an item as it appears on the ground / in hand, centred at the
/// origin of `xf`'s space, roughly `size` blocks across.
fn emit_item(
    gfx: &Gfx,
    item: u16,
    light: f32,
    size: f32,
    batches: &mut [Vec<QuadVertex>],
    xf: impl Fn(V3) -> V3,
) {
    match gfx.item_world.get(item as usize) {
        Some(WorldIcon::Block(s)) => {
            let k = size;
            emit_block(gfx, *s, light, &mut batches[0], |p| {
                xf([(p[0] - 0.5) * k, (p[1] - 0.5) * k, (p[2] - 0.5) * k])
            });
        }
        Some(WorldIcon::Flat(t, uv, tint)) => {
            let h = size * 0.5;
            let c = [tint[0] * light, tint[1] * light, tint[2] * light, 1.0];
            let uvs = [
                [uv[0], uv[1]],
                [uv[2], uv[1]],
                [uv[2], uv[3]],
                [uv[0], uv[3]],
            ];
            // Thin slab: front and back faces a sixteenth apart.
            for z in [-0.03 * size, 0.03 * size] {
                quad(
                    &mut batches[*t],
                    [
                        xf([-h, h, z]),
                        xf([h, h, z]),
                        xf([h, -h, z]),
                        xf([-h, -h, z]),
                    ],
                    uvs,
                    c,
                );
            }
        }
        _ => {}
    }
}

fn model_to_cam(cam: &Cam, base: [f64; 3], yaw: f32, scale: f32, p: V3) -> V3 {
    let w = crate::models::to_world(p, base, yaw, scale);
    cam.rel(w)
}

fn emit_model(
    out: &mut Vec<QuadVertex>,
    quads: &[ModelQuad],
    cam: &Cam,
    base: [f64; 3],
    yaw: f32,
    scale: f32,
    color: [f32; 4],
) {
    for q in quads {
        let n = q.normal;
        let s = shade_for([-n[0], -n[1], -n[2]]);
        let c = [color[0] * s, color[1] * s, color[2] * s, color[3]];
        quad(
            out,
            q.pos.map(|p| model_to_cam(cam, base, yaw, scale, p)),
            q.uv,
            c,
        );
    }
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut d = (b - a) % 360.0;
    if d > 180.0 {
        d -= 360.0;
    } else if d < -180.0 {
        d += 360.0;
    }
    a + d * t
}

/// Daylight factor (0.2..1) for the current time and weather.
#[must_use]
pub fn daylight(g: &Game, partial: f32) -> f32 {
    let day = g.world.meta.day_time.rem_euclid(24000) as f32 / 24000.0;
    let d = ((day - 0.25) * std::f32::consts::TAU)
        .cos()
        .mul_add(2.0, 0.5)
        .clamp(0.0, 1.0);
    (d * g.weather_dim(partial)).max(0.0)
}

/// Fill all world batches for this frame.
pub fn fill(
    gfx: &mut Gfx,
    g: &Game,
    cam: &Cam,
    partial: f64,
    first_person: bool,
    scratch: &mut Vec<ModelQuad>,
) {
    gfx.terrain.clear_batches();
    let pf = partial as f32;
    let day = daylight(g, pf);
    let max_d2 = 96.0f64 * 96.0;
    let mut batches = std::mem::take(&mut gfx.terrain.batches);
    for e in &g.entities.list {
        let pos = e.lerp_pos(partial);
        let d2 = (pos[0] - cam.eye[0]).powi(2)
            + (pos[1] - cam.eye[1]).powi(2)
            + (pos[2] - cam.eye[2]).powi(2);
        if d2 > max_d2 {
            continue;
        }
        let bp = BlockPos::new(
            pos[0].floor() as i32,
            (pos[1] + 0.3).floor() as i32,
            pos[2].floor() as i32,
        );
        let light = light_at(g, bp, day);
        match &e.kind {
            Kind::Item { stack, .. } => {
                let t = e.age as f32 + pf;
                let spin = t / 20.0 + e.bob;
                let lift = ((t / 10.0 + e.bob).sin() * 0.1 + 0.1) as f64 + 0.125;
                let (s, c) = spin.sin_cos();
                let base = cam.rel([pos[0], pos[1] + lift, pos[2]]);
                let copies = match stack.count {
                    1 => 1,
                    2..=16 => 2,
                    17..=32 => 3,
                    _ => 4,
                };
                for k in 0..copies {
                    let off = [k as f32 * 0.04, k as f32 * 0.03, k as f32 * -0.04];
                    emit_item(gfx, stack.item, light, 0.25, &mut batches, |p| {
                        let p = add(p, off);
                        add(base, [p[0] * c + p[2] * s, p[1], -p[0] * s + p[2] * c])
                    });
                }
            }
            Kind::Falling { state } => {
                let base = cam.rel([pos[0] - 0.5, pos[1], pos[2] - 0.5]);
                emit_block(gfx, BlockStateId(*state), light, &mut batches[0], |p| {
                    add(base, p)
                });
            }
            Kind::Xp { value } => {
                let t = e.age as f32 + pf;
                let idx = match value {
                    0..=2 => 0,
                    3..=6 => 1,
                    7..=16 => 2,
                    17..=36 => 3,
                    37..=72 => 4,
                    73..=148 => 5,
                    149..=306 => 6,
                    307..=616 => 7,
                    _ => 8,
                } as f32;
                let (u, v) = ((idx % 4.0) / 4.0, (idx / 4.0).floor() / 4.0);
                let pulse = (t / 2.0).sin() * 0.5 + 0.5;
                let col = [pulse, 1.0, pulse * 0.5, 1.0];
                let c = cam.rel([pos[0], pos[1] + 0.15, pos[2]]);
                billboard(
                    &mut batches[gfx.tex.xp],
                    cam,
                    c,
                    0.15,
                    [u, v, u + 0.25, v + 0.25],
                    col,
                );
            }
            Kind::Lightning => {
                let mut seed = e.id.wrapping_mul(2_654_435_761) | 1;
                let mut rnd = || {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    (seed % 10_000) as f32 / 10_000.0
                };
                let mut x = 0.0f32;
                let mut z = 0.0f32;
                let base = cam.rel(pos);
                let mut y = 128.0f32;
                while y > 0.0 {
                    let nx = x + (rnd() - 0.5) * 2.0;
                    let nz = z + (rnd() - 0.5) * 2.0;
                    let y0 = (y - 8.0).max(0.0);
                    let col = [0.9, 0.9, 1.0, 1.0];
                    for (a, b) in [([0.15, 0.0], [-0.15, 0.0]), ([0.0, 0.15], [0.0, -0.15])] {
                        quad(
                            &mut batches[gfx.tex.white],
                            [
                                add(base, [nx + a[0], y0, nz + a[1]]),
                                add(base, [x + a[0], y, z + a[1]]),
                                add(base, [x + b[0], y, z + b[1]]),
                                add(base, [nx + b[0], y0, nz + b[1]]),
                            ],
                            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                            col,
                        );
                    }
                    (x, z, y) = (nx, nz, y0);
                }
            }
            Kind::Mob(m) => {
                let yaw = lerp_angle(e.prev_yaw, e.yaw, pf);
                let scale = if m.baby { 0.5 } else { 1.0 };
                let amount = m.prev_limb_amount + (m.limb_amount - m.prev_limb_amount) * pf;
                let swing = m.limb_swing + amount * pf;
                let leg = (swing * 0.6662).cos() * 1.4 * amount;
                let head = [
                    m.head_pitch.to_radians(),
                    -(m.head_yaw - yaw).to_radians(),
                    0.0,
                ];
                let chicken = m.kind == MobKind::Chicken;
                let pose = |name: &str| match name {
                    "head" | "bill" | "chin" => head,
                    "leg0" | "leg3" | "right_leg" => [leg, 0.0, 0.0],
                    "leg1" | "leg2" | "left_leg" => [-leg, 0.0, 0.0],
                    "wing0" if chicken && !e.on_ground => [0.0, 0.0, (e.age as f32 * 1.5).sin()],
                    "wing1" if chicken && !e.on_ground => [0.0, 0.0, -(e.age as f32 * 1.5).sin()],
                    _ => [0.0; 3],
                };
                let red = m.hurt_time > 0 || m.death_time > 0;
                let mut col = [light, light, light, 1.0];
                if red {
                    col = [light, light * 0.55, light * 0.55, 1.0];
                }
                let (model, tex) = match m.kind {
                    MobKind::Cow => (&gfx.mob_models.cow, gfx.tex.cow),
                    MobKind::Pig => (&gfx.mob_models.pig, gfx.tex.pig),
                    MobKind::Sheep => (&gfx.mob_models.sheep, gfx.tex.sheep),
                    MobKind::Chicken => (&gfx.mob_models.chicken, gfx.tex.chicken),
                };
                // Dying mobs sink and shrink instead of the vanilla tip-over.
                let dying = (m.death_time as f32 + pf) / 20.0;
                let scale = if m.death_time > 0 {
                    scale * (1.0 - dying * 0.5).max(0.3)
                } else {
                    scale
                };
                scratch.clear();
                build(model, pose, scratch);
                emit_model(&mut batches[tex], scratch, cam, pos, yaw, scale, col);
                if m.kind == MobKind::Sheep && !m.sheared {
                    let d = crate::mobs::dye_rgb(m.color);
                    scratch.clear();
                    build(&gfx.mob_models.sheep_fur, pose, scratch);
                    emit_model(
                        &mut batches[gfx.tex.sheep_fur],
                        scratch,
                        cam,
                        pos,
                        yaw,
                        scale,
                        [col[0] * d[0], col[1] * d[1], col[2] * d[2], 1.0],
                    );
                }
            }
        }
    }
    // Particles.
    for p in &g.particles.list {
        let pos = [
            p.prev[0] + (p.pos[0] - p.prev[0]) * partial,
            p.prev[1] + (p.pos[1] - p.prev[1]) * partial,
            p.prev[2] + (p.pos[2] - p.prev[2]) * partial,
        ];
        let l = light_at(
            g,
            BlockPos::new(
                pos[0].floor() as i32,
                pos[1].floor() as i32,
                pos[2].floor() as i32,
            ),
            day,
        );
        let c = [p.color[0] * l, p.color[1] * l, p.color[2] * l, p.color[3]];
        billboard(&mut batches[0], cam, cam.rel(pos), p.size, p.uv, c);
    }
    gfx.terrain.batches = batches;
    weather(gfx, g, cam, pf, day);
    if first_person {
        hand(gfx, g, cam, pf, day, scratch);
    }
}

fn billboard(out: &mut Vec<QuadVertex>, cam: &Cam, c: V3, s: f32, uv: [f32; 4], col: [f32; 4]) {
    let r = mul(cam.right, s);
    let u = mul(cam.up, s);
    quad(
        out,
        [
            add(add(c, mul(r, -1.0)), u),
            add(add(c, r), u),
            add(add(c, r), mul(u, -1.0)),
            add(add(c, mul(r, -1.0)), mul(u, -1.0)),
        ],
        [
            [uv[0], uv[1]],
            [uv[2], uv[1]],
            [uv[2], uv[3]],
            [uv[0], uv[3]],
        ],
        col,
    );
}

fn hash2(x: i32, z: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (z as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 12)
}

/// Rain and snow columns around the camera (vanilla draws a 5-block radius
/// on Fast and 10 on Fancy).
fn weather(gfx: &mut Gfx, g: &Game, cam: &Cam, pf: f32, day: f32) {
    let rain = g.prev_rain + (g.rain - g.prev_rain) * pf;
    if rain <= 0.01 {
        return;
    }
    let reg = rc_worldgen::biome::registry();
    let (cx, cy, cz) = (
        cam.eye[0].floor() as i32,
        cam.eye[1].floor() as i32,
        cam.eye[2].floor() as i32,
    );
    let radius = 7;
    let t = g.tick as f32 + pf;
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dz * dz > radius * radius {
                continue;
            }
            let (x, z) = (cx + dx, cz + dz);
            let Some(top) = g.world.surface_y(x, z) else {
                continue;
            };
            let y0 = (top + 1).max(cy - 10);
            let y1 = (top + 1).max(cy + 10);
            if y0 >= y1 {
                continue;
            }
            let b = reg.get(g.world.biome(BlockPos::new(x, y0, z)));
            if !b.has_precipitation {
                continue;
            }
            let snow = b.temperature < 0.15;
            let h = hash2(x, z);
            // Face the camera horizontally.
            let (fx, fz) = (
                (f64::from(x) + 0.5 - cam.eye[0]) as f32,
                (f64::from(z) + 0.5 - cam.eye[2]) as f32,
            );
            let len = (fx * fx + fz * fz).sqrt().max(0.01);
            let (sx, sz) = (-fz / len * 0.5, fx / len * 0.5);
            let base = cam.rel([f64::from(x) + 0.5, 0.0, f64::from(z) + 0.5]);
            let fade = (1.0 - (len / radius as f32).powi(2)).clamp(0.0, 1.0);
            let light = light_at(g, BlockPos::new(x, y0.max(top + 1), z), day).max(0.2);
            let (tex, v_off, u_off, alpha) = if snow {
                let fall = (t / 512.0 + (h % 1000) as f32 / 1000.0) % 1.0;
                (
                    gfx.tex.snow,
                    -fall * 4.0,
                    ((h >> 10) % 4) as f32 * 0.25 + t * 0.001,
                    fade * rain,
                )
            } else {
                let fall = ((t * 3.0 + (h % 32) as f32) / 32.0) % 1.0;
                (
                    gfx.tex.rain,
                    fall * -4.0,
                    ((h >> 10) % 4) as f32 * 0.25,
                    fade * rain * 0.8,
                )
            };
            let (v0, v1) = (y0 as f32 / 4.0 + v_off, y1 as f32 / 4.0 + v_off);
            let col = [light, light, light, alpha];
            quad(
                &mut gfx.terrain.blend_batches[tex],
                [
                    [base[0] - sx, y1 as f32 - cam.eye[1] as f32, base[2] - sz],
                    [base[0] + sx, y1 as f32 - cam.eye[1] as f32, base[2] + sz],
                    [base[0] + sx, y0 as f32 - cam.eye[1] as f32, base[2] + sz],
                    [base[0] - sx, y0 as f32 - cam.eye[1] as f32, base[2] - sz],
                ],
                [
                    [u_off, v0],
                    [u_off + 1.0, v0],
                    [u_off + 1.0, v1],
                    [u_off, v1],
                ],
                col,
            );
        }
    }
}

/// The first-person arm or held item.
fn hand(gfx: &mut Gfx, g: &Game, cam: &Cam, pf: f32, day: f32, scratch: &mut Vec<ModelQuad>) {
    use crate::player::GameMode;
    if g.player.mode == GameMode::Spectator {
        return;
    }
    let eye = cam.eye;
    let light = light_at(
        g,
        BlockPos::new(
            eye[0].floor() as i32,
            eye[1].floor() as i32,
            eye[2].floor() as i32,
        ),
        day,
    );
    // Swing progress 0..1 (6-tick animation like vanilla).
    let sw = if g.swing > 0 {
        1.0 - (g.swing as f32 - pf).max(0.0) / crate::game::SWING_TICKS as f32
    } else {
        0.0
    };
    let s1 = (sw * std::f32::consts::PI).sin();
    let s2 = (sw.sqrt() * std::f32::consts::PI).sin();
    let p = &g.player;
    let bob = p.prev_bob + (p.bob - p.prev_bob) * pf;
    let wd = p.prev_walk_dist + (p.walk_dist - p.prev_walk_dist) * pf;
    let ph = wd * std::f32::consts::PI;
    let (bx, by) = ((ph.sin() * bob * 0.5) * 0.5, -(ph.cos() * bob).abs() * 0.5);
    // View space → camera-relative world: x right, y up, z forward.
    let to_cam = |v: V3| {
        add(
            add(mul(cam.right, v[0]), mul(cam.up, v[1])),
            mul(cam.fwd, v[2]),
        )
    };
    let eating = p.eating > 0;
    let held = g.inv.held();
    if let Some(st) = held {
        // Item: right of centre, angled towards the centre.
        let (ox, oy, oz) = (
            0.56 - s2 * 0.4 + bx,
            -0.52
                + s1 * 0.2
                + by
                + if eating {
                    (p.eating as f32 * 0.8).sin().abs() * 0.05 + 0.15
                } else {
                    0.0
                },
            0.72 - s1 * 0.1,
        );
        let (ox, oz) = if eating {
            (ox - 0.3, oz - 0.1)
        } else {
            (ox, oz)
        };
        let is_block = matches!(
            gfx.item_world.get(st.item as usize),
            Some(WorldIcon::Block(_))
        );
        // (yaw about Y, roll about the view axis, pitch about X), size.
        let (yaw, roll, size) = if is_block {
            (45f32, 0.0f32, 0.3)
        } else {
            (-25f32, 20f32, 0.6)
        };
        let pitch = -s2 * 70.0 + if is_block { 0.0 } else { 10.0 };
        let (sy, cy) = yaw.to_radians().sin_cos();
        let (sr, cr) = roll.to_radians().sin_cos();
        let (sp, cp) = pitch.to_radians().sin_cos();
        let (ox, oy, oz) = if is_block {
            (ox, oy + 0.14, oz + 0.25)
        } else {
            (ox - 0.02, oy + 0.12, oz + 0.25)
        };
        let mut hb = std::mem::take(&mut gfx.terrain.hand_batches);
        emit_item(gfx, st.item, light, size, &mut hb, |q| {
            let (x, z) = (q[0] * cy + q[2] * sy, -q[0] * sy + q[2] * cy);
            let (x, y) = (x * cr - q[1] * sr, x * sr + q[1] * cr);
            let (y, z) = (y * cp - z * sp, y * sp + z * cp);
            to_cam([x + ox, y + oy, z + oz])
        });
        gfx.terrain.hand_batches = hb;
    } else {
        // Bare arm from the skin: shoulder low-right, pointing forward-left.
        scratch.clear();
        build(&gfx.mob_models.arm, |_| [0.0; 3], scratch);
        let pivot = gfx.mob_models.arm.parts[0].pivot;
        let (rx, ry, rz) = (
            (-125f32 + s2 * 30.0).to_radians(),
            (-20f32 + s1 * 30.0).to_radians(),
            0.0f32,
        );
        let base = [
            0.62 - s2 * 0.3 + bx,
            -0.74 + s1 * 0.25 + by,
            0.5 + s1 * 0.15,
        ];
        let col = [light, light, light, 1.0];
        let out = &mut gfx.terrain.hand_batches[gfx.tex.player];
        for q in scratch.iter() {
            let n = q.normal;
            let sh = shade_for([-n[0], -n[1], -n[2]]);
            let pts = q.pos.map(|m| {
                // Model px (Y down) relative to the shoulder → blocks, Y up.
                let mut v = [
                    (m[0] - pivot[0]) / 16.0,
                    -(m[1] - pivot[1]) / 16.0,
                    -(m[2] - pivot[2]) / 16.0,
                ];
                let (s, c) = rx.sin_cos();
                v = [v[0], v[1] * c - v[2] * s, v[1] * s + v[2] * c];
                let (s, c) = ry.sin_cos();
                v = [v[0] * c + v[2] * s, v[1], -v[0] * s + v[2] * c];
                let (s, c) = rz.sin_cos();
                v = [v[0] * c - v[1] * s, v[0] * s + v[1] * c, v[2]];
                to_cam(add(v, base))
            });
            quad(out, pts, q.uv, [col[0] * sh, col[1] * sh, col[2] * sh, 1.0]);
        }
    }
}
