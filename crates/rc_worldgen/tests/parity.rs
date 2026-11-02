//! Seed-exact parity against chunks saved by vanilla 1.21.1 (ADR-012).
//!
//! Reference chunks live in `tests/parity/<world>/` (raw chunk NBT, zlib)
//! and were saved by the real game at the `carvers` status, i.e. after
//! biomes, noise, surface and carvers but before that chunk's own features.
//! Neighbouring chunks may already have written features into them, so the
//! block check only compares positions where *both* sides are base terrain.

use std::io::Read;
use std::path::PathBuf;

use rc_core::{BlockStateId, ChunkPos};
use rc_worldgen::{Dimension, DimensionGen};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/parity")
}

struct Case {
    world: String,
    seed: i64,
    dim: Dimension,
    chunk: rc_anvil::AnvilChunk,
}

fn cases() -> Vec<Case> {
    let mut out = Vec::new();
    let mut worlds: Vec<_> = std::fs::read_dir(root())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .collect();
    worlds.sort_by_key(std::fs::DirEntry::path);
    for w in worlds {
        let seed: i64 = std::fs::read_to_string(w.path().join("seed.txt"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let mut files: Vec<_> = std::fs::read_dir(w.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect();
        files.sort();
        for f in files {
            let name = f.file_name().unwrap().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".nbt.zlib") else {
                continue;
            };
            let dim = match stem.split('_').next().unwrap() {
                "nether" => Dimension::Nether,
                "end" => Dimension::End,
                _ => Dimension::Overworld,
            };
            let mut raw = Vec::new();
            flate2::read::ZlibDecoder::new(std::fs::File::open(&f).unwrap())
                .read_to_end(&mut raw)
                .unwrap();
            let tag = rc_anvil::nbt::parse(&raw).unwrap();
            out.push(Case {
                world: w.file_name().to_string_lossy().to_string(),
                seed,
                dim,
                chunk: rc_anvil::AnvilChunk::from_nbt(&tag).unwrap(),
            });
        }
    }
    assert!(!out.is_empty(), "no parity reference chunks found");
    out
}

fn base_terrain(name: &str) -> bool {
    matches!(
        name,
        "air"
            | "cave_air"
            | "stone"
            | "deepslate"
            | "water"
            | "lava"
            | "bedrock"
            | "netherrack"
            | "end_stone"
    )
}

#[test]
fn biomes_and_terrain_match_vanilla() {
    let blocks = rc_data::blocks();
    let biomes = rc_worldgen::biome::registry();
    let mut gens: Vec<((i64, Dimension), DimensionGen)> = Vec::new();
    let mut failures = Vec::new();
    for case in cases() {
        let key = (case.seed, case.dim);
        if !gens.iter().any(|(k, _)| *k == key) {
            gens.push((key, DimensionGen::new(case.dim, case.seed)));
        }
        let dg = &gens.iter().find(|(k, _)| *k == key).unwrap().1;
        let ac = &case.chunk;
        let pc = dg.generate_to_carvers(ChunkPos::new(ac.x, ac.z));
        let (min_y, max_y) = (dg.settings.min_y, dg.settings.min_y + dg.settings.height);
        let mut biome_bad = 0;
        for qy in (min_y >> 2)..(max_y >> 2) {
            for qz in 0..4 {
                for qx in 0..4 {
                    if let Some(exp) = ac.biome(qx, qy, qz)
                        && biomes.get(pc.biome(qx as i32, qy, qz as i32)).name != exp
                    {
                        biome_bad += 1;
                    }
                }
            }
        }
        let mut terrain_bad = 0;
        let mut example = String::new();
        for y in min_y..max_y {
            for z in 0..16 {
                for x in 0..16 {
                    let exp = ac.block(x, y, z).map_or(BlockStateId::AIR, |s| {
                        blocks.parse_state(&s.to_state_string()).unwrap()
                    });
                    let got = pc.get((ac.x << 4) + x as i32, y, (ac.z << 4) + z as i32);
                    if exp == got {
                        continue;
                    }
                    let (e, g) = (&blocks.def(exp).name, &blocks.def(got).name);
                    if base_terrain(e) && base_terrain(g) {
                        terrain_bad += 1;
                        if example.is_empty() {
                            example = format!("{x},{y},{z}: expected {e} got {g}");
                        }
                    }
                }
            }
        }
        if biome_bad > 0 || terrain_bad > 0 {
            failures.push(format!(
                "{} {:?} chunk {},{}: {biome_bad} biome cells, {terrain_bad} terrain blocks differ ({example})",
                case.world, case.dim, ac.x, ac.z
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "parity failures:\n{}",
        failures.join("\n")
    );
}
