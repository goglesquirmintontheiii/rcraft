# 0.2.1 screenshots

Rendered headlessly on an RTX 3060 (Vulkan) with RustCraft's built-in
procedural textures (ADR-017), with `--assets` pointed at a missing path so
the player's own install is not used:

```sh
rustcraft --game-dir <dir> --assets /nonexistent --screenshot nether.png \
          --scene nether --camera "0,surface,0,0,15"
RC_STATION=anvil rustcraft --game-dir <dir> --assets /nonexistent \
          --screenshot anvil.png --scene station
```

| File | Scene |
|---|---|
| `nether.png` | through a nether portal: the arrival portal built on solid ground in the Nether |
| `anvil.png` | anvil repairing an iron pickaxe with iron ingots (level cost shown) |
| `stonecutter.png` | stonecutter with stone, the recipe picker and a selected result |

Other headless scenes added in 0.2.1: `station` (with `RC_STATION` =
`anvil`, `grindstone`, `stonecutter` or `smithing_table`), `creative` with
`RC_TAB` (`search`, `inventory` or a tab index), and `bench` (performance,
see `docs/perf/README.md`).
