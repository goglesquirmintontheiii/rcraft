//! Java Edition world import/export (Anvil, data version 3955 = 1.21.1).
//!
//! Imports blocks, biomes, the player (position, inventory, health, food,
//! XP), time, weather and game rules; chunks that are not fully generated
//! are left out and regenerate identically from the seed. Export writes a
//! level.dat plus overworld region files that Java 1.21.1 opens directly
//! (lighting and heightmaps are recomputed by the game on load).

use std::path::{Path, PathBuf};

use ahash::AHashMap;
use anyhow::{Context as _, Result, bail};
use rc_anvil::{AnvilChunk, AnvilSection, Tag, nbt};
use rc_core::{BlockStateId, ChunkPos, SECTION_COUNT};
use rc_world::rcw::{DimStorage, IdMap, WorldMeta};
use rc_world::{Chunk, PalettedContainer};
use tracing::info;

use crate::items::{Inventory, Stack, items};
use crate::player::{GameMode, Player};

const DATA_VERSION: i32 = 3955;

/// Java worlds in the player's `.minecraft/saves`.
#[must_use]
pub fn find_java_worlds() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for mc in rc_assets::source::game_dirs() {
        let Ok(rd) = std::fs::read_dir(mc.join("saves")) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.join("level.dat").is_file() {
                let name = read_level(&p)
                    .ok()
                    .and_then(|t| {
                        t.get("Data")
                            .and_then(|d| d.get("LevelName"))
                            .and_then(Tag::as_str)
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| e.file_name().to_string_lossy().into_owned());
                out.push((name, p));
            }
        }
    }
    out.sort();
    out
}

fn read_level(dir: &Path) -> Result<Tag> {
    let b = std::fs::read(dir.join("level.dat")).context("reading level.dat")?;
    nbt::parse_compressed(&b)
}

fn tag_f64(t: &Tag) -> f64 {
    match t {
        Tag::Double(v) => *v,
        Tag::Float(v) => f64::from(*v),
        other => other.as_i64().unwrap_or(0) as f64,
    }
}

fn java_slot_to_ours(slot: i64) -> Option<usize> {
    match slot {
        0..=35 => Some(slot as usize),
        100..=103 => Some(36 + (slot - 100) as usize),
        -106 => Some(40),
        _ => None,
    }
}

fn ours_to_java_slot(i: usize) -> i8 {
    match i {
        0..=35 => i as i8,
        36..=39 => 100 + (i - 36) as i8,
        _ => -106,
    }
}

/// Import a Java world folder into `dest` (an RCW world directory).
pub fn import(java_dir: &Path, dest: &Path) -> Result<String> {
    let level = read_level(java_dir)?;
    let data = level.get("Data").context("level.dat has no Data")?;
    let dv = data.get("DataVersion").and_then(Tag::as_i64).unwrap_or(0);
    if dv > i64::from(DATA_VERSION) {
        bail!(
            "this world was saved by a newer Minecraft (data version {dv}); RustCraft reads 1.21.1 (3955) and older 1.18+ worlds"
        );
    }
    let seed = data
        .get("WorldGenSettings")
        .and_then(|w| w.get("seed"))
        .or_else(|| data.get("RandomSeed"))
        .and_then(Tag::as_i64)
        .unwrap_or(0);
    let name = data
        .get("LevelName")
        .and_then(Tag::as_str)
        .unwrap_or("Imported World")
        .to_owned();
    let blocks = rc_data::blocks();
    let reg = rc_worldgen::biome::registry();
    std::fs::create_dir_all(dest)?;
    let storage = DimStorage::new(dest.join("dim/overworld"))?;
    let map = IdMap::identity(blocks.len(), reg.biomes.len());
    let mut state_cache: AHashMap<String, BlockStateId> = AHashMap::new();
    let mut count = 0usize;
    let region_dir = java_dir.join("region");
    for e in std::fs::read_dir(&region_dir)
        .with_context(|| format!("no region folder in {}", java_dir.display()))?
        .flatten()
    {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "mca") {
            continue;
        }
        let Ok(region) = rc_anvil::RegionFile::open(&p) else {
            continue;
        };
        for (lx, lz) in region.present() {
            let Ok(Some(tag)) = region.read_chunk(lx, lz) else {
                continue;
            };
            let Ok(ac) = AnvilChunk::from_nbt(&tag) else {
                continue;
            };
            if ac.status != "minecraft:full" && ac.status != "full" {
                continue;
            }
            let mut c = Chunk::new_empty(ChunkPos::new(ac.x, ac.z));
            let mut vals = vec![0u32; 4096];
            let mut bvals = vec![0u32; 64];
            for s in &ac.sections {
                let si = i32::from(s.y) + 4;
                if !(0..SECTION_COUNT as i32).contains(&si) || s.block_palette.is_empty() {
                    continue;
                }
                let ids: Vec<u32> = s
                    .block_palette
                    .iter()
                    .map(|st| {
                        let key = st.to_state_string();
                        state_cache
                            .entry(key.clone())
                            .or_insert_with(|| {
                                blocks.parse_state(&key).unwrap_or(BlockStateId::AIR)
                            })
                            .0
                    })
                    .collect();
                for (i, v) in vals.iter_mut().enumerate() {
                    *v = ids.get(s.blocks[i] as usize).copied().unwrap_or(0);
                }
                let sec = &mut c.sections[si as usize];
                sec.blocks = PalettedContainer::from_values(&vals);
                let bids: Vec<u32> = s
                    .biome_palette
                    .iter()
                    .map(|n| reg.id(n).map_or(0, |b| b.0))
                    .collect();
                for (i, v) in bvals.iter_mut().enumerate() {
                    *v = bids.get(s.biomes[i] as usize).copied().unwrap_or(0);
                }
                sec.biomes = PalettedContainer::from_values(&bvals);
            }
            // Heightmap.
            for z in 0..16 {
                for x in 0..16 {
                    let mut h = -65;
                    'col: for y in (-64..320).rev() {
                        if c.get_block(rc_core::BlockPos::new(ac.x * 16 + x, y, ac.z * 16 + z))
                            != BlockStateId::AIR
                        {
                            h = y;
                            break 'col;
                        }
                    }
                    c.height_surface[(z * 16 + x) as usize] = h as i16;
                }
            }
            storage.save(&c, &map, DATA_VERSION as u32)?;
            count += 1;
        }
    }
    storage.close()?;
    // Header and player.
    let block_states = (0..blocks.len() as u32)
        .map(|i| blocks.to_string(BlockStateId(i)))
        .collect();
    let biomes = reg.biomes.iter().map(|b| b.name.clone()).collect();
    let mut rules = std::collections::BTreeMap::new();
    for (k, v) in data.get("GameRules").map(Tag::compound).unwrap_or(&[]) {
        if let Some(s) = v.as_str() {
            rules.insert(k.clone(), s.to_owned());
        }
    }
    let mode = match data.get("GameType").and_then(Tag::as_i64).unwrap_or(0) {
        1 => "creative",
        3 => "spectator",
        _ => "survival",
    };
    let mut player_json = None;
    if let Some(pt) = data.get("Player") {
        let mut p = Player::default();
        let pos = pt.get("Pos").map(Tag::as_list).unwrap_or(&[]);
        if pos.len() == 3 {
            p.pos = [tag_f64(&pos[0]), tag_f64(&pos[1]), tag_f64(&pos[2])];
        }
        let rot = pt.get("Rotation").map(Tag::as_list).unwrap_or(&[]);
        if rot.len() == 2 {
            p.yaw = tag_f64(&rot[0]) as f32;
            p.pitch = tag_f64(&rot[1]) as f32;
        }
        p.health = pt.get("Health").map_or(20.0, tag_f64) as f32;
        p.food = pt.get("foodLevel").and_then(Tag::as_i64).unwrap_or(20) as u32;
        p.saturation = pt.get("foodSaturationLevel").map_or(5.0, tag_f64) as f32;
        p.xp_level = pt.get("XpLevel").and_then(Tag::as_i64).unwrap_or(0) as u32;
        p.xp_progress = pt.get("XpP").map_or(0.0, tag_f64) as f32;
        p.xp_total = pt.get("XpTotal").and_then(Tag::as_i64).unwrap_or(0) as u32;
        p.mode = match pt.get("playerGameType").and_then(Tag::as_i64) {
            Some(1) => GameMode::Creative,
            Some(3) => GameMode::Spectator,
            _ => GameMode::parse(mode).unwrap_or_default(),
        };
        p.flying = pt
            .get("abilities")
            .and_then(|a| a.get("flying"))
            .and_then(Tag::as_i64)
            == Some(1);
        let mut inv = Inventory {
            selected: pt
                .get("SelectedItemSlot")
                .and_then(Tag::as_i64)
                .unwrap_or(0)
                .clamp(0, 8) as usize,
            ..Inventory::default()
        };
        for it in pt.get("Inventory").map(Tag::as_list).unwrap_or(&[]) {
            let Some(slot) = it
                .get("Slot")
                .and_then(Tag::as_i64)
                .and_then(java_slot_to_ours)
            else {
                continue;
            };
            let Some(id) = it
                .get("id")
                .and_then(Tag::as_str)
                .and_then(|n| items().id(n))
            else {
                continue;
            };
            let count = it
                .get("count")
                .or_else(|| it.get("Count"))
                .and_then(Tag::as_i64)
                .unwrap_or(1)
                .clamp(1, 99) as u8;
            let damage = it
                .get("components")
                .and_then(|c| c.get("minecraft:damage"))
                .and_then(Tag::as_i64)
                .unwrap_or(0) as u16;
            inv.slots[slot] = Some(Stack {
                item: id,
                count,
                damage,
            });
        }
        player_json = crate::game::saved_player_json(&p, &inv);
    }
    let get_i = |k: &str| data.get(k).and_then(Tag::as_i64).unwrap_or(0);
    let meta = WorldMeta {
        name: name.clone(),
        seed,
        data_version: DATA_VERSION as u32,
        game_version: rc_core::version::VERSION.to_owned(),
        spawn: [
            get_i("SpawnX") as i32,
            get_i("SpawnY") as i32,
            get_i("SpawnZ") as i32,
        ],
        time: get_i("Time"),
        day_time: get_i("DayTime"),
        last_played_ms: 0,
        game_mode: mode.to_owned(),
        difficulty: match get_i("Difficulty") {
            0 => "peaceful",
            1 => "easy",
            3 => "hard",
            _ => "normal",
        }
        .to_owned(),
        player: player_json,
        block_states,
        biomes,
        raining: get_i("raining") != 0,
        thundering: get_i("thundering") != 0,
        rain_time: get_i("rainTime"),
        thunder_time: get_i("thunderTime"),
        clear_weather_time: get_i("clearWeatherTime"),
        rules,
        world_type: "normal".into(),
    };
    meta.write(&dest.join("world.rcw"))?;
    info!(chunks = count, %name, "imported Java world");
    Ok(format!("Imported \"{name}\": {count} chunks"))
}

fn split_state(s: &str) -> rc_anvil::chunk::AnvilState {
    let (name, props) = s
        .split_once('[')
        .map_or((s, ""), |(n, p)| (n, p.trim_end_matches(']')));
    let mut props: Vec<(String, String)> = props
        .split(',')
        .filter_map(|kv| {
            kv.split_once('=')
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
        })
        .collect();
    props.sort();
    let name = if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    };
    rc_anvil::chunk::AnvilState { name, props }
}

/// Export the RCW world in `src` to a new Java world folder `dest`.
pub fn export(src: &Path, dest: &Path) -> Result<String> {
    let meta = WorldMeta::read(&src.join("world.rcw"))?;
    let blocks = rc_data::blocks();
    let reg = rc_worldgen::biome::registry();
    let map = IdMap::from_tables(
        &meta.block_states,
        &meta.biomes,
        blocks.len(),
        reg.biomes.len(),
        |s| blocks.parse_state(s).ok(),
        |s| reg.id(s),
    );
    std::fs::create_dir_all(dest.join("region"))?;
    let dim = src.join("dim/overworld");
    let mut regions: AHashMap<(i32, i32), rc_anvil::region::RegionWriter> = AHashMap::new();
    let mut count = 0;
    let mut state_names: AHashMap<u32, rc_anvil::chunk::AnvilState> = AHashMap::new();
    for e in std::fs::read_dir(&dim)
        .with_context(|| format!("reading {}", dim.display()))?
        .flatten()
    {
        let p = e.path();
        let fname = p
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        let parts: Vec<&str> = fname.split('.').collect();
        if parts.len() != 4 || parts[3] != "rcr" {
            continue;
        }
        let (Ok(rx), Ok(rz)) = (parts[1].parse::<i32>(), parts[2].parse::<i32>()) else {
            continue;
        };
        let mut rf = rc_world::rcw::RegionFile::open(&p)?;
        for i in 0..1024 {
            let pos = ChunkPos::new(rx * 32 + (i % 32), rz * 32 + (i / 32));
            let Some(rec) = rf.read(pos)? else { continue };
            let Ok(c) = rc_world::rcw::decode_chunk(&rec, &map) else {
                continue;
            };
            let mut sections = Vec::new();
            for (si, sec) in c.sections.iter().enumerate() {
                let mut palette: Vec<u32> = Vec::new();
                let mut idx = vec![0u16; 4096];
                for (k, slot) in idx.iter_mut().enumerate() {
                    let v = sec.blocks.get(k);
                    let pi = match palette.iter().position(|&x| x == v) {
                        Some(pi) => pi,
                        None => {
                            palette.push(v);
                            palette.len() - 1
                        }
                    };
                    *slot = pi as u16;
                }
                let block_palette = palette
                    .iter()
                    .map(|&v| {
                        state_names
                            .entry(v)
                            .or_insert_with(|| split_state(&blocks.to_string(BlockStateId(v))))
                            .clone()
                    })
                    .collect();
                let mut bpal: Vec<u32> = Vec::new();
                let mut bidx = vec![0u16; 64];
                for (k, slot) in bidx.iter_mut().enumerate() {
                    let v = sec.biomes.get(k);
                    let pi = match bpal.iter().position(|&x| x == v) {
                        Some(pi) => pi,
                        None => {
                            bpal.push(v);
                            bpal.len() - 1
                        }
                    };
                    *slot = pi as u16;
                }
                sections.push(AnvilSection {
                    y: si as i8 - 4,
                    block_palette,
                    blocks: idx,
                    biome_palette: bpal
                        .iter()
                        .map(|&b| reg.get(rc_core::BiomeId(b)).name.clone())
                        .collect(),
                    biomes: bidx,
                });
            }
            let ac = AnvilChunk {
                x: c.pos.x,
                z: c.pos.z,
                status: "minecraft:full".into(),
                data_version: DATA_VERSION,
                sections,
                heightmaps: Vec::new(),
            };
            regions.entry((pos.x >> 5, pos.z >> 5)).or_default().add(
                pos.x & 31,
                pos.z & 31,
                &ac.to_nbt(),
            )?;
            count += 1;
        }
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as u32);
    for ((rx, rz), w) in regions {
        std::fs::write(
            dest.join("region").join(format!("r.{rx}.{rz}.mca")),
            w.finish(ts),
        )?;
    }
    std::fs::write(dest.join("level.dat"), nbt::write_gzip(&level_dat(&meta))?)?;
    info!(chunks = count, "exported Java world to {}", dest.display());
    Ok(format!("Exported {count} chunks to {}", dest.display()))
}

fn gen_settings(settings: &str, source: Tag) -> Tag {
    nbt::compound(vec![
        ("type", Tag::String("minecraft:noise".into())),
        ("settings", Tag::String(format!("minecraft:{settings}"))),
        ("biome_source", source),
    ])
}

fn level_dat(meta: &WorldMeta) -> Tag {
    let mn = |preset: &str| {
        nbt::compound(vec![
            ("type", Tag::String("minecraft:multi_noise".into())),
            ("preset", Tag::String(format!("minecraft:{preset}"))),
        ])
    };
    let dims = nbt::compound(vec![
        (
            "minecraft:overworld",
            nbt::compound(vec![
                ("type", Tag::String("minecraft:overworld".into())),
                ("generator", gen_settings("overworld", mn("overworld"))),
            ]),
        ),
        (
            "minecraft:the_nether",
            nbt::compound(vec![
                ("type", Tag::String("minecraft:the_nether".into())),
                ("generator", gen_settings("nether", mn("nether"))),
            ]),
        ),
        (
            "minecraft:the_end",
            nbt::compound(vec![
                ("type", Tag::String("minecraft:the_end".into())),
                (
                    "generator",
                    gen_settings(
                        "end",
                        nbt::compound(vec![("type", Tag::String("minecraft:the_end".into()))]),
                    ),
                ),
            ]),
        ),
    ]);
    let game_type = match meta.game_mode.as_str() {
        "creative" => 1,
        "spectator" => 3,
        _ => 0,
    };
    let rules = Tag::Compound(
        meta.rules
            .iter()
            .map(|(k, v)| (k.clone(), Tag::String(v.clone())))
            .collect(),
    );
    let mut data = vec![
        ("DataVersion", Tag::Int(DATA_VERSION)),
        ("version", Tag::Int(19133)),
        ("LevelName", Tag::String(meta.name.clone())),
        ("GameType", Tag::Int(game_type)),
        (
            "Difficulty",
            Tag::Byte(match meta.difficulty.as_str() {
                "peaceful" => 0,
                "easy" => 1,
                "hard" => 3,
                _ => 2,
            }),
        ),
        ("hardcore", Tag::Byte(0)),
        ("allowCommands", Tag::Byte(1)),
        ("initialized", Tag::Byte(1)),
        ("LastPlayed", Tag::Long(meta.last_played_ms as i64)),
        ("Time", Tag::Long(meta.time)),
        ("DayTime", Tag::Long(meta.day_time)),
        ("SpawnX", Tag::Int(meta.spawn[0])),
        ("SpawnY", Tag::Int(meta.spawn[1])),
        ("SpawnZ", Tag::Int(meta.spawn[2])),
        ("SpawnAngle", Tag::Float(0.0)),
        ("raining", Tag::Byte(i8::from(meta.raining))),
        ("rainTime", Tag::Int(meta.rain_time as i32)),
        ("thundering", Tag::Byte(i8::from(meta.thundering))),
        ("thunderTime", Tag::Int(meta.thunder_time as i32)),
        ("clearWeatherTime", Tag::Int(meta.clear_weather_time as i32)),
        ("WasModded", Tag::Byte(0)),
        (
            "ServerBrands",
            Tag::List(vec![Tag::String("rustcraft".into())]),
        ),
        ("GameRules", rules),
        (
            "WorldGenSettings",
            nbt::compound(vec![
                ("seed", Tag::Long(meta.seed)),
                ("generate_features", Tag::Byte(1)),
                ("bonus_chest", Tag::Byte(0)),
                ("dimensions", dims),
            ]),
        ),
        (
            "Version",
            nbt::compound(vec![
                ("Id", Tag::Int(DATA_VERSION)),
                ("Name", Tag::String("1.21.1".into())),
                ("Series", Tag::String("main".into())),
                ("Snapshot", Tag::Byte(0)),
            ]),
        ),
        (
            "DataPacks",
            nbt::compound(vec![
                ("Enabled", Tag::List(vec![Tag::String("vanilla".into())])),
                ("Disabled", Tag::List(Vec::new())),
            ]),
        ),
    ];
    if let Some((p, inv)) = meta
        .player
        .clone()
        .and_then(crate::game::parse_saved_player)
    {
        let items_list: Vec<Tag> = inv
            .slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let s = (*s)?;
                let mut v = vec![
                    ("Slot", Tag::Byte(ours_to_java_slot(i))),
                    (
                        "id",
                        Tag::String(format!("minecraft:{}", items().name(s.item))),
                    ),
                    ("count", Tag::Int(i32::from(s.count))),
                ];
                if s.damage > 0 {
                    v.push((
                        "components",
                        nbt::compound(vec![("minecraft:damage", Tag::Int(i32::from(s.damage)))]),
                    ));
                }
                Some(nbt::compound(v))
            })
            .collect();
        let mode = match p.mode {
            GameMode::Creative => 1,
            GameMode::Spectator => 3,
            GameMode::Survival => 0,
        };
        data.push((
            "Player",
            nbt::compound(vec![
                (
                    "Pos",
                    Tag::List(p.pos.iter().map(|v| Tag::Double(*v)).collect()),
                ),
                (
                    "Rotation",
                    Tag::List(vec![Tag::Float(p.yaw), Tag::Float(p.pitch)]),
                ),
                (
                    "Motion",
                    Tag::List(vec![Tag::Double(0.0), Tag::Double(0.0), Tag::Double(0.0)]),
                ),
                ("OnGround", Tag::Byte(1)),
                ("Health", Tag::Float(p.health)),
                ("foodLevel", Tag::Int(p.food as i32)),
                ("foodSaturationLevel", Tag::Float(p.saturation)),
                ("XpLevel", Tag::Int(p.xp_level as i32)),
                ("XpP", Tag::Float(p.xp_progress)),
                ("XpTotal", Tag::Int(p.xp_total as i32)),
                ("playerGameType", Tag::Int(mode)),
                ("SelectedItemSlot", Tag::Int(inv.selected as i32)),
                ("Air", Tag::Short(300)),
                ("Fire", Tag::Short(-20)),
                ("Dimension", Tag::String("minecraft:overworld".into())),
                ("Inventory", Tag::List(items_list)),
                (
                    "abilities",
                    nbt::compound(vec![
                        ("flying", Tag::Byte(i8::from(p.flying))),
                        ("mayfly", Tag::Byte(i8::from(mode == 1 || mode == 3))),
                        ("instabuild", Tag::Byte(i8::from(mode == 1))),
                        ("invulnerable", Tag::Byte(i8::from(mode == 1 || mode == 3))),
                        ("mayBuild", Tag::Byte(1)),
                        ("flySpeed", Tag::Float(0.05)),
                        ("walkSpeed", Tag::Float(0.1)),
                    ]),
                ),
            ]),
        ));
    }
    nbt::compound(vec![("Data", nbt::compound(data))])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_then_import_round_trip() {
        let base = std::env::temp_dir().join(format!("rc-java-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let src = base.join("rcw");
        let blocks = rc_data::blocks();
        let reg = rc_worldgen::biome::registry();
        let st = DimStorage::new(src.join("dim/overworld")).unwrap();
        let map = IdMap::identity(blocks.len(), reg.biomes.len());
        let mut c = Chunk::new_empty(ChunkPos::new(-1, 2));
        let log = blocks.parse_state("minecraft:oak_log[axis=x]").unwrap();
        c.set_block(rc_core::BlockPos::new(-10, 70, 33), log);
        c.set_block(
            rc_core::BlockPos::new(-10, -60, 33),
            blocks.default_state("stone"),
        );
        st.save(&c, &map, 3955).unwrap();
        st.close().unwrap();
        let meta = WorldMeta {
            name: "RT".into(),
            seed: 42,
            data_version: 3955,
            game_version: "t".into(),
            spawn: [1, 2, 3],
            time: 10,
            day_time: 20,
            last_played_ms: 0,
            game_mode: "creative".into(),
            difficulty: "easy".into(),
            player: None,
            block_states: (0..blocks.len() as u32)
                .map(|i| blocks.to_string(BlockStateId(i)))
                .collect(),
            biomes: reg.biomes.iter().map(|b| b.name.clone()).collect(),
            raining: true,
            thundering: false,
            rain_time: 5,
            thunder_time: 6,
            clear_weather_time: 0,
            rules: Default::default(),
            world_type: "normal".into(),
        };
        meta.write(&src.join("world.rcw")).unwrap();
        let java = base.join("java");
        export(&src, &java).unwrap();
        let back = base.join("back");
        import(&java, &back).unwrap();
        let m2 = WorldMeta::read(&back.join("world.rcw")).unwrap();
        assert_eq!(m2.seed, 42);
        assert_eq!(m2.game_mode, "creative");
        assert!(m2.raining);
        let st2 = DimStorage::new(back.join("dim/overworld")).unwrap();
        let c2 = st2.load(ChunkPos::new(-1, 2), &map).unwrap().unwrap();
        assert_eq!(c2.get_block(rc_core::BlockPos::new(-10, 70, 33)), log);
        assert_eq!(
            c2.get_block(rc_core::BlockPos::new(-10, -60, 33)),
            blocks.default_state("stone")
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
