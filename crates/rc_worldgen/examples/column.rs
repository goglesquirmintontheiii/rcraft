//! Print a generated column: `cargo run -p rc_worldgen --release --example column -- nether 0 0 0`.
use rc_core::ChunkPos;
use rc_worldgen::{Dimension, DimensionGen};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let dim = match a.get(1).map(String::as_str) {
        Some("nether") => Dimension::Nether,
        Some("end") => Dimension::End,
        _ => Dimension::Overworld,
    };
    let seed: i64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let x: i32 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
    let z: i32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    let dg = DimensionGen::new(dim, seed);
    let c = dg.generate_to_carvers(ChunkPos::new(x >> 4, z >> 4));
    let b = rc_data::blocks();
    let mut last = String::new();
    for y in (c.min_y..c.min_y + c.height).rev() {
        let s = c.get(x, y, z);
        let n = b.def(s).name.clone();
        if n != last {
            println!("y={y:4} {n}");
            last = n;
        }
    }
    println!("min_y={} height={}", c.min_y, c.height);
}
