//! Client particles: block breaking/hitting debris and rain splashes.
//! Simulated on the CPU (a few hundred at most) and drawn as camera-facing
//! quads from the block atlas.

use rand::Rng as _;
use rc_core::BlockPos;

use crate::world::World;

#[derive(Clone, Copy, Debug)]
pub struct Particle {
    pub pos: [f64; 3],
    pub prev: [f64; 3],
    pub vel: [f64; 3],
    pub age: u32,
    pub life: u32,
    /// Atlas UV rectangle.
    pub uv: [f32; 4],
    pub color: [f32; 4],
    pub size: f32,
    pub gravity: f64,
}

#[derive(Debug, Default)]
pub struct Particles {
    pub list: Vec<Particle>,
}

/// Upper bound so heavy mining can never overload the frame.
const MAX: usize = 4000;

impl Particles {
    fn push(&mut self, p: Particle) {
        if self.list.len() < MAX {
            self.list.push(p);
        }
    }

    /// Debris when a block breaks (vanilla spawns a 4×4×4 grid).
    pub fn block_break(&mut self, p: BlockPos, sprite: [f32; 4], tint: [f32; 3]) {
        let mut r = rand::rng();
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    let o = [
                        (f64::from(i) + 0.5) / 4.0,
                        (f64::from(j) + 0.5) / 4.0,
                        (f64::from(k) + 0.5) / 4.0,
                    ];
                    let pos = [
                        f64::from(p.x) + o[0],
                        f64::from(p.y) + o[1],
                        f64::from(p.z) + o[2],
                    ];
                    let vel = [
                        (o[0] - 0.5) * 0.2 + r.random_range(-0.04..0.04),
                        (o[1] - 0.5) * 0.2 + r.random_range(0.0..0.1),
                        (o[2] - 0.5) * 0.2 + r.random_range(-0.04..0.04),
                    ];
                    self.push(debris(pos, vel, sprite, tint, &mut r));
                }
            }
        }
    }

    /// A few chips while a block is being mined.
    pub fn block_hit(&mut self, p: BlockPos, normal: [i32; 3], sprite: [f32; 4], tint: [f32; 3]) {
        let mut r = rand::rng();
        let pos = [
            f64::from(p.x)
                + 0.5
                + f64::from(normal[0]) * 0.55
                + if normal[0] == 0 {
                    r.random_range(-0.4..0.4)
                } else {
                    0.0
                },
            f64::from(p.y)
                + 0.5
                + f64::from(normal[1]) * 0.55
                + if normal[1] == 0 {
                    r.random_range(-0.4..0.4)
                } else {
                    0.0
                },
            f64::from(p.z)
                + 0.5
                + f64::from(normal[2]) * 0.55
                + if normal[2] == 0 {
                    r.random_range(-0.4..0.4)
                } else {
                    0.0
                },
        ];
        let vel = [
            r.random_range(-0.03..0.03),
            r.random_range(0.0..0.06),
            r.random_range(-0.03..0.03),
        ];
        let mut d = debris(pos, vel, sprite, tint, &mut r);
        d.size *= 0.6;
        self.push(d);
    }

    pub fn splash(&mut self, pos: [f64; 3], sprite: [f32; 4]) {
        let mut r = rand::rng();
        self.push(Particle {
            pos,
            prev: pos,
            vel: [
                r.random_range(-0.02..0.02),
                0.08,
                r.random_range(-0.02..0.02),
            ],
            age: 0,
            life: 8,
            uv: sprite,
            color: [0.6, 0.7, 1.0, 1.0],
            size: 0.06,
            gravity: 0.04,
        });
    }

    pub fn tick(&mut self, world: &World) {
        let b = rc_data::blocks();
        for p in &mut self.list {
            p.prev = p.pos;
            p.age += 1;
            p.vel[1] -= p.gravity;
            let next = [
                p.pos[0] + p.vel[0],
                p.pos[1] + p.vel[1],
                p.pos[2] + p.vel[2],
            ];
            let cell = BlockPos::new(
                next[0].floor() as i32,
                next[1].floor() as i32,
                next[2].floor() as i32,
            );
            let s = world.get_block(cell);
            let solid = b.collision(s).iter().any(|bb| {
                let l = [
                    next[0] - f64::from(cell.x),
                    next[1] - f64::from(cell.y),
                    next[2] - f64::from(cell.z),
                ];
                l[0] >= bb[0]
                    && l[0] <= bb[3]
                    && l[1] >= bb[1]
                    && l[1] <= bb[4]
                    && l[2] >= bb[2]
                    && l[2] <= bb[5]
            });
            if solid {
                p.vel = [p.vel[0] * 0.5, 0.0, p.vel[2] * 0.5];
            } else {
                p.pos = next;
            }
            p.vel[0] *= 0.98;
            p.vel[1] *= 0.98;
            p.vel[2] *= 0.98;
        }
        self.list.retain(|p| p.age < p.life);
    }
}

fn debris(
    pos: [f64; 3],
    vel: [f64; 3],
    sprite: [f32; 4],
    tint: [f32; 3],
    r: &mut impl rand::Rng,
) -> Particle {
    // A random quarter of the block's particle texture.
    let (du, dv) = ((sprite[2] - sprite[0]) / 4.0, (sprite[3] - sprite[1]) / 4.0);
    let (ox, oy) = (r.random_range(0..3) as f32, r.random_range(0..3) as f32);
    let u0 = sprite[0] + du * ox;
    let v0 = sprite[1] + dv * oy;
    Particle {
        pos,
        prev: pos,
        vel,
        age: 0,
        life: r.random_range(8..24),
        uv: [u0, v0, u0 + du, v0 + dv],
        color: [tint[0] * 0.6, tint[1] * 0.6, tint[2] * 0.6, 1.0],
        size: 0.1 * r.random_range(0.5..1.0),
        gravity: 0.04,
    }
}
