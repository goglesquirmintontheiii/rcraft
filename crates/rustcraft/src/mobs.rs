//! Passive mobs: cow, pig, sheep and chicken, with vanilla-style goals
//! (panic, breed, tempt, follow parent, wander, look at player), growth,
//! drops and natural spawning at world generation.

use rand::Rng as _;
use rc_core::BlockPos;
use rc_physics::{Aabb, Collider as _, move_and_collide};
use serde::{Deserialize, Serialize};

use crate::entity::{Entity, Event, Kind};
use crate::world::World;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MobKind {
    Cow,
    Pig,
    Sheep,
    Chicken,
}

impl MobKind {
    pub const ALL: [MobKind; 4] = [MobKind::Cow, MobKind::Pig, MobKind::Sheep, MobKind::Chicken];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Cow => "cow",
            Self::Pig => "pig",
            Self::Sheep => "sheep",
            Self::Chicken => "chicken",
        }
    }

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|k| k.name() == s.trim_start_matches("minecraft:"))
    }

    /// (width, height) from the entity table.
    #[must_use]
    pub fn size(self, baby: bool) -> (f64, f64) {
        let (w, h) = match self {
            Self::Cow => (0.9, 1.4),
            Self::Pig => (0.9, 0.9),
            Self::Sheep => (0.9, 1.3),
            Self::Chicken => (0.4, 0.7),
        };
        if baby { (w * 0.5, h * 0.5) } else { (w, h) }
    }

    #[must_use]
    pub fn max_health(self) -> f32 {
        match self {
            Self::Cow | Self::Pig => 10.0,
            Self::Sheep => 8.0,
            Self::Chicken => 4.0,
        }
    }

    /// Ground speed attribute.
    #[must_use]
    pub fn speed(self) -> f64 {
        match self {
            Self::Cow => 0.2,
            Self::Pig | Self::Chicken => 0.25,
            Self::Sheep => 0.23,
        }
    }

    /// Items this mob follows and breeds with.
    #[must_use]
    pub fn food(self) -> &'static [&'static str] {
        match self {
            Self::Cow | Self::Sheep => &["wheat"],
            Self::Pig => &["carrot", "potato", "beetroot"],
            Self::Chicken => &[
                "wheat_seeds",
                "melon_seeds",
                "pumpkin_seeds",
                "beetroot_seeds",
                "torchflower_seeds",
                "pitcher_pod",
            ],
        }
    }
}

/// Wool colours (sheep).
pub const DYES: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

/// Wool tint for the fur layer (vanilla dye texture diffuse colours).
#[must_use]
pub fn dye_rgb(c: u8) -> [f32; 3] {
    let v: u32 = match c {
        0 => 0xF9FFFE,
        1 => 0xF9801D,
        2 => 0xC74EBD,
        3 => 0x3AB3DA,
        4 => 0xFED83D,
        5 => 0x80C71F,
        6 => 0xF38BAA,
        7 => 0x474F52,
        8 => 0x9D9D97,
        9 => 0x169C9C,
        10 => 0x8932B8,
        11 => 0x3C44AA,
        12 => 0x835432,
        13 => 0x5E7C16,
        14 => 0xB02E26,
        _ => 0x1D1D21,
    };
    [
        ((v >> 16) & 255) as f32 / 255.0,
        ((v >> 8) & 255) as f32 / 255.0,
        (v & 255) as f32 / 255.0,
    ]
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mob {
    pub kind: MobKind,
    pub health: f32,
    pub baby: bool,
    /// Ticks until adulthood (babies) or until breeding is allowed again.
    pub age: i32,
    pub love: u32,
    #[serde(skip)]
    pub panic: u32,
    #[serde(skip)]
    pub target: Option<[f64; 3]>,
    #[serde(skip)]
    pub head_yaw: f32,
    #[serde(skip)]
    pub head_pitch: f32,
    #[serde(skip)]
    pub limb_swing: f32,
    #[serde(skip)]
    pub limb_amount: f32,
    #[serde(skip)]
    pub prev_limb_amount: f32,
    #[serde(skip)]
    pub hurt_time: u32,
    #[serde(skip)]
    pub death_time: u32,
    #[serde(skip)]
    pub look_ticks: u32,
    #[serde(skip)]
    pub fall: f32,
    #[serde(default)]
    pub sheared: bool,
    #[serde(default)]
    pub color: u8,
    #[serde(default)]
    pub egg_timer: u32,
    #[serde(skip)]
    pub eat_timer: u32,
    #[serde(skip)]
    pub breed_partner_close: u32,
}

impl Mob {
    #[must_use]
    pub fn new(kind: MobKind, baby: bool) -> Self {
        let mut r = rand::rng();
        // Natural sheep colours (minecraft.wiki "Sheep").
        let color = {
            let x: f32 = r.random();
            if x < 0.05 {
                15
            } else if x < 0.10 {
                7
            } else if x < 0.15 {
                8
            } else if x < 0.18 {
                12
            } else if x < 0.181 {
                6
            } else {
                0
            }
        };
        Self {
            kind,
            health: kind.max_health(),
            baby,
            age: if baby { 24000 } else { 0 },
            love: 0,
            panic: 0,
            target: None,
            head_yaw: 0.0,
            head_pitch: 0.0,
            limb_swing: 0.0,
            limb_amount: 0.0,
            prev_limb_amount: 0.0,
            hurt_time: 0,
            death_time: 0,
            look_ticks: 0,
            fall: 0.0,
            sheared: false,
            color,
            egg_timer: r.random_range(6000..12000),
            eat_timer: 0,
            breed_partner_close: 0,
        }
    }
}

/// A new mob entity at `pos`.
#[must_use]
pub fn spawn(kind: MobKind, pos: [f64; 3], baby: bool) -> Entity {
    let mut e = Entity::new(Kind::Mob(Box::new(Mob::new(kind, baby))), pos);
    e.yaw = rand::rng().random_range(0.0..360.0);
    e
}

fn sound(events: &mut Vec<Event>, kind: MobKind, what: &str, pos: [f64; 3], baby: bool) {
    let mut r = rand::rng();
    events.push(Event::Sound {
        event: format!("entity.{}.{what}", kind.name()),
        pos,
        volume: if kind == MobKind::Cow { 0.4 } else { 1.0 },
        pitch: (r.random::<f32>() - r.random::<f32>()) * 0.2 + if baby { 1.5 } else { 1.0 },
    });
}

fn wrap(a: f32) -> f32 {
    (a + 180.0).rem_euclid(360.0) - 180.0
}

/// Per-tick context about the player.
#[derive(Clone, Copy, Debug)]
pub struct PlayerInfo {
    pub pos: [f64; 3],
    /// Item the player holds, if any.
    pub held: Option<u16>,
}

thread_local! {
    pub static PLAYER: std::cell::Cell<PlayerInfo> = const { std::cell::Cell::new(PlayerInfo { pos: [0.0; 3], held: None }) };
}

/// Advance one mob.
pub fn tick(
    e: &mut Entity,
    world: &World,
    _player_pos: [f64; 3],
    others: &[(u32, [f64; 3], Option<(MobKind, bool, bool)>)],
    scratch: &mut Vec<Aabb>,
    events: &mut Vec<Event>,
) {
    let player = PLAYER.with(std::cell::Cell::get);
    let id = e.id;
    let pos = e.pos;
    let Kind::Mob(m) = &mut e.kind else { return };
    let mut r = rand::rng();
    m.prev_limb_amount = m.limb_amount;
    if m.death_time > 0 {
        m.death_time += 1;
        if m.death_time >= 20 {
            e.dead = true;
            for st in crate::loot::entity_drops(m.kind.name()) {
                events.push(Event::Drop { stack: st, pos });
            }
            if m.kind == MobKind::Sheep
                && !m.sheared
                && let Some(w) =
                    crate::items::items().id(&format!("{}_wool", DYES[m.color as usize % 16]))
            {
                events.push(Event::Drop {
                    stack: crate::items::Stack::new(w, 1),
                    pos,
                });
            }
            if !m.baby {
                events.push(Event::Spawn(Box::new(Entity::new(
                    Kind::Xp {
                        value: r.random_range(1..=3),
                    },
                    pos,
                ))));
            }
        }
        return;
    }
    m.hurt_time = m.hurt_time.saturating_sub(1);
    m.panic = m.panic.saturating_sub(1);
    m.love = m.love.saturating_sub(1);
    if m.baby {
        m.age -= 1;
        if m.age <= 0 {
            m.baby = false;
            m.age = 0;
        }
    } else if m.age > 0 {
        m.age -= 1;
    }
    let kind = m.kind;
    let food_ids: Vec<u16> = kind
        .food()
        .iter()
        .filter_map(|n| crate::items::items().id(n))
        .collect();

    // --- goal selection ---------------------------------------------------
    let mut speed_mod = 1.0;
    let mut goal: Option<[f64; 3]> = None;
    let dist2 = |a: [f64; 3], b: [f64; 3]| {
        (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
    };
    if m.panic > 0 {
        speed_mod = if kind == MobKind::Chicken { 1.4 } else { 2.0 };
        if m.target.is_none() || m.panic % 20 == 0 {
            m.target = Some([
                pos[0] + r.random_range(-5.0..5.0),
                pos[1],
                pos[2] + r.random_range(-5.0..5.0),
            ]);
        }
        goal = m.target;
    } else if m.love > 0 {
        // Find a partner of the same kind in love.
        let partner = others
            .iter()
            .filter(|o| o.0 != id && o.2.is_some_and(|(k, b, l)| k == kind && !b && l))
            .min_by(|a, b| dist2(a.1, pos).total_cmp(&dist2(b.1, pos)));
        if let Some(p) = partner
            && dist2(p.1, pos) < 64.0
        {
            goal = Some(p.1);
            if dist2(p.1, pos) < 9.0 {
                m.breed_partner_close += 1;
                // The lower id spawns the baby so it happens once.
                if m.breed_partner_close >= 60 && id < p.0 {
                    m.love = 0;
                    m.age = 6000;
                    events.push(Event::Spawn(Box::new(spawn(kind, pos, true))));
                    events.push(Event::Spawn(Box::new(Entity::new(
                        Kind::Xp {
                            value: r.random_range(1..=7),
                        },
                        pos,
                    ))));
                }
            }
        }
    }
    if goal.is_none()
        && let Some(h) = player.held
        && food_ids.contains(&h)
        && dist2(player.pos, pos) < 100.0
    {
        speed_mod = if kind == MobKind::Chicken { 1.0 } else { 1.25 };
        if dist2(player.pos, pos) > 6.25 {
            goal = Some(player.pos);
        }
        m.look_ticks = 2;
    }
    if goal.is_none() && m.baby {
        let parent = others
            .iter()
            .filter(|o| o.2.is_some_and(|(k, b, _)| k == kind && !b))
            .min_by(|a, b| dist2(a.1, pos).total_cmp(&dist2(b.1, pos)));
        if let Some(p) = parent
            && (9.0..64.0).contains(&dist2(p.1, pos))
        {
            goal = Some(p.1);
            speed_mod = 1.1;
        }
    }
    if goal.is_none() {
        if m.target.is_none() && r.random_range(0..120) == 0 {
            m.target = Some([
                pos[0] + r.random_range(-10.0..10.0),
                pos[1],
                pos[2] + r.random_range(-10.0..10.0),
            ]);
        }
        goal = m.target;
    }
    // Sheep regrow wool by eating grass.
    if kind == MobKind::Sheep
        && goal.is_none()
        && r.random_range(0..if m.baby { 50 } else { 1000 }) == 0
    {
        m.eat_timer = 40;
    }
    if m.eat_timer > 0 {
        m.eat_timer -= 1;
        goal = None;
        if m.eat_timer == 4 {
            let below = BlockPos::new(
                pos[0].floor() as i32,
                (pos[1] - 0.5).floor() as i32,
                pos[2].floor() as i32,
            );
            let b = rc_data::blocks();
            if b.def(world.get_block(below)).name == "grass_block" {
                events.push(Event::SetBlock {
                    pos: below,
                    state: b.default_state("dirt"),
                });
                m.sheared = false;
                if m.baby {
                    m.age -= 1200;
                }
            }
        }
    }
    // Chickens lay eggs.
    if kind == MobKind::Chicken && !m.baby {
        m.egg_timer = m.egg_timer.saturating_sub(1);
        if m.egg_timer == 0 {
            m.egg_timer = r.random_range(6000..12000);
            if let Some(egg) = crate::items::items().id("egg") {
                events.push(Event::Drop {
                    stack: crate::items::Stack::new(egg, 1),
                    pos,
                });
                events.push(Event::Sound {
                    event: "entity.chicken.egg".into(),
                    pos,
                    volume: 1.0,
                    pitch: 1.0,
                });
            }
        }
    }
    if r.random_range(0..240) == 0 {
        sound(events, kind, "ambient", pos, m.baby);
    }

    // --- steering ---------------------------------------------------------
    let mut forward = 0.0;
    if let Some(g) = goal {
        let dx = g[0] - pos[0];
        let dz = g[2] - pos[2];
        let d = (dx * dx + dz * dz).sqrt();
        if d < 0.6 {
            if m.target == Some(g) {
                m.target = None;
            }
        } else {
            let want = (-dx).atan2(dz).to_degrees() as f32;
            let diff = wrap(want - e.yaw).clamp(-30.0, 30.0);
            e.yaw = wrap(e.yaw + diff);
            forward = 1.0;
        }
        if r.random_range(0..200) == 0 && m.panic == 0 {
            m.target = None;
        }
    }
    // Head looks at the player sometimes.
    if m.look_ticks == 0 && r.random_range(0..50) == 0 && dist2(player.pos, pos) < 36.0 {
        m.look_ticks = 40 + r.random_range(0..40);
    }
    if m.look_ticks > 0 {
        m.look_ticks -= 1;
        let dx = player.pos[0] - pos[0];
        let dz = player.pos[2] - pos[2];
        let want = (-dx).atan2(dz).to_degrees() as f32;
        m.head_yaw = wrap(want - e.yaw).clamp(-75.0, 75.0);
        let dy = player.pos[1] + 1.62 - (pos[1] + kind.size(m.baby).1 * 0.85);
        m.head_pitch =
            (-(dy.atan2((dx * dx + dz * dz).sqrt())).to_degrees() as f32).clamp(-40.0, 40.0);
    } else {
        m.head_yaw *= 0.8;
        m.head_pitch *= 0.8;
    }

    // --- physics ----------------------------------------------------------
    let b = rc_data::blocks();
    let feet = world.get_block(BlockPos::new(
        pos[0].floor() as i32,
        (pos[1] + 0.2).floor() as i32,
        pos[2].floor() as i32,
    ));
    let in_water = b.fluid(feet).is_water();
    let (w, h) = kind.size(m.baby);
    let bb = Aabb::at_feet(pos, w, h);
    let mut vel = e.vel;
    let yaw = f64::from(e.yaw).to_radians();
    let accel = if e.on_ground {
        kind.speed() * speed_mod * 0.45
    } else {
        0.02
    } * if in_water { 0.4 } else { 1.0 };
    vel[0] += -yaw.sin() * forward * accel;
    vel[2] += yaw.cos() * forward * accel;
    // Jump when blocked by a full block.
    if forward > 0.0 && e.on_ground {
        let probe = bb.offset([-yaw.sin() * 0.4, 0.6, yaw.cos() * 0.4]);
        let low = bb.offset([-yaw.sin() * 0.4, 0.0, yaw.cos() * 0.4]);
        scratch.clear();
        world.boxes(&low.inflate(0.01), scratch);
        let blocked_low = scratch.iter().any(|x| x.intersects(&low));
        scratch.clear();
        world.boxes(&probe, scratch);
        let blocked_high = scratch.iter().any(|x| x.intersects(&probe));
        if blocked_low && !blocked_high {
            vel[1] = 0.42;
        }
    }
    if in_water {
        vel[1] += 0.04;
        if vel[1] > 0.1 {
            vel[1] = 0.1;
        }
    } else {
        vel[1] -= 0.08;
    }
    let res = move_and_collide(&bb, vel, 0.6, e.on_ground, world, scratch);
    for k in 0..3 {
        e.pos[k] += res.moved[k];
    }
    if res.hit_x {
        vel[0] = 0.0;
        m.target = None;
    }
    if res.hit_z {
        vel[2] = 0.0;
        m.target = None;
    }
    let landing = res.on_ground && !e.on_ground;
    e.on_ground = res.on_ground;
    if res.hit_y {
        vel[1] = 0.0;
    }
    if res.moved[1] < 0.0 && !e.on_ground {
        m.fall -= res.moved[1] as f32;
    }
    if landing {
        if m.fall > 3.0 && kind != MobKind::Chicken && !in_water {
            hurt_mob(m, (m.fall - 3.0).ceil(), pos, events);
        }
        m.fall = 0.0;
    }
    if in_water {
        m.fall = 0.0;
    }
    // Chickens flap and fall slowly.
    if kind == MobKind::Chicken && !e.on_ground && vel[1] < 0.0 {
        vel[1] *= 0.6;
    }
    let f = if e.on_ground { 0.6 * 0.91 } else { 0.91 };
    vel[0] *= f;
    vel[2] *= f;
    vel[1] *= 0.98;
    e.vel = vel;
    let moved = (res.moved[0].powi(2) + res.moved[2].powi(2)).sqrt() as f32;
    m.limb_amount += ((moved * 4.0).min(1.0) - m.limb_amount) * 0.4;
    m.limb_swing += m.limb_amount;
    if e.pos[1] < -128.0 {
        e.dead = true;
    }
}

/// Apply damage; returns true if the mob died.
pub fn hurt_mob(m: &mut Mob, amount: f32, pos: [f64; 3], events: &mut Vec<Event>) -> bool {
    if m.death_time > 0 || m.hurt_time > 0 {
        return false;
    }
    m.health -= amount;
    m.hurt_time = 10;
    m.panic = 100;
    m.target = None;
    if m.health <= 0.0 {
        m.death_time = 1;
        sound(events, m.kind, "death", pos, m.baby);
        true
    } else {
        sound(events, m.kind, "hurt", pos, m.baby);
        false
    }
}

/// Natural spawning when a chunk is first generated (biome "creature"
/// spawners, as in vanilla world generation).
pub fn spawn_for_new_chunk(world: &World, cx: i32, cz: i32, out: &mut Vec<Entity>) {
    let mut r = rand::rng();
    let x = cx * 16 + 8;
    let z = cz * 16 + 8;
    let Some(top) = world.surface_y(x, z) else {
        return;
    };
    let biome_id = world.biome(BlockPos::new(x, top, z));
    let reg = rc_worldgen::biome::registry();
    let name = reg
        .get(biome_id)
        .name
        .trim_start_matches("minecraft:")
        .to_owned();
    let Some(j) = rc_data::files().json(&format!("worldgen/biome/{name}.json")) else {
        return;
    };
    let prob = j["creature_spawn_probability"].as_f64().unwrap_or(0.1);
    if r.random::<f64>() >= prob {
        return;
    }
    let spawners: Vec<(MobKind, u32, u32, u32)> = j["spawners"]["creature"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            Some((
                MobKind::parse(s["type"].as_str()?)?,
                s["weight"].as_u64().unwrap_or(1) as u32,
                s["minCount"].as_u64().unwrap_or(1) as u32,
                s["maxCount"].as_u64().unwrap_or(1) as u32,
            ))
        })
        .collect();
    let total: u32 = spawners.iter().map(|s| s.1).sum();
    if total == 0 {
        return;
    }
    let mut pick = r.random_range(0..total);
    let Some(&(kind, _, lo, hi)) = spawners.iter().find(|s| {
        if pick < s.1 {
            true
        } else {
            pick -= s.1;
            false
        }
    }) else {
        return;
    };
    let b = rc_data::blocks();
    let n = r.random_range(lo..=hi.max(lo));
    for _ in 0..n {
        let px = cx * 16 + r.random_range(0..16);
        let pz = cz * 16 + r.random_range(0..16);
        let Some(y) = world.surface_y(px, pz) else {
            continue;
        };
        let ground = world.get_block(BlockPos::new(px, y, pz));
        // Animals spawn on grass with headroom.
        let name = b.def(ground).name.as_str();
        if name != "grass_block" {
            continue;
        }
        let above = world.get_block(BlockPos::new(px, y + 1, pz));
        if !b.collision(above).is_empty() || !b.fluid(above).is_empty() {
            continue;
        }
        out.push(spawn(
            kind,
            [f64::from(px) + 0.5, f64::from(y) + 1.0, f64::from(pz) + 0.5],
            false,
        ));
    }
}
