//! In-world game state: world, player, inventory, entities, interaction,
//! containers, time and weather. Runs at a fixed 20 ticks per second.

use rand::Rng as _;
use rc_core::{BlockPos, BlockStateId, ChunkPos};
use serde::{Deserialize, Serialize};

use crate::blockentity::BlockEntity;
use crate::blocklogic::{self, Change};
use crate::entity::{Entities, Entity, Event, Kind};
use crate::items::{self, Inventory, Stack, items};
use crate::mobs::{self, MobKind, PlayerInfo};
use crate::particles::Particles;
use crate::player::{GameMode, Hit, MoveInput, Player, outline_boxes};
use crate::world::World;

/// Input sampled for one tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct TickInput {
    pub mv: MoveInput,
    pub attack: bool,
    pub attack_pressed: bool,
    pub use_held: bool,
    pub use_pressed: bool,
    pub pick: bool,
    pub jump_pressed: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct SavedPlayer {
    player: Player,
    inventory: Inventory,
}

/// Serialise a player + inventory the way `world.rcw` stores them.
#[must_use]
pub fn saved_player_json(p: &Player, inv: &Inventory) -> Option<serde_json::Value> {
    serde_json::to_value(SavedPlayer {
        player: p.clone(),
        inventory: inv.clone(),
    })
    .ok()
}

/// Inverse of [`saved_player_json`].
#[must_use]
pub fn parse_saved_player(v: serde_json::Value) -> Option<(Player, Inventory)> {
    serde_json::from_value::<SavedPlayer>(v)
        .ok()
        .map(|s| (s.player, s.inventory))
}

#[derive(Clone, Debug)]
pub struct ChatLine {
    pub text: String,
    pub tick: u64,
}

/// A sound for the audio system.
#[derive(Clone, Debug)]
pub struct SoundOut {
    pub event: String,
    pub pos: Option<[f64; 3]>,
    pub volume: f32,
    pub pitch: f32,
    pub cat: rc_audio::Category,
}

/// An open block GUI.
#[derive(Clone, Debug, PartialEq)]
pub enum Container {
    Crafting { pos: BlockPos },
    Chest { pos: BlockPos },
    Furnace { pos: BlockPos },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Difficulty {
    Peaceful,
    Easy,
    #[default]
    Normal,
    Hard,
}

impl Difficulty {
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "peaceful" => Self::Peaceful,
            "easy" => Self::Easy,
            "hard" => Self::Hard,
            _ => Self::Normal,
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Peaceful => "peaceful",
            Self::Easy => "easy",
            Self::Normal => "normal",
            Self::Hard => "hard",
        }
    }
}

pub struct Game {
    pub world: World,
    pub player: Player,
    pub inv: Inventory,
    pub entities: Entities,
    pub particles: Particles,
    pub chat: Vec<ChatLine>,
    pub target: Option<Hit>,
    /// Index into `entities.list` of the mob under the crosshair.
    pub target_entity: Option<usize>,
    pub breaking: Option<(BlockPos, f32)>,
    pub tick: u64,
    break_cooldown: u32,
    place_cooldown: u32,
    attack_cooldown: u32,
    last_jump: Option<u64>,
    pub spawned: bool,
    pub spawn: BlockPos,
    pub swing: u32,
    pub sounds: Vec<SoundOut>,
    pub container: Option<Container>,
    /// 2×2 inventory crafting grid, or 3×3 at a crafting table.
    pub craft: Vec<Option<Stack>>,
    pub rain: f32,
    pub prev_rain: f32,
    pub thunder: f32,
    pub prev_thunder: f32,
    pub flash: u32,
    pub difficulty: Difficulty,
    music_timer: u32,
    step_accum: f32,
}

impl std::fmt::Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Game({:?})", self.world)
    }
}

/// Blocks that placement may replace.
#[must_use]
pub fn replaceable(s: BlockStateId) -> bool {
    let b = rc_data::blocks();
    if s == BlockStateId::AIR {
        return true;
    }
    let name = b.def(s).name.as_str();
    let f = b.fluid(s);
    (!f.is_empty() && (name == "water" || name == "lava"))
        || name.ends_with("air")
        || matches!(
            name,
            "short_grass"
                | "tall_grass"
                | "fern"
                | "large_fern"
                | "dead_bush"
                | "vine"
                | "snow"
                | "seagrass"
                | "tall_seagrass"
                | "glow_lichen"
                | "fire"
        ) && !(name == "snow" && b.get(s, "layers").is_some_and(|l| l != "1"))
}

fn is_falling(s: BlockStateId) -> bool {
    let n = rc_data::blocks().def(s).name.as_str();
    matches!(
        n,
        "sand"
            | "red_sand"
            | "gravel"
            | "suspicious_sand"
            | "suspicious_gravel"
            | "anvil"
            | "chipped_anvil"
            | "damaged_anvil"
            | "dragon_egg"
    ) || n.ends_with("concrete_powder")
}

fn name_of(s: BlockStateId) -> &'static str {
    rc_data::blocks().def(s).name.as_str()
}

/// Rule value with a vanilla default.
#[must_use]
pub fn rule_bool(meta: &rc_world::rcw::WorldMeta, rule: &str) -> bool {
    meta.rules.get(rule).map_or_else(
        || !matches!(rule, "keepInventory" | "doImmediateRespawn" | "doInsomnia"),
        |v| v == "true",
    )
}

impl Game {
    #[must_use]
    pub fn new(world: World) -> Self {
        let spawn = BlockPos::new(
            world.meta.spawn[0],
            world.meta.spawn[1],
            world.meta.spawn[2],
        );
        let (player, inv, spawned) = match world
            .meta
            .player
            .clone()
            .and_then(|v| serde_json::from_value::<SavedPlayer>(v).ok())
        {
            Some(s) => (s.player, s.inventory, true),
            None => {
                let mut p = Player::default();
                p.mode = GameMode::parse(&world.meta.game_mode).unwrap_or_default();
                p.pos = [f64::from(spawn.x) + 0.5, 200.0, f64::from(spawn.z) + 0.5];
                (p, Inventory::default(), false)
            }
        };
        let mut player = player;
        player.prev_pos = player.pos;
        let mut inv = inv;
        inv.normalize();
        let difficulty = Difficulty::parse(&world.meta.difficulty);
        let rain = if world.meta.raining { 1.0 } else { 0.0 };
        let thunder = if world.meta.thundering { 1.0 } else { 0.0 };
        Self {
            world,
            player,
            inv,
            entities: Entities::default(),
            particles: Particles::default(),
            chat: Vec::new(),
            target: None,
            target_entity: None,
            breaking: None,
            tick: 0,
            break_cooldown: 0,
            place_cooldown: 0,
            attack_cooldown: 0,
            last_jump: None,
            spawned,
            spawn,
            swing: 0,
            sounds: Vec::new(),
            container: None,
            craft: vec![None; 9],
            rain,
            prev_rain: rain,
            thunder,
            prev_thunder: thunder,
            flash: 0,
            difficulty,
            music_timer: 600,
            step_accum: 0.0,
        }
    }

    pub fn message(&mut self, s: impl Into<String>) {
        self.chat.push(ChatLine {
            text: s.into(),
            tick: self.tick,
        });
        if self.chat.len() > 100 {
            self.chat.remove(0);
        }
    }

    pub fn sound(
        &mut self,
        event: impl Into<String>,
        pos: Option<[f64; 3]>,
        volume: f32,
        pitch: f32,
        cat: rc_audio::Category,
    ) {
        self.sounds.push(SoundOut {
            event: event.into(),
            pos,
            volume,
            pitch,
            cat,
        });
    }

    fn block_sound(&mut self, s: BlockStateId, what: &str, p: BlockPos, volume: f32, pitch: f32) {
        let group = rc_audio::block_sound_group(name_of(s), crate::audio_has);
        let c = [
            f64::from(p.x) + 0.5,
            f64::from(p.y) + 0.5,
            f64::from(p.z) + 0.5,
        ];
        self.sound(
            format!("block.{group}.{what}"),
            Some(c),
            volume,
            pitch,
            rc_audio::Category::Blocks,
        );
    }

    /// Place the player on the surface once the spawn chunk exists.
    fn try_spawn(&mut self) -> bool {
        if self.spawned {
            return true;
        }
        let b = rc_data::blocks();
        for r in 0i32..48 {
            for dx in -r..=r {
                for dz in -r..=r {
                    if dx.abs() != r && dz.abs() != r {
                        continue;
                    }
                    let (x, z) = (self.spawn.x + dx * 4, self.spawn.z + dz * 4);
                    if !self.world.is_loaded(BlockPos::new(x, 0, z)) {
                        return false;
                    }
                    let Some(top) = self.world.surface_y(x, z) else {
                        return false;
                    };
                    let valid = rc_data::tags().block("valid_spawn");
                    let free = |w: &World, y: i32| {
                        let s = w.get_block(BlockPos::new(x, y, z));
                        b.collision(s).is_empty() && b.fluid(s).is_empty()
                    };
                    for y in (top - 24..=top).rev() {
                        let s = self.world.get_block(BlockPos::new(x, y, z));
                        if valid.contains_state(b, s)
                            && free(&self.world, y + 1)
                            && free(&self.world, y + 2)
                        {
                            self.player.pos =
                                [f64::from(x) + 0.5, f64::from(y) + 1.0, f64::from(z) + 0.5];
                            self.player.prev_pos = self.player.pos;
                            self.spawn = BlockPos::new(x, y + 1, z);
                            self.world.meta.spawn = [x, y + 1, z];
                            self.spawned = true;
                            return true;
                        }
                    }
                }
            }
        }
        let y = self
            .world
            .surface_y(self.spawn.x, self.spawn.z)
            .unwrap_or(64);
        self.player.pos = [
            f64::from(self.spawn.x) + 0.5,
            f64::from(y) + 1.0,
            f64::from(self.spawn.z) + 0.5,
        ];
        self.spawned = true;
        true
    }

    #[must_use]
    pub fn player_chunk_ready(&self) -> bool {
        let p = BlockPos::new(
            self.player.pos[0].floor() as i32,
            0,
            self.player.pos[2].floor() as i32,
        );
        self.world.is_loaded(p)
    }

    /// Stream chunks, keeping entities with the chunks that own them.
    pub fn stream(&mut self, rd: i32, budget: std::time::Duration) {
        for c in self.world.chunks_to_unload(self.player.pos, rd) {
            let blobs: Vec<Vec<u8>> = self
                .entities
                .take_chunk(c)
                .iter()
                .filter_map(|e| serde_json::to_vec(e).ok())
                .collect();
            self.world.stash_entities(c, blobs);
        }
        self.world.update(self.player.pos, rd, budget);
        let events = std::mem::take(&mut self.world.loaded_events);
        for (p, fresh) in events {
            for blob in self.world.take_entities(p) {
                if let Ok(e) = serde_json::from_slice::<Entity>(&blob) {
                    self.entities.spawn(e);
                }
            }
            if fresh && self.entities.mob_count() < 300 {
                let mut out = Vec::new();
                mobs::spawn_for_new_chunk(&self.world, p.x, p.z, &mut out);
                for e in out {
                    self.entities.spawn(e);
                }
            }
        }
    }

    /// Store all live entities into their chunks (before a save).
    fn stash_all_entities(&mut self) {
        let mut by_chunk: ahash::AHashMap<ChunkPos, Vec<Vec<u8>>> = ahash::AHashMap::new();
        for e in &self.entities.list {
            if matches!(e.kind, Kind::Lightning) {
                continue;
            }
            if let Ok(b) = serde_json::to_vec(e) {
                by_chunk.entry(e.chunk()).or_default().push(b);
            }
        }
        let loaded: Vec<ChunkPos> = self.world.chunks.keys().copied().collect();
        for c in loaded {
            self.world
                .stash_entities(c, by_chunk.remove(&c).unwrap_or_default());
        }
    }

    /// Set a block and notify neighbours (fluids, shapes, falling, support).
    pub fn set_block(&mut self, p: BlockPos, s: BlockStateId) {
        let old = self.world.get_block(p);
        if old == s || !self.world.set_block(p, s) {
            return;
        }
        if !BlockEntity::for_block(name_of(s)).is_some() || name_of(old) != name_of(s) {
            if let Some(be) = self.world.block_entities.remove(&p) {
                for st in be.contents() {
                    self.drop_item(
                        st,
                        [
                            f64::from(p.x) + 0.5,
                            f64::from(p.y) + 0.5,
                            f64::from(p.z) + 0.5,
                        ],
                    );
                }
            }
            if let Some(be) = BlockEntity::for_block(name_of(s)) {
                self.world.block_entities.insert(p, be);
            }
            self.world.mark_dirty(p);
        }
        self.neighbours_changed(p);
    }

    fn neighbours_changed(&mut self, p: BlockPos) {
        let now = self.tick;
        for (dx, dy, dz) in [
            (0, 0, 0),
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ] {
            let q = BlockPos::new(p.x + dx, p.y + dy, p.z + dz);
            let s = self.world.get_block(q);
            let f = rc_data::blocks().fluid(s);
            if !f.is_empty() {
                self.world.schedule(q, now, blocklogic::fluid_delay(f.kind));
            }
            if is_falling(s) || name_of(s).ends_with("leaves") || (dx, dy, dz) != (0, 0, 0) {
                self.world.schedule(q, now, 2);
            }
        }
    }

    fn drop_item(&mut self, st: Stack, pos: [f64; 3]) {
        if !rule_bool(&self.world.meta, "doTileDrops") {
            return;
        }
        self.entities.spawn(Entity::item(st, pos, 10));
    }

    fn update_time_and_weather(&mut self) {
        let meta = &mut self.world.meta;
        meta.time += 1;
        if rule_bool(meta, "doDaylightCycle") {
            meta.day_time += 1;
        }
        if rule_bool(meta, "doWeatherCycle") {
            let mut r = rand::rng();
            if meta.clear_weather_time > 0 {
                meta.clear_weather_time -= 1;
                meta.raining = false;
                meta.thundering = false;
            } else {
                meta.rain_time -= 1;
                if meta.rain_time <= 0 {
                    meta.raining = !meta.raining;
                    meta.rain_time = if meta.raining {
                        r.random_range(12000..24000)
                    } else {
                        r.random_range(12000..180000)
                    };
                }
                meta.thunder_time -= 1;
                if meta.thunder_time <= 0 {
                    meta.thundering = !meta.thundering;
                    meta.thunder_time = if meta.thundering {
                        r.random_range(3600..15600)
                    } else {
                        r.random_range(12000..180000)
                    };
                }
            }
        }
        let (raining, thundering) = (meta.raining, meta.thundering);
        self.prev_rain = self.rain;
        self.prev_thunder = self.thunder;
        self.rain = (self.rain + if raining { 0.01 } else { -0.01 }).clamp(0.0, 1.0);
        self.thunder =
            (self.thunder + if thundering && raining { 0.01 } else { -0.01 }).clamp(0.0, 1.0);
        self.flash = self.flash.saturating_sub(1);
        if self.thunder > 0.9 && rand::rng().random_range(0..2000) == 0 {
            self.lightning();
        }
    }

    fn lightning(&mut self) {
        let mut r = rand::rng();
        let x = self.player.pos[0] as i32 + r.random_range(-64..64);
        let z = self.player.pos[2] as i32 + r.random_range(-64..64);
        let Some(y) = self.world.surface_y(x, z) else {
            return;
        };
        let pos = [f64::from(x) + 0.5, f64::from(y) + 1.0, f64::from(z) + 0.5];
        self.entities.spawn(Entity::new(Kind::Lightning, pos));
        self.flash = 6;
        let d =
            ((pos[0] - self.player.pos[0]).powi(2) + (pos[2] - self.player.pos[2]).powi(2)).sqrt();
        self.sound(
            "entity.lightning_bolt.thunder",
            None,
            (1.0 - d / 128.0).max(0.2) as f32 * 0.8,
            0.8 + r.random::<f32>() * 0.2,
            rc_audio::Category::Weather,
        );
    }

    /// Sky darkness from rain/thunder (0 = clear).
    #[must_use]
    pub fn weather_dim(&self, t: f32) -> f32 {
        let rain = self.prev_rain + (self.rain - self.prev_rain) * t;
        let thunder = self.prev_thunder + (self.thunder - self.prev_thunder) * t;
        rain * 0.3 + thunder * 0.2
    }

    #[must_use]
    pub fn is_night(&self) -> bool {
        let t = self.world.meta.day_time.rem_euclid(24000);
        (12542..23460).contains(&t)
    }

    /// One game tick.
    pub fn tick(&mut self, input: &TickInput) {
        self.tick += 1;
        self.update_time_and_weather();
        if !self.try_spawn() {
            return;
        }
        if !self.player_chunk_ready() {
            self.player.prev_pos = self.player.pos;
            return;
        }
        if input.jump_pressed && matches!(self.player.mode, GameMode::Creative) {
            if let Some(t) = self.last_jump
                && self.tick - t < 7
            {
                self.player.flying = !self.player.flying;
                self.last_jump = None;
            } else {
                self.last_jump = Some(self.tick);
            }
        }
        let alive = !self.player.dead();
        if alive && self.player.sleeping > 0 {
            self.player.sleeping += 1;
            self.player.prev_pos = self.player.pos;
            if self.player.sleeping >= 100 {
                self.player.sleeping = 0;
                let t = self.world.meta.day_time;
                self.world.meta.day_time = t + (24000 - t.rem_euclid(24000));
                self.world.meta.raining = false;
                self.world.meta.thundering = false;
                self.world.meta.rain_time = 0;
            }
        } else if alive {
            let hp = self.player.health;
            let was_in_water = self.player.in_water;
            let fall_speed = self.player.vel[1];
            self.player.tick(&input.mv, &self.world);
            if self.player.in_water && !was_in_water && fall_speed < -0.2 {
                let pos = self.player.pos;
                let uv = crate::PARTICLE_SPRITES.with(|p| {
                    p.borrow()
                        .get(rc_data::blocks().default_state("water").0 as usize)
                        .copied()
                        .unwrap_or([0.0; 4])
                });
                self.particles.splash(pos, uv);
                self.sound(
                    "entity.player.splash",
                    Some(pos),
                    (-fall_speed as f32).min(1.0) * 0.6,
                    1.0,
                    rc_audio::Category::Players,
                );
            }
            if self.player.health < hp {
                self.sound(
                    "entity.player.hurt",
                    Some(self.player.pos),
                    1.0,
                    1.0,
                    rc_audio::Category::Players,
                );
            }
            if self.player.dead() && !rule_bool(&self.world.meta, "keepInventory") {
                for i in 0..self.inv.slots.len() {
                    if let Some(s) = self.inv.slots[i].take() {
                        let mut e = Entity::item(
                            s,
                            [
                                self.player.pos[0],
                                self.player.pos[1] + 1.0,
                                self.player.pos[2],
                            ],
                            40,
                        );
                        let mut r = rand::rng();
                        e.vel = [r.random_range(-0.3..0.3), 0.2, r.random_range(-0.3..0.3)];
                        self.entities.spawn(e);
                    }
                }
            }
            self.footsteps();
            self.interact(input);
        }
        if self.swing > 0 {
            self.swing -= 1;
        }
        // Entities.
        PlayerInfoGuard::set(PlayerInfo {
            pos: self.player.pos,
            held: self.inv.held().map(|s| s.item),
        });
        let pb = self.player.aabb();
        let alive = !self.player.dead() && self.player.mode != GameMode::Spectator;
        let events = self.entities.tick(&self.world, &pb, alive, self.tick);
        for ev in events {
            self.handle_event(ev);
        }
        self.block_ticks();
        self.random_ticks();
        self.furnaces();
        self.particles.tick(&self.world);
        self.ambient();
    }

    fn handle_event(&mut self, ev: Event) {
        match ev {
            Event::PickedUp { stack, pos } => {
                let left = self.inv.add_stack(stack);
                if let Some(l) = left {
                    if l.count != stack.count {
                        self.sound(
                            "entity.item.pickup",
                            Some(pos),
                            0.2,
                            1.4 + rand::rng().random::<f32>() * 0.6,
                            rc_audio::Category::Players,
                        );
                    }
                    let mut e = Entity::item(l, pos, 10);
                    e.vel = [0.0; 3];
                    self.entities.spawn(e);
                } else {
                    self.sound(
                        "entity.item.pickup",
                        Some(pos),
                        0.2,
                        1.4 + rand::rng().random::<f32>() * 0.6,
                        rc_audio::Category::Players,
                    );
                }
            }
            Event::Xp { value } => {
                let lvl = self.player.xp_level;
                self.player.add_xp(value);
                self.sound(
                    "entity.experience_orb.pickup",
                    None,
                    0.1,
                    0.9 + rand::rng().random::<f32>() * 0.3,
                    rc_audio::Category::Players,
                );
                if self.player.xp_level > lvl && self.player.xp_level.is_multiple_of(5) {
                    self.sound(
                        "entity.player.levelup",
                        None,
                        0.75,
                        1.0,
                        rc_audio::Category::Players,
                    );
                }
            }
            Event::Landed { pos, state } => {
                if replaceable(self.world.get_block(pos)) {
                    self.set_block(pos, state);
                    self.block_sound(state, "place", pos, 1.0, 0.8);
                } else if let Some(it) = items().for_block(state) {
                    self.drop_item(
                        Stack::new(it, 1),
                        [
                            f64::from(pos.x) + 0.5,
                            f64::from(pos.y) + 0.5,
                            f64::from(pos.z) + 0.5,
                        ],
                    );
                }
            }
            Event::Sound {
                event,
                pos,
                volume,
                pitch,
            } => {
                let cat = if event.starts_with("entity.") {
                    rc_audio::Category::Neutral
                } else {
                    rc_audio::Category::Blocks
                };
                self.sound(event, Some(pos), volume, pitch, cat);
            }
            Event::Drop { stack, pos } => {
                if rule_bool(&self.world.meta, "doMobLoot") {
                    self.entities
                        .spawn(Entity::item(stack, [pos[0], pos[1] + 0.5, pos[2]], 10));
                }
            }
            Event::Spawn(e) => {
                self.entities.spawn(*e);
            }
            Event::SetBlock { pos, state } => self.set_block(pos, state),
        }
    }

    fn footsteps(&mut self) {
        let p = &self.player;
        if !p.on_ground || p.sneaking || p.flying {
            return;
        }
        self.step_accum += (p.walk_dist - p.prev_walk_dist).abs();
        if self.step_accum >= 1.0 {
            self.step_accum = 0.0;
            let below = BlockPos::new(
                p.pos[0].floor() as i32,
                (p.pos[1] - 0.2).floor() as i32,
                p.pos[2].floor() as i32,
            );
            let s = self.world.get_block(below);
            if s != BlockStateId::AIR {
                self.block_sound(s, "step", below, 0.15, 1.0);
            }
        }
    }

    fn ambient(&mut self) {
        // Music every few minutes, like vanilla's situational music.
        self.music_timer = self.music_timer.saturating_sub(1);
        if self.music_timer == 0 {
            self.music_timer = rand::rng().random_range(12000..24000);
            let ev = if self.player.mode == GameMode::Creative {
                "music.creative"
            } else {
                "music.game"
            };
            self.sound(ev, None, 1.0, 1.0, rc_audio::Category::Music);
        }
        if self.rain > 0.2 && self.tick.is_multiple_of(20) {
            let p = self.player.pos;
            let x = p[0].floor() as i32;
            let z = p[2].floor() as i32;
            if self
                .world
                .surface_y(x, z)
                .is_some_and(|y| f64::from(y) < p[1] + 8.0)
            {
                self.sound(
                    "weather.rain",
                    Some([p[0], p[1] + 4.0, p[2]]),
                    0.2 * self.rain,
                    1.0,
                    rc_audio::Category::Weather,
                );
            }
        }
    }

    fn eye_ray(&self) -> ([f64; 3], [f64; 3]) {
        (self.player.eye(1.0), self.player.look_dir())
    }

    fn interact(&mut self, input: &TickInput) {
        let reach = if self.player.mode == GameMode::Creative {
            5.0
        } else {
            4.5
        };
        self.target = if self.player.mode == GameMode::Spectator {
            None
        } else {
            self.player.raycast(&self.world, reach, 1.0)
        };
        let (o, d) = self.eye_ray();
        self.target_entity = self.entities.raycast(o, d, 3.0).and_then(|(i, t)| {
            let block_t = self.target.map_or(f64::MAX, |h| {
                let c = [
                    f64::from(h.pos.x) + h.local[0],
                    f64::from(h.pos.y) + h.local[1],
                    f64::from(h.pos.z) + h.local[2],
                ];
                ((c[0] - o[0]).powi(2) + (c[1] - o[1]).powi(2) + (c[2] - o[2]).powi(2)).sqrt()
            });
            (t < block_t).then_some(i)
        });
        if self.target_entity.is_some() {
            self.target = None;
        }
        self.break_cooldown = self.break_cooldown.saturating_sub(1);
        self.place_cooldown = self.place_cooldown.saturating_sub(1);
        self.attack_cooldown = self.attack_cooldown.saturating_sub(1);
        // Eating: hold use with food.
        if input.use_held
            && let Some(st) = self.inv.held()
            && let Some((n, sat)) = items().food[st.item as usize]
            && (self.player.food < 20
                || self.player.mode == GameMode::Creative
                || items().name(st.item) == "golden_apple")
            && self.target_entity.is_none()
            && !self.target.is_some_and(|h| self.usable_block(h.pos))
        {
            self.player.eating += 1;
            if self.player.eating.is_multiple_of(4) {
                self.sound(
                    "entity.generic.eat",
                    Some(self.player.pos),
                    0.5,
                    1.0,
                    rc_audio::Category::Players,
                );
            }
            if self.player.eating >= 32 {
                self.player.eating = 0;
                self.player.eat(n, sat);
                self.sound(
                    "entity.player.burp",
                    Some(self.player.pos),
                    0.5,
                    1.0,
                    rc_audio::Category::Players,
                );
                if self.player.mode != GameMode::Creative {
                    let name = items().name(st.item);
                    self.inv.consume_held();
                    if name.ends_with("_stew") || name.ends_with("_soup") {
                        self.inv
                            .set_held(items().id("bowl").map(|b| Stack::new(b, 1)));
                    }
                }
            }
            return;
        }
        self.player.eating = 0;
        // Attack a mob.
        if input.attack_pressed
            && let Some(i) = self.target_entity
        {
            self.swing = 6;
            if self.attack_cooldown == 0 {
                self.attack_cooldown = 10;
                let dmg = 1.0
                    + self
                        .inv
                        .held()
                        .map_or(0.0, |s| items().attack[s.item as usize]);
                let yaw = f64::from(self.player.yaw).to_radians();
                let mut evs = Vec::new();
                let e = &mut self.entities.list[i];
                if let Kind::Mob(m) = &mut e.kind {
                    mobs::hurt_mob(m, dmg, e.pos, &mut evs);
                    e.vel[0] += -yaw.sin() * 0.4;
                    e.vel[2] += yaw.cos() * 0.4;
                    e.vel[1] = 0.36;
                }
                for ev in evs {
                    self.handle_event(ev);
                }
                if self.player.mode == GameMode::Survival
                    && self
                        .inv
                        .held()
                        .is_some_and(|s| items().durability[s.item as usize] > 0)
                {
                    let name = self
                        .inv
                        .held()
                        .map(|s| items().name(s.item).to_owned())
                        .unwrap_or_default();
                    let n = if name.ends_with("_sword") { 1 } else { 2 };
                    if self.inv.damage_held(n) {
                        self.sound(
                            "entity.item.break",
                            Some(self.player.pos),
                            0.8,
                            1.0,
                            rc_audio::Category::Players,
                        );
                    }
                }
                self.player.exhaustion += 0.1;
            }
            return;
        }
        // Breaking.
        match (input.attack, self.target) {
            (true, Some(hit)) => {
                self.swing = 6;
                let s = self.world.get_block(hit.pos);
                if self.player.mode == GameMode::Creative {
                    let sword = self
                        .inv
                        .held()
                        .is_some_and(|st| items().name(st.item).ends_with("_sword"));
                    if self.break_cooldown == 0 && !sword {
                        self.break_block(hit.pos, false);
                        self.break_cooldown = 6;
                    }
                    self.breaking = None;
                } else if let Some(ticks) = items::break_ticks(
                    s,
                    self.inv.held().map(|st| st.item),
                    self.player.eyes_in_water,
                    self.player.on_ground || self.player.flying,
                ) {
                    let prog = match self.breaking {
                        Some((p, v)) if p == hit.pos => v,
                        _ => 0.0,
                    };
                    let step = if ticks == 0 { 1.0 } else { 1.0 / ticks as f32 };
                    let np = prog + step;
                    if self.tick.is_multiple_of(4) {
                        self.block_sound(s, "hit", hit.pos, 0.25, 0.5);
                        let sp = self.particle_sprite(s);
                        let tint = self.tint_of(s);
                        self.particles.block_hit(hit.pos, hit.normal, sp, tint);
                    }
                    if np >= 1.0 && self.break_cooldown == 0 {
                        self.break_block(hit.pos, true);
                        self.breaking = None;
                        self.break_cooldown = 5;
                    } else {
                        self.breaking = Some((hit.pos, np.min(1.0)));
                    }
                }
            }
            _ => self.breaking = None,
        }
        // Using.
        if input.use_pressed || (input.use_held && self.place_cooldown == 0) {
            if let Some(i) = self.target_entity {
                if input.use_pressed {
                    self.use_on_entity(i);
                }
            } else if let Some(hit) = self.target {
                let used = !self.player.sneaking && input.use_pressed && self.use_block(hit.pos);
                if !used {
                    self.use_item_on(hit);
                }
                self.place_cooldown = 4;
            } else if input.use_pressed {
                self.use_item_air();
            }
        }
        if input.pick
            && let Some(hit) = self.target
            && self.player.mode == GameMode::Creative
            && let Some(it) = items().for_block(self.world.get_block(hit.pos))
        {
            self.inv.pick(it);
        }
    }

    /// Particle sprite UVs for a block (set by the engine from the models).
    fn particle_sprite(&self, s: BlockStateId) -> [f32; 4] {
        crate::PARTICLE_SPRITES.with(|p| p.borrow().get(s.0 as usize).copied().unwrap_or([0.0; 4]))
    }

    fn tint_of(&self, s: BlockStateId) -> [f32; 3] {
        let n = name_of(s);
        if n.contains("grass") || n.contains("leaves") || n == "vine" || n.contains("fern") {
            [0.55, 0.75, 0.35]
        } else {
            [1.0; 3]
        }
    }

    fn break_block(&mut self, p: BlockPos, survival: bool) {
        let s = self.world.get_block(p);
        if s == BlockStateId::AIR
            || items::break_ticks(s, None, false, true).is_none()
                && self.player.mode != GameMode::Creative
        {
            return;
        }
        let b = rc_data::blocks();
        let n = name_of(s);
        let melts = n == "ice"
            && survival
            && !self
                .world
                .get_block(BlockPos::new(p.x, p.y - 1, p.z))
                .eq(&BlockStateId::AIR);
        let replacement = if b.get(s, "waterlogged") == Some("true")
            || matches!(
                n,
                "seagrass" | "kelp" | "kelp_plant" | "tall_seagrass" | "bubble_column"
            )
            || melts
        {
            b.default_state("water")
        } else {
            BlockStateId::AIR
        };
        let sprite = self.particle_sprite(s);
        let tint = self.tint_of(s);
        self.particles.block_break(p, sprite, tint);
        self.block_sound(s, "break", p, 1.0, 0.8);
        self.set_block(p, replacement);
        // Multi-block structures come down together.
        let other = match (b.get(s, "half"), b.get(s, "part")) {
            (Some("lower"), _) => Some(BlockPos::new(p.x, p.y + 1, p.z)),
            (Some("upper"), _) => Some(BlockPos::new(p.x, p.y - 1, p.z)),
            (_, Some(part)) => b.get(s, "facing").map(|f| {
                let (dx, dz) = facing_xz(f);
                if part == "foot" {
                    BlockPos::new(p.x + dx, p.y, p.z + dz)
                } else {
                    BlockPos::new(p.x - dx, p.y, p.z - dz)
                }
            }),
            _ => None,
        };
        if let Some(o) = other
            && name_of(self.world.get_block(o)) == n
        {
            self.set_block(o, BlockStateId::AIR);
        }
        if survival && self.player.mode == GameMode::Survival {
            let tool = self.inv.held();
            let (_, correct) = items().mining(tool.map(|t| t.item), s);
            if correct {
                for st in crate::loot::block_drops(s, tool) {
                    self.drop_item(
                        st,
                        [
                            f64::from(p.x) + 0.5,
                            f64::from(p.y) + 0.5,
                            f64::from(p.z) + 0.5,
                        ],
                    );
                }
                if n.ends_with("_ore")
                    && !n.contains("iron")
                    && !n.contains("gold")
                    && !n.contains("copper")
                {
                    let xp = rand::rng().random_range(0..=3);
                    if xp > 0 {
                        self.entities.spawn(Entity::new(
                            Kind::Xp { value: xp },
                            [
                                f64::from(p.x) + 0.5,
                                f64::from(p.y) + 0.5,
                                f64::from(p.z) + 0.5,
                            ],
                        ));
                    }
                }
            }
            if tool.is_some_and(|t| items().durability[t.item as usize] > 0)
                && b.def(s).hardness > 0.0
            {
                let sword = tool.is_some_and(|t| items().name(t.item).ends_with("_sword"));
                if self.inv.damage_held(if sword { 2 } else { 1 }) {
                    self.sound(
                        "entity.item.break",
                        Some(self.player.pos),
                        0.8,
                        1.0,
                        rc_audio::Category::Players,
                    );
                }
            }
            self.player.exhaustion += 0.005;
        }
        // Plants lose their support.
        let above = BlockPos::new(p.x, p.y + 1, p.z);
        let sa = self.world.get_block(above);
        if sa != BlockStateId::AIR
            && b.collision(sa).is_empty()
            && b.fluid(sa).is_empty()
            && !name_of(sa).contains("vine")
            && b.get(sa, "half") != Some("upper")
        {
            self.break_block(above, survival);
        }
    }

    fn falling_check(&mut self, p: BlockPos) {
        let s = self.world.get_block(p);
        if !is_falling(s) {
            return;
        }
        let below = self.world.get_block(BlockPos::new(p.x, p.y - 1, p.z));
        if replaceable(below) || blocklogic::fluid_can_replace(below) {
            self.world.set_block(p, BlockStateId::AIR);
            self.neighbours_changed(p);
            self.entities.spawn(Entity::new(
                Kind::Falling { state: s.0 },
                [f64::from(p.x) + 0.5, f64::from(p.y), f64::from(p.z) + 0.5],
            ));
        }
    }

    fn block_ticks(&mut self) {
        let due = self.world.due_ticks(self.tick, 512);
        let b = rc_data::blocks();
        for p in due {
            let s = self.world.get_block(p);
            if !b.fluid(s).is_empty() {
                let r = blocklogic::fluid_tick(&self.world, p);
                for (q, ns) in r.set {
                    let old = self.world.get_block(q);
                    if old != ns {
                        if blocklogic::fluid_can_replace(old)
                            && old != BlockStateId::AIR
                            && b.fluid(old).is_empty()
                        {
                            // Washed-away plants drop their item.
                            for st in crate::loot::block_drops(old, None) {
                                self.drop_item(
                                    st,
                                    [
                                        f64::from(q.x) + 0.5,
                                        f64::from(q.y) + 0.5,
                                        f64::from(q.z) + 0.5,
                                    ],
                                );
                            }
                        }
                        self.world.set_block(q, ns);
                        self.neighbours_changed(q);
                    }
                }
                for (q, d) in r.schedule {
                    self.world.schedule(q, self.tick, d);
                }
                if let Some(f) = r.fizz {
                    self.sound(
                        "block.lava.extinguish",
                        Some([
                            f64::from(f.x) + 0.5,
                            f64::from(f.y) + 0.5,
                            f64::from(f.z) + 0.5,
                        ]),
                        0.5,
                        2.6,
                        rc_audio::Category::Blocks,
                    );
                }
                continue;
            }
            if is_falling(s) {
                self.falling_check(p);
                continue;
            }
            if let Some(ns) = blocklogic::leaf_distance(&self.world, p) {
                self.world.set_block(p, ns);
                for (dx, dy, dz) in [
                    (1, 0, 0),
                    (-1, 0, 0),
                    (0, 1, 0),
                    (0, -1, 0),
                    (0, 0, 1),
                    (0, 0, -1),
                ] {
                    self.world
                        .schedule(BlockPos::new(p.x + dx, p.y + dy, p.z + dz), self.tick, 1);
                }
                continue;
            }
            if let Some(ns) = blocklogic::update_shape(&self.world, p) {
                self.world.set_block(p, ns);
                continue;
            }
            // Unsupported plants pop off.
            if s != BlockStateId::AIR
                && b.collision(s).is_empty()
                && b.fluid(s).is_empty()
                && !b.def(s).name.ends_with("air")
            {
                let below = self.world.get_block(BlockPos::new(p.x, p.y - 1, p.z));
                let needs_ground = !name_of(s).contains("vine")
                    && !name_of(s).contains("torch")
                    && !name_of(s).contains("lichen")
                    && !name_of(s).contains("sign")
                    && b.get(s, "half") != Some("upper")
                    && !name_of(s).contains("button")
                    && !name_of(s).contains("ladder")
                    && !name_of(s).contains("lever")
                    && !name_of(s).contains("rail")
                    && !name_of(s).contains("fire");
                if needs_ground && below == BlockStateId::AIR {
                    for st in crate::loot::block_drops(s, None) {
                        self.drop_item(
                            st,
                            [
                                f64::from(p.x) + 0.5,
                                f64::from(p.y) + 0.5,
                                f64::from(p.z) + 0.5,
                            ],
                        );
                    }
                    self.world.set_block(p, BlockStateId::AIR);
                    self.neighbours_changed(p);
                }
            }
        }
    }

    fn random_ticks(&mut self) {
        let speed: u32 = self
            .world
            .meta
            .rules
            .get("randomTickSpeed")
            .and_then(|v| v.parse().ok())
            .unwrap_or(3);
        if speed == 0 {
            return;
        }
        let pc = self.player_chunk();
        let mut r = rand::rng();
        let mut changes = Vec::new();
        let radius = 6;
        for cx in pc.x - radius..=pc.x + radius {
            for cz in pc.z - radius..=pc.z + radius {
                let Some(c) = self.world.chunks.get(&ChunkPos::new(cx, cz)) else {
                    continue;
                };
                for (si, sec) in c.sections.iter().enumerate() {
                    if sec.is_empty() {
                        continue;
                    }
                    for _ in 0..speed {
                        let p = BlockPos::new(
                            cx * 16 + r.random_range(0..16),
                            -64 + si as i32 * 16 + r.random_range(0..16),
                            cz * 16 + r.random_range(0..16),
                        );
                        if let Some(ch) = blocklogic::random_tick(&self.world, p, &mut r) {
                            changes.push(ch);
                        }
                    }
                }
            }
        }
        for ch in changes {
            match ch {
                Change::Set(p, s) => self.set_block(p, s),
                Change::Break(p) => {
                    let s = self.world.get_block(p);
                    for st in crate::loot::block_drops(s, None) {
                        self.drop_item(
                            st,
                            [
                                f64::from(p.x) + 0.5,
                                f64::from(p.y) + 0.5,
                                f64::from(p.z) + 0.5,
                            ],
                        );
                    }
                    self.set_block(p, BlockStateId::AIR);
                }
                Change::Tree(p, kind) => {
                    let blocks = blocklogic::tree_blocks(&self.world, p, &kind, &mut r);
                    for (q, s) in blocks {
                        self.world.set_block(q, s);
                    }
                }
            }
        }
    }

    fn furnaces(&mut self) {
        let b = rc_data::blocks();
        let pos: Vec<BlockPos> = self
            .world
            .block_entities
            .iter()
            .filter(|(_, be)| matches!(be, BlockEntity::Furnace { .. }))
            .map(|(p, _)| *p)
            .collect();
        for p in pos {
            let Some(be) = self.world.block_entities.get_mut(&p) else {
                continue;
            };
            let changed = be.tick();
            if let Some(lit) = changed {
                let s = self.world.get_block(p);
                self.world
                    .set_block(p, b.with(s, "lit", if lit { "true" } else { "false" }));
            }
            if self.tick.is_multiple_of(40) {
                self.world.mark_dirty(p);
            }
        }
    }

    fn usable_block(&self, p: BlockPos) -> bool {
        let n = name_of(self.world.get_block(p));
        n.contains("door") && n != "iron_door" && n != "iron_trapdoor"
            || n.ends_with("fence_gate")
            || matches!(
                n,
                "crafting_table"
                    | "chest"
                    | "trapped_chest"
                    | "barrel"
                    | "furnace"
                    | "blast_furnace"
                    | "smoker"
                    | "cake"
            )
            || n.ends_with("_bed")
            || n.ends_with("shulker_box")
    }

    /// Right-click on a block; returns true if the block consumed it.
    fn use_block(&mut self, p: BlockPos) -> bool {
        let b = rc_data::blocks();
        let s = self.world.get_block(p);
        let n = name_of(s);
        if (n.ends_with("_door") || n.ends_with("_trapdoor") || n.ends_with("fence_gate"))
            && !n.starts_with("iron")
        {
            let open = b.get(s, "open") == Some("true");
            let ns = b.with(s, "open", if open { "false" } else { "true" });
            self.world.set_block(p, ns);
            if n.ends_with("_door") {
                let other = if b.get(s, "half") == Some("lower") {
                    BlockPos::new(p.x, p.y + 1, p.z)
                } else {
                    BlockPos::new(p.x, p.y - 1, p.z)
                };
                let os = self.world.get_block(other);
                if name_of(os) == n {
                    self.world.set_block(
                        other,
                        b.with(os, "open", if open { "false" } else { "true" }),
                    );
                }
            }
            let kind = if n.ends_with("fence_gate") {
                "fence_gate"
            } else if n.ends_with("trapdoor") {
                "wooden_trapdoor"
            } else {
                "wooden_door"
            };
            let ev = format!("block.{kind}.{}", if open { "close" } else { "open" });
            self.sound(
                ev,
                Some([
                    f64::from(p.x) + 0.5,
                    f64::from(p.y) + 0.5,
                    f64::from(p.z) + 0.5,
                ]),
                1.0,
                1.0,
                rc_audio::Category::Blocks,
            );
            self.swing = 6;
            return true;
        }
        match n {
            "crafting_table" => {
                self.container = Some(Container::Crafting { pos: p });
                return true;
            }
            "furnace" | "blast_furnace" | "smoker" => {
                self.world
                    .block_entities
                    .entry(p)
                    .or_insert_with(|| BlockEntity::for_block(n).expect("furnace"));
                self.container = Some(Container::Furnace { pos: p });
                return true;
            }
            "chest" | "trapped_chest" | "barrel" => {
                self.world
                    .block_entities
                    .entry(p)
                    .or_insert_with(|| BlockEntity::for_block(n).expect("chest"));
                self.container = Some(Container::Chest { pos: p });
                let ev = if n == "barrel" {
                    "block.barrel.open"
                } else {
                    "block.chest.open"
                };
                self.sound(
                    ev,
                    Some([
                        f64::from(p.x) + 0.5,
                        f64::from(p.y) + 0.5,
                        f64::from(p.z) + 0.5,
                    ]),
                    0.5,
                    1.0,
                    rc_audio::Category::Blocks,
                );
                return true;
            }
            n if n.ends_with("shulker_box") => {
                self.world
                    .block_entities
                    .entry(p)
                    .or_insert_with(|| BlockEntity::for_block(n).expect("box"));
                self.container = Some(Container::Chest { pos: p });
                return true;
            }
            n if n.ends_with("_bed") => {
                if !self.is_night() && !self.world.meta.thundering {
                    self.message("You can only sleep at night or during thunderstorms");
                } else {
                    self.player.sleeping = 1;
                    self.spawn = BlockPos::new(p.x, p.y + 1, p.z);
                    self.world.meta.spawn = [p.x, p.y + 1, p.z];
                    self.message("Respawn point set");
                }
                return true;
            }
            "cake" => {
                if self.player.food < 20 || self.player.mode == GameMode::Creative {
                    let bites: u32 = b.get(s, "bites").and_then(|x| x.parse().ok()).unwrap_or(0);
                    self.player.eat(2, 0.4);
                    if bites >= 6 {
                        self.set_block(p, BlockStateId::AIR);
                    } else {
                        self.world
                            .set_block(p, b.with(s, "bites", &(bites + 1).to_string()));
                    }
                }
                return true;
            }
            _ => {}
        }
        false
    }

    fn use_on_entity(&mut self, i: usize) {
        let held = self.inv.held();
        let held_name = held
            .map(|s| items().name(s.item).to_owned())
            .unwrap_or_default();
        let creative = self.player.mode == GameMode::Creative;
        let e = &mut self.entities.list[i];
        let pos = e.pos;
        let Kind::Mob(m) = &mut e.kind else { return };
        if m.kind.food().contains(&held_name.as_str()) {
            if m.baby {
                m.age -= 2400;
            } else if m.age == 0 && m.love == 0 {
                m.love = 600;
            } else {
                return;
            }
            if !creative {
                self.inv.consume_held();
            }
            self.swing = 6;
            return;
        }
        if m.kind == MobKind::Sheep && held_name == "shears" && !m.sheared && !m.baby {
            m.sheared = true;
            let color = m.color;
            let n = rand::rng().random_range(1..=3);
            if let Some(w) = items().id(&format!("{}_wool", mobs::DYES[color as usize % 16])) {
                for _ in 0..n {
                    self.entities.spawn(Entity::item(
                        Stack::new(w, 1),
                        [pos[0], pos[1] + 1.0, pos[2]],
                        10,
                    ));
                }
            }
            self.sound(
                "entity.sheep.shear",
                Some(pos),
                1.0,
                1.0,
                rc_audio::Category::Players,
            );
            if !creative && self.inv.damage_held(1) {
                self.sound(
                    "entity.item.break",
                    Some(pos),
                    0.8,
                    1.0,
                    rc_audio::Category::Players,
                );
            }
            self.swing = 6;
            return;
        }
        if let Some(d) = held_name.strip_suffix("_dye")
            && m.kind == MobKind::Sheep
            && let Some(c) = mobs::DYES.iter().position(|x| *x == d)
        {
            m.color = c as u8;
            if !creative {
                self.inv.consume_held();
            }
            return;
        }
        if m.kind == MobKind::Cow && held_name == "bucket" && !m.baby {
            let _ = m;
            if let Some(milk) = items().id("milk_bucket") {
                if creative {
                    self.inv.add(milk, 1);
                } else {
                    self.inv.consume_held();
                    if self.inv.held().is_none() {
                        self.inv.set_held(Some(Stack::new(milk, 1)));
                    } else {
                        self.inv.add(milk, 1);
                    }
                }
            }
            self.sound(
                "entity.cow.milk",
                Some(pos),
                1.0,
                1.0,
                rc_audio::Category::Players,
            );
            self.swing = 6;
        }
    }

    fn use_item_air(&mut self) {
        let Some(st) = self.inv.held() else { return };
        let name = items().name(st.item);
        if name == "milk_bucket" && self.player.mode != GameMode::Creative {
            self.inv
                .set_held(items().id("bucket").map(|b| Stack::new(b, 1)));
            self.sound(
                "entity.generic.drink",
                Some(self.player.pos),
                0.5,
                1.0,
                rc_audio::Category::Players,
            );
        }
        if name == "bucket" {
            // Scoop a fluid source in reach.
            let (o, d) = self.eye_ray();
            for i in 1..=10 {
                let t = f64::from(i) * 0.5;
                let p = BlockPos::new(
                    (o[0] + d[0] * t).floor() as i32,
                    (o[1] + d[1] * t).floor() as i32,
                    (o[2] + d[2] * t).floor() as i32,
                );
                let s = self.world.get_block(p);
                let f = rc_data::blocks().fluid(s);
                if f.is_source() && b_name_is_fluid(s) {
                    let out = if f.is_water() {
                        "water_bucket"
                    } else {
                        "lava_bucket"
                    };
                    self.set_block(p, BlockStateId::AIR);
                    self.give_bucket_result(out);
                    let ev = if f.is_water() {
                        "item.bucket.fill"
                    } else {
                        "item.bucket.fill_lava"
                    };
                    self.sound(
                        ev,
                        Some(self.player.pos),
                        1.0,
                        1.0,
                        rc_audio::Category::Players,
                    );
                    return;
                }
                if !rc_data::blocks().collision(s).is_empty() {
                    return;
                }
            }
        }
    }

    fn give_bucket_result(&mut self, out: &str) {
        let Some(id) = items().id(out) else { return };
        if self.player.mode == GameMode::Creative {
            return;
        }
        self.inv.consume_held();
        if self.inv.held().is_none() {
            self.inv.set_held(Some(Stack::new(id, 1)));
        } else {
            self.inv.add(id, 1);
        }
    }

    fn use_item_on(&mut self, hit: Hit) {
        let Some(stack) = self.inv.held() else { return };
        let b = rc_data::blocks();
        let name = items().name(stack.item).to_owned();
        let clicked = self.world.get_block(hit.pos);
        let adj = BlockPos::new(
            hit.pos.x + hit.normal[0],
            hit.pos.y + hit.normal[1],
            hit.pos.z + hit.normal[2],
        );
        let creative = self.player.mode == GameMode::Creative;
        match name.as_str() {
            "water_bucket" | "lava_bucket" => {
                let target = if replaceable(clicked) { hit.pos } else { adj };
                if replaceable(self.world.get_block(target)) {
                    let fluid = if name == "water_bucket" {
                        "water"
                    } else {
                        "lava"
                    };
                    self.set_block(target, b.default_state(fluid));
                    if !creative {
                        self.inv
                            .set_held(items().id("bucket").map(|i| Stack::new(i, 1)));
                    }
                    let ev = if name == "water_bucket" {
                        "item.bucket.empty"
                    } else {
                        "item.bucket.empty_lava"
                    };
                    self.sound(
                        ev,
                        Some(self.player.pos),
                        1.0,
                        1.0,
                        rc_audio::Category::Players,
                    );
                }
                return;
            }
            "bucket" => {
                self.use_item_air();
                return;
            }
            "bone_meal" => {
                let n = name_of(clicked);
                let grown = if b.get(clicked, "age").is_some()
                    && !n.contains("cane")
                    && !n.contains("cactus")
                {
                    let max: u32 = match n {
                        "beetroots" | "sweet_berry_bush" | "nether_wart" => 3,
                        _ => 7,
                    };
                    let age: u32 = b
                        .get(clicked, "age")
                        .and_then(|a| a.parse().ok())
                        .unwrap_or(0);
                    if age < max {
                        let na = (age + rand::rng().random_range(2..=5)).min(max);
                        self.set_block(hit.pos, b.with(clicked, "age", &na.to_string()));
                        true
                    } else {
                        false
                    }
                } else if n.ends_with("_sapling") {
                    let mut r = rand::rng();
                    let blocks = blocklogic::tree_blocks(
                        &self.world,
                        hit.pos,
                        n.trim_end_matches("_sapling"),
                        &mut r,
                    );
                    let ok = !blocks.is_empty();
                    for (q, s) in blocks {
                        self.world.set_block(q, s);
                    }
                    ok
                } else if n == "grass_block" {
                    let mut r = rand::rng();
                    let grass = b.default_state("short_grass");
                    for _ in 0..24 {
                        let q = BlockPos::new(
                            hit.pos.x + r.random_range(-3..=3),
                            hit.pos.y + 1,
                            hit.pos.z + r.random_range(-3..=3),
                        );
                        if self.world.get_block(q) == BlockStateId::AIR
                            && name_of(self.world.get_block(BlockPos::new(q.x, q.y - 1, q.z)))
                                == "grass_block"
                        {
                            self.world.set_block(q, grass);
                        }
                    }
                    true
                } else {
                    false
                };
                if grown {
                    if !creative {
                        self.inv.consume_held();
                    }
                    self.sound(
                        "item.bone_meal.use",
                        Some(self.player.pos),
                        1.0,
                        1.0,
                        rc_audio::Category::Blocks,
                    );
                    self.swing = 6;
                }
                return;
            }
            n if n.ends_with("_hoe") => {
                let cn = name_of(clicked);
                if matches!(cn, "grass_block" | "dirt" | "dirt_path")
                    && self
                        .world
                        .get_block(BlockPos::new(hit.pos.x, hit.pos.y + 1, hit.pos.z))
                        == BlockStateId::AIR
                    && hit.normal[1] >= 0
                {
                    self.set_block(hit.pos, b.default_state("farmland"));
                    self.sound(
                        "item.hoe.till",
                        Some(self.player.pos),
                        1.0,
                        1.0,
                        rc_audio::Category::Blocks,
                    );
                    if !creative {
                        self.inv.damage_held(1);
                    }
                    self.swing = 6;
                }
                return;
            }
            n if n.ends_with("_shovel") => {
                if name_of(clicked) == "grass_block" && hit.normal[1] >= 0 {
                    self.set_block(hit.pos, b.default_state("dirt_path"));
                    self.sound(
                        "item.shovel.flatten",
                        Some(self.player.pos),
                        1.0,
                        1.0,
                        rc_audio::Category::Blocks,
                    );
                    if !creative {
                        self.inv.damage_held(1);
                    }
                    self.swing = 6;
                }
                return;
            }
            n if n.ends_with("_axe") => {
                let cn = name_of(clicked);
                let logs = rc_data::tags().block("logs");
                if logs.contains_state(b, clicked) && !cn.starts_with("stripped_") {
                    let stripped = format!("stripped_{cn}");
                    if b.index_of(&stripped).is_some() {
                        let mut ns = b.default_state(&stripped);
                        if let Some(a) = b.get(clicked, "axis") {
                            ns = b.with(ns, "axis", a);
                        }
                        self.set_block(hit.pos, ns);
                        self.sound(
                            "item.axe.strip",
                            Some(self.player.pos),
                            1.0,
                            1.0,
                            rc_audio::Category::Blocks,
                        );
                        if !creative {
                            self.inv.damage_held(1);
                        }
                        self.swing = 6;
                    }
                }
                return;
            }
            "flint_and_steel" => {
                if replaceable(self.world.get_block(adj)) && !b.collision(clicked).is_empty() {
                    self.set_block(adj, b.default_state("fire"));
                    self.sound(
                        "item.flintandsteel.use",
                        Some(self.player.pos),
                        1.0,
                        1.0,
                        rc_audio::Category::Blocks,
                    );
                    if !creative {
                        self.inv.damage_held(1);
                    }
                }
                return;
            }
            _ => {}
        }
        // Seeds and crops plant on farmland.
        let plant = match name.as_str() {
            "wheat_seeds" => Some("wheat"),
            "carrot" => Some("carrots"),
            "potato" => Some("potatoes"),
            "beetroot_seeds" => Some("beetroots"),
            "melon_seeds" => Some("melon_stem"),
            "pumpkin_seeds" => Some("pumpkin_stem"),
            _ => None,
        };
        if let Some(crop) = plant {
            if name_of(clicked) == "farmland"
                && hit.normal == [0, 1, 0]
                && replaceable(self.world.get_block(adj))
            {
                self.set_block(adj, b.default_state(crop));
                self.block_sound(b.default_state(crop), "place", adj, 1.0, 0.8);
                if !creative {
                    self.inv.consume_held();
                }
                self.swing = 6;
            }
            return;
        }
        self.place_block(hit, stack);
    }

    fn place_block(&mut self, hit: Hit, stack: Stack) {
        let Some(mut state) = items().block[stack.item as usize] else {
            return;
        };
        let b = rc_data::blocks();
        let it_name = items().name(stack.item);
        // Wall variants for torches/signs on walls.
        let wall = match it_name {
            "torch" => Some("wall_torch"),
            "soul_torch" => Some("soul_wall_torch"),
            n if n.ends_with("_sign") && !n.contains("hanging") => None,
            _ => None,
        };
        if let (Some(w), true) = (wall, hit.normal[1] == 0) {
            state = b.default_state(w);
        } else if (it_name == "torch" || it_name == "soul_torch") && hit.normal[1] != 1 {
            return;
        }
        let clicked = self.world.get_block(hit.pos);
        // Clicking a bottom slab of the same kind from above makes it double.
        if name_of(clicked) == name_of(state)
            && b.get(clicked, "type") == Some("bottom")
            && hit.normal == [0, 1, 0]
        {
            self.set_block(hit.pos, b.with(clicked, "type", "double"));
            self.block_sound(state, "place", hit.pos, 1.0, 0.8);
            if self.player.mode != GameMode::Creative {
                self.inv.consume_held();
            }
            self.swing = 6;
            return;
        }
        let p = if replaceable(clicked)
            && clicked != BlockStateId::AIR
            && b.fluid(clicked).is_empty()
        {
            hit.pos
        } else {
            BlockPos::new(
                hit.pos.x + hit.normal[0],
                hit.pos.y + hit.normal[1],
                hit.pos.z + hit.normal[2],
            )
        };
        if !replaceable(self.world.get_block(p)) || !(-64..320).contains(&p.y) {
            return;
        }
        let yaw = self.player.yaw.rem_euclid(360.0);
        let facing = match ((yaw + 45.0) / 90.0) as i32 % 4 {
            0 => "south",
            1 => "west",
            2 => "north",
            _ => "east",
        };
        let opposite = opposite_of(facing);
        if b.get(state, "axis").is_some() {
            let axis = match hit.normal {
                [_, 0, 0] if hit.normal[0] != 0 => "x",
                [0, 0, _] if hit.normal[2] != 0 => "z",
                _ => "y",
            };
            state = b.with(state, "axis", axis);
        }
        let n = name_of(state);
        if b.get(state, "facing").is_some() {
            // Wall-mounted things face away from the wall; most blocks face the player.
            let f = if wall.is_some() || n.ends_with("wall_torch") {
                dir_name(hit.normal)
            } else if n.ends_with("_stairs")
                || n.ends_with("_door")
                || n.ends_with("fence_gate")
                || n.ends_with("_bed")
                || n.ends_with("_trapdoor")
            {
                facing
            } else {
                opposite
            };
            state = b.with(state, "facing", f);
            if b.get(state, "facing") != Some(f) {
                state = b.with(state, "facing", facing);
            }
        }
        let upper_half = hit.normal[1] == -1 || (hit.normal[1] == 0 && hit.local[1] > 0.5);
        if b.get(state, "type") == Some("bottom") && upper_half {
            state = b.with(state, "type", "top");
        }
        if b.get(state, "half") == Some("bottom") && upper_half {
            state = b.with(state, "half", "top");
        }
        if b.get(state, "waterlogged").is_some() && b.fluid(self.world.get_block(p)).is_water() {
            state = b.with(state, "waterlogged", "true");
        }
        if n.ends_with("leaves") {
            state = b.with(state, "persistent", "true");
        }
        // Don't place inside the player or mobs.
        let pb = self.player.aabb();
        for bb in b.collision(state) {
            let block = rc_physics::Aabb::new(
                [
                    f64::from(p.x) + bb[0],
                    f64::from(p.y) + bb[1],
                    f64::from(p.z) + bb[2],
                ],
                [
                    f64::from(p.x) + bb[3],
                    f64::from(p.y) + bb[4],
                    f64::from(p.z) + bb[5],
                ],
            );
            if (block.intersects(&pb) && self.player.mode != GameMode::Spectator)
                || self
                    .entities
                    .list
                    .iter()
                    .any(|e| matches!(e.kind, Kind::Mob(_)) && e.aabb().intersects(&block))
            {
                return;
            }
        }
        // Two-block structures need room for their second half.
        let second = if b.get(state, "half") == Some("lower") {
            Some((
                BlockPos::new(p.x, p.y + 1, p.z),
                b.with(state, "half", "upper"),
            ))
        } else if b.get(state, "part") == Some("foot") {
            let (dx, dz) = facing_xz(b.get(state, "facing").unwrap_or("north"));
            Some((
                BlockPos::new(p.x + dx, p.y, p.z + dz),
                b.with(state, "part", "head"),
            ))
        } else {
            None
        };
        if let Some((q, _)) = second
            && !replaceable(self.world.get_block(q))
        {
            return;
        }
        // Plants need suitable ground.
        if b.collision(state).is_empty()
            && b.fluid(state).is_empty()
            && !n.contains("torch")
            && !n.contains("vine")
            && !n.contains("lichen")
            && !n.contains("ladder")
        {
            let below = self.world.get_block(BlockPos::new(p.x, p.y - 1, p.z));
            if b.collision(below).is_empty() {
                return;
            }
        }
        self.set_block(p, state);
        if let Some((q, s2)) = second {
            self.set_block(q, s2);
        }
        if let Some(ns) = blocklogic::update_shape(&self.world, p) {
            self.world.set_block(p, ns);
        }
        self.block_sound(state, "place", p, 1.0, 0.8);
        self.swing = 6;
        if self.player.mode == GameMode::Survival {
            self.inv.consume_held();
        }
    }

    pub fn respawn(&mut self) {
        let mode = self.player.mode;
        let keep = rule_bool(&self.world.meta, "keepInventory");
        let xp = (
            self.player.xp_level,
            self.player.xp_progress,
            self.player.xp_total,
        );
        self.player = Player::default();
        self.player.mode = mode;
        if keep {
            (
                self.player.xp_level,
                self.player.xp_progress,
                self.player.xp_total,
            ) = xp;
        }
        self.player.pos = [
            f64::from(self.spawn.x) + 0.5,
            f64::from(self.spawn.y),
            f64::from(self.spawn.z) + 0.5,
        ];
        self.player.prev_pos = self.player.pos;
        if mode == GameMode::Survival && !keep {
            self.inv = Inventory::default();
        }
    }

    /// Return crafting-grid items to the inventory (on closing a GUI).
    pub fn close_container(&mut self) {
        for i in 0..self.craft.len() {
            if let Some(s) = self.craft[i].take()
                && let Some(left) = self.inv.add_stack(s)
            {
                self.entities
                    .spawn(Entity::item(left, self.player.eye(1.0), 40));
            }
        }
        if let Some(c) = self.inv.cursor.take()
            && let Some(left) = self.inv.add_stack(c)
        {
            self.entities
                .spawn(Entity::item(left, self.player.eye(1.0), 40));
        }
        if let Some(Container::Chest { pos }) = self.container {
            let n = name_of(self.world.get_block(pos));
            let ev = if n == "barrel" {
                "block.barrel.close"
            } else {
                "block.chest.close"
            };
            self.sound(
                ev,
                Some([
                    f64::from(pos.x) + 0.5,
                    f64::from(pos.y) + 0.5,
                    f64::from(pos.z) + 0.5,
                ]),
                0.5,
                1.0,
                rc_audio::Category::Blocks,
            );
            self.world.mark_dirty(pos);
        }
        if let Some(Container::Furnace { pos }) = self.container {
            self.world.mark_dirty(pos);
        }
        self.container = None;
    }

    /// Drop the held item (Q).
    pub fn drop_held(&mut self, whole: bool) {
        let Some(st) = self.inv.held() else { return };
        let n = if whole { st.count } else { 1 };
        let rest = st.count - n;
        self.inv
            .set_held((rest > 0).then_some(Stack { count: rest, ..st }));
        let d = self.player.look_dir();
        let mut e = Entity::item(
            Stack { count: n, ..st },
            [
                self.player.pos[0],
                self.player.pos[1] + 1.32,
                self.player.pos[2],
            ],
            40,
        );
        e.vel = [d[0] * 0.3, d[1] * 0.3 + 0.1, d[2] * 0.3];
        self.entities.spawn(e);
    }

    /// Persist player, inventory and entities.
    pub fn save(&mut self) {
        self.close_container();
        let sp = SavedPlayer {
            player: self.player.clone(),
            inventory: self.inv.clone(),
        };
        self.world.meta.player = serde_json::to_value(&sp).ok();
        self.world.meta.game_mode = self.player.mode.name().to_owned();
        self.world.meta.difficulty = self.difficulty.name().to_owned();
        self.stash_all_entities();
        if let Err(e) = self.world.save_all() {
            tracing::warn!("saving world: {e:#}");
        }
    }

    /// Run a chat line (command or message).
    pub fn submit_chat(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        if let Some(cmd) = line.strip_prefix('/') {
            let out = crate::commands::execute(self, cmd);
            for l in out.lines() {
                self.message(l.to_owned());
            }
        } else {
            self.message(format!("<Player> {line}"));
        }
    }

    /// Outline boxes of the targeted block.
    #[must_use]
    pub fn target_outline(&self) -> Option<rc_render::terrain::Outline> {
        if let Some(i) = self.target_entity
            && let Some(e) = self.entities.list.get(i)
        {
            let bb = e.aabb();
            let p = [
                bb.min[0].floor() as i32,
                bb.min[1].floor() as i32,
                bb.min[2].floor() as i32,
            ];
            return Some(rc_render::terrain::Outline {
                pos: p,
                boxes: vec![[
                    bb.min[0] - f64::from(p[0]),
                    bb.min[1] - f64::from(p[1]),
                    bb.min[2] - f64::from(p[2]),
                    bb.max[0] - f64::from(p[0]),
                    bb.max[1] - f64::from(p[1]),
                    bb.max[2] - f64::from(p[2]),
                ]],
            });
        }
        let h = self.target?;
        let s = self.world.get_block(h.pos);
        Some(rc_render::terrain::Outline {
            pos: [h.pos.x, h.pos.y, h.pos.z],
            boxes: outline_boxes(s),
        })
    }

    #[must_use]
    pub fn player_chunk(&self) -> ChunkPos {
        ChunkPos::new(
            (self.player.pos[0].floor() as i32) >> 4,
            (self.player.pos[2].floor() as i32) >> 4,
        )
    }
}

fn b_name_is_fluid(s: BlockStateId) -> bool {
    matches!(name_of(s), "water" | "lava")
}

#[must_use]
pub fn facing_xz(f: &str) -> (i32, i32) {
    match f {
        "north" => (0, -1),
        "south" => (0, 1),
        "east" => (1, 0),
        _ => (-1, 0),
    }
}

fn opposite_of(f: &str) -> &'static str {
    match f {
        "south" => "north",
        "west" => "east",
        "north" => "south",
        _ => "west",
    }
}

fn dir_name(n: [i32; 3]) -> &'static str {
    match n {
        [1, _, _] => "east",
        [-1, _, _] => "west",
        [_, _, 1] => "south",
        _ => "north",
    }
}

/// Sets the per-thread player info read by mob AI.
struct PlayerInfoGuard;

impl PlayerInfoGuard {
    fn set(p: PlayerInfo) {
        mobs::PLAYER.with(|c| c.set(p));
    }
}
