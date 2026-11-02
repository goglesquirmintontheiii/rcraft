//! The local player: movement physics, health, and block targeting.

use rc_core::{BlockPos, BlockStateId};
use rc_physics::consts::*;
use rc_physics::{Aabb, Collider, move_and_collide};
use serde::{Deserialize, Serialize};

use crate::world::World;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GameMode {
    #[default]
    Survival,
    Creative,
    Spectator,
}

impl GameMode {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "survival" | "s" | "0" => Self::Survival,
            "creative" | "c" | "1" => Self::Creative,
            "spectator" | "sp" | "3" => Self::Spectator,
            _ => return None,
        })
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Survival => "survival",
            Self::Creative => "creative",
            Self::Spectator => "spectator",
        }
    }
}

/// Movement input for one tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct MoveInput {
    pub forward: f32,
    pub strafe: f32,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    pub pos: [f64; 3],
    #[serde(skip)]
    pub prev_pos: [f64; 3],
    pub vel: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub flying: bool,
    pub mode: GameMode,
    pub health: f32,
    pub food: u32,
    pub air: i32,
    pub fall_distance: f32,
    #[serde(default = "default_saturation")]
    pub saturation: f32,
    #[serde(default)]
    pub exhaustion: f32,
    #[serde(default)]
    pub xp_level: u32,
    #[serde(default)]
    pub xp_progress: f32,
    #[serde(default)]
    pub xp_total: u32,
    #[serde(skip)]
    pub eating: u32,
    #[serde(skip)]
    pub sleeping: u32,
    #[serde(skip)]
    pub food_timer: u32,
    #[serde(skip)]
    pub sneaking: bool,
    #[serde(skip)]
    pub sprinting: bool,
    #[serde(skip)]
    pub in_water: bool,
    #[serde(skip)]
    pub in_lava: bool,
    #[serde(skip)]
    pub eyes_in_water: bool,
    #[serde(skip)]
    pub hurt_time: u32,
    #[serde(skip)]
    pub walk_dist: f32,
    #[serde(skip)]
    pub prev_walk_dist: f32,
    #[serde(skip)]
    pub bob: f32,
    #[serde(skip)]
    pub prev_bob: f32,
    #[serde(skip)]
    pub eye_height: f64,
    #[serde(skip)]
    pub prev_eye_height: f64,
    #[serde(skip)]
    pub last_damage_cause: String,
    #[serde(skip)]
    scratch: Vec<Aabb>,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            pos: [0.5, 100.0, 0.5],
            prev_pos: [0.5, 100.0, 0.5],
            vel: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            flying: false,
            mode: GameMode::Survival,
            health: 20.0,
            food: 20,
            air: 300,
            fall_distance: 0.0,
            saturation: 5.0,
            exhaustion: 0.0,
            xp_level: 0,
            xp_progress: 0.0,
            xp_total: 0,
            eating: 0,
            sleeping: 0,
            food_timer: 0,
            sneaking: false,
            sprinting: false,
            in_water: false,
            in_lava: false,
            eyes_in_water: false,
            hurt_time: 0,
            walk_dist: 0.0,
            prev_walk_dist: 0.0,
            bob: 0.0,
            prev_bob: 0.0,
            eye_height: EYE,
            prev_eye_height: EYE,
            last_damage_cause: String::new(),
            scratch: Vec::new(),
        }
    }
}

fn default_saturation() -> f32 {
    5.0
}

fn slipperiness(s: BlockStateId) -> f64 {
    match rc_data::blocks().def(s).name.as_str() {
        "ice" | "packed_ice" | "frosted_ice" => 0.98,
        "blue_ice" => 0.989,
        "slime_block" => 0.8,
        _ => DEFAULT_SLIP,
    }
}

/// A block hit by the crosshair.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub pos: BlockPos,
    /// Face normal of the hit side.
    pub normal: [i32; 3],
    /// Hit point relative to the block (0..1).
    pub local: [f64; 3],
}

/// Outline/selection boxes for a block state.
#[must_use]
pub fn outline_boxes(s: BlockStateId) -> Vec<[f64; 6]> {
    let b = rc_data::blocks();
    if s == BlockStateId::AIR || b.def(s).name.ends_with("air") {
        return Vec::new();
    }
    let f = b.fluid(s);
    let col = b.collision(s);
    if !col.is_empty() {
        return col.to_vec();
    }
    if !f.is_empty() && b.def(s).name == if f.is_water() { "water" } else { "lava" } {
        return Vec::new();
    }
    let name = b.def(s).name.as_str();
    if name == "snow" {
        return vec![[0.0, 0.0, 0.0, 1.0, 0.125, 1.0]];
    }
    if name.contains("carpet")
        || name.contains("pressure_plate")
        || name == "lily_pad"
        || name.ends_with("petals")
    {
        return vec![[0.0, 0.0, 0.0, 1.0, 0.0625, 1.0]];
    }
    vec![[0.125, 0.0, 0.125, 0.875, 0.8125, 0.875]]
}

impl Player {
    #[must_use]
    pub fn height(&self) -> f64 {
        if self.sneaking && !self.flying {
            PLAYER_H_SNEAK
        } else {
            PLAYER_H
        }
    }

    #[must_use]
    pub fn aabb(&self) -> Aabb {
        Aabb::at_feet(self.pos, PLAYER_W, self.height())
    }

    #[must_use]
    pub fn eye(&self, partial: f64) -> [f64; 3] {
        let p = self.lerp_pos(partial);
        let eh = self.prev_eye_height + (self.eye_height - self.prev_eye_height) * partial;
        [p[0], p[1] + eh, p[2]]
    }

    #[must_use]
    pub fn lerp_pos(&self, t: f64) -> [f64; 3] {
        [
            self.prev_pos[0] + (self.pos[0] - self.prev_pos[0]) * t,
            self.prev_pos[1] + (self.pos[1] - self.prev_pos[1]) * t,
            self.prev_pos[2] + (self.pos[2] - self.prev_pos[2]) * t,
        ]
    }

    #[must_use]
    pub fn look_dir(&self) -> [f64; 3] {
        let (sy, cy) = f64::from(self.yaw).to_radians().sin_cos();
        let (sp, cp) = f64::from(self.pitch).to_radians().sin_cos();
        [-sy * cp, -sp, cy * cp]
    }

    pub fn damage(&mut self, amount: f32, cause: &str) {
        if self.mode != GameMode::Survival || self.hurt_time > 0 || self.health <= 0.0 {
            return;
        }
        self.health = (self.health - amount).max(0.0);
        self.hurt_time = 10;
        self.last_damage_cause = cause.to_owned();
    }

    #[must_use]
    pub fn dead(&self) -> bool {
        self.health <= 0.0
    }

    fn fluid_at(world: &World, bb: &Aabb) -> (bool, bool) {
        let b = rc_data::blocks();
        let (mut water, mut lava) = (false, false);
        for x in bb.min[0].floor() as i32..=bb.max[0].floor() as i32 {
            for y in bb.min[1].floor() as i32..=bb.max[1].floor() as i32 {
                for z in bb.min[2].floor() as i32..=bb.max[2].floor() as i32 {
                    let f = b.fluid(world.get_block(BlockPos::new(x, y, z)));
                    if f.is_empty() {
                        continue;
                    }
                    let top = f64::from(y) + f64::from(f.own_height());
                    if bb.min[1] < top {
                        water |= f.is_water();
                        lava |= f.is_lava();
                    }
                }
            }
        }
        (water, lava)
    }

    /// Advance one game tick (1/20 s).
    pub fn tick(&mut self, input: &MoveInput, world: &World) {
        self.prev_pos = self.pos;
        self.prev_walk_dist = self.walk_dist;
        self.prev_bob = self.bob;
        self.prev_eye_height = self.eye_height;
        if self.hurt_time > 0 {
            self.hurt_time -= 1;
        }
        if self.mode != GameMode::Survival {
            self.health = 20.0;
            self.air = 300;
            if self.mode == GameMode::Spectator {
                self.flying = true;
            }
        }
        if self.mode == GameMode::Survival && self.flying {
            self.flying = false;
        }
        let bb = self.aabb();
        let (w, l) = Self::fluid_at(world, &bb);
        self.in_water = w;
        self.in_lava = l;
        let eye = [
            self.pos[0],
            self.pos[1] + self.eye_height - 0.11,
            self.pos[2],
        ];
        let ef = rc_data::blocks().fluid(world.get_block(BlockPos::new(
            eye[0].floor() as i32,
            eye[1].floor() as i32,
            eye[2].floor() as i32,
        )));
        self.eyes_in_water =
            ef.is_water() && eye[1] < eye[1].floor() + f64::from(ef.own_height()) + 0.01;
        self.sneaking = input.sneak && !self.flying;
        let target_eye = if self.sneaking { EYE_SNEAK } else { EYE };
        self.eye_height += (target_eye - self.eye_height) * 0.5;
        if input.sprint
            && input.forward > 0.0
            && !self.sneaking
            && (self.food > 6 || self.mode != GameMode::Survival)
        {
            self.sprinting = true;
        }
        if input.forward <= 0.0 || self.sneaking {
            self.sprinting = false;
        }

        // Vanilla damps raw input by 0.98 before applying it.
        let mut fwd = f64::from(input.forward) * 0.98;
        let mut strafe = f64::from(input.strafe) * 0.98;
        if self.sneaking {
            fwd *= SNEAK_MULT;
            strafe *= SNEAK_MULT;
        }
        let sprint = if self.sprinting { SPRINT_MULT } else { 1.0 };
        let noclip = self.mode == GameMode::Spectator;

        if self.flying {
            let fly = FLY_SPEED * if self.sprinting { 2.0 } else { 1.0 };
            let vy =
                (f64::from(u8::from(input.jump)) - f64::from(u8::from(input.sneak))) * fly * 3.0;
            self.vel[1] += vy;
            self.move_relative(fwd, strafe, fly * if noclip { 2.0 } else { 1.0 });
            self.do_move(world, noclip);
            self.vel[0] *= AIR_FRICTION;
            self.vel[2] *= AIR_FRICTION;
            self.vel[1] *= 0.6;
            if self.on_ground && self.mode == GameMode::Creative {
                self.flying = false;
            }
            self.fall_distance = 0.0;
        } else if self.in_water || self.in_lava {
            let y0 = self.pos[1];
            self.move_relative(fwd, strafe, 0.02 * if self.sprinting { 1.5 } else { 1.0 });
            self.do_move(world, false);
            let drag = if self.in_water { 0.8 } else { 0.5 };
            self.vel[0] *= drag;
            self.vel[1] *= 0.8;
            self.vel[2] *= drag;
            self.vel[1] -= 0.02;
            if input.jump {
                self.vel[1] += 0.04;
            }
            if input.sneak {
                self.vel[1] -= 0.02;
            }
            // Climb out onto ledges.
            let probe = self.aabb().offset([
                self.vel[0],
                self.vel[1] + 0.6 - self.pos[1] + y0,
                self.vel[2],
            ]);
            self.scratch.clear();
            world.boxes(&probe, &mut self.scratch);
            let blocked_h = self.scratch.iter().any(|b| b.intersects(&probe));
            if (self.vel[0].abs() > 1e-4 || self.vel[2].abs() > 1e-4)
                && !blocked_h
                && self.hit_horizontal(world)
            {
                self.vel[1] = 0.3;
            }
            self.fall_distance = 0.0;
        } else {
            let below = world.get_block(BlockPos::new(
                self.pos[0].floor() as i32,
                (self.pos[1] - 0.5).floor() as i32,
                self.pos[2].floor() as i32,
            ));
            let slip = if self.on_ground {
                slipperiness(below)
            } else {
                1.0
            };
            let f = slip * AIR_FRICTION;
            let accel = if self.on_ground {
                WALK_SPEED * sprint * (0.216_000_02 / (slip * slip * slip))
            } else {
                AIR_ACCEL * sprint
            };
            if input.jump && self.on_ground {
                self.vel[1] = JUMP;
                self.exhaustion += if self.sprinting { 0.2 } else { 0.05 };
                if self.sprinting {
                    let y = f64::from(self.yaw).to_radians();
                    self.vel[0] -= y.sin() * 0.2;
                    self.vel[2] += y.cos() * 0.2;
                }
            }
            self.move_relative(fwd, strafe, accel);
            // Ladders, vines and scaffolding (minecraft.wiki "Ladder"):
            // horizontal speed and descent capped at 0.15 b/t, sneaking holds
            // position, pushing into a wall or jumping climbs at 0.2 b/t.
            let feet = world.get_block(BlockPos::new(
                self.pos[0].floor() as i32,
                self.pos[1].floor() as i32,
                self.pos[2].floor() as i32,
            ));
            let climbing = !self.flying && climbable(feet);
            if climbing {
                self.vel[0] = self.vel[0].clamp(-0.15, 0.15);
                self.vel[2] = self.vel[2].clamp(-0.15, 0.15);
                self.vel[1] = self.vel[1].max(-0.15);
                if input.sneak && self.vel[1] < 0.0 {
                    self.vel[1] = 0.0;
                }
                self.fall_distance = 0.0;
            }
            let pushing = climbing && self.hit_horizontal(world);
            self.do_move(world, false);
            if climbing && (pushing || input.jump) {
                self.vel[1] = 0.2;
            }
            self.vel[1] -= GRAVITY;
            self.vel[1] *= DRAG_Y;
            self.vel[0] *= f;
            self.vel[2] *= f;
        }
        // Walk distance for view bobbing.
        let dx = self.pos[0] - self.prev_pos[0];
        let dz = self.pos[2] - self.prev_pos[2];
        let d = (dx * dx + dz * dz).sqrt() as f32;
        self.walk_dist += d * 0.6;
        let target_bob = if self.on_ground && !self.flying {
            d.min(0.1)
        } else {
            0.0
        };
        self.bob += (target_bob - self.bob) * 0.4;

        self.survival_tick(world);
    }

    fn hit_horizontal(&mut self, world: &World) -> bool {
        let probe = self
            .aabb()
            .offset([self.vel[0] * 2.0, 0.0, self.vel[2] * 2.0]);
        self.scratch.clear();
        world.boxes(&probe, &mut self.scratch);
        self.scratch.iter().any(|b| b.intersects(&probe))
    }

    fn move_relative(&mut self, fwd: f64, strafe: f64, speed: f64) {
        let mut len = fwd * fwd + strafe * strafe;
        if len < 1e-7 {
            return;
        }
        len = len.sqrt().max(1.0);
        let (f, s) = (fwd / len * speed, strafe / len * speed);
        let (sy, cy) = f64::from(self.yaw).to_radians().sin_cos();
        self.vel[0] += s * cy - f * sy;
        self.vel[2] += f * cy + s * sy;
    }

    fn do_move(&mut self, world: &World, noclip: bool) {
        if noclip {
            for k in 0..3 {
                self.pos[k] += self.vel[k];
            }
            self.on_ground = false;
            return;
        }
        let mut d = self.vel;
        // Sneaking keeps you on edges.
        if self.sneaking && self.on_ground {
            let step = 0.05;
            let support = |pl: &mut Player, ox: f64, oz: f64| -> bool {
                let probe = pl.aabb().offset([ox, -0.6, oz]);
                pl.scratch.clear();
                world.boxes(&probe, &mut pl.scratch);
                pl.scratch.iter().any(|b| b.intersects(&probe))
            };
            while d[0] != 0.0 && !support(self, d[0], 0.0) {
                d[0] = if d[0].abs() < step {
                    0.0
                } else {
                    d[0] - step * d[0].signum()
                };
            }
            while d[2] != 0.0 && !support(self, 0.0, d[2]) {
                d[2] = if d[2].abs() < step {
                    0.0
                } else {
                    d[2] - step * d[2].signum()
                };
            }
            while d[0] != 0.0 && d[2] != 0.0 && !support(self, d[0], d[2]) {
                d[0] = if d[0].abs() < step {
                    0.0
                } else {
                    d[0] - step * d[0].signum()
                };
                d[2] = if d[2].abs() < step {
                    0.0
                } else {
                    d[2] - step * d[2].signum()
                };
            }
        }
        let bb = self.aabb();
        let r = move_and_collide(&bb, d, STEP, self.on_ground, world, &mut self.scratch);
        for k in 0..3 {
            self.pos[k] += r.moved[k];
        }
        if r.hit_x {
            self.vel[0] = 0.0;
        }
        if r.hit_z {
            self.vel[2] = 0.0;
        }
        let was_air = !self.on_ground;
        self.on_ground = r.on_ground;
        if r.hit_y {
            self.vel[1] = 0.0;
        }
        if self.on_ground {
            if was_air && self.fall_distance > 3.0 && !self.in_water {
                let dmg = (self.fall_distance - 3.0).ceil();
                self.damage(dmg, "fell from a high place");
            }
            self.fall_distance = 0.0;
        } else if r.moved[1] < 0.0 {
            self.fall_distance -= r.moved[1] as f32;
        }
    }

    fn survival_tick(&mut self, _world: &World) {
        if self.mode != GameMode::Survival {
            return;
        }
        if self.eyes_in_water {
            self.air -= 1;
            if self.air <= -20 {
                self.air = 0;
                self.hurt_time = 0;
                self.damage(2.0, "drowned");
            }
        } else if self.air < 300 {
            self.air = (self.air + 4).min(300);
        }
        if self.in_lava {
            self.damage(4.0, "tried to swim in lava");
        }
        if self.pos[1] < -128.0 {
            self.hurt_time = 0;
            self.damage(4.0, "fell out of the world");
        }
        // Hunger (minecraft.wiki "Hunger"): exhaustion drains saturation,
        // then food; full food heals, empty food starves.
        let moved = ((self.pos[0] - self.prev_pos[0]).powi(2)
            + (self.pos[2] - self.prev_pos[2]).powi(2))
        .sqrt() as f32;
        if self.sprinting {
            self.exhaustion += 0.1 * moved;
        } else if self.in_water && self.eyes_in_water {
            self.exhaustion += 0.01 * moved;
        }
        while self.exhaustion >= 4.0 {
            self.exhaustion -= 4.0;
            if self.saturation > 0.0 {
                self.saturation = (self.saturation - 1.0).max(0.0);
            } else {
                self.food = self.food.saturating_sub(1);
            }
        }
        self.food_timer += 1;
        if self.food >= 20 && self.saturation > 0.0 && self.health < 20.0 {
            if self.food_timer >= 10 {
                let k = self.saturation.min(6.0);
                self.health = (self.health + k / 6.0).min(20.0);
                self.exhaustion += k;
                self.saturation -= k.min(self.saturation);
                self.food_timer = 0;
            }
        } else if self.food >= 18 && self.health < 20.0 {
            if self.food_timer >= 80 {
                self.health = (self.health + 1.0).min(20.0);
                self.exhaustion += 6.0;
                self.food_timer = 0;
            }
        } else if self.food == 0 {
            if self.food_timer >= 80 {
                if self.health > 10.0 {
                    self.hurt_time = 0;
                    self.damage(1.0, "starved to death");
                }
                self.food_timer = 0;
            }
        } else {
            self.food_timer = 0;
        }
    }

    /// Eat food: (nutrition, saturation modifier as in the item component).
    pub fn eat(&mut self, nutrition: u8, saturation: f32) {
        self.food = (self.food + u32::from(nutrition)).min(20);
        self.saturation = (self.saturation + saturation).min(self.food as f32);
    }

    /// Add experience points, levelling up per vanilla's curve.
    pub fn add_xp(&mut self, points: u32) {
        self.xp_total += points;
        let mut p = self.xp_progress * Self::xp_needed(self.xp_level) as f32 + points as f32;
        while p >= Self::xp_needed(self.xp_level) as f32 {
            p -= Self::xp_needed(self.xp_level) as f32;
            self.xp_level += 1;
        }
        self.xp_progress = p / Self::xp_needed(self.xp_level) as f32;
    }

    #[must_use]
    pub fn xp_needed(level: u32) -> u32 {
        if level >= 30 {
            112 + (level - 30) * 9
        } else if level >= 15 {
            37 + (level - 15) * 5
        } else {
            7 + level * 2
        }
    }

    /// Ray-cast through the voxel grid to the first targetable block.
    #[must_use]
    pub fn raycast(&self, world: &World, reach: f64, partial: f64) -> Option<Hit> {
        let o = self.eye(partial);
        let d = self.look_dir();
        let mut cell = [
            o[0].floor() as i32,
            o[1].floor() as i32,
            o[2].floor() as i32,
        ];
        let step: [i32; 3] = std::array::from_fn(|k| if d[k] > 0.0 { 1 } else { -1 });
        let t_delta: [f64; 3] = std::array::from_fn(|k| {
            if d[k] == 0.0 {
                f64::INFINITY
            } else {
                (1.0 / d[k]).abs()
            }
        });
        let mut t_max: [f64; 3] = std::array::from_fn(|k| {
            if d[k] == 0.0 {
                f64::INFINITY
            } else {
                let next = if d[k] > 0.0 {
                    f64::from(cell[k]) + 1.0
                } else {
                    f64::from(cell[k])
                };
                (next - o[k]) / d[k]
            }
        });
        let mut t = 0.0;
        while t <= reach {
            let p = BlockPos::new(cell[0], cell[1], cell[2]);
            let s = world.get_block(p);
            if s != BlockStateId::AIR {
                let mut best: Option<(f64, usize, bool)> = None;
                for b in outline_boxes(s) {
                    let bb = Aabb::new(
                        [
                            f64::from(p.x) + b[0],
                            f64::from(p.y) + b[1],
                            f64::from(p.z) + b[2],
                        ],
                        [
                            f64::from(p.x) + b[3],
                            f64::from(p.y) + b[4],
                            f64::from(p.z) + b[5],
                        ],
                    );
                    if let Some(h) = bb.ray(o, d)
                        && best.is_none_or(|b| h.0 < b.0)
                    {
                        best = Some(h);
                    }
                }
                if let Some((th, axis, _)) = best
                    && th <= reach
                {
                    let mut normal = [0; 3];
                    normal[axis] = if d[axis] > 0.0 { -1 } else { 1 };
                    let hp = [o[0] + d[0] * th, o[1] + d[1] * th, o[2] + d[2] * th];
                    return Some(Hit {
                        pos: p,
                        normal,
                        local: [
                            hp[0] - f64::from(p.x),
                            hp[1] - f64::from(p.y),
                            hp[2] - f64::from(p.z),
                        ],
                    });
                }
            }
            let k = if t_max[0] < t_max[1] {
                if t_max[0] < t_max[2] { 0 } else { 2 }
            } else if t_max[1] < t_max[2] {
                1
            } else {
                2
            };
            t = t_max[k];
            t_max[k] += t_delta[k];
            cell[k] += step[k];
        }
        None
    }
}

/// Blocks the player can climb (the `#climbable` tag's members).
fn climbable(s: BlockStateId) -> bool {
    let n = &rc_data::blocks().def(s).name;
    matches!(
        n.as_str(),
        "ladder"
            | "vine"
            | "scaffolding"
            | "weeping_vines"
            | "weeping_vines_plant"
            | "twisting_vines"
            | "twisting_vines_plant"
            | "cave_vines"
            | "cave_vines_plant"
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rc_core::ChunkPos;

    use super::*;
    use crate::world::{MeshShared, World, biome_colors};

    fn flat_world() -> World {
        let dir = std::env::temp_dir().join(format!(
            "rc-phys-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let models = Arc::new(rc_assets::fallback::block_models(rc_data::blocks(), 1));
        let mesh = Arc::new(MeshShared {
            tables: rc_render::mesh::MeshTables::new(&models, true),
            models,
            colors: biome_colors(None, None),
            smooth: true,
        });
        let mut w = World::open(&dir, "t", 1, 1, mesh).unwrap();
        let stone = rc_data::blocks().default_state("stone");
        for cx in -2..=2 {
            for cz in -2..=2 {
                let mut c = rc_world::Chunk::new_empty(ChunkPos::new(cx, cz));
                for x in 0..16 {
                    for z in 0..16 {
                        for y in 60..64 {
                            c.set_block(BlockPos::new(cx * 16 + x, y, cz * 16 + z), stone);
                        }
                    }
                }
                // A one-block wall to step against at z = 5.
                for x in 0..16 {
                    if cz == 0 {
                        c.set_block(BlockPos::new(cx * 16 + x, 64, 5), stone);
                    }
                }
                w.chunks.insert(c.pos, Arc::new(c));
            }
        }
        w
    }

    #[test]
    fn falls_lands_walks_and_jumps() {
        let w = flat_world();
        // Food below 18 and no saturation: no natural regeneration.
        let mut p = Player {
            pos: [0.5, 70.0, 0.5],
            food: 17,
            saturation: 0.0,
            ..Player::default()
        };
        let idle = MoveInput::default();
        for _ in 0..60 {
            p.tick(&idle, &w);
        }
        assert!(p.on_ground, "{:?}", p.pos);
        assert!((p.pos[1] - 64.0).abs() < 1e-6, "{:?}", p.pos);
        // Fall of 6 blocks: 3 damage.
        assert!((p.health - 17.0).abs() < 1e-6, "health {}", p.health);
        // Walk forward (+Z at yaw 0) into the wall at z = 5.
        let fwd = MoveInput {
            forward: 1.0,
            ..MoveInput::default()
        };
        for _ in 0..40 {
            p.tick(&fwd, &w);
        }
        assert!(p.pos[2] > 4.0 && p.pos[2] < 4.71, "{:?}", p.pos);
        // Walking speed ~4.3 m/s: from z 0.5 the wall stops us within 1 s.
        // Jump over it.
        let jump = MoveInput {
            forward: 1.0,
            jump: true,
            ..MoveInput::default()
        };
        for _ in 0..30 {
            p.tick(&jump, &w);
        }
        assert!(p.pos[2] > 6.0, "{:?}", p.pos);
        assert!(p.pos[1] >= 64.0);
    }

    #[test]
    fn walking_speed_matches_vanilla() {
        let w = flat_world();
        let mut p = Player {
            pos: [0.5, 64.0, -20.5],
            on_ground: true,
            ..Player::default()
        };
        let fwd = MoveInput {
            forward: 1.0,
            ..MoveInput::default()
        };
        for _ in 0..20 {
            p.tick(&fwd, &w);
        }
        let z0 = p.pos[2];
        for _ in 0..20 {
            p.tick(&fwd, &w);
        }
        let speed = p.pos[2] - z0;
        // Vanilla walking speed is ~4.317 blocks/s.
        assert!((speed - 4.317).abs() < 0.05, "speed {speed}");
    }
}
