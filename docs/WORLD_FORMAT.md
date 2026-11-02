# RCW — RustCraft World Format (draft v1)

Design goals, in priority order:

1. **Fast loads**: a chunk column should be readable with one `pread` (or one mmap
   slice) plus one zstd decompress; no tag-walking parser in the hot path.
2. **Fast saves**: append-only region writes with periodic compaction; dirty
   sections only.
3. **Compact**: palette + bit-packing per section, zstd (level 3, trained
   dictionary shipped with the game) per chunk.
4. **Robust**: every record has an xxh3-64 checksum; truncated/corrupt records are
   detected and the previous version of the chunk is used.
5. **Versioned**: explicit format version + per-world registry table so block ids
   never drift between game versions.

## Directory layout

```
saves/<world>/
├── world.rcw            world header: format version, seed, name, game rules,
│                        spawn, time, weather, registry table (block states,
│                        biomes, entity types, items) → stable u32 ids
├── player/<uuid>.rcp    player data
├── dim/overworld/
│   ├── r.<x>.<z>.rcr    region files (32×32 chunk columns)
│   └── ...
├── dim/the_nether/ …
├── dim/the_end/ …
└── backups/
```

## Region file (`.rcr`)

```
Header (16 KiB = 4 sectors, fixed)
  magic        "RCWR"          4 bytes
  version      u16
  flags        u16
  chunk table  1024 × { offset: u32 (4 KiB units), length: u32 (bytes), ver: u32 }
               - offset 0 ⇒ chunk absent
               - ver increments on every rewrite; readers pick the highest valid
Body
  Chunk records, 4 KiB aligned, appended; a record is never overwritten in place.
  A compaction pass rewrites the file when dead space > 25 % (done by the io
  thread on world close or when idle).
```

## Chunk record

```
record_header
  magic      "RCWC"
  xxh3_64    of everything after this field
  cx, cz     i32
  data_ver   u32         game data version this chunk was last saved with
  status     u8          empty | noise | surface | carved | features | light | full
  flags      u8          bit0: light valid, bit1: has entities, bit2: has block entities
  raw_len    u32         uncompressed payload length
  payload    zstd-compressed bytes (dictionary id in flags)
payload (uncompressed)
  section_mask     u32   which of the 24 sections are non-empty (bit per section)
  per non-empty section:
    block_palette  { len: u16, ids: [u32] }     if len == 1 ⇒ whole section uniform
    block_bits     u8  (0, 4, 5, 6, 7, 8, or 16=direct)
    block_data     packed u64 words, 4096 entries, vanilla-style no-straddle packing
    biome_palette  { len: u8, ids: [u32] }
    biome_bits     u8
    biome_data     packed, 64 entries
    sky_light      2048 bytes or absent (flag)
    block_light    2048 bytes or absent (flag)
  heightmaps       4 × 256 × 9-bit packed  (WORLD_SURFACE, MOTION_BLOCKING,
                                            MOTION_BLOCKING_NO_LEAVES, OCEAN_FLOOR)
  block_entities   count + [ pos: u16, type: u32, data: RCB ]
  entities         count + [ type: u32, uuid: u128, data: RCB ]
  scheduled_ticks  block & fluid tick lists
  post_processing  list of positions needing update on load
```

**RCB** (RustCraft Binary) is a tiny self-describing binary encoding (typed tags,
length-prefixed, little-endian) used only for the irregular parts (block entity
contents, entity NBT-like data). It maps 1:1 onto Java NBT types so Anvil
import/export (Phase 2) is lossless.

## Why faster than Anvil

| Anvil (Java)                                   | RCW                                              |
|------------------------------------------------|--------------------------------------------------|
| zlib (slow decompress)                         | zstd + dictionary (3–5× faster decompress)       |
| NBT tree parsing for every chunk               | fixed binary layout, direct slice into sections  |
| string block-state palette per section         | u32 world-registry ids per section               |
| 4 KiB sector reuse, no checksums               | append + checksum + versioned records            |
| synchronous region I/O                         | mmap reads, async batched writes                 |

Benchmarks live in `crates/rc_world/benches/` and compare load/save throughput
against an Anvil reader (`rc_anvil`) on the same worlds once Phase 2 lands.

## World header (`world.rcw`)

Little-endian, versioned, xxh3-checksummed; holds the registry table:

```
registries: { "block_state": [ "minecraft:stone", "minecraft:grass_block[snowy=false]", … ],
              "biome": [...], "entity_type": [...], "item": [...] }
```

Index in each list == id used inside region files. New states are appended; ids
are never reused. Data-version upgrades run a migration that rewrites the table
(and, lazily, chunks on load).
