#!/usr/bin/env python3
"""Regenerate data/vanilla/ from Mojang's official data-generator output.

Sources (both are *generated data*, not game code or art):
  - misode/mcmeta tags 1.21.1-data-json and 1.21.1-summary: output of the
    vanilla data generator (`java -cp server.jar net.minecraft.data.Main --all`)
  - PrismarineJS minecraft-data (MIT): block hardness/light/shape tables
    extracted from the game, and the global block-state id layout

Usage: tools/import_vanilla_data.py <mcmeta-data-json> <mcmeta-summary> <minecraft-data/data/pc>
See docs/CLEAN_ROOM.md for why this is in-bounds.
"""
import json, os, shutil, sys

data_json, summary, mcd = sys.argv[1:4]
out = os.path.join(os.path.dirname(__file__), "..", "data", "vanilla")
shutil.rmtree(out, ignore_errors=True)
os.makedirs(out)

def copy_tree(rel, dest):
    src = os.path.join(data_json, "data", "minecraft", rel)
    for root, _, files in os.walk(src):
        for f in files:
            if not f.endswith(".json"):
                continue
            p = os.path.join(root, f)
            r = os.path.relpath(p, src)
            d = os.path.join(out, dest, r)
            os.makedirs(os.path.dirname(d), exist_ok=True)
            with open(p) as fi:
                obj = json.load(fi)
            with open(d, "w") as fo:
                json.dump(obj, fo, separators=(",", ":"), sort_keys=False)

for rel in ["worldgen/biome", "worldgen/configured_carver", "worldgen/configured_feature",
            "worldgen/placed_feature", "worldgen/density_function", "worldgen/noise",
            "worldgen/noise_settings", "worldgen/structure", "worldgen/structure_set",
            "worldgen/template_pool", "worldgen/processor_list",
            "dimension", "dimension_type", "tags/block", "tags/fluid", "tags/item",
            "tags/worldgen/biome", "tags/entity_type", "recipe", "loot_table/blocks",
            "loot_table/entities", "loot_table/gameplay"]:
    copy_tree(rel, rel)

# ---- blocks -------------------------------------------------------------
blocks = json.load(open(os.path.join(mcd, "1.21.1", "blocks.json")))
items = json.load(open(os.path.join(mcd, "1.21.1", "items.json")))
item_names = {i["id"]: i["name"] for i in items}
shapes = json.load(open(os.path.join(mcd, "1.20.5", "blockCollisionShapes.json")))
summ = json.load(open(os.path.join(summary, "blocks", "data.min.json")))

out_blocks = []
for b in sorted(blocks, key=lambda b: b["id"]):
    name = b["name"]
    props = []
    for s in b["states"]:
        if s["type"] == "bool":
            vals = ["true", "false"]
        elif s["type"] == "int":
            vals = [str(v) for v in s["values"]]
        else:
            vals = s["values"]
        props.append([s["name"], vals])
    # Cross-check with the data generator's property definitions.
    sp, sdef = summ[name]
    assert sorted(sp.keys()) == sorted(p[0] for p in props), name
    for pn, vals in props:
        assert sp[pn] == vals, (name, pn, sp[pn], vals)
    nstates = 1
    for _, v in props:
        nstates *= len(v)
    assert nstates == b["maxStateId"] - b["minStateId"] + 1, name
    sh = shapes["blocks"].get(name, 1 if b["boundingBox"] == "block" else 0)
    if isinstance(sh, int):
        sh = [sh]
    out_blocks.append({
        "name": name,
        "props": props,
        "default": b["defaultState"] - b["minStateId"],
        "first_id": b["minStateId"],
        "hardness": b["hardness"] if b["hardness"] is not None else -1.0,
        "resistance": b["resistance"],
        "light": b["emitLight"],
        "filter_light": b["filterLight"],
        "transparent": b["transparent"],
        "tool": b.get("material", "default"),
        "harvest_tools": sorted(item_names[int(k)] for k in (b.get("harvestTools") or {})),
        "shapes": sh,
    })
with open(os.path.join(out, "blocks.json"), "w") as f:
    json.dump({"blocks": out_blocks, "shapes": shapes["shapes"]}, f, separators=(",", ":"))

# Item components from the data generator: durability, food, tool rules.
comps = json.load(open(os.path.join(summary, "item_components", "data.min.json")))
def item_entry(i):
    c = comps.get("minecraft:" + i["name"]) or comps.get(i["name"]) or {}
    e = {"name": i["name"], "stack": i["stackSize"]}
    if "minecraft:max_damage" in c:
        e["durability"] = c["minecraft:max_damage"]
    if "minecraft:food" in c:
        f = c["minecraft:food"]
        e["food"] = [f.get("nutrition", 0), f.get("saturation", 0.0)]
    if "minecraft:tool" in c:
        e["tool"] = [[r["blocks"] if isinstance(r["blocks"], str) else ",".join(r["blocks"]), r.get("speed"), r.get("correct_for_drops")] for r in c["minecraft:tool"]["rules"]]
    for m in c.get("minecraft:attribute_modifiers", {}).get("modifiers", []):
        if m["type"].endswith("attack_damage"):
            e["attack"] = m["amount"]
    return e
with open(os.path.join(out, "items.json"), "w") as f:
    json.dump([item_entry(i) for i in sorted(items, key=lambda i: i["id"])], f, separators=(",", ":"))

ents = json.load(open(os.path.join(mcd, "1.20.5", "entities.json")))
with open(os.path.join(out, "entities.json"), "w") as f:
    json.dump([{"name": e["name"], "width": e["width"], "height": e["height"], "kind": e["type"]}
               for e in ents], f, separators=(",", ":"))

tints = json.load(open(os.path.join(mcd, "1.21.1", "tints.json")))
with open(os.path.join(out, "tints.json"), "w") as f:
    json.dump(tints, f, separators=(",", ":"))

with open(os.path.join(out, "SOURCE.md"), "w") as f:
    f.write("Generated by tools/import_vanilla_data.py from the Minecraft 1.21.1 data "
            "generator output (misode/mcmeta 1.21.1-data-json, 1.21.1-summary) and "
            "PrismarineJS minecraft-data 3.119.0. Do not edit by hand.\n")
print("blocks", len(out_blocks), "states", sum(1 for _ in out_blocks))
