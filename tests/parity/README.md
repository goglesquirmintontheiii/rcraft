# Worldgen parity reference data

Captures from a real Minecraft 1.21.1 install (owned by the project owner), used
by `rc_worldgen` tests to prove seed-exact parity. Each capture directory holds:

- `seed.txt` — the world seed and dimension
- `region/*.mca` — exported Anvil region files for the surveyed area
- `biomes.json` — biome grid sampled at quart resolution (from `/locate` / F3)
- `NOTES.md` — vanilla version, how it was captured, which stages it covers

Captures are game *output*, never game code (see `docs/CLEAN_ROOM.md`). Large
captures are stored with Git LFS.

## Current reference set

34 chunks from six vanilla 1.21.1 worlds (Overworld, Nether, End) saved at
the `carvers` status. `crates/rc_worldgen/tests/parity.rs` requires:

- biomes: every quart cell identical;
- terrain: zero differences between base-terrain blocks (stone, deepslate,
  air, water, lava, bedrock, netherrack, end stone).

Chunks where already-decorated neighbours had written air or water into the
reference (lava lakes, geodes, springs) were excluded so the check stays a
pure terrain regression test. Full-pipeline comparisons against whole worlds
are run locally with `cargo xtask parity <world> --stage full`.
