//! Generation throughput: `cargo run -p rc_worldgen --profile fast --example genbench`.
use std::time::Instant;

use rc_core::ChunkPos;
use rc_worldgen::pipeline::ChunkStore;
use rc_worldgen::{Dimension, DimensionGen};

fn main() {
    let dg = DimensionGen::new(Dimension::Overworld, 12345);
    let n = 6;
    let pos: Vec<ChunkPos> = (0..n * n).map(|i| ChunkPos::new(i % n, i / n)).collect();
    let t = Instant::now();
    let carved: Vec<_> = pos.iter().map(|&p| dg.generate_to_carvers(p)).collect();
    let tc = t.elapsed();
    println!(
        "carve: {:.1} ms/chunk (single thread)",
        tc.as_secs_f64() * 1000.0 / carved.len() as f64
    );
    let mut store = ChunkStore::new(&dg);
    for c in carved {
        store.chunks.insert(c.pos, c);
    }
    let inner: Vec<ChunkPos> = pos
        .iter()
        .copied()
        .filter(|p| p.x > 0 && p.z > 0 && p.x < n - 1 && p.z < n - 1)
        .collect();
    let t = Instant::now();
    for &p in &inner {
        store.decorate(p);
    }
    println!(
        "decorate: {:.1} ms/chunk",
        t.elapsed().as_secs_f64() * 1000.0 / inner.len() as f64
    );
}
