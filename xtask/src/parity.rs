//! `cargo xtask parity <world-dir>` — regenerate chunks from a vanilla world's
//! seed and diff them against the chunks the game saved.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context as _, Result};
use rayon::prelude::*;
use rc_anvil::{AnvilChunk, RegionFile, nbt};
use rc_core::{BlockStateId, ChunkPos};
use rc_worldgen::{Dimension, DimensionGen};

#[derive(Default)]
struct Report {
    chunks: usize,
    blocks: u64,
    block_mismatch: u64,
    biome_cells: u64,
    biome_mismatch: u64,
    pairs: BTreeMap<(String, String), u64>,
    examples: Vec<String>,
}

pub fn seed_of(world: &Path) -> Result<i64> {
    let lvl = nbt::parse_compressed(&std::fs::read(world.join("level.dat"))?)?;
    lvl.get("Data")
        .and_then(|d| d.get("WorldGenSettings"))
        .and_then(|w| w.get("seed"))
        .and_then(nbt::Tag::as_i64)
        .context("seed not found in level.dat")
}

fn region_dir(world: &Path, dim: Dimension) -> std::path::PathBuf {
    match dim {
        Dimension::Overworld => world.join("region"),
        Dimension::Nether => world.join("DIM-1/region"),
        Dimension::End => world.join("DIM1/region"),
    }
}

/// Collect chunks with the given saved status.
pub fn collect(
    world: &Path,
    dim: Dimension,
    statuses: &[&str],
    limit: usize,
) -> Result<Vec<AnvilChunk>> {
    let mut out = Vec::new();
    let dir = region_dir(world, dim);
    let mut files: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    files.sort();
    'outer: for f in files {
        if std::fs::metadata(&f)?.len() < 8192 {
            continue;
        }
        let r = RegionFile::open(&f)?;
        for (lx, lz) in r.present() {
            let Some(t) = r.read_chunk(lx, lz)? else {
                continue;
            };
            let c = AnvilChunk::from_nbt(&t)?;
            if statuses.iter().any(|s| c.status == *s) {
                out.push(c);
                if out.len() >= limit {
                    break 'outer;
                }
            }
        }
    }
    Ok(out)
}

pub fn run(world: &Path, dim: Dimension, stage: &str, limit: usize) -> Result<()> {
    let seed = seed_of(world)?;
    println!(
        "world {} seed {seed} dim {dim:?} stage {stage}",
        world.display()
    );
    let t0 = std::time::Instant::now();
    let dg = DimensionGen::new(dim, seed);
    println!("generator built in {:?}", t0.elapsed());
    let statuses: &[&str] = match stage {
        "biomes" => &[
            "minecraft:biomes",
            "minecraft:carvers",
            "minecraft:full",
            "minecraft:initialize_light",
        ],
        "carvers" => &["minecraft:carvers"],
        _ => &["minecraft:full", "minecraft:initialize_light"],
    };
    let chunks = collect(world, dim, statuses, limit)?;
    println!("{} reference chunks", chunks.len());
    let blocks = rc_data::blocks();
    let biomes = rc_worldgen::biome::registry();
    let report = Mutex::new(Report::default());
    let t1 = std::time::Instant::now();
    chunks.par_iter().for_each(|ac| {
        let pos = ChunkPos::new(ac.x, ac.z);
        let mut local = Report {
            chunks: 1,
            ..Report::default()
        };
        if stage == "biomes" {
            let mut pc = rc_worldgen::ProtoChunk::new(pos, dg.settings.min_y, dg.settings.height);
            dg.fill_biomes(&mut pc);
            for qy in (dg.settings.min_y >> 2)..((dg.settings.min_y + dg.settings.height) >> 2) {
                for qz in 0..4 {
                    for qx in 0..4 {
                        let Some(exp) = ac.biome(qx, qy, qz) else {
                            continue;
                        };
                        let got = &biomes.get(pc.biome(qx as i32, qy, qz as i32)).name;
                        local.biome_cells += 1;
                        if got != exp {
                            local.biome_mismatch += 1;
                            *local
                                .pairs
                                .entry((exp.to_owned(), got.clone()))
                                .or_default() += 1;
                            if local.examples.len() < 3 {
                                local.examples.push(format!(
                                    "chunk {},{} quart {qx},{qy},{qz}: {exp} vs {got}",
                                    ac.x, ac.z
                                ));
                            }
                        }
                    }
                }
            }
        } else {
            let pc = dg.generate_to_carvers(pos);
            for y in dg.settings.min_y..(dg.settings.min_y + dg.settings.height) {
                for z in 0..16 {
                    for x in 0..16 {
                        let exp = ac.block(x, y, z).map_or(BlockStateId::AIR, |s| {
                            blocks
                                .parse_state(&s.to_state_string())
                                .unwrap_or(BlockStateId::AIR)
                        });
                        let got = pc.get((ac.x << 4) + x as i32, y, (ac.z << 4) + z as i32);
                        local.blocks += 1;
                        if exp != got {
                            local.block_mismatch += 1;
                            let e = blocks.to_string(exp);
                            let g = blocks.to_string(got);
                            let want = std::env::var("PARITY_PAIR").ok();
                            let hit = want.as_deref().is_none_or(|w| w == format!("{e}|{g}"));
                            if hit && local.examples.len() < 3 {
                                local.examples.push(format!(
                                    "chunk {},{} at {},{y},{}: expected {e} got {g}",
                                    ac.x,
                                    ac.z,
                                    (ac.x << 4) + x as i32,
                                    (ac.z << 4) + z as i32
                                ));
                            }
                            *local.pairs.entry((e, g)).or_default() += 1;
                        }
                    }
                }
            }
        }
        let mut r = report.lock().unwrap();
        r.chunks += local.chunks;
        r.blocks += local.blocks;
        r.block_mismatch += local.block_mismatch;
        r.biome_cells += local.biome_cells;
        r.biome_mismatch += local.biome_mismatch;
        for (k, v) in local.pairs {
            *r.pairs.entry(k).or_default() += v;
        }
        if r.examples.len() < 12 {
            r.examples.extend(local.examples);
        }
    });
    let r = report.into_inner().unwrap();
    println!("checked {} chunks in {:?}", r.chunks, t1.elapsed());
    if stage == "biomes" {
        println!(
            "biomes: {}/{} match ({:.4}%)",
            r.biome_cells - r.biome_mismatch,
            r.biome_cells,
            100.0 * (r.biome_cells - r.biome_mismatch) as f64 / r.biome_cells.max(1) as f64
        );
    } else {
        println!(
            "blocks: {}/{} match ({:.4}%)",
            r.blocks - r.block_mismatch,
            r.blocks,
            100.0 * (r.blocks - r.block_mismatch) as f64 / r.blocks.max(1) as f64
        );
    }
    let mut pairs: Vec<_> = r.pairs.into_iter().collect();
    pairs.sort_by_key(|p| std::cmp::Reverse(p.1));
    let top: usize = std::env::var("PARITY_TOP")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(15);
    for ((e, g), n) in pairs.iter().take(top) {
        println!("  {n:>8}  expected {e}  got {g}");
    }
    for e in &r.examples {
        println!("  e.g. {e}");
    }
    Ok(())
}
