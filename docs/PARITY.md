# RustCraft v0.2.1 — parity report vs Minecraft Java 1.21.1

Checklist for "full parity before and after 0.3.0". Status is taken from the
code on branch `v0.2.1` (not from the roadmap's checkboxes). The reference is the
Minecraft Wiki plus the vendored data in `data/vanilla/` (1333 items,
1060 blocks, 130 entity types, 1291 recipes, 1103 loot tables, 196 configured
features, 34 structures).

Legend: ✅ done · 🟡 partial (what is missing is noted) · ❌ missing.
Re-run this audit after 0.3.0 by flipping rows; keep the table shapes so diffs
stay readable.

---

## 1. Summary

### Counts per section

<!-- counts: table rows per section; §5 counts each mob once -->
| Section | ✅ | 🟡 | ❌ | Rows |
|---|---:|---:|---:|---:|
| 2. Screens / UI | 10 | 23 | 32 | 65 |
| 3. Interactive blocks | 14 | 23 | 40 | 77 |
| 4. Items with use behaviour | 8 | 6 | 20 | 34 |
| 5. Mobs (82 vanilla mobs) | 0 | 4 | 78 | 82 |
| 6. Systems | 18 | 16 | 19 | 53 |
| 7. World generation | 8 | 3 | 12 | 23 |
| **Total** | **58** | **75** | **201** | **334** |

### Top 15 gaps by player impact

| # | Gap | Why it matters |
|---|---|---|
| 1 | **No hostile mobs at all** (78 of 82 mobs missing; only cow/pig/sheep/chicken) | No combat, no night danger, no mob drops for string, bones, gunpowder, ender pearls, blaze rods — survival progression stops |
| 2 | **No structures** (0 of 34: villages, strongholds, fortresses, monuments, end cities…) | Nothing to explore; blaze rods/ender eyes/elytra unobtainable; End unreachable |
| 3 | **No redstone** (dust, torches, repeaters, comparators, pistons, observers, hoppers, dispensers, levers/buttons/plates do nothing; Redstone creative tab disabled) | A whole gameplay pillar; iron doors/trapdoors cannot be opened |
| 4 | **No item components** (`Stack` is item+count+damage only) → no enchantments, custom names, potions, written books, maps, firework data, dyed armour, shulker-box contents in item form | Blocks enchanting, anvil renaming/combining, brewing, trims, books, maps |
| 5 | **Armour does nothing** (slots exist; no protection points, no armour bar, no right-click equip) | Combat/survival balance; armour bar missing from HUD |
| 6 | **No status effects** (no effect system, no potions, no golden-apple/suspicious-stew effects, no beacon) | Core to food, brewing, beacons, mobs (poison, wither…) |
| 7 | **Enchanting table, brewing stand, beacon, loom, cartography table, lectern, crafter, hopper, dispenser/dropper, ender chest — no UI/behaviour** | Most workstation GUIs missing |
| 8 | **Bows, crossbows, tridents, shields, snowballs, eggs, ender pearls, fishing rods — no projectile entities** | Ranged combat and many core items are inert |
| 9 | **Villagers & trading missing** (villager, wandering trader, professions, workstations, trading GUI) | Key mid-game loop |
| 10 | **End dimension unreachable** (no ender eye, end portal frame, End portal, dragon fight); End terrain generates but cannot be visited in survival | Game cannot be "finished" |
| 11 | **Large (double) chests missing** — two chests never connect; the chest UI is always 27 slots | Storage; highly visible |
| 12 | **Minecarts, rails, boats** — no vehicle entities | Transport |
| 13 | **Signs cannot be edited**; no banners, item frames, paintings, armour stands, flower pots | Building / decoration |
| 14 | **Key rebinding, Resource Packs, Language, Chat, Accessibility, Skin options screens missing** | Options parity; rebinding is a usability blocker |
| 15 | **Recipe book, advancements, statistics** missing; special crafting recipes (firework, map, armour dye, banner, repair-by-crafting, tipped arrows, shulker recolour, decorated pot) and smithing trims not loaded | Progression feedback and ~32 recipes unusable |

---

## 2. Screens / UI

### Menus and options

| Screen | Status | Notes |
|---|---|---|
| Title screen | 🟡 | Panorama, splash, Singleplayer, Options, Quit. Multiplayer (v0.4), Mods (v0.5) disabled; no Realms, language/accessibility icon buttons, copyright/attribution |
| Singleplayer world list | ✅ | List, play, edit (rename/backup), delete with confirm, plus RustCraft "Import" (Anvil) |
| Create world — Game tab | 🟡 | Name, seed, mode (Survival/Creative/Spectator), difficulty. Missing Hardcore, "Allow Commands" toggle, Adventure in cycle |
| Create world — World tab | ❌ | No world type (Default/Superflat/Large Biomes/Amplified/Single Biome), "Generate Structures", "Bonus Chest", customise |
| Create world — More tab | 🟡 | Game Rules page with 10 boolean rules; no categories, no integer rules, no Data Packs, no Experiments |
| Options (main) | 🟡 | FOV, difficulty, Video, Controls, Music & Sounds, plus RustCraft Performance/Updates/Minecraft Directory. Missing Skin Customization, Chat, Accessibility, Online, Telemetry, Credits & Attribution; Resource Packs and Language are disabled stubs |
| Video Settings | 🟡 | Graphics, render distance, smooth lighting, max fps, VSync, GUI scale, brightness, clouds, view bobbing, fullscreen, mipmaps, show FPS. Missing simulation distance, biome blend, entity shadows, entity distance, distortion/FOV effects, attack indicator, chunk builder, fullscreen resolution, glint options |
| Controls → Mouse settings | 🟡 | Sensitivity and Invert Mouse; no scroll sensitivity, discrete scrolling, touchscreen, raw input |
| Controls → Key Binds | ❌ | Static, read-only list; **no rebinding** (carried over twice) |
| Music & Sounds | 🟡 | Volume per category; missing device selection, subtitles toggle, directional audio |
| Chat Settings | ❌ | |
| Accessibility | ❌ | (narrator, text background, high-contrast, darkness pulsing, damage tilt, toggle sneak/sprint…) |
| Language | ❌ | Disabled button; English only |
| Resource Packs | ❌ | Disabled button (Phase 3 item) |
| Skin Customization | ❌ | |
| Pause menu | 🟡 | Back to Game, Options, Save & Quit. Advancements, Statistics, Report Bugs, Open to LAN are disabled; no Feedback |
| Death screen | 🟡 | Respawn / Title; death message; no score line, no hardcore variant |
| Loading / terrain screen | ✅ | |
| Chat | 🟡 | Input, history, formatting codes, command suggestions; no clickable/hover text components, no chat scrolling settings |
| F3 debug screen | 🟡 | FPS, XYZ, block, chunk, facing, biome, light etc.; no right-hand column (system, targeted block/fluid state & tags), no F3+key combos (F3+B hitboxes, F3+G chunk borders, F3+H tooltips, F3+T reload…) |
| F1 hide HUD / F2 screenshot | ✅ | |
| F5 perspective (third person) | ❌ | |
| Sleep screen ("Leave Bed") | ❌ | Sleeping fades/skips night, no bed UI |

### HUD

| Element | Status | Notes |
|---|---|---|
| Hotbar + selected-item name | ✅ | |
| Offhand slot | ✅ | Swap with F |
| Hearts (incl. low-health shake, hurt blink) | 🟡 | No absorption/poison/wither/frozen heart variants, no hardcore hearts, no regeneration wave |
| Hunger | 🟡 | No hunger-effect (green) variant, no saturation jitter |
| Air bubbles | ✅ | |
| Armour bar | ❌ | Sprite loaded, never drawn (armour has no points) |
| XP bar and level number | ❌ | Background drawn only ("empty in this version"); XP is tracked in `Player` but not shown |
| Mount health / jump bar | ❌ | No mounts |
| Crosshair, attack indicator | 🟡 | Crosshair yes; no attack-cooldown indicator |
| Boss bar | ❌ | |
| Titles / actionbar | 🟡 | Action-bar style messages exist; no `/title` |
| Subtitles | ❌ | |
| Scoreboard sidebar | ❌ | |
| Vignette, pumpkin blur, powder-snow, portal overlay | ❌ | |
| Tooltips | 🟡 | Name only; no lore/enchantments/attributes (no components) |

### Container GUIs

| GUI | Status | Notes |
|---|---|---|
| Survival inventory (2×2 crafting, armour, offhand, player model) | ✅ | Shift-click quick move; no recipe-book button |
| Crafting table | ✅ | Shaped/shapeless; special recipes not loaded (see §6) |
| Chest / trapped chest / barrel | ✅ | 27 slots |
| Large chest | ❌ | Chests never join; UI fixed at 3 rows |
| Furnace / blast furnace / smoker | 🟡 | Fuel, progress, lit state; smelting XP accumulates in the block entity but is never paid out to the player |
| Anvil | 🟡 | Repair with material, combine two damaged items, level cost. Rename field disabled; no enchantment merging |
| Grindstone | 🟡 | Combines durability (+5 %); no disenchanting / XP return |
| Stonecutter | ✅ | Scrollable recipe list from vanilla data |
| Smithing table | 🟡 | 9 netherite `smithing_transform` recipes; 18 `smithing_trim` recipes not loaded |
| Shulker box | 🟡 | Opens like a chest; contents drop when broken instead of staying in the item |
| Enchanting table | ❌ | |
| Brewing stand | ❌ | |
| Beacon | ❌ | |
| Loom | ❌ | |
| Cartography table | ❌ | |
| Lectern / book screens (book & quill, written book) | ❌ | |
| Hopper | ❌ | |
| Dispenser / dropper | ❌ | |
| Crafter | ❌ | |
| Ender chest | ❌ | |
| Villager / wandering-trader trading | ❌ | |
| Horse / donkey / llama / mule / camel inventory | ❌ | |
| Sign editing screen (incl. hanging signs) | ❌ | |
| Command block / structure block / jigsaw editors | ❌ | |
| Recipe book (crafting, furnace family) | ❌ | Carried over from Phase 2 |
| Creative inventory | 🟡 | Building, Coloured, Natural, Functional, Tools, Combat, Food, Ingredients, Search, Survival Inventory. **Redstone** and **Spawn Eggs** tabs disabled; no Operator tab, no saved hotbars, no destroy-item slot behaviour check |
| Inventory gestures | 🟡 | Click, right-click split, shift-click, Q/ctrl-Q drop; missing drag-distribute, double-click collect, number-key swap, offhand-key swap in GUI, middle-click clone in creative GUI |

---

## 3. Interactive blocks

### Doors, gates, climbing, shapes

| Block family | Status | Notes |
|---|---|---|
| Wooden doors / trapdoors / fence gates | ✅ | Open/close with sounds, two-half doors |
| Iron door / iron trapdoor | ❌ | Need redstone |
| Copper doors / trapdoors (incl. oxidised) | 🟡 | Open via the wooden path; no oxidation, waxing, scraping |
| Fence / pane / wall / stair connection shapes | ✅ | Neighbour updates |
| Ladders, vines, scaffolding (climb) | ✅ | Scaffolding distance, falling, sneak-descend |
| Twisting/weeping vines, cave vines climbing | 🟡 | Climbable tag only; no growth, glow berries harvest |
| Slabs (double), waterlogging on place | ✅ | |

### Containers & workstations

| Block | Status | Notes |
|---|---|---|
| Chest, trapped chest, barrel | 🟡 | No double chests, no lid animation/open-state, trapped chest emits no signal |
| Ender chest | ❌ | |
| Shulker boxes (17) | 🟡 | See §2 |
| Crafting table | ✅ | |
| Furnace, blast furnace, smoker | ✅ | Lit state, fuels, cook times |
| Anvil (3 damage states) | 🟡 | Falls, degrades with use; see §2 |
| Grindstone, stonecutter, smithing table | 🟡 | See §2 |
| Enchanting table + bookshelf power | ❌ | |
| Brewing stand | ❌ | |
| Beacon (pyramid, effects, beam) | ❌ | |
| Loom, cartography table, fletching table (no-op in vanilla) | ❌ | Fletching is a profession block only |
| Lectern, chiseled bookshelf | ❌ | |
| Hopper, dispenser, dropper, crafter | ❌ | |
| Composter | ❌ | |
| Cauldron (water/lava/powder snow, filling, dye washing, rain fill, dripstone fill) | ❌ | |
| Decorated pot, suspicious sand/gravel brushing | 🟡 | Suspicious blocks fall; no brushing, no pot inventory |
| Jukebox + music discs | ❌ | |
| Note block (instrument, pitch) | ❌ | |
| Bell | ❌ | |
| Campfire / soul campfire (cooking, smoke, light/extinguish) | ❌ | 9 campfire recipes are loaded but unused |
| Flower pots (all variants) | ❌ | |
| Item frame / glow item frame, painting, armour stand | ❌ | Entities |
| Signs / hanging signs (edit, dye, glow ink, wax) | 🟡 | Place only (wall variant chosen); no text |
| Banners (patterns) | 🟡 | Placeable blocks only |
| Beehive / bee nest (honey level, shears/bottle, smoke) | ❌ | |
| Respawn anchor | ❌ | |
| Lodestone (compass link) | ❌ | |
| Conduit | ❌ | |
| Cake (+ candle cakes) | 🟡 | Eat slices; no candle placing/lighting |
| Candles (place up to 4, light, extinguish) | 🟡 | Place only |
| Beds (17 colours) | 🟡 | Sleep at night/thunder, set spawn, skip night. No explosion in Nether/End, no "monsters nearby", no "too far", no bounce on bed, no `playersSleepingPercentage` |
| Spawner / trial spawner / vault | ❌ | Monster-room spawners are inert blocks |
| Command block, structure block, jigsaw | ❌ | |
| Dragon egg (teleport on click) | 🟡 | Falls; no teleport |
| End portal frame + eye of ender | ❌ | |
| End gateway | 🟡 | Generated by worldgen; no teleport |

### Redstone (Phase 3 target)

| Component | Status |
|---|---|
| Redstone dust (power levels, connections, update order) | ❌ |
| Redstone torch / block, lever, buttons (all woods + stone + polished blackstone), pressure plates (incl. weighted), tripwire + hook, target, daylight detector, lightning rod, sculk sensor / calibrated, trapped chest output | ❌ (levers/buttons place but do nothing) |
| Repeater, comparator (incl. container/cake/composter/jukebox readings) | ❌ |
| Piston / sticky piston (QC, slime/honey), observer | ❌ |
| Redstone lamp, copper bulb, TNT (ignite, explode), note block, doors, rails, dispensers | ❌ |
| Sculk shrieker / catalyst / vein spreading | ❌ |

### Rails & vehicles

| Item | Status |
|---|---|
| Rail shapes (straight/curve/slope auto-connect) | ❌ (places as-is) |
| Powered, detector, activator rails | ❌ |

### Farming, plants, natural blocks

| Behaviour | Status | Notes |
|---|---|---|
| Wheat, carrots, potatoes, beetroots, nether wart, torchflower, sweet berry growth | 🟡 | Random-tick growth by age; ignores light level, farmland moisture and neighbouring-crop growth chance; sweet-berry harvest by click and bush damage missing |
| Melon / pumpkin stems | 🟡 | Stems age but **never produce fruit** (no attached stem / fruit placement) |
| Hoe tilling, farmland | 🟡 | Till grass/dirt/path; no moisture (hydration), no trampling, no reverting to dirt |
| Bone meal | 🟡 | Crops, saplings, grass spreading; not on flowers, tall plants, mushrooms, fungi, moss, kelp/seagrass, bamboo, cocoa, sea pickles, big dripleaf… |
| Saplings → trees | 🟡 | Simplified tree shapes (oak/birch/spruce/jungle/acacia/dark oak/cherry/mangrove log+leaves); no 2×2 giant trees, fancy oak, bee nests, azalea, mushrooms/fungi from bone meal |
| Leaf decay | ✅ | Distance propagation + random-tick decay |
| Grass / mycelium spread and die-back | ✅ | Nylium, moss spread missing |
| Sugar cane, cactus growth | ✅ | Cactus damage missing |
| Bamboo, kelp, cave vines, twisting/weeping vines, chorus, cocoa, sea pickles, glow lichen, dripleaf tilt, sculk growth, pointed dripstone growth/drip, budding amethyst | ❌ | No random-tick behaviour |
| Ice / snow layer melting by light | ✅ | |
| Snow & ice forming during weather | ❌ | Snow is visual only |
| Copper oxidation / waxing / scraping (axe, honeycomb) | ❌ | |
| Axe stripping logs | 🟡 | Logs only; no wood/hyphae stems, bamboo block |
| Shovel → dirt path | 🟡 | Grass only; no campfire dousing |
| Concrete powder hardening in water | ❌ | Falls only |
| Sponge absorbing water | ❌ | |
| Turtle eggs, sniffer eggs, frogspawn hatching | ❌ | |
| Farmland trampling | ❌ | |

### Fluids, gravity, fire, portals

| Behaviour | Status | Notes |
|---|---|---|
| Water / lava flow, sources, levels | ✅ | Vanilla spread rules, washes away plants |
| Lava + water → obsidian / cobblestone / stone | ✅ | Basalt (soul soil + blue ice) missing |
| Waterlogged blocks flowing out, bubble columns, drip from dripstone | ❌ | |
| Gravity blocks (sand, gravel, concrete powder, anvils, dragon egg, suspicious) | ✅ | Pointed dripstone, scaffolding handled separately |
| Fire (placement) | 🟡 | Placed by flint & steel / fire charge; **no spread, burn-out, burning blocks, damage, soul fire on soul soil** |
| Nether portal | ✅ | Any-size frame, ignite, 4 s transfer, 8:1 scaling, link/build destination |
| End portal, end gateway travel | ❌ | |
| Lightning (strike, fire, charged creeper, mooshroom, villager→witch) | 🟡 | Strike entity + sound/flash; no fire or conversions |

---

## 4. Items with use behaviour

| Item / family | Status | Notes |
|---|---|---|
| Block placement (facing/axis/half/slab/waterlog rules) | ✅ | |
| Tools: mining speed, correct-tool drops, durability | ✅ | From the item-component report |
| Tools: special uses (hoe till, axe strip, shovel path) | 🟡 | See §3; no axe scrape/wax-off, shears on pumpkins/beehives/tripwire |
| Swords / melee | 🟡 | Damage per item, 10-tick cooldown; no attack-speed cooldown curve, crits, sweep, knockback modifiers |
| Mace (wind-burst, smash) | ❌ | |
| Armour equip & protection (incl. elytra, turtle helmet, carved pumpkin, heads) | ❌ | Slots only |
| Shield (block, disable by axe) | ❌ | |
| Bow, crossbow, arrows (tipped, spectral) | ❌ | |
| Trident (throw, riptide, loyalty) | ❌ | |
| Snowball, egg (chicks), ender pearl, eye of ender, wind charge, experience bottle | ❌ | |
| Fishing rod | ❌ | |
| Food (eat, nutrition, saturation, bowls returned) | 🟡 | Fixed 32-tick eat time (dried kelp 16 missing); no food effects (golden apples, suspicious stew, pufferfish, rotten flesh, spider eye, chorus teleport, honey cures poison); milk bucket empties but clears nothing |
| Potions, splash, lingering, tipped arrows | ❌ | Tinting only |
| Buckets: water, lava, empty, milk (cow) | ✅ | |
| Powder snow bucket, fish buckets, axolotl/tadpole buckets | ❌ | |
| Glass bottle (fill from water), honey bottle | ❌ | |
| Bone meal | 🟡 | See §3 |
| Dyes on sheep | ✅ | Signs/glow ink missing |
| Shears on sheep | ✅ | Other shear uses missing |
| Flint & steel, fire charge | 🟡 | Lights fire & portals; no TNT, campfire, candle |
| Seeds / crop planting | ✅ | |
| Spawn eggs | ❌ | Tab disabled; `/summon` works for the 4 mobs |
| Minecarts (all), boats & chest boats, rafts | ❌ | |
| Saddle, lead, name tag, horse armour, wolf armour, carrot/fungus on a stick | ❌ | |
| Maps (empty, filled, locator, zoom, cloning) | ❌ | |
| Compass, recovery compass, lodestone compass, clock | ❌ | Static icons |
| Book & quill, written book, enchanted book | ❌ | |
| Firework rockets (boost, launch), firework stars | ❌ | |
| Spyglass, goat horn, brush, bundle (experimental), totem of undying | ❌ | |
| Music discs | ❌ | |
| Ender pearls / eyes | ❌ | |
| Armour trims, smithing templates | 🟡 | Netherite upgrade only |
| Item entities (drop, merge, pickup, 5-min despawn, cap) | ✅ | |
| Pick block (middle click) | ✅ | |

---

## 5. Mobs (82 in 1.21.1, from `entities.json`)

Implemented: **4** (cow, pig, sheep, chicken) with wander, look, panic, tempt,
breed, baby growth, loot-table drops, XP, biome spawning on chunk generation.
Missing everywhere: ongoing natural spawning/despawning (light-level hostile
spawning, mob caps), pathfinding beyond simple goals, mob AI attacking the
player, riding, leashing, name tags.

| Group | Mobs | Status |
|---|---|---|
| Farm animals | cow, pig, sheep, chicken | 🟡 (pig saddle/carrot riding, chicken jockey, lightning→zombified piglin missing) |
| Farm animals | mooshroom (incl. brown), rabbit, goat, turtle, armadillo, sniffer | ❌ |
| Rideable | horse, donkey, mule, skeleton horse, zombie horse, llama, trader llama, camel, strider | ❌ |
| Tameable / pets | wolf, cat, ocelot, parrot, fox, panda, polar bear, allay, axolotl, frog, tadpole | ❌ |
| Insects / ambient | bee, bat | ❌ |
| Aquatic | cod, salmon, pufferfish, tropical fish, squid, glow squid, dolphin | ❌ |
| Utility | iron golem, snow golem | ❌ |
| Villagers | villager (professions, trades, gossip, breeding, raids), wandering trader | ❌ |
| Zombies | zombie, husk, drowned, zombie villager, giant | ❌ |
| Skeletons | skeleton, stray, bogged, wither skeleton | ❌ |
| Arthropods | spider, cave spider, silverfish, endermite | ❌ |
| Overworld hostiles | creeper, witch, slime, phantom, enderman | ❌ |
| Water hostiles | guardian, elder guardian | ❌ |
| Illagers | pillager, vindicator, evoker (+vex, fangs), illusioner, ravager | ❌ |
| Deep dark / trial | warden, breeze | ❌ |
| Nether | blaze, ghast, magma cube, piglin, piglin brute, zombified piglin, hoglin, zoglin, wither skeleton, strider | ❌ |
| End | enderman, shulker, endermite, ender dragon (fight, crystals, exit portal) | ❌ |
| Bosses | wither (summon, fight), ender dragon | ❌ |
| Non-mob entities | item ✅, falling block ✅, XP orb ✅, lightning ✅; arrow, trident, fireballs, wither skull, snowball, egg, pearl, eye, potion, firework, fishing bobber, llama spit, shulker bullet, wind charges, evoker fangs, TNT, end crystal, minecarts ×6, boats, painting, item frames, leash knot, armour stand, area-effect cloud, display/interaction/marker entities | 🟡 (4 of 39) |

Row tally for §5 counts each named mob once: 4 🟡 (cow, pig, sheep, chicken), 78 ❌.

---

## 6. Systems

### Survival mechanics

| System | Status | Notes |
|---|---|---|
| Movement (walk, sprint, sneak edge-clamp, jump, swim, fly, step, climb) | ✅ | No crawling (1-block gaps), no elytra gliding, no swim-sprint in 1-high tunnels check |
| Fall / void / lava / drowning damage, starvation | ✅ | |
| Other damage sources (fire, cactus, berry bush, magma, suffocation, freezing, lightning, explosions, anvil landing) | ❌ | |
| Hunger, saturation, exhaustion, natural regen | ✅ | `naturalRegeneration` rule not honoured; difficulty does not change starvation limits |
| Experience (orbs, levels, curve) | 🟡 | Ores and mobs drop XP; furnace XP never paid out, no breeding XP, no HUD display, no XP use (no enchanting/anvil levels effectively unspendable beyond anvil) |
| Status effects (all 39) | ❌ | No effect system |
| Enchantments (all 42), enchanting, books, anvil combining | ❌ | Loot conditions for Silk Touch/Fortune evaluate as "no enchantment" |
| Armour / toughness / damage reduction | ❌ | |
| Knockback, invulnerability frames, critical hits | 🟡 | Mob hurt + basic knockback; no player knockback/crits |
| Item durability & breaking | ✅ | Unbreaking/Mending absent with enchantments |
| Respawn, keepInventory, spawn point (bed) | ✅ | No respawn anchor; `doImmediateRespawn` stored but not honoured |

### World & environment

| System | Status | Notes |
|---|---|---|
| Day/night cycle, `/time`, sleeping | ✅ | |
| Moon phases | 🟡 | Moon texture sheet bound, phase not driven by day count (carried over) |
| Weather: rain, thunder, lightning, snow by biome, sky dimming | ✅ | No snow/ice accumulation, no cauldron filling, no fire extinguishing |
| Random ticks (`randomTickSpeed`) | ✅ | Only 6-chunk radius around player |
| Scheduled block ticks | ✅ | Fluids, gravity, leaves, scaffolding, portal, support |
| Light engine (sky + block) | ✅ | |
| Dimensions: Overworld | ✅ | |
| Dimensions: Nether (terrain, biomes, features, portal travel) | ✅ | No Nether structures (fortress, bastion), no Nether mobs |
| Dimensions: End (terrain, islands, spikes, gateways) | 🟡 | Generates, but unreachable (no End portal); no dragon fight, no exit portal |
| Structures (34) | ❌ | See §7 |
| Particles | 🟡 | Block break/hit, rain splash only; no torch flame/smoke, portal, crit, potion, villager, campfire, dripping… |
| Sounds & music | ✅ | Player's own assets; no subtitles, no jukebox/note block |
| World border | ❌ | |
| Difficulty | 🟡 | Stored and switchable; no gameplay effect (no hostiles, no starvation/damage scaling, no regional difficulty); no hardcore |

### Progression & data

| System | Status | Notes |
|---|---|---|
| Recipes: shaped, shapeless, smelting/blasting/smoking, stonecutting | ✅ | 1230 of 1291 recipe files |
| Recipes: campfire_cooking | 🟡 | Loaded, no campfire block entity |
| Recipes: smithing_transform | ✅ | 9 netherite upgrades |
| Recipes: smithing_trim (18), crafting_special_* (13), crafting_decorated_pot | ❌ | |
| Recipe unlocking / recipe book | ❌ | |
| Loot tables: blocks (982), entities (100) | 🟡 | Supported functions: `set_count`, `limit_count`; missing `apply_bonus` (Fortune), `explosion_decay`, `furnace_smelt`, `set_components`, `copy_components`, `enchant_*`, `looting_enchant`; conditions `entity_properties`, `location_check`, `damage_source_properties` always false |
| Loot tables: chests / archaeology / fishing / gift / barter | ❌ | Not vendored in `data/vanilla/loot_table/` (only blocks, entities, gameplay) — needed for structures |
| Advancements | ❌ | |
| Statistics | ❌ | |
| Data packs | ❌ | |
| Tags | ✅ | Block/item tags loaded from `data/vanilla/tags` |
| Anvil (.mca) import/export | 🟡 | Blocks, biomes, player, inventory, weather, rules; entities and POI not imported |
| Multiplayer, LAN | ❌ | Phase 4 |

### Commands (vanilla 1.21.1 has ~80)

Present (16): `/clear`, `/difficulty`, `/gamemode`, `/gamerule`, `/give`,
`/help`, `/kill`, `/me`, `/say`, `/seed`, `/setworldspawn`, `/spawnpoint`,
`/summon` (4 mobs), `/teleport` / `/tp`, `/time` (set/add/query),
`/weather`, `/xp` (add only).

| Command group | Status | Notes |
|---|---|---|
| `/tp`, `/teleport` | 🟡 | No rotation args, `facing`, relative `^` coords |
| `/give`, `/clear` | 🟡 | No item components / NBT, `/clear` has no item filter or count |
| `/xp` / `/experience` | 🟡 | `add` only; no `set`, `query`; `/experience` alias missing |
| `/summon` | 🟡 | 4 mob types; no NBT |
| `/gamerule` | 🟡 | 9 rules in the list (see below) |
| `/time`, `/weather`, `/difficulty`, `/seed`, `/kill`, `/say`, `/me`, `/help`, `/setworldspawn`, `/spawnpoint`, `/gamemode` | ✅ | |
| Selectors | 🟡 | `@s @p @a @r @e` with `type=` and `limit=` only; no `distance`, `x/y/z`, `dx`, `tag`, `name`, `scores`, `sort`, `nbt`, `gamemode`, `level` |
| World editing: `/setblock`, `/fill`, `/clone`, `/fillbiome`, `/place` | ❌ | High value for testing parity |
| Entities: `/effect`, `/enchant`, `/attribute`, `/damage`, `/ride`, `/tag`, `/data`, `/item`, `/loot`, `/spreadplayers`, `/spectate` | ❌ | |
| Info: `/locate` (structure/biome/poi), `/list`, `/random`, `/tick`, `/debug`, `/perf`, `/jfr` | ❌ | `/locate biome` is cheap to add and useful for parity captures |
| Scripting: `/execute`, `/function`, `/schedule`, `/return`, `/scoreboard`, `/team`, `/teammsg`, `/trigger`, `/bossbar`, `/title`, `/tellraw`, `/msg` `/tell` `/w`, `/particle`, `/playsound`, `/stopsound`, `/recipe`, `/advancement`, `/datapack`, `/reload`, `/forceload`, `/worldborder`, `/defaultgamemode` | ❌ | |
| Server: `/ban`, `/ban-ip`, `/banlist`, `/kick`, `/op`, `/deop`, `/pardon`, `/pardon-ip`, `/whitelist`, `/save-all`, `/save-on`, `/save-off`, `/stop`, `/publish`, `/setidletimeout`, `/transfer` | ❌ | Phase 4 (multiplayer) |

### Game rules (51 in 1.21.1)

| Rule set | Status |
|---|---|
| Honoured: `doDaylightCycle`, `doWeatherCycle`, `keepInventory`, `doMobLoot`, `doTileDrops`, `randomTickSpeed` | ✅ |
| Stored but not honoured: `doMobSpawning`, `naturalRegeneration`, `doImmediateRespawn`, `mobGriefing`, `doFireTick` | 🟡 |
| Missing: all others (`doEntityDrops`, `doInsomnia`, `doLimitedCrafting`, `doPatrolSpawning`, `doTraderSpawning`, `doVinesSpread`, `doWardenSpawning`, `drowningDamage`, `fallDamage`, `fireDamage`, `freezeDamage`, `playersSleepingPercentage`, `showDeathMessages`, `spawnRadius`, `snowAccumulationHeight`, `waterSourceConversion`, `lavaSourceConversion`, `maxEntityCramming`, `announceAdvancements`, `sendCommandFeedback`, `reducedDebugInfo`, `commandBlockOutput`, explosion-decay rules, portal-delay rules, …) | ❌ |

---

## 7. World generation

Terrain, biomes, carvers, aquifers and ore veins are seed-exact (parity
harness, 34 reference chunks across Overworld/Nether/End). Feature coverage
below compares `crates/rc_worldgen/src/feature/*` with the 56 feature types used
by the 196 files in `data/vanilla/worldgen/configured_feature/`.

### Configured feature types

| Feature types | Status | Notes |
|---|---|---|
| `tree` (31) with all trunk placers (straight, forking, giant, mega jungle, dark oak, fancy, bending, upwards branching, cherry), foliage placers (11) and decorators (beehive, leave/trunk vine, cocoa, alter ground, attached to leaves) | ✅ | |
| `ore`, `scattered_ore` | ✅ | |
| `random_patch`, `flower`, `simple_block`, `block_column`, `random_selector`, `simple_random_selector`, `random_boolean_selector` | ✅ | |
| `disk`, `lake`, `spring_feature`, `freeze_top_layer`, `forest_rock`, `ice_spike`, `blue_ice`, `iceberg`, `desert_well`, `huge_red/brown_mushroom` | ✅ | |
| `monster_room` | 🟡 | Room, spawner and chests placed; chests empty (no chest loot tables), spawner inert |
| `fossil` | ✅ | |
| Aquatic: `seagrass`, `kelp`, `sea_pickle`, `coral_tree/claw/mushroom`, `underwater_magma` | ✅ | |
| Caves: `geode`, `pointed_dripstone`, `dripstone_cluster`, `large_dripstone`, `vegetation_patch`, `waterlogged_vegetation_patch`, `root_system`, `multiface_growth`, `vines`, `bamboo`, `sculk_patch` | ✅ | |
| Nether: `huge_fungus`, `nether_forest_vegetation`, `twisting_vines`, `weeping_vines`, `basalt_columns`, `basalt_pillar`, `delta_feature`, `netherrack_replace_blobs`, `glowstone_blob` | ✅ | Roadmap still lists Nether/End features as carried over — update the roadmap |
| End: `end_island`, `chorus_plant`, `end_spike`, `end_gateway` | 🟡 | Spikes without end crystals (no entity), gateways without teleport |
| `block_pile` (hay/melon/pumpkin/snow/ice piles) | ❌ | No-op; used by village structures |
| `bonus_chest`, `void_start_platform`, `end_platform` | ❌ | No-op (Bonus Chest option missing; End platform needed once End travel exists) |
| Decoration order quirks between neighbours | 🟡 | 99.7–99.9 % block match (ADR-016) |

### Structures (`data/vanilla/worldgen/structure`, 34)

No structure placement, jigsaw assembly, template-pool or processor-list code
exists yet (`structure`, `structure_set`, `template_pool`, `processor_list` data
is vendored but unused).

| Structure | Status |
|---|---|
| Villages (plains, desert, savanna, snowy, taiga) | ❌ |
| Pillager outpost, woodland mansion | ❌ |
| Desert pyramid, jungle pyramid, swamp hut, igloo | ❌ |
| Mineshaft, mineshaft (mesa) | ❌ |
| Stronghold | ❌ |
| Ocean monument, shipwreck, shipwreck (beached), ocean ruins (cold, warm), buried treasure | ❌ |
| Ruined portals (7 variants) | ❌ |
| Ancient city, trail ruins, trial chambers | ❌ |
| Nether fortress, bastion remnant, nether fossil | ❌ |
| End city (+ ship, elytra) | ❌ |

---

## 8. Known deviations noted in code

| Location | Note |
|---|---|
| `crates/rustcraft/src/stations.rs:8` | Renaming, enchantment combining and armour trims wait for item components / enchantments (Phase 3) |
| `crates/rustcraft/src/containers.rs:517` | Anvil rename field disabled until item names exist |
| `crates/rustcraft/src/items.rs:95` | Redstone and Spawn Eggs creative tabs disabled "until v0.3" |
| `crates/rustcraft/src/screens.rs:285,287` | Resource Packs (v0.3) and Language buttons are disabled stubs |
| `crates/rustcraft/src/screens.rs:391-405` | Pause menu Advancements / Statistics / Report Bugs / Open to LAN disabled |
| `crates/rustcraft/src/screens.rs:664,671` | Multiplayer (v0.4), Mods (v0.5) disabled |
| `crates/rustcraft/src/screens.rs` (`hud`) | "XP bar (empty in this version)" — level/progress never drawn |
| `crates/rustcraft/src/blocklogic.rs` (`tree_blocks`) | Sapling growth uses simplified log+leaf shapes rather than the worldgen tree features (which exist in `rc_worldgen`) |
| `crates/rustcraft/src/game.rs` (`interact`) | Creative breaking repeats every 5 ticks while held (vanilla-like); eating is a fixed 32 ticks for every food |
| `crates/rustcraft/src/game.rs` (`break_block`) | Ore XP is a flat 0–3 for all non-iron/gold/copper ores (vanilla: coal 0–2, diamond/emerald 3–7, lapis/quartz 2–5, redstone 1–5, nether gold 0–1) |
| `crates/rustcraft/src/loot.rs:83,143` | `survives_explosion`/`killed_by_player` always true; entity/location/damage-source predicates always false |
| `crates/rustcraft/src/world.rs:444` | Biome tint falls back to an approximation when colormaps are absent |
| `crates/rc_assets/src/model.rs:995` | Chest model approximated as a single 14³ box (no lid, no double-chest model) |
| `crates/rc_worldgen/src/feature/basic.rs:80` | `bonus_chest`, `void_start_platform`, `end_platform`, `block_pile` are no-ops until structures land |
| `crates/rustcraft/src/mobs.rs:595` | Mobs spawn only once, at chunk generation (no ongoing spawn cycle or despawn) |
| `crates/rustcraft/src/game.rs` (`random_ticks`) | Random ticks run in a fixed 6-chunk radius instead of the simulation distance |
| `crates/rustcraft/src/game.rs` (`block_ticks`) | Generic "unsupported plant pops off" rule driven by name heuristics instead of per-block survival rules |
| `docs/ROADMAP.md` | Phase 2 list says "chest / large chest" containers, but double chests are not implemented |
