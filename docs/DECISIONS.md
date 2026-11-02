# Architecture Decision Records

Short, numbered, append-only. Supersede rather than edit.

## ADR-001 Language & toolchain
Rust stable (edition 2024, MSRV = latest stable at time of release), Cargo
workspace, `cargo xtask` for tooling. No nightly features.

## ADR-002 Graphics API: wgpu
wgpu gives Vulkan/DX12/Metal/GL from one code base, WGSL shaders, and `naga`
for translating Iris GLSL shader packs in Phase 4. Trade-off: Iris packs assume
OpenGL semantics (y-flip, uniform layouts, legacy GL state); the translation layer
is the main Phase 4 risk and is budgeted as such. Alternative rejected: raw OpenGL
via `glow` (simpler Iris parity, but worse multi-threading and no DX12/Metal).

## ADR-003 Windowing: winit 0.30
Stable, wgpu-compatible, Wayland + X11 + Win32. Will track 0.31 when released.

## ADR-004 UI: custom vanilla-style framework, not egui
Pixel parity with vanilla screens requires vanilla's layout/scaling/font rules;
egui's look and layout engine cannot reproduce them. egui may be used for
developer-only debug windows behind a feature flag.

## ADR-005 Assets are not redistributed
The repo ships an original CC0 fallback pack. Users import their own legally owned
vanilla assets at runtime for parity. See ROADMAP Decision 1 and CLEAN_ROOM.md.

## ADR-006 Registries generated from data
Blocks, items, biomes etc. are defined in `data/**/*.toml` and compiled into Rust
enums/tables by `rc_data/build.rs`. This keeps the 1000+ block definitions
reviewable, diff-able, and reusable by the Anvil/Bedrock converters and mod loader.

## ADR-007 World format: RCW
Custom format (docs/WORLD_FORMAT.md) rather than Anvil. Anvil import/export is a
converter (`rc_anvil`), not the native store.

## ADR-008 Releases built by GitHub Actions only
The development container has no Windows toolchain or GPU. All release artefacts
are produced from a tag by `release.yml` on GitHub-hosted runners (windows-latest
MSVC for the portable `.exe`, ubuntu-22.04 for the AppImage) so they are
reproducible and independent of any developer machine.

## ADR-009 Self-update via GitHub Releases + minisign
`latest.json` on each release lists per-platform asset URL, size, SHA-256, and a
minisign signature over the checksum file. The public key is compiled into the
binary; the private key lives only in the repository's Actions secrets.
Replacement uses the `self-replace` crate (rename-and-relaunch on Windows).

## ADR-010 Simulation is deterministic and single-threaded per world
Game logic runs on one sim thread at 20 TPS like vanilla so that block-update
order quirks (needed for redstone parity in Phase 3) are reproducible. Parallelism
is used for generation, meshing, lighting init and I/O, not for the tick itself.

## ADR-011 Java-mod support strategy (provisional)
Native WASM mods first; data-driven Java mods second; JVM bridge only as a
research spike. To be confirmed by the owner before Phase 5 (see ROADMAP).

## ADR-012 Seed-exact worldgen parity is a hard requirement
Decided 2026-11-02. All RNG and noise primitives are bit-exact reimplementations
from public documentation, and every generation stage is diff-tested against
reference captures taken from the owner's legally owned vanilla install. The
Anvil *reader* (`rc_anvil`) therefore ships in Phase 1 as test infrastructure;
the writer stays in Phase 2. Clean-room rule unchanged: reference data is
observed game *output*, never game code.

## ADR-013 Multiplayer must not depend on Microsoft credentials
Decided 2026-11-02. Offline-mode (`online-mode=false`) servers are supported with
no account at all; Microsoft login is an additive layer for online-mode servers
and is gated on the owner registering an Azure application when Phase 4 begins.

## ADR-014 Licence
Decided 2026-11-02. Code: MIT OR Apache-2.0. Original bundled assets: CC0-1.0.
Third-party assets are never bundled; vanilla assets are imported from the
user's own installation at runtime (ADR-005).
