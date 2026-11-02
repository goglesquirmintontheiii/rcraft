# RustCraft Architecture

## Workspace layout

```
rcraft/
├── Cargo.toml                workspace, shared deps & lints
├── crates/
│   ├── rustcraft/            the game binary (window, main loop, wiring, CLI)
│   ├── rc_core/              math, ids, registries, block/item states, events, config
│   ├── rc_data/              generated registries (build.rs from /data/*.toml)
│   ├── rc_assets/            asset packs: bundled fallback, vanilla import, resource packs
│   ├── rc_world/             chunks, light engine, ticking, RCW save format
│   ├── rc_worldgen/          noise, density functions, biomes, surface, carvers, features
│   ├── rc_physics/           AABB collision, voxel shapes, entity movement
│   ├── rc_entity/            entity framework, player, mobs (Phase 2+)
│   ├── rc_render/            wgpu renderer, meshing, atlas, camera, screenshots
│   ├── rc_ui/                vanilla-style GUI framework, screens, HUD, font
│   ├── rc_audio/             sound engine (kira)
│   ├── rc_update/            self-updater (GitHub Releases manifest, verify, swap)
│   ├── rc_anvil/             Java Edition world import/export (Phase 2)
│   ├── rc_net/               Java Edition protocol client (Phase 4)
│   ├── rc_shaders/           Iris-compatible shader pack pipeline (Phase 4)
│   ├── rc_lod/               Distant-Horizons-style LOD (Phase 4)
│   └── rc_mod/               mod ABI & loaders (Phase 5)
├── data/                     block/item/biome definitions (TOML), worldgen params
├── assets/fallback/          original CC0 placeholder pack shipped with the game
├── docs/                     roadmap, specs, ADRs, screenshots/
├── xtask/                    cargo xtask: package, screenshot, datagen, release
└── .github/workflows/        ci.yml, release.yml
```

Dependency direction is strictly downward:

```
rustcraft → rc_ui, rc_render, rc_audio, rc_update, rc_net, rc_mod
rc_ui     → rc_render, rc_assets, rc_core
rc_render → rc_world, rc_assets, rc_core
rc_entity → rc_physics, rc_world, rc_core
rc_worldgen → rc_world, rc_core
rc_world  → rc_data, rc_core
rc_data   → rc_core
```

Nothing depends on `rustcraft`. `rc_core` depends on nothing in the workspace.

## Threads

| Thread            | Responsibility                                                  |
|-------------------|-----------------------------------------------------------------|
| main              | winit event loop, input, render submission (wgpu)               |
| sim               | fixed 20 TPS world tick, entity update, block ticks             |
| worldgen pool     | rayon: chunk generation & decoration, light init                |
| mesh pool         | rayon: section meshing, LOD building                            |
| io                | save/load (RCW), async writes, screenshot encoding              |
| net (Phase 4)     | tokio: protocol I/O                                             |
| audio             | kira's own mixer thread                                         |

Pool sizes derive from the in-game CPU limit (default: all logical cores minus
two reserved for main + sim). Stages have priorities (meshing near the player >
light > worldgen > LOD > I/O) so a 16+ core machine stays fully busy without
starving the frame. Memory lives in budgeted arenas (chunk sections, mesh
vertex buffers, light arrays) that evict LRU against the in-game memory limit;
hot paths never allocate per frame.

Main ↔ sim communicate through lock-free channels (`crossbeam`) plus a
double-buffered render snapshot of entity transforms; chunk data is shared via
`Arc<RwLock<Section>>` with copy-on-mesh so meshing never blocks the tick.

## Core data model

- **Block state**: `BlockStateId(u32)` — dense, generated at build time from
  `data/blocks/*.toml`. Properties are enumerated per block; the id encodes
  `(block_id, property_combination)`. A `BlockState` is `Copy` and all hot paths
  use the id only.
- **Chunk**: 16×(−64..320)×16 column = 24 `Section`s. Each section stores
  `PalettedContainer<BlockStateId>` (bit-packed, 4/5/6/8/… bits per entry, direct
  storage above 8 bits) for blocks and `PalettedContainer<BiomeId>` at 4×4×4
  resolution, plus `u4` block-light and sky-light nibble arrays.
- **Registries**: blocks, items, biomes, sounds, entities; string keys
  (`minecraft:stone`) resolve to dense ids at load time. World saves carry their
  own key→id table, so ids are stable per world (see WORLD_FORMAT.md).
- **Events**: typed `EventBus` for block updates, entity events, UI actions.

## Rendering pipeline (rc_render)

1. **Atlas**: textures stitched into a 2D array / large atlas with mipmaps;
   animations advanced per frame.
2. **Meshing**: per section, per render layer (solid, cutout-mipped, cutout,
   translucent). Face culling against neighbours, smooth lighting + AO per vertex.
   Vertex format: position (3×f32 or packed u16), normal/face id (u8), uv (2×u16),
   light (u8 sky, u8 block), AO (u8), tint (rgb8).
3. **Passes**: shadow (Phase 4), opaque, cutout, sky, translucent (sorted back to
   front by section distance), weather, particles, hand, UI.
4. **Culling**: frustum per section; CPU occlusion via visibility graph flood-fill
   (vanilla "chunk visibility" algorithm from the public description).
5. **Screenshots**: render to an offscreen texture, copy to a mapped buffer, encode
   PNG on the io thread. `--screenshot path.png` runs one frame headlessly.

Shaders are WGSL. In Phase 4 Iris GLSL passes are translated with `naga` and
substituted into the same pass graph.

## UI (rc_ui)

Immediate-mode drawing on top of rc_render's 2D batch (textured quads + text),
retained *screen* objects with vanilla's widget semantics (buttons, sliders, cycle
buttons, edit boxes, scroll lists, tabs). GUI scale and layout rules match vanilla
so screens can be built from the wiki's pixel measurements. Font: vanilla bitmap
font format (`ascii.png` grid + unihex fallback), rendered with the same glyph
advance and shadow rules.

## World format

See `WORLD_FORMAT.md` for the RCW specification.

## Configuration & paths

Game directory (`--game-dir` overrides):
- Windows: `%APPDATA%\.rustcraft\`
- Linux: `$XDG_DATA_HOME/rustcraft/` (default `~/.local/share/rustcraft/`)

Contents: `options.toml`, `saves/`, `screenshots/`, `resourcepacks/`,
`shaderpacks/`, `mods/`, `logs/`, `crash-reports/`, `cache/` (imported vanilla
assets, LOD cache), `updates/` (staged updater downloads).

A portable mode (`portable.txt` next to the executable) keeps everything beside the
binary.

## Build & release

- `cargo xtask screenshot` – headless screenshot (used by CI and docs)
- `cargo xtask package` – produce release artefacts locally
- CI: `.github/workflows/ci.yml` runs fmt/clippy/test on Linux & Windows.
- Release: push tag `vX.Y.Z` → `release.yml` builds Windows MSVC `.exe`
  (static CRT, portable) and Linux AppImage, generates `SHA256SUMS`, signs with
  minisign, writes `latest.json` (updater manifest), publishes the GitHub Release.
