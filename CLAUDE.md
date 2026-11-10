# RustCraft — project conventions for AI sessions

- Read `docs/ROADMAP.md` to find the current phase and its checklist; update the
  checkboxes as work lands. `docs/DECISIONS.md` holds ADRs — append, don't edit.
- **Clean room**: never consult or reproduce decompiled Minecraft code. Use the
  Minecraft Wiki / protocol wiki / public pack formats; log sources in
  `docs/CLEAN_ROOM.md`. Never add Mojang textures, sounds, fonts or text.
- Before each phase: `gh api repos/goglesquirmintontheiii/rcraft/issues?state=open`
  (GraphQL is unavailable; use REST) and work issues first.
- Toolchain: stable Rust, edition 2024, `#![forbid(unsafe_code)]` unless a crate
  documents why it needs unsafe. Workspace deps live in the root `Cargo.toml`.
- Quality gate before every commit: `cargo fmt --all && cargo clippy --workspace
  --all-targets && cargo test --workspace`. CI runs the same with `-D warnings`.
- Headless rendering in the dev container: install `mesa-vulkan-drivers`, then
  `WGPU_BACKEND=vulkan VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json`.
  Screenshots go in `docs/screenshots/phase<N>/` and are committed.
- Prefer local checks over GitHub CI (it is slow): `tools/local-ci.sh check` runs
  fmt, clippy and tests for Linux and for Windows (cross-compiled with MinGW,
  tests under Wine); `tools/local-ci.sh dist <dir>` builds the `.exe`, the
  `.AppImage` (in an Ubuntu 22.04 podman container for glibc 2.35) plus
  `latest.json` and `SHA256SUMS`. GitHub releases still come from
  `.github/workflows/release.yml` on tag push `vX.Y.Z` (bump
  `workspace.package.version` first).
- Worldgen is **seed-exact** with vanilla 1.21.1 (ADR-012): RNG/noise must be
  bit-exact; every stage is diff-tested against reference captures in
  `tests/parity/` taken from the owner's own game. Ask the owner for new
  captures rather than approximating.
- Multiplayer never requires a Microsoft account for offline-mode servers (ADR-013).
- Performance rules: no per-frame allocation in render/tick paths; all heavy work
  on rayon pools sized from the in-game CPU limit; memory caches evict against the
  in-game memory limit; GPU does GPU work (culling, particles, LOD gen).
- Commit style: imperative subject, body explains *why*; reference issues.
