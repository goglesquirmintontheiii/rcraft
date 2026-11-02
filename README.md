# RustCraft

A clean-room reimplementation of **Minecraft: Java Edition 1.21.1** in Rust —
fast, multi-core, GPU-first, and built to run from a Core 2 Duo laptop in under
1 GB of RAM up to a 16-core desktop with 96 GB.

> Status: **Phase 0 — roadmap & skeleton.** Nothing playable yet.
> See [`docs/ROADMAP.md`](docs/ROADMAP.md) for the plan and
> [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for how it fits together.

## Goals in one breath

Vanilla 1.21.1 parity (terrain, blocks, mobs, structures, UI) · a proprietary
world format that loads faster than Anvil, with Java/Bedrock import & export ·
official-server multiplayer · built-in Iris-compatible PBR shaders, DLSS/FSR and
Distant-Horizons-style LOD · built-in mod support (native RustCraft mods and
Java-ecosystem mods) · self-updating portable `.exe` / `.AppImage` releases.

## Building

```sh
cargo build --release -p rustcraft          # the game
cargo test --workspace                       # tests
cargo xtask screenshot out.png               # headless screenshot (needs any Vulkan/DX12 driver, lavapipe is fine)
```

Linux build deps: `libxkbcommon-dev libwayland-dev libasound2-dev libudev-dev`.

## Legal

RustCraft is not affiliated with Mojang or Microsoft. It ships **no** Mojang
assets; for vanilla textures, sounds and fonts you import your own legally owned
copy of the game. See [`docs/CLEAN_ROOM.md`](docs/CLEAN_ROOM.md).

Code: MIT OR Apache-2.0. Bundled original assets: CC0.
