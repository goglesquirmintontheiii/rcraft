# RCW — RustCraft World Format (v1)

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
payload (uncompressed, little-endian) — as implemented in v1
  section_count    u32   always 24 for the overworld layout
  per section (bottom to top):
    blocks         palette_len u16, palette [u32 stable id], bits u8,
                   packed u64 words (4096 entries, no straddling; bits 0 ⇒ uniform)
    biomes         palette_len u8,  palette [u32 stable id], bits u8, packed words (64)
    sky_light      tag u8: 0 absent (implicit 15) | 1 uniform + u8 value | 2 + 2048 bytes
    block_light    same encoding (absent ⇒ 0)
  heights          256 × i16 WORLD_SURFACE (other heightmaps are rebuilt on load)
  block_entities   u32 count (0 in v0.1)
  entities         u32 count (0 in v0.1)
  scheduled_ticks  u32 count (0 in v0.1)
  post_processing  u32 count (0 in v0.1)
```

Record header in v1 is 30 bytes: magic, xxh3-64 of bytes 12.., cx, cz,
data_version, status (4 = features, 6 = full), flags (bit 0: light valid),
raw payload length, then the zstd frame (level 3, no dictionary yet).

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
