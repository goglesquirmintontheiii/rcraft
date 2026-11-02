# Clean-room policy

RustCraft contains no code derived from Mojang's binaries or decompiled sources, and
no Mojang assets.

## Allowed sources
- Minecraft Wiki (minecraft.wiki) — game mechanics, block data, worldgen parameters,
  UI layouts, formats (Anvil, NBT, resource/data packs)
- Protocol documentation at wiki.vg / minecraft.wiki protocol pages
- Public data-pack / resource-pack format documentation and the worldgen JSON
  format as documented (density functions, noise settings, surface rules)
- Black-box observation of the real game (screenshots, measurements, behaviour)
- Open-source projects under permissive licences, for *technique* reference only
  (e.g. wgpu examples, noise algorithms from academic papers)

- Mojang's data-generator output (block-state reports, worldgen JSON) and
  community-extracted block property tables — data, not code (ADR-015)
- Reference chunks saved by the owner's own copy of the game (`tests/parity/`)

## Not allowed
- Decompiled Minecraft code (MCP, Yarn/Mojmap-mapped sources, Fabric/Forge
  internals that embed vanilla code)
- Copying textures, sounds, fonts, language files or the splash list
- Copying any text that is Mojang's expression (the credits, end poem, etc.)

## Provenance log
When a non-obvious behaviour is implemented, add a line here or a doc comment:

| Area | Behaviour | Source |
|------|-----------|--------|
| worldgen | multi-noise biome parameters | minecraft.wiki "Biome/Noise parameters" |
| worldgen | density function types | minecraft.wiki "Density function" |
| world | paletted container bit widths | minecraft.wiki "Chunk format" |
| net | protocol 767 packets | minecraft.wiki "Java Edition protocol" |
| worldgen | RNG (LCG, Xoroshiro128++, positional factories) | JDK docs, xoshiro paper, minecraft.wiki "Random number generator" |
| worldgen | density-function semantics, cell interpolation order | minecraft.wiki "Density function", "Noise router"; verified against saved chunks |
| worldgen | multi-noise R-tree selection | minecraft.wiki "Biome/Multi-noise"; verified 100% vs reference worlds |
| worldgen | feature placement modifiers & feature configs | minecraft.wiki "Placed feature", "Configured feature" |
| gameplay | block breaking time, tool tiers, correct-tool drops | minecraft.wiki "Breaking"; tool rules from the data generator's item components |
| gameplay | hunger, saturation, exhaustion, regeneration | minecraft.wiki "Hunger" |
| gameplay | experience levels and orb values | minecraft.wiki "Experience" |
| gameplay | fluid spread (levels, falling, slope search, ticks) | minecraft.wiki "Water", "Lava", "Fluid" |
| gameplay | crop growth, bone meal, farmland, saplings, leaf decay | minecraft.wiki "Crops", "Bone Meal", "Farmland", "Leaves" |
| gameplay | weather timers, lightning, sleeping | minecraft.wiki "Weather", "Bed" |
| gameplay | ladder / vine climbing speeds | minecraft.wiki "Ladder" |
| gameplay | recipes, loot tables, tags | Mojang data-generator output (data, ADR-015) |
| entities | item entity pickup delay, merging, despawn | minecraft.wiki "Item (entity)" |
| entities | passive mob behaviour, breeding, baby growth, sheep colours | minecraft.wiki "Cow", "Pig", "Sheep", "Chicken", "Breeding" |
| entities | model shapes in the box-UV texture layout | minecraft.wiki "Model" and the public texture layouts; geometry authored by hand |
| audio | sound events and variants | the player's own `sounds.json` (asset index), minecraft.wiki "Sounds.json" |
| formats | `level.dat`, region `.mca`, chunk NBT, player NBT | minecraft.wiki "Java Edition level format", "Region file format", "Chunk format", "Player.dat format" |

