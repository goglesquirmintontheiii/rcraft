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

/// All saved chunk statuses in the world, by position.
fn status_map(world: &Path, dim: Dimension) -> Result<ahash::AHashMap<ChunkPos, String>> {
    let mut m = ahash::AHashMap::new();
    for c in collect(
        world,
        dim,
        &[
            "minecraft:full",
            "minecraft:initialize_light",
            "minecraft:light",
            "minecraft:spawn",
            "minecraft:features",
            "minecraft:carvers",
            "minecraft:surface",
            "minecraft:noise",
            "minecraft:biomes",
        ],
        usize::MAX,
    )? {
        m.insert(ChunkPos::new(c.x, c.z), c.status.clone());
    }
    Ok(m)
}

fn decorated(s: &str) -> bool {
    matches!(
        s,
        "minecraft:full"
            | "minecraft:initialize_light"
            | "minecraft:light"
            | "minecraft:spawn"
            | "minecraft:features"
    )
}

/// Full-pipeline parity on an `n`×`n` block of chunks around the reference
/// chunk closest to `(cx, cz)`.
pub fn run_full(world: &Path, dim: Dimension, n: i32, around: Option<(i32, i32)>) -> Result<()> {
    let seed = seed_of(world)?;
    let dg = DimensionGen::new(dim, seed);
    let statuses = status_map(world, dim)?;
    // Choose a block of fully decorated chunks whose 1-ring is also decorated.
    let mut cands: Vec<ChunkPos> = statuses
        .iter()
        .filter(|(p, s)| {
            decorated(s)
                && (-1..=n).all(|dx| {
                    (-1..=n).all(|dz| {
                        statuses
                            .get(&ChunkPos::new(p.x + dx, p.z + dz))
                            .is_some_and(|s| decorated(s))
                    })
                })
        })
        .map(|(p, _)| *p)
        .collect();
    cands.sort_by_key(|p| {
        let (ax, az) = around.unwrap_or((0, 0));
        (
            i64::from(p.x - ax).pow(2) + i64::from(p.z - az).pow(2),
            p.x,
            p.z,
        )
    });
    let Some(&origin) = cands.first() else {
        anyhow::bail!("no fully decorated {n}x{n} block found")
    };
    println!(
        "world {} seed {seed} dim {dim:?}: full parity on {n}x{n} block at chunk {},{}",
        world.display(),
        origin.x,
        origin.z
    );
    let mut order = Vec::new();
    for dz in -1..=n {
        for dx in -1..=n {
            let p = ChunkPos::new(origin.x + dx, origin.z + dz);
            if statuses.get(&p).is_some_and(|s| decorated(s)) {
                order.push(p);
            }
        }
    }
    match std::env::var("PARITY_ORDER").as_deref() {
        Ok("reverse") => order.reverse(),
        Ok(o) if o.starts_with("from:") => {
            let v: Vec<i32> = o[5..].split(',').map(|x| x.parse().unwrap()).collect();
            order.sort_by_key(|p| {
                (
                    i64::from(p.x - v[0]).pow(2) + i64::from(p.z - v[1]).pow(2),
                    p.x,
                    p.z,
                )
            });
        }
        Ok("spiral") => {
            let (cx, cz) = (origin.x + n / 2, origin.z + n / 2);
            order.sort_by_key(|p| {
                (
                    i64::from(p.x - cx).pow(2) + i64::from(p.z - cz).pow(2),
                    p.x,
                    p.z,
                )
            });
        }
        _ => {}
    }
    if let Ok(t) = std::env::var("PARITY_TRACE") {
        let v: Vec<i32> = t.split(',').map(|x| x.trim().parse().unwrap()).collect();
        rc_worldgen::feature::region::TRACE
            .with(|tr| tr.borrow_mut().0 = Some(rc_core::BlockPos::new(v[0], v[1], v[2])));
    }
    let t0 = std::time::Instant::now();
    let mut store = rc_worldgen::pipeline::ChunkStore::new(&dg);
    store.generate(&order);
    println!(
        "generated {} chunks, decorated {} in {:?}",
        store.chunks.len(),
        order.len(),
        t0.elapsed()
    );
    let blocks = rc_data::blocks();
    let mut report = Report::default();
    let dir = region_dir(world, dim);
    if let Ok(d) = std::env::var("PARITY_DUMP") {
        // x0,x1,y0,y1,z  → grid of (vanilla / ours) short names
        let v: Vec<i32> = d.split(',').map(|x| x.trim().parse().unwrap()).collect();
        let short = |s: BlockStateId| {
            let n = blocks.to_string(s).replace("minecraft:", "");
            let dist = blocks
                .get(s, "distance")
                .map(|d| d.to_string())
                .unwrap_or_default();
            let base: String = n.split('[').next().unwrap().chars().take(6).collect();
            format!("{base}{dist}")
        };
        for y in (v[2]..=v[3]).rev() {
            let mut a = String::new();
            let mut b = String::new();
            for x in v[0]..=v[1] {
                let p = ChunkPos::new(x >> 4, v[4] >> 4);
                let rf = RegionFile::open(&dir.join(format!("r.{}.{}.mca", p.x >> 5, p.z >> 5)))?;
                let ac = AnvilChunk::from_nbt(&rf.read_chunk(p.x, p.z)?.unwrap())?;
                let e = ac.block((x & 15) as usize, y, (v[4] & 15) as usize).map_or(
                    BlockStateId::AIR,
                    |s| {
                        blocks
                            .parse_state(&s.to_state_string())
                            .unwrap_or(BlockStateId::AIR)
                    },
                );
                let g = store
                    .chunks
                    .get(&p)
                    .map_or(BlockStateId::AIR, |c| c.get(x, y, v[4]));
                a.push_str(&format!("{:>8}", short(e)));
                b.push_str(&format!("{:>8}", short(g)));
            }
            println!("{y:>4} V {a}");
            println!("{y:>4} R {b}");
        }
    }
    for dz in 0..n {
        for dx in 0..n {
            let p = ChunkPos::new(origin.x + dx, origin.z + dz);
            let (rx, rz) = (p.x >> 5, p.z >> 5);
            let rf = RegionFile::open(&dir.join(format!("r.{rx}.{rz}.mca")))?;
            let Some(t) = rf.read_chunk(p.x, p.z)? else {
                continue;
            };
            let ac = AnvilChunk::from_nbt(&t)?;
            let pc = &store.chunks[&p];
            report.chunks += 1;
            for y in dg.settings.min_y..(dg.settings.min_y + dg.settings.height) {
                for z in 0..16 {
                    for x in 0..16 {
                        let exp = ac.block(x, y, z).map_or(BlockStateId::AIR, |s| {
                            blocks
                                .parse_state(&s.to_state_string())
                                .unwrap_or(BlockStateId::AIR)
                        });
                        let got = pc.get((p.x << 4) + x as i32, y, (p.z << 4) + z as i32);
                        report.blocks += 1;
                        if exp != got {
                            report.block_mismatch += 1;
                            let e = blocks.to_string(exp);
                            let g = blocks.to_string(got);
                            let want = std::env::var("PARITY_PAIR").ok();
                            if want.as_deref().is_none_or(|w| w == format!("{e}|{g}"))
                                && report.examples.len() < 12
                            {
                                report.examples.push(format!(
                                    "{},{y},{}: expected {e} got {g}",
                                    (p.x << 4) + x as i32,
                                    (p.z << 4) + z as i32
                                ));
                            }
                            *report.pairs.entry((e, g)).or_default() += 1;
                        }
                    }
                }
            }
        }
    }
    print_report(report, "full");
    Ok(())
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
    print_report(r, stage);
    Ok(())
}

fn print_report(r: Report, stage: &str) {
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
}

/// Copy a few reference chunks (raw NBT) into `tests/parity/` for CI.
pub fn extract(world: &Path, name: &str, out: &Path) -> Result<()> {
    use std::io::Write;
    let seed = seed_of(world)?;
    let dir = out.join(name);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("seed.txt"), seed.to_string())?;
    for (dim, sub, tag) in [
        (Dimension::Overworld, "region", "overworld"),
        (Dimension::Nether, "DIM-1/region", "nether"),
        (Dimension::End, "DIM1/region", "end"),
    ] {
        let rd = world.join(sub);
        if !rd.is_dir() || std::fs::read_dir(&rd)?.next().is_none() {
            continue;
        }
        let _ = dim;
        let mut picked = 0;
        let mut files: Vec<_> = std::fs::read_dir(&rd)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        files.sort();
        for f in files {
            if std::fs::metadata(&f)?.len() < 8192 || picked >= 3 {
                continue;
            }
            let r = RegionFile::open(&f)?;
            for (lx, lz) in r.present() {
                if picked >= 3 {
                    break;
                }
                let Some(raw) = r.read_raw(lx, lz)? else {
                    continue;
                };
                let t = nbt::parse(&raw)?;
                let c = AnvilChunk::from_nbt(&t)?;
                if c.status != "minecraft:carvers" {
                    continue;
                }
                let mut enc =
                    flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
                enc.write_all(&raw)?;
                std::fs::write(
                    dir.join(format!("{tag}_{}_{}.nbt.zlib", c.x, c.z)),
                    enc.finish()?,
                )?;
                picked += 1;
            }
        }
    }
    Ok(())
}
