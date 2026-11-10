# RustCraft: session handoff

Use this file to pick the project back up in a new Claude Code session. It
records what happened in the earlier chat sessions, what state things were left
in, and what comes next. Keep it with the repository; the project docs it points
to hold most of the detail.

**Last updated:** 2026-11-09, after the v0.2.0 release.

---

## 1. How to start a new session

Paste something like this as your first message:

> Read `CLAUDE.md`, `docs/HANDOFF.md`, `docs/ROADMAP.md` and `docs/DECISIONS.md`.
> Then check the open GitHub issues with
> `gh api repos/goglesquirmintontheiii/rcraft/issues?state=open` and tell me
> what you'd work on first.

The repository's `CLAUDE.md` is loaded automatically and holds the standing
rules. This file covers the history and the things that aren't written down
anywhere else.

---

## 2. The project in one paragraph

**RustCraft** is a clean-room reimplementation of *Minecraft: Java Edition
1.21.1* in Rust. The repo is <https://github.com/goglesquirmintontheiii/rcraft>.
The game is built in phases, and each phase ends with a GitHub release:

- Phase *N* ships as **v0.N.0** (Phase 1 is 0.1.0, Phase 2 is 0.2.0, Phase 3 is 0.3.0, and so on).
- The **final phase skips straight to 1.0.0**.

The full phase plan is in `docs/ROADMAP.md`. Phases 3–7 cover content and
redstone, multiplayer with shaders and LOD, mods, parity with a later Minecraft
version, and vehicles, Bedrock and polish.

---

## 3. Where things stand

| Phase | Version | Status |
|---|---|---|
| 0: roadmap and skeleton | none | done |
| 1: core engine (seed-exact worldgen, renderer, RCW saves, UI, physics) | v0.1.0 | released |
| 2: survival (worlds, crafting, mobs, sound, weather, Java import/export) | **v0.2.0** | **released 2026-11-03** |
| 3: content complete (redstone, minecarts, all blocks, items, mobs, structures) | v0.3.0 | **next** |

**State of v0.2.0**
- The release commit is `a95cc5c` on `main`, and the tag `v0.2.0` points to it.
- Release assets, all verified:
  - `rustcraft-0.2.0-windows-x86_64.exe`
  - `rustcraft-0.2.0-linux-x86_64.AppImage`
  - `latest.json`
  - `SHA256SUMS`
- Verification done:
  - Both checksums match `SHA256SUMS`.
  - The AppImage reports `rustcraft 0.2.0` and rendered the title screen headlessly.
  - The `.exe` contains the 0.2.0 version string. It was **not run on Windows**,
    so the owner should launch it once on the laptop.
- CI on `main` passes.

**Phase 2 was only partly completed.** Part of the Phase 2 scope moved to
Phase 3. The "Carried over to Phase 3" list in `docs/ROADMAP.md` has the
details. In short:
- **Blocks:** signs, banners, enchanting, anvils, brewing, the other special blocks.
- **Passive mobs:** horses, wolves, cats, villagers and the rest of the list.
- **Entities:** boats, arrows, paintings, item frames.
- **Systems:** recipe book, status effects, enchantments and repair, moon phases.
- **Other:** key rebinding, Anvil entity/POI import, an Anvil benchmark, minisign
  verification of updates.

---

## 4. Open loose ends (do these first)

1. **Reply on issue #1 ("Many issues").** The item-by-item reply below was never
   posted, because posting to GitHub was blocked by a permission setting in the
   session. Either the owner pastes it, or the owner allows posting and Claude
   does it. Leave the issue open until the owner has tried 0.2.0.
2. **Check the `.exe` on Windows.** The owner's laptop runs Windows 11 with an
   integrated GPU.
3. **Fix a doc mismatch.** `docs/ROADMAP.md` Phase 4 says LOD goes "up to 4096
   blocks". The owner later asked for **LOD up to 1024 chunks**, and ADR-021
   records that. Update the roadmap line when Phase 4 starts, or before.
4. **Check open issues** before starting Phase 3. That's a project rule.
5. **Optional: a leftover branch.** A remote branch `release-test-probe` was
   created during Phase 1 and could not be deleted from the session. The owner
   was asked to delete it. If it still exists, it can be removed.

### Draft reply for issue #1

```markdown
Thanks for the detailed list. Everything below is in `main` (a95cc5c) and ships as 0.2.0.

| Report | What changed |
|---|---|
| No hand | The first-person arm and held items render now, with swing, bobbing and an eating animation. |
| No player model in the inventory | The survival inventory shows the player model, and it follows the mouse. |
| Inventory blocks bigger than slots | Block icons are scaled to fit the slot. |
| Command autocompletion | Tab completes commands, subcommands, item and mob ids, game rules and selectors. Up/Down picks a suggestion, and a grey hint shows the expected arguments. |
| Glass etc. in the Natural tab | Natural Blocks only has naturally generated blocks now. Crafted ones went to Building, Coloured and the other vanilla tabs. |
| No sounds | Sounds now come from your launcher's asset index: blocks, footsteps, mobs, UI clicks, rain and music. There's a Music & Sounds screen for volume. |
| No breaking particles | Particles show when you hit and break blocks, and when you splash into water. |
| Lighting feels flat | The AO curve is softer and the light curve brighter. Entities are lit from the world light too. |
| Slow loading after a ~1000-block teleport | Chunks next to you are loaded first, and the game spends more of each frame loading until the area around you is ready. Generation itself is still the main cost on a laptop CPU. LOD and GPU work in Phase 4 should help further. |
| Leaves too see-through (spruce, birch) | The cutout threshold now matches vanilla's (alpha < 0.1). This fixes the holes that mipmapping opened up. |
| 900 MB with a ~400 MB limit | The game now measures its actual memory use and lowers render distance while it's over the limit. The up-front estimate also counts the fixed ~250 MB. Note that a 400 MB limit only leaves room for a small render distance. |
| `/tp` should take `@s`/`@e` | `/tp` accepts `@s`, `@p`, `@a`, `@r` and `@e[type=…,limit=…]`, plus relative `~` coordinates. |
| Search only in its own tab | Search has its own tab, and the item tabs no longer react to typing. |
| Flashing oak leaves / grass / flowers | Same fix as the leaves: those textures were losing alpha at a distance. |
| Liquids not flowing | Water and lava flow with vanilla levels, falling and slope search. Lava meeting water makes obsidian or cobblestone. |
| No background on menus | The panorama is used when present. Otherwise there's a tiled dirt background. |
| Inventory scrolling too fast on a laptop | Touchpad scrolling now builds up to whole notches before it moves. |
| Items should drop on the ground | Drops are item entities now, with pickup delay and merging. Each chunk is capped so large farms stay cheap, and Q / Ctrl+Q drop items. |
| Sharp FOV change when sprinting | The FOV eases between walking and sprinting the same way vanilla does. |

Also added from your messages: Options → Performance → **Open Data Folder**, and render distance up to **128 chunks** (LOD later goes to 1024).

Leaving this open until you've tried 0.2.0.
```

---

## 5. How releases work (important)

The cloud session **cannot push tags or create releases**:
- Tag pushes and creating refs or tags through the REST API are blocked.
- An attempt to make `release.yml` trigger from a branch instead was refused as a
  workaround. Don't try that again.

So the steps are:
1. Claude bumps `workspace.package.version` in the root `Cargo.toml`, writes
   `docs/releases/vX.Y.Z.md`, runs the quality gate, then commits and pushes to `main`.
2. **The owner creates the release in the GitHub web UI.** In "Choose a tag", type
   `vX.Y.Z` and pick **"Create new tag on publish"**, with the target set to
   **main**. Creating the tag fresh is what triggers
   `.github/workflows/release.yml`.
3. The workflow builds the Windows `.exe` and the Linux `.AppImage` (the AppImage
   is skipped for `v0.1.*`), then attaches `latest.json` and `SHA256SUMS`.
4. Claude checks the result:
   - Watch the run through `gh api repos/.../actions/runs`.
   - Download the assets through `gh api -H "Accept: application/octet-stream" repos/.../releases/assets/<id>`.
     Note that `gh release download` fails because it uses GraphQL.
   - Check them with `sha256sum -c SHA256SUMS`.
   - Run the AppImage with `--appimage-extract`, then `rustcraft --version`.

**Lesson from 0.2.0.** The owner first created the release *before* the code was
pushed. That tagged the old 0.1.0 commit and built a mislabelled "0.2.0", which
would have caused an update loop for 0.1.0 users. Deleting a release does **not**
delete its tag, so the tag also has to be deleted on the Tags page. Before
asking the owner to create a release, make sure the code is on `main`, and tell
them to create a *new* tag.

The game's self-updater reads `latest.json` from the latest release, so the
version in `Cargo.toml` must match the tag.

---

## 6. Standing rules (summary of `CLAUDE.md` and the ADRs)

- **Clean room.** Never consult or reproduce decompiled Minecraft code. Use the
  Minecraft Wiki, the protocol wiki and public pack formats, and log sources in
  `docs/CLEAN_ROOM.md`. Never commit Mojang textures, sounds, fonts or text.
  - Vanilla data-generator output (recipes, loot tables, tags, items) *is*
    vendored as data under ADR-015.
  - Repository screenshots use the built-in fallback textures only (ADR-017).
- **Quality gate before every commit:**
  `cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace`.
  CI runs the same with `-D warnings`.
- **Toolchain:** stable Rust, edition 2024, `#![forbid(unsafe_code)]`. Workspace
  dependencies live in the root `Cargo.toml`.
- **Issues:** check them via REST (`gh api repos/goglesquirmintontheiii/rcraft/issues?state=open`).
  GraphQL is unavailable, so `gh issue`, `gh pr` and `gh release download` fail.
- **Worldgen is seed-exact** with vanilla 1.21.1 (ADR-012). Every stage is
  diff-tested against `tests/parity/` captures from the owner's own game. Ask the
  owner for new captures rather than approximating.
- **Multiplayer must never require a Microsoft account** for offline-mode servers (ADR-013).
- **Performance:**
  - No per-frame allocation in render or tick paths.
  - Heavy work runs on rayon pools sized from the in-game CPU limit.
  - Caches evict against the in-game memory limit.
  - The GPU does GPU work.
  - The memory limit is also enforced from measured usage (ADR-021).
- **Commits:**
  - Imperative subject; the body explains *why*; reference issues.
  - Author: `goglesquirmintontheiii`.
  - End the message with:
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: <the current session's link>
    ```
- **The owner's preferences seen so far:**
  - Wants everything in an issue fixed before a phase is released.
  - Tests on a Windows 11 laptop with an integrated GPU.
  - Wants smooth feel: FOV easing, scroll speed.
  - Wants vanilla-accurate behaviour and layout.

---

## 7. Dev environment notes

- **Headless rendering** in the container needs `mesa-vulkan-drivers`, then:
  ```
  export WGPU_BACKEND=vulkan VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
  ```
- **Screenshots:**
  ```
  rustcraft --game-dir <dir> --screenshot out.png --scene <scene> --seed 0 \
            --size 1280x720 --camera "0,64.5,0,0,12" [--assets <jar>]
  ```
  - Scenes: `title select create world f3 inventory creative pause chat survival crafting furnace mobs rain hand`.
  - Setting `RC_SLOT=<n>` picks the held hotbar slot in the `hand` scene.
  - Phase screenshots go in `docs/screenshots/phase<N>/` with a README.
  - Leave out `--assets` so the screenshots use the fallback textures.
  - At spawn on seed 0 the camera starts over ocean, so use an explicit y such
    as 64.5 rather than `surface+1`.
- **Test fixtures outside the repo** were in the old container and **are gone in
  a new session**. Rebuild them if needed:
  - `/home/claude/fakemc`: a fake `.minecraft` with `versions/1.21.1/1.21.1.jar`,
    a launcher asset index (`assets/indexes/17.json`) and a subset of `objects/`,
    used to test real-asset loading and sounds locally. Never commit any of this.
  - `/home/claude/testassets/mcassets` and `/home/claude/vanilla`: data-generator
    and asset sources used by `tools/import_vanilla_data.py`.
- There is no GPU and no Windows toolchain in the container. All Windows builds
  come from GitHub Actions.

---

## 8. Code map: what Phase 2 added

These are all in `crates/rustcraft/src/` unless a path says otherwise. See
`docs/ARCHITECTURE.md` for the overall structure.

| File | Purpose |
|---|---|
| `engine.rs` | The frame loop and screen stack. Wires up audio, opening and creating worlds, and world actions (rename/delete/backup/import/export). Also the memory guard (`render_distance()`, `memory_guard()`) and the faster loading after a teleport. |
| `game.rs` | Per-world simulation: ticking, breaking and placing, using blocks and items, containers, fluids, random ticks, weather, sleeping, sounds out. Also `saved_player_json` and `parse_saved_player`. |
| `worlds.rs` | Listing the saves, safe folder names, rename/delete/backup, and the select / create / edit / delete / import / sounds screens. |
| `java.rs` | Java Edition import and export (`level.dat`, region `.mca` files, player and inventory). Has a round-trip test. |
| `scene.rs` | Fills the renderer batches: mobs, items, falling blocks, XP orbs, lightning, particles, rain and snow, the first-person hand and held item. |
| `fallback_tex.rs` | Procedural textures for mobs, skin, rain, snow and orbs, plus generated item icons for runs without a Minecraft install. |
| `entity.rs`, `mobs.rs`, `models.rs` | The entity system, passive mob AI, and box-UV models. |
| `blockentity.rs`, `blocklogic.rs` | Chests and furnaces; fluids, block shapes, random ticks, trees. |
| `items.rs`, `recipes.rs`, `loot.rs` | Item registry and inventory, crafting and smelting, loot tables. |
| `containers.rs` | Container screens and the creative screen (tabs and search). |
| `commands.rs` | Commands, autocompletion and selectors. |
| `particles.rs`, `player.rs` | Particles; player physics, hunger, XP and climbing. |
| `crates/rc_render/src/terrain.rs` | `add_texture`, plus the `batches`, `blend_batches` and `hand_batches` lists, and a hand pass that clears depth. |
| `crates/rc_audio` | Sound playback with rodio; volumes per category. |
| `crates/rc_anvil` | NBT write and gzip, `RegionWriter`, and `AnvilChunk::to_nbt`. |
| `crates/rc_world` | Chunk records now carry entities and block entities as JSON blobs (ADR-020; format in `docs/WORLD_FORMAT.md`). |
| `crates/rc_assets` | Reads the launcher's object store (`AssetSource::Objects`) and has `AssetStack::reopen()`. |

**Known rough edges worth revisiting:**
- The positions and sizes of the first-person hand and held item were tuned by
  eye from screenshots. Compare against the real game when possible.
- Dying mobs shrink instead of tipping over.
- Flat items are drawn as two quads rather than an extruded 3D shape.
- The title-screen splash text overlaps the subtitle.
- Fallback mode: hearts and hunger icons show as squares, and the generated item
  icons are simple shapes.
- `world.update` builds a list of wanted chunks every frame. That's fine at the
  old maximum of 32 chunks but costly at 128, so make it incremental before LOD
  work.
- Loading after a teleport is still bound by worldgen speed (about 50 ms per chunk
  for noise; aquifers and surface rules are heavy).

---

## 9. Conversation log (the owner's messages)

The earliest messages (project setup: the phase plan, hardware targets, the
BeamNG/Automation vehicle mod idea, Phase 1 decisions, a worlds zip for parity
captures, and "Build phase 1 and release version 0.1.0") were in a session whose
transcript was compacted. What came out of them is recorded in
`docs/ROADMAP.md` and `docs/DECISIONS.md`. The owner's messages from then on:

1. "I created a release, can you add the files to it?" *(about v0.1.0. The
   workflow attached the `.exe`, `latest.json` and `SHA256SUMS`.)*
2. "Ok I think I'm ready for 0.2.0. I went ahead and made the release so the
   files can be added. Also add a button to open the data folden under settings
   -> performance. I made a github issue, so check it."
3. "(Clarification just to make sure you understand - version 0.2.0 is phase 2,
   0.3.0 is phase 3, etc. The final phase will skip straight to 1.0.0 - so
   you'll fix everything and look over everything in the github issue then build
   phase 2 and release)"
4. *(sent mid-task)* "Also make the max render distance 128 chunks (LOD rendering
   will have a max of 1024 chunks)"
5. "Ok, I deleted the existing 0.2.0 and make a new 0.2.0" *(The tag was reused,
   so it still pointed at the old commit and nothing was built. The owner was
   asked to delete the tag as well and create a new one.)*
6. "Done. Rebuilt release and tag. Should be ready for executables" *(The build
   ran from `a95cc5c`, and all assets were verified.)*
7. "Can you turn this entire chat into an MD file to be continued later using
   Claude Code?" *(This file.)*

---

## 10. Suggested next steps

1. Close out the loose ends in section 4.
2. Fix any new issues the owner files after trying 0.2.0.
3. Start **Phase 3** (content complete → v0.3.0). Work in this order:
   - Begin with the items carried over from Phase 2 and key rebinding.
   - Then redstone. Its vanilla update-order quirks make it the riskiest part,
     so plan behaviour tests against the wiki.
   - Then minecarts.
   - Then the remaining blocks and items, hostile and neutral mobs, and structures
     (jigsaw/template pools). Structures must stay seed-exact; ask the owner for
     parity captures.
4. Release as described in section 5.
