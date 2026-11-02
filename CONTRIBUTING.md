# Contributing

- Read `docs/CLEAN_ROOM.md` first. No decompiled Minecraft code, ever.
- `cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace` must pass.
- Each crate's `lib.rs` says which roadmap phase it belongs to; keep changes in scope.
- Record non-obvious vanilla behaviour sources in `docs/CLEAN_ROOM.md`'s provenance table.
- Screenshots that document progress go in `docs/screenshots/phase<N>/`.
