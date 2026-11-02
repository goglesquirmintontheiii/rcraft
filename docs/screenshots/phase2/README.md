# Phase 2 screenshots

Rendered headlessly with `rustcraft --screenshot <png> --scene <scene> --seed 0
--camera 0,64.5,0,0,12` on Mesa lavapipe (software Vulkan). As in Phase 1 they
use RustCraft's built-in procedural textures (ADR-017), including the
generated stand-ins for item icons, mobs, the player skin and rain; with a
Minecraft install the player's own vanilla assets are used instead.

| File | Scene |
|---|---|
| `title.png` | title screen |
| `select.png`, `create.png` | world list, create world |
| `mobs.png` | cows, pigs, sheep and chickens (adults and babies), dropped items, block particles, the first-person arm |
| `survival.png` | survival inventory with the player model, 2×2 crafting, armour and off-hand slots |
| `crafting.png` | crafting table with a recipe in progress |
| `furnace.png` | furnace smelting raw iron |
| `creative.png` | creative inventory tabs |
| `rain.png` | rain |
| `hand.png` | a held tool |
