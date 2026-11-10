# Benchmark results per phase and reference machine

## 0.2.1: streaming benchmark (`--scene bench`)

Run with `rustcraft --game-dir <dir> --screenshot /dev/null --scene bench`
and the environment variables documented in `crates/rustcraft/src/headless.rs`
(`RC_BENCH_SECS`, `RC_BENCH_SPEED`, `RC_BENCH_SPIN`, `RC_BENCH_Y`,
`RC_BENCH_MEM`, `RC_BENCH_FRAMES`). Fresh world, seed 0, real 1.21.1 assets.

Machine: Ryzen 7 9850X3D (8 cores / 16 threads), RTX 3060, Linux, 14 workers.

| Scenario | 0.2.0 | 0.2.1 |
|---|---|---|
| Chunk generation, steady state | ~20 chunks/s (owner report), CPU ≤ 80 % | 150–200 chunks/s, CPU ~1450 % |
| Raw parallel carving ceiling (`parbench`, 14 threads) | n/a | ~180 chunks/s |
| RD 32 from scratch: all 4,761 chunks ready | n/a | ~30 s; frame p50 1.7 ms, p99 10 ms |
| RD 128, flying 20 b/s while turning, 60 s | multi-hundred-ms stalls | p50 4 ms, p99 18 ms, max 61 ms |
| Memory at RD 2 (process) | ~780 MB | ~425 MB |
| Memory limit 512 MB, RD 32 requested | 690–1,000+ MB | ~525 MB at RD 7–12 |

CPU time per generated chunk at RD 32 (all workers): carving 84 ms,
decoration 1.6 ms, lighting 3.1 ms, meshing 1.6 ms, export 0.3 ms. Carving
(noise sampling in particular) is now the only significant cost; speeding up
the density-function evaluation is a Phase 3 item.
