//! Entities: dropped items, falling blocks, experience orbs and mobs.
//!
//! Storage is a flat vector (entities are few and short-lived compared with
//! blocks); dropped items merge with nearby stacks and are capped per chunk
//! so a mined-out area never floods the simulation.

use ahash::AHashMap;
use rand::Rng as _;
use rc_core::{BlockPos, BlockStateId, ChunkPos};
use rc_physics::{Aabb, move_and_collide};
use serde::{Deserialize, Serialize};

use crate::items::Stack;
use crate::mobs::{Mob, MobKind};
use crate::world::World;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Kind {
    Item { stack: Stack, pickup_delay: u32 },
    Falling { state: u32 },
    Xp { value: u32 },
    Mob(Box<Mob>),
    Lightning,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entity {
    #[serde(skip)]
    pub id: u32,
    pub kind: Kind,
    pub pos: [f64; 3],
    #[serde(skip)]
    pub prev_pos: [f64; 3],
    pub vel: [f64; 3],
    pub yaw: f32,
    #[serde(skip)]
    pub prev_yaw: f32,
    pub on_ground: bool,
    pub age: u32,
    #[serde(skip)]
    pub dead: bool,
    /// Random phase for item bobbing.
    #[serde(default)]
    pub bob: f32,
}

/// Something the game must react to after an entity tick.
#[derive(Clone, Debug)]
pub enum Event {
    PickedUp {
        stack: Stack,
        pos: [f64; 3],
    },
    Xp {
        value: u32,
    },
    Landed {
        pos: BlockPos,
        state: BlockStateId,
    },
    Sound {
        event: String,
        pos: [f64; 3],
        volume: f32,
        pitch: f32,
    },
    Drop {
        stack: Stack,
        pos: [f64; 3],
    },
    SetBlock {
        pos: BlockPos,
        state: BlockStateId,
    },
    Spawn(Box<Entity>),
}

impl Entity {
    #[must_use]
    pub fn new(kind: Kind, pos: [f64; 3]) -> Self {
        Self {
            id: 0,
            kind,
            pos,
            prev_pos: pos,
            vel: [0.0; 3],
            yaw: 0.0,
            prev_yaw: 0.0,
            on_ground: false,
            age: 0,
            dead: false,
            bob: rand::rng().random_range(0.0..std::f32::consts::TAU),
        }
    }

    /// A dropped item with vanilla's small random toss.
    #[must_use]
    pub fn item(stack: Stack, pos: [f64; 3], delay: u32) -> Self {
        let mut r = rand::rng();
        let mut e = Self::new(
            Kind::Item {
                stack,
                pickup_delay: delay,
            },
            pos,
        );
        e.vel = [r.random_range(-0.1..0.1), 0.2, r.random_range(-0.1..0.1)];
        e.yaw = r.random_range(0.0..360.0);
        e
    }

    #[must_use]
    pub fn size(&self) -> (f64, f64) {
        match &self.kind {
            Kind::Item { .. } | Kind::Xp { .. } => (0.25, 0.25),
            Kind::Falling { .. } => (0.98, 0.98),
            Kind::Mob(m) => m.kind.size(m.baby),
            Kind::Lightning => (0.0, 0.0),
        }
    }

    #[must_use]
    pub fn aabb(&self) -> Aabb {
        let (w, h) = self.size();
        Aabb::at_feet(self.pos, w, h)
    }

    #[must_use]
    pub fn lerp_pos(&self, t: f64) -> [f64; 3] {
        std::array::from_fn(|k| self.prev_pos[k] + (self.pos[k] - self.prev_pos[k]) * t)
    }

    #[must_use]
    pub fn chunk(&self) -> ChunkPos {
        ChunkPos::new(
            (self.pos[0].floor() as i32) >> 4,
            (self.pos[2].floor() as i32) >> 4,
        )
    }

    fn physics(&mut self, world: &World, gravity: f64, drag: f64, scratch: &mut Vec<Aabb>) {
        let in_water = rc_data::blocks()
            .fluid(world.get_block(BlockPos::new(
                self.pos[0].floor() as i32,
                (self.pos[1] + 0.1).floor() as i32,
                self.pos[2].floor() as i32,
            )))
            .is_water();
        if in_water {
            // Items float up in water.
            self.vel[1] = (self.vel[1] + 0.005).min(0.06);
            self.vel[0] *= 0.99;
            self.vel[2] *= 0.99;
        } else {
            self.vel[1] -= gravity;
        }
        let bb = self.aabb();
        let r = move_and_collide(&bb, self.vel, 0.0, self.on_ground, world, scratch);
        for k in 0..3 {
            self.pos[k] += r.moved[k];
        }
        self.on_ground = r.on_ground;
        let slip = if self.on_ground { 0.6 * 0.98 } else { drag };
        self.vel[0] *= slip;
        self.vel[2] *= slip;
        self.vel[1] *= drag;
        if r.hit_y {
            self.vel[1] *= if self.on_ground { -0.5 } else { 0.0 };
            if self.vel[1].abs() < 0.02 {
                self.vel[1] = 0.0;
            }
        }
        if r.hit_x {
            self.vel[0] = 0.0;
        }
        if r.hit_z {
            self.vel[2] = 0.0;
        }
    }
}

/// All live entities.
#[derive(Debug, Default)]
pub struct Entities {
    pub list: Vec<Entity>,
    next_id: u32,
    scratch: Vec<Aabb>,
}

/// Item entities older than this despawn (5 minutes, as in vanilla).
const ITEM_LIFETIME: u32 = 6000;
/// Upper bound on dropped item entities per chunk (extra merge aggressively).
const MAX_ITEMS_PER_CHUNK: usize = 48;

impl Entities {
    pub fn spawn(&mut self, mut e: Entity) -> u32 {
        self.next_id += 1;
        e.id = self.next_id;
        e.prev_pos = e.pos;
        e.prev_yaw = e.yaw;
        self.list.push(e);
        self.next_id
    }

    #[must_use]
    pub fn mob_count(&self) -> usize {
        self.list
            .iter()
            .filter(|e| matches!(e.kind, Kind::Mob(_)))
            .count()
    }

    /// Advance one tick. `player` is the feet position and box.
    pub fn tick(
        &mut self,
        world: &World,
        player: &Aabb,
        player_alive: bool,
        tick: u64,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        let mut scratch = std::mem::take(&mut self.scratch);
        let pickup = player.inflate(1.0).expand_towards([0.0, 0.5, 0.0]);
        let player_pos = [
            (player.min[0] + player.max[0]) * 0.5,
            player.min[1],
            (player.min[2] + player.max[2]) * 0.5,
        ];
        let n = self.list.len();
        for i in 0..n {
            let e = &mut self.list[i];
            e.prev_pos = e.pos;
            e.prev_yaw = e.yaw;
            e.age += 1;
            // Entities in unloaded chunks freeze.
            if !world.is_loaded(BlockPos::new(
                e.pos[0].floor() as i32,
                0,
                e.pos[2].floor() as i32,
            )) {
                continue;
            }
            match &mut e.kind {
                Kind::Item {
                    pickup_delay,
                    stack,
                } => {
                    *pickup_delay = pickup_delay.saturating_sub(1);
                    let (delay, st) = (*pickup_delay, *stack);
                    if e.age > ITEM_LIFETIME || e.pos[1] < -128.0 {
                        e.dead = true;
                        continue;
                    }
                    // Resting items only re-run physics every 4 ticks.
                    let resting = e.on_ground && e.vel.iter().all(|v| v.abs() < 1e-3);
                    if !resting || (tick + u64::from(e.id)).is_multiple_of(4) {
                        e.physics(world, 0.04, 0.98, &mut scratch);
                    }
                    if player_alive && delay == 0 && e.aabb().intersects(&pickup) {
                        events.push(Event::PickedUp {
                            stack: st,
                            pos: e.pos,
                        });
                        e.dead = true;
                    }
                }
                Kind::Xp { value } => {
                    let v = *value;
                    // Orbs drift toward the player within 8 blocks.
                    let d = [
                        player_pos[0] - e.pos[0],
                        player_pos[1] + 0.9 - e.pos[1],
                        player_pos[2] - e.pos[2],
                    ];
                    let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    if dist < 8.0 && player_alive {
                        let k = (1.0 - dist / 8.0).powi(2) * 0.1;
                        for (j, dj) in d.iter().enumerate() {
                            e.vel[j] += dj / dist.max(0.1) * k;
                        }
                    }
                    e.physics(world, 0.03, 0.98, &mut scratch);
                    if player_alive && e.age > 10 && e.aabb().intersects(&pickup) {
                        events.push(Event::Xp { value: v });
                        e.dead = true;
                    }
                    if e.age > ITEM_LIFETIME {
                        e.dead = true;
                    }
                }
                Kind::Falling { state } => {
                    let s = BlockStateId(*state);
                    e.physics(world, 0.04, 0.98, &mut scratch);
                    if e.on_ground {
                        let p = BlockPos::new(
                            e.pos[0].floor() as i32,
                            (e.pos[1] + 0.05).floor() as i32,
                            e.pos[2].floor() as i32,
                        );
                        events.push(Event::Landed { pos: p, state: s });
                        e.dead = true;
                    } else if e.age > 600 || e.pos[1] < -128.0 {
                        e.dead = true;
                    }
                }
                Kind::Mob(_) => {
                    // Mob AI runs below with access to the whole list.
                }
                Kind::Lightning => {
                    if e.age > 8 {
                        e.dead = true;
                    }
                }
            }
        }
        // Mobs.
        let snapshot: Vec<(u32, [f64; 3], Option<(MobKind, bool, bool)>)> = self
            .list
            .iter()
            .map(|e| {
                (
                    e.id,
                    e.pos,
                    match &e.kind {
                        Kind::Mob(m) => Some((m.kind, m.baby, m.love > 0)),
                        _ => None,
                    },
                )
            })
            .collect();
        for i in 0..self.list.len() {
            if !matches!(self.list[i].kind, Kind::Mob(_)) || self.list[i].dead {
                continue;
            }
            let e = &mut self.list[i];
            if !world.is_loaded(BlockPos::new(
                e.pos[0].floor() as i32,
                0,
                e.pos[2].floor() as i32,
            )) {
                continue;
            }
            crate::mobs::tick(e, world, player_pos, &snapshot, &mut scratch, &mut events);
        }
        self.merge_items(tick);
        self.list.retain(|e| !e.dead);
        self.scratch = scratch;
        events
    }

    /// Merge nearby identical item stacks (vanilla merges within 0.5 blocks)
    /// and enforce the per-chunk cap.
    fn merge_items(&mut self, tick: u64) {
        if !tick.is_multiple_of(10) {
            return;
        }
        let mut by_chunk: AHashMap<ChunkPos, Vec<usize>> = AHashMap::new();
        for (i, e) in self.list.iter().enumerate() {
            if !e.dead && matches!(e.kind, Kind::Item { .. }) {
                by_chunk.entry(e.chunk()).or_default().push(i);
            }
        }
        for idxs in by_chunk.values() {
            let radius = if idxs.len() > MAX_ITEMS_PER_CHUNK {
                4.0
            } else {
                0.75
            };
            for a in 0..idxs.len() {
                for b in (a + 1)..idxs.len() {
                    let (ia, ib) = (idxs[a], idxs[b]);
                    if self.list[ia].dead || self.list[ib].dead {
                        continue;
                    }
                    let (pa, pb) = (self.list[ia].pos, self.list[ib].pos);
                    let d2 =
                        (pa[0] - pb[0]).powi(2) + (pa[1] - pb[1]).powi(2) + (pa[2] - pb[2]).powi(2);
                    if d2 > radius * radius {
                        continue;
                    }
                    let (Kind::Item { stack: sa, .. }, Kind::Item { stack: sb, .. }) =
                        (&self.list[ia].kind, &self.list[ib].kind)
                    else {
                        continue;
                    };
                    let (sa, sb) = (*sa, *sb);
                    if !sa.stacks_with(&sb)
                        || u32::from(sa.count) + u32::from(sb.count) > u32::from(sa.max())
                    {
                        continue;
                    }
                    if let Kind::Item {
                        stack,
                        pickup_delay,
                    } = &mut self.list[ia].kind
                    {
                        stack.count += sb.count;
                        let _ = pickup_delay;
                    }
                    self.list[ia].age = self.list[ia].age.min(self.list[ib].age);
                    self.list[ib].dead = true;
                }
            }
            // Hard cap: drop the oldest beyond the limit.
            let alive: Vec<usize> = idxs
                .iter()
                .copied()
                .filter(|&i| !self.list[i].dead)
                .collect();
            if alive.len() > MAX_ITEMS_PER_CHUNK * 2 {
                let mut by_age = alive;
                by_age.sort_by_key(|&i| std::cmp::Reverse(self.list[i].age));
                for &i in by_age.iter().take(by_age.len() - MAX_ITEMS_PER_CHUNK * 2) {
                    self.list[i].dead = true;
                }
            }
        }
    }

    /// Remove (and return) entities in chunks that are being unloaded.
    pub fn take_chunk(&mut self, c: ChunkPos) -> Vec<Entity> {
        let mut out = Vec::new();
        self.list.retain(|e| {
            if e.chunk() == c && !matches!(e.kind, Kind::Lightning) {
                out.push(e.clone());
                false
            } else {
                true
            }
        });
        out
    }

    /// Nearest mob hit by a ray (for attacking / interacting).
    #[must_use]
    pub fn raycast(&self, o: [f64; 3], d: [f64; 3], reach: f64) -> Option<(usize, f64)> {
        let mut best: Option<(usize, f64)> = None;
        for (i, e) in self.list.iter().enumerate() {
            if !matches!(e.kind, Kind::Mob(_)) || e.dead {
                continue;
            }
            if let Some((t, _, _)) = e.aabb().inflate(0.05).ray(o, d)
                && t <= reach
                && best.is_none_or(|b| t < b.1)
            {
                best = Some((i, t));
            }
        }
        best
    }
}
