# RustCraft Roadmap

RustCraft is a clean-room reimplementation of Minecraft: Java Edition 1.21.1 in Rust.
"Clean room" means: no Mojang code is read, decompiled, or copied; behaviour is
reimplemented from public documentation (the Minecraft Wiki, wiki.vg protocol docs,
the public data-pack/worldgen format, and black-box observation of the real game).
No Mojang assets are redistributed by this repository or its releases.

This document is the master plan. Each phase ends with a GitHub release, and we pause
between phases for review. The plan is deliberately opinionated; anything marked
**DECISION** is something the project owner should confirm or override before the
phase that depends on it begins.

Status legend: `[ ]` not started · `[~]` in progress · `[x]` done

---

## Phase 0 — Roadmap & skeleton `[x]`

**Goal:** agree on the plan, lock the architecture, and have a repo that builds,
tests, lints, and ships through CI from day one.

Deliverables
- [x] `docs/ROADMAP.md` (this file), `docs/ARCHITECTURE.md`, `docs/WORLD_FORMAT.md`,
      `docs/DECISIONS.md` (architecture decision records), `docs/CLEAN_ROOM.md`
- [x] Cargo workspace with every planned crate stubbed and compiling
- [x] `rustcraft` binary that opens a window, initialises wgpu, clears the screen,
      and can take a `--screenshot <path>` (headless proof that the screenshot
      pipeline works in CI and in this dev container)
- [x] Self-updater crate skeleton (`rc_update`) with the release-manifest format fixed
- [x] GitHub Actions: `ci.yml` (fmt, clippy, test on Linux + Windows) and
      `release.yml` (tag `v*` → portable Windows `.exe`, AppImage from Phase 2 on,
      SHA-256 checksums, minisign signature, GitHub Release)
- [x] `CLAUDE.md` with project conventions so every future session follows them

Exit criteria: `cargo test --workspace` green, CI green on `main`, owner has
reviewed the decisions below. **Met 2026-11-02.**

### Decisions (confirmed by the owner, 2026-11-02)
1. **Assets.** Textures, sounds, fonts, and language files are Mojang's copyright
   and cannot ship in a clean-room build. RustCraft ships an **original, CC0
   fallback pack** and offers a one-click **"Import vanilla assets"** that reads the
   user's own legally installed `.minecraft` (client jar + `assets/` index) and
   uses those textures, sounds and fonts at runtime. Full visual parity requires
   owning Java Edition (the owner does). The importer also understands a Bedrock
   install's resource pack as a secondary source for textures.
2. **Licence.** MIT OR Apache-2.0 for code; CC0 for bundled original assets.
3. **Minimum platform.** Windows 10+ x86_64 and Linux x86_64 (glibc 2.35+).
   Vulkan 1.1 / DX12 class GPU (wgpu); GL fallback for very old hardware. macOS is
   "should work" but untested and not released.
4. **Worldgen parity target: seed-exact.** A given seed must produce the same
   terrain, biomes, caves, ores and feature placement as vanilla 1.21.1. This is
   a hard Phase 1 requirement, not a stretch goal. Consequences:
   - Every noise/RNG primitive is implemented to the bit: Java `Random` (LCG),
     Xoroshiro128++ with the vanilla seed-mixing (MD5 of the seed for the
     positional-random factory), `LegacyRandom` where 1.21 still uses it, the
     exact octave/amplitude tables of every named noise, Perlin permutation
     construction, the density-function evaluation order, cell interpolation
     (4×8×4 cells in the overworld), aquifer and ore-vein sampling, surface-rule
     evaluation, carver RNG consumption order and feature/decorator RNG order.
   - A **parity test harness** is part of 1.4: reference data is captured from
     the real game that the owner owns (block samples, biome grids, heightmaps
     at known seeds via vanilla `/locate`, F3 and exported Anvil chunks read by
     `rc_anvil`'s reader, which is pulled forward from Phase 2 to Phase 1 for
     this purpose). CI diff-tests every generation stage against that data.
   - Parity is proven stage by stage (biome source → base terrain → surface →
     carvers → features) so regressions are localised. Phase 1 exit criterion:
     byte-identical block output for the reference seeds over the surveyed
     regions in all three dimensions' terrain, and identical feature placement
     for every feature type in scope.

---

## Phase 1 — Core engine `[ ]`

**Goal:** a playable single world you can walk around, break and place naturally
generated blocks in, with a UI that looks and behaves like vanilla.

### 1.1 Engine foundation
- [ ] Main loop: fixed 20 TPS simulation tick, uncapped/vsync render, interpolation
- [ ] Input mapping with vanilla defaults and rebinding (options screen)
- [ ] Settings persistence (`options.toml`, mirrors vanilla `options.txt` keys)
- [ ] Logging, crash reports (`crash-reports/` like vanilla), panic hook with dialog
- [ ] Asset system: bundled fallback pack + vanilla-asset import (see Decision 1)
- [ ] Audio engine (kira): positional sounds, music, volume categories

### 1.2 Rendering (wgpu)
- [ ] Chunk meshing: greedy-ish per-face culling, per-section (16³) meshes,
      opaque / cutout / translucent passes, sorted translucency
- [ ] Texture atlas builder with mipmaps, animated textures (`.mcmeta` parity)
- [ ] Block models: full vanilla JSON block-model + blockstate format support
      (multipart, variants, rotations, UV lock, ambient occlusion)
- [ ] Lighting: vanilla 0–15 sky + block light, smooth lighting, AO
- [ ] Sky, sun/moon, clouds, fog, underwater/lava overlay, render distance 2–32
- [ ] Frustum + occlusion culling, multi-threaded meshing with rayon
- [ ] Held item / block rendering, block breaking overlay, selection box
- [ ] F2 screenshots → `screenshots/` (PNG), `--screenshot` CLI for CI
- [ ] Performance target: 60 fps @ 16 chunks on an integrated GPU; 144+ on mid-range;
      Potato preset playable on Core 2 Duo-class hardware in ≤ 1 GB RAM
- [ ] Options → Performance screen: memory limit, worker threads / CPU limit,
      FPS cap & GPU target, presets (Potato / Low / Medium / High / Ultra / Auto)

### 1.3 World
- [ ] Chunk columns −64..320, 24 sections, palette-compressed block storage
- [ ] Block state registry generated at build time from `data/blocks/*.toml`
- [ ] Block ticking (random + scheduled), fluid flow (water, lava), gravity blocks
- [ ] Light engine (sky light propagation, block light, incremental updates)
- [ ] **RCW world format** (`docs/WORLD_FORMAT.md`): zstd-compressed, palette +
      bit-packed sections, 32×32 region files with index, xxh3 checksums, mmap reads,
      async save thread. Benchmarked against Anvil in `benches/`.
- [ ] One built-in world ("World") created on first launch; seed configurable only
      through the config file in this phase

### 1.4 Terrain generation (vanilla 1.21.1 structure)
- [ ] Noise primitives: improved Perlin, octave/“normal” noise, simplex (biome/End)
- [ ] Density-function graph (all vanilla function types), noise router, cave noise
      (cheese / spaghetti / noodle), aquifers, ore veins (copper/iron)
- [ ] Multi-noise biome source with the vanilla overworld parameter list
- [ ] Surface rules (all vanilla overworld biomes)
- [ ] Carvers (cave, canyon)
- [ ] Features needed for "naturally generated blocks": trees (all overworld
      types), ores, grass/flowers, lakes, springs, disks, kelp/seagrass, ice,
      dripstone, moss/lush caves, amethyst geodes
- [ ] Nether & End generation (structure of the pipeline; biomes + terrain only)
- [ ] Exact RNG: Java LCG `Random`, Xoroshiro128++, positional random factories
      (MD5 seed hashing), `LegacyRandom` paths
- [ ] Seed-exact parity harness: reference captures from the owner's vanilla
      install (Anvil chunks via `rc_anvil` reader, pulled forward to Phase 1),
      per-stage diff tests in CI, parity dashboard in `docs/perf/parity.md`

### 1.5 Player
- [ ] Vanilla movement physics: walking, sprinting, sneaking, jumping, swimming,
      ladders, falling damage, step height, collision with vanilla block shapes
- [ ] Survival & creative modes (no hunger drain yet; health + damage present)
- [ ] Block breaking with correct hardness/tool speed; block placing with vanilla
      placement rules for natural blocks (logs axis, slabs, stairs, snow layers…)
- [ ] Inventory (36 + armour + offhand), hotbar, creative inventory tabs & search,
      item drops as entities, pickup

### 1.6 UI — vanilla parity
- [ ] UI framework: 9-slice sprites, vanilla GUI scale rules, vanilla bitmap font
      (ascii + unifont fallback), text shadows, formatting codes
- [ ] Title screen (panorama, splash text, buttons), Options (all sub-screens:
      video, controls incl. key binds, music & sounds, chat, accessibility, language),
      Pause menu, Loading screen, Death screen
- [ ] HUD: hotbar, hearts, armour, air bubbles, crosshair, experience bar, chat,
      scoreboard sidebar placeholder, F3 debug overlay (vanilla layout), F1 hide
- [ ] Inventory & creative screens, tooltip rendering with vanilla styling
- [ ] Chat with command input; core client commands (`/tp`, `/gamemode`, `/time`,
      `/give` for natural blocks, `/seed`, `/kill`)

### 1.7 Self-update
- [ ] On launch: fetch `latest.json` from GitHub Releases, compare semver,
      download platform asset, verify SHA-256 + minisign signature, replace self,
      relaunch. Setting: *Automatic / Ask / Off*.

### 1.8 Release 0.1.0
- [ ] Portable `rustcraft-0.1.0-windows-x86_64.exe` via GitHub Actions
- [ ] Screenshots in `docs/screenshots/phase1/`

---

## Phase 2 — Core extension `[ ]`

- [ ] World selection screen & create-world screen (vanilla parity: seed, game
      mode, difficulty, world type, game rules, data packs tab placeholder)
- [ ] Multiple worlds in `saves/` (RCW), rename/delete/backup
- [ ] **All non-redstone, non-minecart blocks**: crafting-table, furnace family,
      chests/barrels/shulkers, beds, doors/trapdoors/fence gates, signs & hanging
      signs (text editing), banners, beacons, enchanting table, anvil, grindstone,
      stonecutter, loom, cartography, smithing, brewing stand, campfires, lanterns,
      candles, composters, bells, scaffolding, ladders/vines, pots, cauldrons,
      copper oxidation, crops & farming, bone meal, honey/slime (no piston logic),
      portal frames (Nether/End travel), respawn anchors, lodestones, sculk family
      (sensor/shrieker are redstone-adjacent → Phase 3)
- [ ] Container UIs for all of the above with vanilla layout, recipe book
- [ ] Crafting, smelting, brewing, enchanting, smithing recipes (data-driven,
      vanilla recipe JSON format)
- [ ] Hunger, saturation, food, status effects, experience, enchantments (effects
      on tools/armour), durability, repair
- [ ] Day/night cycle (`/time`, sleeping), weather (rain, thunder, snow by biome,
      lightning), moon phases
- [ ] Entity framework (ECS-style), item entities, falling blocks, XP orbs, boats,
      arrows (as a projectile test), paintings/item frames
- [ ] **Passive mobs**: cow, pig, sheep (shearing, dye), chicken, horse family
      (riding/taming), wolf, cat, parrot, rabbit, fox, bee (basic), fish, squid,
      turtle, villager (wander/idle only; trading in Phase 3), bat, frog, axolotl.
      Vanilla AI goals (wander, panic, breed, follow, tempt), spawning rules,
      drops, breeding, baby growth
- [ ] Mob models & animations from the vanilla entity-texture layout
- [ ] **Anvil import/export**: read `level.dat`, region `.mca`, entities, POI;
      write back a valid 1.21.1 Java world. Round-trip test suite.
- [ ] Release 0.2.0: `.exe` + `.AppImage`

---

## Phase 3 — Content complete `[ ]`

- [ ] Resource packs: full vanilla pack format (pack 34), `pack.mcmeta`, overlays,
      block/item models, custom fonts, sounds, language, shaders stub, pack
      selection screen with drag-and-drop ordering
- [ ] Redstone: dust (vanilla update order quirks included), torches, repeaters,
      comparators, pistons/sticky pistons (quasi-connectivity), observers, hoppers,
      droppers/dispensers, note blocks, TNT, levers/buttons/plates, daylight sensor,
      target, tripwire, trapped chests, sculk sensor/shrieker/calibrated, copper bulb,
      crafters
- [ ] Minecarts & rails (all variants, physics parity)
- [ ] All remaining blocks and all items (bows, crossbows, tridents, elytra, shields,
      spyglass, brushes, maps, books, fireworks, potions, tipped arrows…)
- [ ] All mobs: hostile (zombie family, skeleton family, creeper, spider, enderman,
      witch, slime, magma cube, phantom, drowned, guardian family, illagers,
      ravager, vex, warden, breeze, bogged), neutral (iron golem, piglin family,
      zombified piglin, polar bear, dolphin, llama, panda, goat, enderman), bosses
      (Ender Dragon with full fight, Wither), villager trading & profession
      workstations, wandering trader, raids, allay, sniffer, camel, armadillo
- [ ] All structures via vanilla structure/jigsaw/template-pool format: villages,
      strongholds, mineshafts, temples, monuments, mansions, outposts, bastions,
      fortresses, ancient cities, trial chambers, end cities, ruined portals,
      shipwrecks, ocean ruins, trail ruins, igloos, witch huts, dungeons, fossils
- [ ] Loot tables, advancements, statistics, data packs
- [ ] Release 0.3.0

---

## Phase 4 — Multiplayer, shaders, LOD `[ ]`

- [ ] **Java Edition protocol 767 (1.21.1)** client: handshake, status, login
      (encryption, compression), configuration, play. Full packet coverage.
- [ ] **Offline-mode servers first**: full protocol support for servers with
      `online-mode=false` (username login, no encryption/session auth, no chat
      signing) — this must work with no Microsoft credentials at all.
- [ ] Microsoft account login for `online-mode=true` servers (device-code OAuth
      → XBL → XSTS → Minecraft services), profile/skin/cape fetch, chat signing
      (session keys, 1.19.1+ secure chat). Needs an Azure AD application
      client-ID registered by the owner (agreed; set up when Phase 4 starts).
      Until it exists, the Multiplayer screen shows an "Offline (no account)"
      profile and online servers show a clear "requires sign-in" status.
- [ ] Multiplayer screen, server list, LAN discovery, server resource packs,
      server-side prediction reconciliation
- [ ] **Iris-compatible shader packs**: parse `shaders.properties`, GLSL
      gbuffers/shadow/composite/deferred/final passes translated via naga
      (GLSL → WGSL/SPIR-V) with the Iris/OptiFine uniform set, custom uniforms,
      shadow mapping, buffer formats, flipping, `#include`. Target packs for
      validation: Complementary, BSL, Sildur's, Solas. Shader pack screen with
      options UI.
- [ ] **LOD rendering (Distant Horizons-like)**: hierarchical LOD columns generated
      from chunk data (and pre-generated beyond loaded chunks on a background
      worker), separate LOD mesh pipeline, fog blending, configurable LOD distance
      (up to 4096 blocks), quality presets, persistent LOD cache per world
- [ ] Release 0.4.0

---

## Phase 5 — Mods `[ ]`

> **Risk flag (please read).** Fabric/Quilt/Forge/NeoForge mods are JVM bytecode that
> patch Mojang's own Java classes with mixins and call vanilla internals directly.
> There is no way to run arbitrary Java mods against a Rust engine without (a) a JVM
> and (b) the exact vanilla class surface they patch. Realistic routes, in order of
> fidelity vs. cost:
>
> 1. **Native RustCraft mods** (WASM, `rc_mod` ABI) — first-class, sandboxed, fast.
> 2. **Data-driven mods** — Fabric/Forge mods whose content is resources + data packs
>    + a thin registration layer. We can parse the jar, load its assets/data, and
>    map common registration patterns (blocks/items with vanilla-like behaviour).
>    Covers a large share of "content" mods.
> 3. **Embedded JVM bridge** — bundle a JRE, load the real Fabric/NeoForge loader,
>    and expose a RustCraft "vanilla facade" via JNI. This is a huge, fragile
>    surface and effectively requires re-exposing Mojang's class layout, which
>    conflicts with clean-room goals. Proposed as a research spike only.
>
> Confirmed by the owner: 1 + 2 fully, 3 as a research spike.

- [ ] `rc_mod` WASM ABI (wasmtime), capability-based permissions, hot reload,
      mod menu, dependency resolution, versioning
- [ ] Java-mod jar loader for data-driven mods; loader-type switch setting
      (Fabric / Quilt / Forge / NeoForge / RustCraft) selecting which mods folder &
      metadata format (`fabric.mod.json`, `quilt.mod.json`, `mods.toml`,
      `neoforge.mods.toml`) is active
- [ ] Modded multiplayer: Fabric networking & NeoForge channel negotiation at the
      protocol level; optional automatic download of missing mods from
      Modrinth/CurseForge (hash-verified, user-confirmed)
- [ ] Release 0.5.0

---

## Phase 6 — Parity with Minecraft 26.60 `[ ]`

Minecraft moved to year-based versioning in 2026; "26.60" is a future release.
When this phase begins we snapshot that version's public changelog and produce a
gap list (protocol version, new blocks/items/mobs, worldgen changes, data-pack format)
and work through it. Release 0.6.0.

---

## Phase 7 — Vehicles, Bedrock, polish `[ ]`

- [ ] **BeamNG.drive / Automation vehicles as a first-party RustCraft mod**
      (`rustcraft-vehicles`, see "Mods that ship as first-party RustCraft mods"
      below): thorough `.jbeam` support, Collada meshes, soft-body physics,
      drivetrain/engine sim, sounds, driving UI. Any engine hooks the mod needs
      (custom physics bodies, mesh rendering, input capture) are added to the
      `rc_mod` ABI rather than special-cased.
- [ ] Bedrock (LevelDB-based) world import and export, including palette/NBT
      conversion, biome & block-state mapping tables
- [ ] Final performance pass (profiling, memory, startup time), bug bash, 1.0.0

---

## Hardware & performance requirements (apply from Phase 1)

**Supported hardware.** NVIDIA, AMD and Intel GPUs including iGPUs from Phase 1
(wgpu: Vulkan on Linux, DX12 primary / Vulkan fallback on Windows, GL fallback for
very old hardware). Reference machines: owner's laptop (`.exe`) and a desktop with a
Ryzen 9850X3D, 96 GB DDR5, RTX 3060 (`.AppImage`).

**Scales down.**
- "Potato" preset must run on a Core 2 Duo-class CPU and an old iGPU: render
  distance 2–4, no smooth lighting, no clouds, fast graphics, single meshing
  thread, LOD off, small atlas (no mipmaps), 30 fps target.
- Memory floor: **≤ 1 GB RSS** on lowest settings. This is enforced with a CI job
  that runs the game headless under a cgroup/`ulimit` cap and walks a test world.
- No GC, so no GC stutter — but Rust still needs discipline: chunk/mesh/light
  buffers come from pooled arenas sized at startup from the memory budget; no
  per-frame allocation in render or tick hot paths (tracked by an allocation
  counter in debug builds).

**Scales up.**
- Worker scheduling uses *all* cores (16+ verified): worldgen, decoration,
  lighting, meshing, LOD building, I/O and audio decoding run on rayon pools with
  per-stage priorities; the sim thread and render thread are pinned-priority.
  Nothing that belongs on the GPU runs on the CPU (culling via compute shader at
  high settings, frustum/occlusion on GPU, particle sim on GPU, LOD mesh
  generation on GPU in Phase 4).
- Memory budget is configurable in-game (Options → Performance → "Memory
  limit", 512 MB … all installed RAM). Extra RAM buys: larger chunk cache (no
  re-generation/re-load when you walk back), persistent mesh cache, bigger LOD
  cache, preloaded texture mips, uncompressed chunk storage in RAM. The budget is
  a hard target that the caches evict against, so raising it never causes stutter.
- **CPU limit** (Options → Performance): worker-thread count and a frame-time
  budget per stage; **GPU limit**: FPS cap, render-scale, and a "GPU busy %"
  target that auto-tunes render distance/LOD when enabled.
- Benchmarks (`cargo bench` + `rustcraft --bench`) run on both reference machines
  each phase and are recorded in `docs/perf/`.

**Rendering roadmap additions.**
- Phase 1: physically-plausible lighting (even without shader packs) using
  per-pixel lighting from the vanilla light map, with a PBR-ready material
  pipeline (albedo/normal/specular slots, LabPBR texture format support when a
  resource pack provides it).
- Phase 4: Iris shader packs **with PBR** (LabPBR / OldPBR normal + specular
  maps), plus **DLSS** (Super Resolution, and Frame Generation where the GPU
  supports it) via the NVIDIA Streamline / NGX SDK on Windows and the Linux
  Vulkan NGX library on Linux where NVIDIA ships it; FSR 3 as the vendor-neutral
  counterpart for AMD/Intel/iGPUs. The DLSS SDK has a licence agreement and
  redistribution rules — the owner accepts it when Phase 4 starts; the DLL is
  bundled only in release builds, never in the repo. DLSS is always optional:
  the game is complete without it.

## Mods that ship as first-party RustCraft mods

Some features are built as RustCraft mods (Phase 5 `rc_mod` ABI) rather than
engine features, so they prove the mod system can carry heavy content:

- **BeamNG.drive / Automation vehicles** (Phase 7): full `.jbeam` parsing
  (nodes, beams, hydros, slots, wheels, powertrain, controllers), Collada mesh +
  material import, soft-body physics running on worker threads with GPU-assisted
  solvers at high settings, deformation, drivetrain and engine simulation, sound,
  camera modes, a vehicle spawner UI. Delivered in `mods/rustcraft-vehicles/`
  with its own release artefact.

## Cross-cutting, every phase

- **Screenshots** in `docs/screenshots/phaseN/`, captured headlessly in CI/dev
  container (Xvfb + Mesa lavapipe) and on real hardware when available.
- **GitHub issues first**: before each phase, open issues are triaged and fixed.
- **Releases**: tag `vX.Y.Z` → CI builds → GitHub Release with portable `.exe`
  (Phase 1), plus `.AppImage` (Phase 2+), checksums and signature.
- **Auto-update** ships in every build from 0.1.0.
- **Tests**: unit tests per crate, worldgen reference-seed tests, world-format
  round-trip & fuzz tests, headless render smoke tests.
- **Clean-room log**: `docs/CLEAN_ROOM.md` records which public source each
  behaviour was derived from.

## Engineering constraints of the dev environment
- No GPU and no Windows toolchain in the cloud dev container: rendering is verified
  through Mesa lavapipe (software Vulkan) and all Windows artefacts are produced by
  GitHub Actions. Local builds here are Linux x86_64.
