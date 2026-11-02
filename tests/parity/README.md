# Worldgen parity reference data

Captures from a real Minecraft 1.21.1 install (owned by the project owner), used
by `rc_worldgen` tests to prove seed-exact parity. Each capture directory holds:

- `seed.txt` — the world seed and dimension
- `region/*.mca` — exported Anvil region files for the surveyed area
- `biomes.json` — biome grid sampled at quart resolution (from `/locate` / F3)
- `NOTES.md` — vanilla version, how it was captured, which stages it covers

Captures are game *output*, never game code (see `docs/CLEAN_ROOM.md`). Large
captures are stored with Git LFS.
