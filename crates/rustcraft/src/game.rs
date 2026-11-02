//! In-world game state: world, player, inventory, interaction, chat and
//! commands. Runs at a fixed 20 ticks per second.

use rc_core::{BlockPos, BlockStateId, ChunkPos};
use serde::{Deserialize, Serialize};

use crate::items::{self, Inventory, items};
use crate::player::{GameMode, Hit, MoveInput, Player, outline_boxes};
use crate::world::World;

/// Input sampled for one tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct TickInput {
    pub mv: MoveInput,
    pub attack: bool,
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

#[derive(Clone, Debug)]
pub struct ChatLine {
    pub text: String,
    pub tick: u64,
}

pub struct Game {
    pub world: World,
    pub player: Player,
    pub inv: Inventory,
    pub chat: Vec<ChatLine>,
    pub target: Option<Hit>,
    pub breaking: Option<(BlockPos, f32)>,
    pub tick: u64,
    break_cooldown: u32,
    place_cooldown: u32,
    last_jump: Option<u64>,
    pub spawned: bool,
    pub spawn: BlockPos,
    pub swing: u32,
}

impl std::fmt::Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Game({:?})", self.world)
    }
}

/// Blocks that placement may replace.
fn replaceable(s: BlockStateId) -> bool {
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
        )
}

fn is_falling(s: BlockStateId) -> bool {
    let n = rc_data::blocks().def(s).name.as_str();
    matches!(
        n,
        "sand" | "red_sand" | "gravel" | "suspicious_sand" | "suspicious_gravel"
    ) || n.ends_with("concrete_powder")
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
        Self {
            world,
            player,
            inv,
            chat: Vec::new(),
            target: None,
            breaking: None,
            tick: 0,
            break_cooldown: 0,
            place_cooldown: 0,
            last_jump: None,
            spawned,
            spawn,
            swing: 0,
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

    /// Place the player on the surface once the spawn chunk exists.
    fn try_spawn(&mut self) -> bool {
        if self.spawned {
            return true;
        }
        let b = rc_data::blocks();
        // Search outward for dry land near the world spawn.
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
                    let Some(y) = self.world.surface_y(x, z) else {
                        return false;
                    };
                    let top = self.world.get_block(BlockPos::new(x, y, z));
                    if b.fluid(top).is_empty() && !b.collision(top).is_empty() && y > 0 {
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

    /// One game tick.
    pub fn tick(&mut self, input: &TickInput) {
        self.tick += 1;
        if !self.try_spawn() {
            return;
        }
        if !self.player_chunk_ready() {
            // Hold still until the ground under the player exists.
            self.player.prev_pos = self.player.pos;
            return;
        }
        // Double-tap jump toggles flight in creative.
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
        if self.player.dead() {
            return;
        }
        self.player.tick(&input.mv, &self.world);
        if self.swing > 0 {
            self.swing -= 1;
        }
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
        self.break_cooldown = self.break_cooldown.saturating_sub(1);
        self.place_cooldown = self.place_cooldown.saturating_sub(1);
        // Breaking.
        match (input.attack, self.target) {
            (true, Some(hit)) => {
                self.swing = 6;
                let s = self.world.get_block(hit.pos);
                if self.player.mode == GameMode::Creative {
                    if self.break_cooldown == 0 {
                        self.break_block(hit.pos, false);
                        self.break_cooldown = 6;
                    }
                    self.breaking = None;
                } else if let Some(secs) = items::break_seconds(s) {
                    let prog = match self.breaking {
                        Some((p, v)) if p == hit.pos => v,
                        _ => 0.0,
                    };
                    let mut step = if secs <= 0.0 {
                        1.0
                    } else {
                        1.0 / (secs * 20.0)
                    };
                    if self.player.eyes_in_water {
                        step /= 5.0;
                    }
                    if !self.player.on_ground && !self.player.flying {
                        step /= 5.0;
                    }
                    let np = prog + step;
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
        // Placing.
        if (input.use_pressed || (input.use_held && self.place_cooldown == 0))
            && let Some(hit) = self.target
        {
            self.place(hit);
            self.place_cooldown = 4;
        }
        if input.pick
            && let Some(hit) = self.target
            && self.player.mode == GameMode::Creative
            && let Some(it) = items().for_block(self.world.get_block(hit.pos))
        {
            self.inv.pick(it);
        }
    }

    fn break_block(&mut self, p: BlockPos, drops: bool) {
        let s = self.world.get_block(p);
        if s == BlockStateId::AIR
            || items::break_seconds(s).is_none() && self.player.mode != GameMode::Creative
        {
            return;
        }
        let b = rc_data::blocks();
        // Waterlogged blocks leave water behind.
        let replacement = if b.get(s, "waterlogged") == Some("true")
            || b.def(s).name == "seagrass"
            || b.def(s).name == "kelp"
            || b.def(s).name == "kelp_plant"
            || b.def(s).name == "tall_seagrass"
        {
            b.default_state("water")
        } else {
            BlockStateId::AIR
        };
        self.world.set_block(p, replacement);
        // Double plants come down together.
        if b.get(s, "half") == Some("lower") || b.get(s, "half") == Some("upper") {
            let other = if b.get(s, "half") == Some("lower") {
                BlockPos::new(p.x, p.y + 1, p.z)
            } else {
                BlockPos::new(p.x, p.y - 1, p.z)
            };
            if b.def(self.world.get_block(other)).name == b.def(s).name {
                self.world.set_block(other, BlockStateId::AIR);
            }
        }
        if drops
            && self.player.mode == GameMode::Survival
            && let Some(it) = items::drop_for(s)
        {
            self.inv.add(it, 1);
        }
        // Plants lose their support; falling blocks above drop.
        let above = BlockPos::new(p.x, p.y + 1, p.z);
        let sa = self.world.get_block(above);
        if sa != BlockStateId::AIR
            && b.collision(sa).is_empty()
            && b.fluid(sa).is_empty()
            && !b.def(sa).name.contains("vine")
        {
            self.break_block(above, drops);
        }
        self.settle(above);
    }

    /// Drop sand/gravel columns that lost support.
    fn settle(&mut self, mut p: BlockPos) {
        let b = rc_data::blocks();
        for _ in 0..64 {
            let s = self.world.get_block(p);
            if !is_falling(s) {
                return;
            }
            let mut y = p.y;
            while y > -64 {
                let below = self.world.get_block(BlockPos::new(p.x, y - 1, p.z));
                if !replaceable(below) && !b.collision(below).is_empty() {
                    break;
                }
                y -= 1;
            }
            if y == p.y {
                return;
            }
            self.world.set_block(p, BlockStateId::AIR);
            self.world.set_block(BlockPos::new(p.x, y, p.z), s);
            p = BlockPos::new(p.x, p.y + 1, p.z);
        }
    }

    fn place(&mut self, hit: Hit) {
        let Some(stack) = self.inv.held() else { return };
        let Some(mut state) = items().block[stack.item as usize] else {
            return;
        };
        let b = rc_data::blocks();
        let clicked = self.world.get_block(hit.pos);
        let p = if replaceable(clicked) && clicked != BlockStateId::AIR {
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
        // Orient the state from the click like vanilla's placement context.
        let yaw = self.player.yaw.rem_euclid(360.0);
        let facing = match ((yaw + 45.0) / 90.0) as i32 % 4 {
            0 => "south",
            1 => "west",
            2 => "north",
            _ => "east",
        };
        let opposite = match facing {
            "south" => "north",
            "west" => "east",
            "north" => "south",
            _ => "west",
        };
        if b.get(state, "axis").is_some() {
            let axis = match hit.normal {
                [0, _, 0] => "y",
                [_, 0, 0] => "x",
                _ => "z",
            };
            state = b.with(state, "axis", if hit.normal[1] != 0 { "y" } else { axis });
        }
        if b.get(state, "facing").is_some() {
            state = b.with(state, "facing", opposite);
            if b.get(state, "facing") != Some(opposite) {
                state = b.with(state, "facing", facing);
            }
        }
        if b.get(state, "type") == Some("bottom")
            && (hit.normal[1] == -1 || (hit.normal[1] == 0 && hit.local[1] > 0.5))
        {
            state = b.with(state, "type", "top");
        }
        if b.get(state, "half") == Some("bottom")
            && (hit.normal[1] == -1 || (hit.normal[1] == 0 && hit.local[1] > 0.5))
        {
            state = b.with(state, "half", "top");
        }
        if b.get(state, "waterlogged").is_some() && b.fluid(self.world.get_block(p)).is_water() {
            state = b.with(state, "waterlogged", "true");
        }
        // Don't place inside the player.
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
            if block.intersects(&pb) && self.player.mode != GameMode::Spectator {
                return;
            }
        }
        // Plants need ground.
        if b.collision(state).is_empty() && b.fluid(state).is_empty() {
            let below = self.world.get_block(BlockPos::new(p.x, p.y - 1, p.z));
            if b.collision(below).is_empty() {
                return;
            }
        }
        self.world.set_block(p, state);
        if b.get(state, "half") == Some("lower") {
            let up = BlockPos::new(p.x, p.y + 1, p.z);
            if replaceable(self.world.get_block(up)) {
                self.world.set_block(up, b.with(state, "half", "upper"));
            }
        }
        self.swing = 6;
        self.settle(p);
        if self.player.mode == GameMode::Survival {
            self.inv.consume_held();
        }
    }

    pub fn respawn(&mut self) {
        let mode = self.player.mode;
        self.player = Player::default();
        self.player.mode = mode;
        self.player.pos = [
            f64::from(self.spawn.x) + 0.5,
            f64::from(self.spawn.y),
            f64::from(self.spawn.z) + 0.5,
        ];
        self.player.prev_pos = self.player.pos;
        if mode == GameMode::Survival {
            self.inv = Inventory::default();
        }
    }

    /// Persist player and inventory into the world header.
    pub fn save(&mut self) {
        let sp = SavedPlayer {
            player: self.player.clone(),
            inventory: self.inv.clone(),
        };
        self.world.meta.player = serde_json::to_value(&sp).ok();
        self.world.meta.game_mode = self.player.mode.name().to_owned();
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
            let out = self.command(cmd);
            self.message(out);
        } else {
            self.message(format!("<Player> {line}"));
        }
    }

    fn command(&mut self, cmd: &str) -> String {
        let args: Vec<&str> = cmd.split_whitespace().collect();
        let coord = |s: &str, cur: f64| -> Option<f64> {
            if let Some(r) = s.strip_prefix('~') {
                Some(cur + if r.is_empty() { 0.0 } else { r.parse().ok()? })
            } else {
                s.parse().ok()
            }
        };
        match args.as_slice() {
            ["help"] => "§eCommands: /gamemode <mode>, /tp <x> <y> <z>, /give <item> [count], /time set <day|noon|night|midnight|ticks>, /seed, /kill, /clear, /spawnpoint".into(),
            ["gamemode", m] => match GameMode::parse(m) {
                Some(g) => {
                    self.player.mode = g;
                    if g == GameMode::Survival {
                        self.player.flying = false;
                    }
                    format!("Set own game mode to {}", g.name())
                }
                None => format!("§cUnknown game mode: {m}"),
            },
            ["tp", x, y, z] => {
                let p = self.player.pos;
                match (coord(x, p[0]), coord(y, p[1]), coord(z, p[2])) {
                    (Some(x), Some(y), Some(z)) => {
                        self.player.pos = [x, y, z];
                        self.player.prev_pos = self.player.pos;
                        self.player.vel = [0.0; 3];
                        format!("Teleported Player to {x:.2}, {y:.2}, {z:.2}")
                    }
                    _ => "§cInvalid position".into(),
                }
            }
            ["give", item] | ["give", "@s" | "@p", item] => self.give(item, 1),
            ["give", item, n] | ["give", "@s" | "@p", item, n] => self.give(item, n.parse().unwrap_or(1)),
            ["time", "set", t] => {
                let v = match *t {
                    "day" => 1000,
                    "noon" => 6000,
                    "night" => 13000,
                    "midnight" => 18000,
                    n => match n.parse::<i64>() {
                        Ok(v) => v,
                        Err(_) => return "§cInvalid time".into(),
                    },
                };
                self.world.meta.day_time = v;
                format!("Set the time to {v}")
            }
            ["seed"] => format!("Seed: [{}]", self.world.meta.seed),
            ["kill"] => {
                self.player.health = 0.0;
                self.player.last_damage_cause = "fell out of the world".into();
                "Killed Player".into()
            }
            ["clear"] => {
                self.inv = Inventory::default();
                "Cleared the inventory of Player".into()
            }
            ["spawnpoint"] => {
                let p = self.player.pos;
                self.spawn = BlockPos::new(p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32);
                self.world.meta.spawn = [self.spawn.x, self.spawn.y, self.spawn.z];
                format!("Set spawn point to {}, {}, {}", self.spawn.x, self.spawn.y, self.spawn.z)
            }
            _ => format!("§cUnknown or incomplete command: /{cmd}. Type /help for help."),
        }
    }

    fn give(&mut self, item: &str, n: u32) -> String {
        match items().id(item) {
            Some(id) if items().block[id as usize].is_some() => {
                let left = self.inv.add(id, n.min(6400).min(255) as u8);
                format!(
                    "Gave {} [{}] to Player",
                    n.saturating_sub(u32::from(left)),
                    items().display_name(id)
                )
            }
            Some(_) => "§cOnly block items exist in this version".into(),
            None => format!("§cUnknown item '{item}'"),
        }
    }

    /// Outline boxes of the targeted block.
    #[must_use]
    pub fn target_outline(&self) -> Option<rc_render::terrain::Outline> {
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
