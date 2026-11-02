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
