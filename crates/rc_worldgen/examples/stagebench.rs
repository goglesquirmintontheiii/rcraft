use std::time::{Duration, Instant};

use rc_core::ChunkPos;
use rc_worldgen::generator::PrelimSurface;
use rc_worldgen::{Dimension, DimensionGen, ProtoChunk};

fn main() {
    let dg = DimensionGen::new(Dimension::Overworld, 12345);
    let mut t = [Duration::ZERO; 5];
    let n: i32 = std::env::var("N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16);
    for i in 0..n {
        let pos = ChunkPos::new(i % 4, i / 4);
        let s0 = Instant::now();
        let mut chunk = ProtoChunk::new(pos, -64, 384);
        dg.fill_biomes(&mut chunk);
        let s1 = Instant::now();
        let mut cc = dg.chunk_cache(pos);
        let mut aq = dg.new_aquifer(pos);
        let mut pre = PrelimSurface::default();
        dg.fill_noise(&mut chunk, &mut cc, &mut aq, &mut pre);
        let s2 = Instant::now();
        dg.build_surface(&mut chunk, &cc, &mut pre);
        let s3 = Instant::now();
        dg.apply_carvers(&mut chunk, &cc, &mut aq, &mut pre);
        let s4 = Instant::now();
        t[0] += s1 - s0;
        t[1] += s2 - s1;
        t[2] += s3 - s2;
        t[3] += s4 - s3;
    }
    for (name, d) in ["biomes", "noise", "surface", "carvers"].iter().zip(t) {
        println!(
            "{name}: {:.1} ms/chunk",
            d.as_secs_f64() * 1000.0 / f64::from(n)
        );
    }
}
