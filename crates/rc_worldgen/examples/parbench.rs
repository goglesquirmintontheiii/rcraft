//! Parallel carving throughput:
//! `RAYON_NUM_THREADS=14 cargo run -p rc_worldgen --release --example parbench`.
use std::time::Instant;

use rayon::prelude::*;
use rc_core::ChunkPos;
use rc_worldgen::{Dimension, DimensionGen};

fn main() {
    let dg = DimensionGen::new(Dimension::Overworld, 0);
    let n = 16;
    let pos: Vec<ChunkPos> = (0..n * n).map(|i| ChunkPos::new(i % n, i / n)).collect();
    // Warm up lazily built tables.
    let _ = dg.generate_to_carvers(ChunkPos::new(100, 100));
    let t = Instant::now();
    let made: Vec<_> = pos.par_iter().map(|&p| dg.generate_to_carvers(p)).collect();
    let el = t.elapsed().as_secs_f64();
    println!(
        "{} chunks on {} threads: {:.1} chunks/s ({:.1} ms/chunk/thread)",
        made.len(),
        rayon::current_num_threads(),
        made.len() as f64 / el,
        el * 1000.0 * rayon::current_num_threads() as f64 / made.len() as f64
    );
}
