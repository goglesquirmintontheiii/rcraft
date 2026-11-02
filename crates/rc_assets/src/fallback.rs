//! Procedural fallback pack (CC0, generated at runtime).
//!
//! Used when no Minecraft install is found so RustCraft is always playable.
//! Every texture is generated from a small colour table and value noise;
//! shapes come from the block collision boxes in the vanilla data tables.

use ahash::AHashMap;
use rc_data::Blocks;

use crate::atlas::{self, Image};
use crate::model::{BakedModel, BakedQuad, BlockModels, Dir, Layer, StateModel, Tint, tint_for};

fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

fn name_hash(s: &str) -> u32 {
    s.bytes().fold(2_166_136_261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16_777_619)
    })
}

/// Base colour and style for a block name.
fn style(name: &str) -> ([u8; 3], Style) {
    let has = |k: &str| name.contains(k);
    let rgb = |c: u32| [(c >> 16) as u8, (c >> 8) as u8, c as u8];
    if has("ore") {
        let speck = if has("coal") {
            0x2a2a2a
        } else if has("iron") {
            0xd8af93
        } else if has("gold") {
            0xfcee4b
        } else if has("diamond") {
            0x5decf5
        } else if has("emerald") {
            0x17dd62
        } else if has("redstone") {
            0xff0000
        } else if has("lapis") {
            0x1f4fc9
        } else if has("copper") {
            0xe0734d
        } else {
            0xeeeeee
        };
        let base = if has("deepslate") {
            0x4d4d50
        } else if has("nether") {
            0x723232
        } else {
            0x7f7f7f
        };
        return (rgb(base), Style::Specks(rgb(speck)));
    }
    let (c, st) = if has("leaves") {
        (0x9a9a9a, Style::Holes)
    } else if name == "grass_block" {
        (0x9a9a9a, Style::Noise)
    } else if has("grass") || has("fern") {
        (0x9a9a9a, Style::Cross)
    } else if has("water") {
        (0x3f76e4, Style::Noise)
    } else if has("lava") {
        (0xd96415, Style::Noise)
    } else if has("log") || has("wood") || has("stem") && !has("mushroom") {
        (0x6b5130, Style::Stripes)
    } else if has("planks") {
        (0xa2824e, Style::Stripes)
    } else if has("deepslate") {
        (0x4d4d50, Style::Noise)
    } else if has("cobble") {
        (0x6f6f6f, Style::Specks(rgb(0x505050)))
    } else if has("stone") && !has("sandstone") && !has("glowstone") && !has("blackstone") {
        (0x7f7f7f, Style::Noise)
    } else if has("dirt") || has("farmland") || has("podzol") || has("mud") {
        (0x8b5a2b, Style::Noise)
    } else if has("red_sand") {
        (0xbe6621, Style::Noise)
    } else if has("sandstone") || has("sand") {
        (0xdbcf8e, Style::Noise)
    } else if has("gravel") {
        (0x857f7e, Style::Specks(rgb(0x5c5856)))
    } else if has("snow") || has("powder") {
        (0xf5fbfb, Style::Noise)
    } else if has("ice") {
        (0x91b4fe, Style::Translucent)
    } else if has("glass") {
        (0xd0e8f0, Style::Translucent)
    } else if has("clay") {
        (0xa0a6b3, Style::Noise)
    } else if has("terracotta") {
        (0x985e43, Style::Noise)
    } else if has("granite") {
        (0x956755, Style::Specks(rgb(0xb07a66)))
    } else if has("diorite") {
        (0xbcbcbc, Style::Specks(rgb(0x8f8f8f)))
    } else if has("andesite") {
        (0x888888, Style::Specks(rgb(0x6e6e6e)))
    } else if has("tuff") {
        (0x6c6d66, Style::Noise)
    } else if has("calcite") {
        (0xdfe0dc, Style::Noise)
    } else if has("bedrock") {
        (0x555555, Style::Specks(rgb(0x222222)))
    } else if has("obsidian") {
        (0x14121e, Style::Noise)
    } else if has("netherrack") {
        (0x723232, Style::Noise)
    } else if has("basalt") || has("blackstone") {
        (0x37343b, Style::Noise)
    } else if has("end_stone") {
        (0xdbde9e, Style::Noise)
    } else if has("moss") {
        (0x596e2d, Style::Noise)
    } else if has("dripstone") {
        (0x866b5c, Style::Noise)
    } else if has("amethyst") {
        (0x8561c3, Style::Noise)
    } else if has("kelp") || has("seagrass") || has("vine") || has("sapling") || has("bush") {
        (0x5a8c2e, Style::Cross)
    } else if has("mushroom") {
        (0xb5463a, Style::Noise)
    } else {
        let h = name_hash(name);
        (h & 0x00ff_ffff | 0x0040_4040, Style::Noise)
    };
    (rgb(c), st)
}

#[derive(Clone, Copy)]
enum Style {
    Noise,
    Specks([u8; 3]),
    Stripes,
    Holes,
    Cross,
    Translucent,
}

fn texture(name: &str, flower: Option<[u8; 3]>) -> Image {
    let (base, st) = style(name);
    let seed = name_hash(name);
    let mut img = Image::new(16, 16);
    for y in 0..16u32 {
        for x in 0..16u32 {
            let n = (hash(seed ^ (y * 16 + x)) & 0xff) as i32;
            let v = (n - 128) / 6;
            let shade = |c: u8, d: i32| (i32::from(c) + d).clamp(0, 255) as u8;
            let mut c = [shade(base[0], v), shade(base[1], v), shade(base[2], v), 255];
            match st {
                Style::Noise => {}
                Style::Specks(s) => {
                    if n > 215 {
                        c = [s[0], s[1], s[2], 255];
                    }
                }
                Style::Stripes => {
                    let d = if x % 4 == 0 { -18 } else { 0 };
                    c = [shade(c[0], d), shade(c[1], d), shade(c[2], d), 255];
                }
                Style::Holes => {
                    if n < 40 {
                        c[3] = 0;
                    }
                }
                Style::Translucent => c[3] = 160,
                Style::Cross => {
                    // A few blades / a stem.
                    let blade = (x == 7
                        || x == 8
                        || (x + y) % 5 == 0 && y > 4
                        || (x * 3 + y) % 7 == 0 && y > 7)
                        && y > 2;
                    c[3] = if blade { 255 } else { 0 };
                    if let Some(f) = flower
                        && y < 7
                        && (5..11).contains(&x)
                        && (2..7).contains(&y)
                    {
                        c = [f[0], f[1], f[2], 255];
                    }
                }
            }
            img.put(x, y, c);
        }
    }
    img
}

fn is_plant(name: &str) -> bool {
    [
        "grass",
        "fern",
        "flower",
        "sapling",
        "tulip",
        "poppy",
        "dandelion",
        "orchid",
        "allium",
        "bluet",
        "daisy",
        "cornflower",
        "lily_of",
        "rose",
        "kelp",
        "seagrass",
        "mushroom",
        "bush",
        "roots",
        "vine",
        "sprouts",
        "fungus",
        "wart",
        "cane",
        "lilac",
        "peony",
        "sunflower",
        "berry",
        "dripleaf",
        "spore",
        "azalea",
        "pitcher",
        "torchflower",
        "wheat",
        "carrots",
        "potatoes",
        "beetroots",
        "cobweb",
        "torch",
        "lichen",
        "petals",
        "hanging",
    ]
    .iter()
    .any(|k| name.contains(k))
}

fn flower_colour(name: &str) -> Option<[u8; 3]> {
    let c = match () {
        () if name.contains("poppy") || name.contains("rose") || name.contains("red_tulip") => {
            [200, 30, 30]
        }
        () if name.contains("dandelion") || name.contains("sunflower") => [250, 220, 40],
        () if name.contains("orchid") || name.contains("cornflower") => [70, 110, 230],
        () if name.contains("allium") || name.contains("lilac") => [180, 100, 220],
        () if name.contains("bluet") || name.contains("daisy") || name.contains("white") => {
            [240, 240, 240]
        }
        () if name.contains("tulip") || name.contains("peony") => [240, 140, 170],
        _ => return None,
    };
    Some(c)
}

fn cuboid(sprite: crate::atlas::Sprite, f: [f32; 3], t: [f32; 3], tint: i8) -> Vec<BakedQuad> {
    let ([x0, y0, z0], [x1, y1, z1]) = (f, t);
    let faces: [(Dir, [[f32; 3]; 4]); 6] = [
        (
            Dir::North,
            [[x1, y1, z0], [x1, y0, z0], [x0, y0, z0], [x0, y1, z0]],
        ),
        (
            Dir::South,
            [[x0, y1, z1], [x0, y0, z1], [x1, y0, z1], [x1, y1, z1]],
        ),
        (
            Dir::West,
            [[x0, y1, z0], [x0, y0, z0], [x0, y0, z1], [x0, y1, z1]],
        ),
        (
            Dir::East,
            [[x1, y1, z1], [x1, y0, z1], [x1, y0, z0], [x1, y1, z0]],
        ),
        (
            Dir::Up,
            [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
        ),
        (
            Dir::Down,
            [[x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1]],
        ),
    ];
    let full = |d: Dir| match d {
        Dir::Down => y0 <= 0.0,
        Dir::Up => y1 >= 1.0,
        Dir::North => z0 <= 0.0,
        Dir::South => z1 >= 1.0,
        Dir::West => x0 <= 0.0,
        Dir::East => x1 >= 1.0,
    };
    faces
        .iter()
        .map(|&(d, pos)| BakedQuad {
            pos,
            uv: [
                sprite.map(0.0, 0.0),
                sprite.map(0.0, 16.0),
                sprite.map(16.0, 16.0),
                sprite.map(16.0, 0.0),
            ],
            dir: d,
            cull: full(d).then_some(d),
            tint,
            shade: true,
        })
        .collect()
}

fn cross(sprite: crate::atlas::Sprite, tint: i8) -> Vec<BakedQuad> {
    let uv = [
        sprite.map(0.0, 0.0),
        sprite.map(0.0, 16.0),
        sprite.map(16.0, 16.0),
        sprite.map(16.0, 0.0),
    ];
    let (a, b) = (0.15, 0.85);
    let planes = [
        [[a, 1.0, a], [a, 0.0, a], [b, 0.0, b], [b, 1.0, b]],
        [[b, 1.0, b], [b, 0.0, b], [a, 0.0, a], [a, 1.0, a]],
        [[a, 1.0, b], [a, 0.0, b], [b, 0.0, a], [b, 1.0, a]],
        [[b, 1.0, a], [b, 0.0, a], [a, 0.0, b], [a, 1.0, b]],
    ];
    planes
        .iter()
        .map(|&pos| BakedQuad {
            pos,
            uv,
            dir: Dir::Up,
            cull: None,
            tint,
            shade: false,
        })
        .collect()
}

/// Build the fallback block models.
#[must_use]
pub fn block_models(blocks: &Blocks, mip_levels: u32) -> BlockModels {
    let mut tex: Vec<(String, Image, Option<atlas::Animation>)> = Vec::new();
    for def in &blocks.defs {
        tex.push((
            def.name.clone(),
            texture(&def.name, flower_colour(&def.name)),
            None,
        ));
    }
    tex.push(("water_still".into(), texture("water", None), None));
    tex.push(("lava_still".into(), texture("lava", None), None));
    for i in 0..10u32 {
        let mut img = Image::new(16, 16);
        for y in 0..16 {
            for x in 0..16 {
                let n = hash(x * 131 + y * 7 + 99) % 10;
                if n < i {
                    img.put(x, y, [20, 20, 20, 170]);
                }
            }
        }
        tex.push((format!("destroy_{i}"), img, None));
    }
    let atlas = atlas::build(tex, mip_levels);
    let mut models = Vec::new();
    let mut states = vec![StateModel::default(); blocks.len()];
    let mut layers = vec![Layer::Invisible; blocks.len()];
    let mut tints = vec![Tint::None; blocks.len()];
    let mut particle = vec![atlas.missing; blocks.len()];
    let mut shape_models: AHashMap<(u16, u16), u32> = AHashMap::new();
    for (bi, def) in blocks.defs.iter().enumerate() {
        let sprite = atlas.sprite(&def.name);
        let tint = tint_for(&def.name);
        let ti = if tint == Tint::None { -1 } else { 0 };
        let (_, st) = style(&def.name);
        let name = def.name.as_str();
        let invisible =
            name.ends_with("air") || name == "water" || name == "lava" || name == "bubble_column";
        for s in def.first..def.first + def.count {
            let i = s as usize;
            tints[i] = tint;
            particle[i] = sprite;
            if invisible {
                continue;
            }
            let info = &blocks.states[i];
            let key = (bi as u16, info.shape);
            let m = *shape_models.entry(key).or_insert_with(|| {
                let boxes = &blocks.shapes[info.shape as usize];
                let quads = if boxes.is_empty() || is_plant(name) {
                    cross(sprite, ti)
                } else {
                    boxes
                        .iter()
                        .flat_map(|b| {
                            cuboid(
                                sprite,
                                [b[0] as f32, b[1] as f32, b[2] as f32],
                                [b[3] as f32, b[4] as f32, b[5] as f32],
                                ti,
                            )
                        })
                        .collect()
                };
                models.push(BakedModel { quads, ao: true });
                models.len() as u32 - 1
            });
            states[i] = StateModel {
                parts: vec![vec![(m, 1)]],
            };
            layers[i] = match st {
                Style::Translucent => Layer::Translucent,
                Style::Holes | Style::Cross => Layer::Cutout,
                _ if blocks.shapes[info.shape as usize].is_empty() || is_plant(name) => {
                    Layer::Cutout
                }
                _ => Layer::Solid,
            };
        }
    }
    BlockModels::from_parts(atlas, models, states, layers, tints, particle)
}
