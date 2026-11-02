//! Procedural stand-in textures used when no Minecraft install provides
//! the real ones. Generated here (no external art), so they are safe to
//! ship and to appear in repository screenshots.

use rc_assets::Image;

fn noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut h = x.wrapping_mul(374_761_393)
        ^ y.wrapping_mul(668_265_263)
        ^ seed.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h ^ (h >> 16)) & 255) as f32 / 255.0
}

fn px(c: [f32; 3], n: f32) -> [u8; 4] {
    let k = 0.9 + n * 0.2;
    [
        (c[0] * k * 255.0).clamp(0.0, 255.0) as u8,
        (c[1] * k * 255.0).clamp(0.0, 255.0) as u8,
        (c[2] * k * 255.0).clamp(0.0, 255.0) as u8,
        255,
    ]
}

fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Image {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            out.extend_from_slice(&f(x, y));
        }
    }
    Image { w, h, px: out }
}

/// A 64×32 mob texture: `base` colour with `spots` patches.
#[must_use]
pub fn mob(base: [f32; 3], spots: [f32; 3]) -> Image {
    image(64, 32, |x, y| {
        let blob = noise(x / 3, y / 3, 7) > 0.72;
        px(if blob { spots } else { base }, noise(x, y, 1))
    })
}

/// A 64×64 player skin in the standard layout: skin-tone head/arms ends,
/// teal shirt, blue trousers, dark hair.
#[must_use]
pub fn skin() -> Image {
    let skin = [0.78, 0.58, 0.45];
    let hair = [0.25, 0.17, 0.1];
    let shirt = [0.0, 0.6, 0.6];
    let pants = [0.25, 0.25, 0.6];
    image(64, 64, |x, y| {
        let n = noise(x, y, 3);
        // Outer layers (hat, jacket, sleeves, trouser overlays) transparent.
        let outer =
            (y < 16 && x >= 32) || (32..48).contains(&y) || (y >= 48 && !(16..48).contains(&x));
        if outer {
            return [0, 0, 0, 0];
        }
        let c = if y < 16 {
            // Head: hair on top and the upper band of each side.
            let top = y < 8;
            let face_band = (8..16).contains(&y) && (8..16).contains(&x);
            if top || (y < 10 && !face_band) || (y < 9) {
                hair
            } else if face_band && (y == 12) && (x == 9 || x == 10 || x == 13 || x == 14) {
                if x == 10 || x == 13 {
                    [0.3, 0.2, 0.6]
                } else {
                    [0.95, 0.95, 0.95]
                }
            } else {
                skin
            }
        } else if y < 32 {
            if x < 16 {
                pants
            } else if x < 40 || y < 26 {
                shirt
            } else {
                skin
            }
        } else if x < 32 {
            pants
        } else if y < 58 {
            shirt
        } else {
            skin
        };
        px(c, n)
    })
}

/// 64×64 sheet of 4×4 glowing orbs (XP).
#[must_use]
pub fn orbs() -> Image {
    image(64, 64, |x, y| {
        let (cx, cy) = ((x % 16) as f32 - 7.5, (y % 16) as f32 - 7.5);
        let size = 2.0 + ((y / 16) * 4 + x / 16) as f32 * 0.4;
        let d = (cx * cx + cy * cy).sqrt();
        if d < size {
            let k = 1.0 - d / size * 0.5;
            [(200.0 * k) as u8, 255, (60.0 * k) as u8, 255]
        } else {
            [0, 0, 0, 0]
        }
    })
}

/// 64×256 rain streaks.
#[must_use]
pub fn rain() -> Image {
    image(64, 256, |x, y| {
        let col = x / 2;
        let len = (noise(col, 0, 11) * 24.0) as u32 + 8;
        let start = (noise(col, 1, 11) * 256.0) as u32;
        let on = x % 2 == 0 && noise(col, 2, 11) > 0.45 && (y + 256 - start) % 256 < len;
        if on {
            [170, 190, 255, 200]
        } else {
            [0, 0, 0, 0]
        }
    })
}

/// 64×256 snowflakes.
#[must_use]
pub fn snow() -> Image {
    image(64, 256, |x, y| {
        if noise(x / 2, y / 2, 13) > 0.97 {
            [255, 255, 255, 230]
        } else {
            [0, 0, 0, 0]
        }
    })
}

/// 1×1 white.
#[must_use]
pub fn white() -> Image {
    Image {
        w: 1,
        h: 1,
        px: vec![255; 4],
    }
}

/// Material colour guessed from an item name.
fn material(name: &str) -> [f32; 3] {
    const M: [(&str, [f32; 3]); 22] = [
        ("netherite", [0.3, 0.27, 0.28]),
        ("diamond", [0.4, 0.9, 0.9]),
        ("emerald", [0.2, 0.8, 0.35]),
        ("gold", [0.95, 0.8, 0.2]),
        ("iron", [0.85, 0.85, 0.85]),
        ("copper", [0.85, 0.5, 0.3]),
        ("stone", [0.55, 0.55, 0.55]),
        ("wooden", [0.6, 0.45, 0.25]),
        ("stick", [0.55, 0.4, 0.2]),
        ("coal", [0.15, 0.15, 0.15]),
        ("redstone", [0.8, 0.1, 0.1]),
        ("lapis", [0.2, 0.3, 0.8]),
        ("bread", [0.8, 0.6, 0.3]),
        ("apple", [0.85, 0.15, 0.15]),
        ("beef", [0.75, 0.3, 0.3]),
        ("pork", [0.9, 0.6, 0.6]),
        ("chicken", [0.95, 0.8, 0.7]),
        ("mutton", [0.8, 0.35, 0.35]),
        ("seeds", [0.4, 0.7, 0.3]),
        ("wheat", [0.85, 0.75, 0.35]),
        ("leather", [0.6, 0.35, 0.2]),
        ("bone", [0.9, 0.9, 0.85]),
    ];
    if let Some((_, c)) = M.iter().find(|(k, _)| name.contains(k)) {
        return *c;
    }
    let h = name.bytes().fold(2_166_136_261u32, |a, b| {
        (a ^ u32::from(b)).wrapping_mul(16_777_619)
    });
    [
        0.35 + (h & 255) as f32 / 400.0,
        0.35 + ((h >> 8) & 255) as f32 / 400.0,
        0.35 + ((h >> 16) & 255) as f32 / 400.0,
    ]
}

/// A generated 16×16 item icon: a tool silhouette for tools, otherwise a
/// rounded gem shape, tinted by material.
#[must_use]
pub fn item_icon(name: &str) -> Image {
    let c = material(name);
    let handle = [0.5, 0.36, 0.18];
    let tool = ["pickaxe", "axe", "shovel", "hoe", "sword"]
        .iter()
        .find(|t| name.ends_with(*t))
        .copied();
    image(16, 16, |x, y| {
        let (xi, yi) = (x as i32, y as i32);
        // Diagonal handle from bottom-left to the centre for tools / sticks.
        let on_diag = (xi + yi - 15).abs() <= 0 && xi <= 9;
        let col = match tool {
            Some("sword") => {
                if (xi + yi - 15).abs() <= 1 && xi >= 5 {
                    Some(c)
                } else if on_diag || (xi == 3 && yi == 10) || (xi == 5 && yi == 12) {
                    Some(handle)
                } else {
                    None
                }
            }
            Some(t) => {
                let head = match t {
                    "pickaxe" => {
                        (yi <= 4 && xi >= 4 && xi + yi >= 7 && xi - yi <= 13)
                            || (xi >= 11 && yi <= 9 && xi - yi >= 6)
                    }
                    "axe" => xi >= 8 && yi <= 7 && xi + yi >= 9,
                    "shovel" => (xi - 11).abs() + (yi - 4).abs() <= 3,
                    _ => yi <= 3 && xi >= 7,
                };
                if head {
                    Some(c)
                } else if on_diag {
                    Some(handle)
                } else {
                    None
                }
            }
            None if name == "stick" => on_diag.then_some(handle),
            None => {
                let (dx, dy) = (x as f32 - 7.5, y as f32 - 7.5);
                (dx * dx + dy * dy < 30.0).then_some(c)
            }
        };
        match col {
            Some(c) => {
                let edge = if tool.is_none() && name != "stick" {
                    let (dx, dy) = (x as f32 - 7.5, y as f32 - 7.5);
                    if dx * dx + dy * dy > 20.0 {
                        0.7
                    } else if dx + dy < -3.0 {
                        1.25
                    } else {
                        1.0
                    }
                } else {
                    1.0
                };
                px(
                    [c[0] * edge, c[1] * edge, c[2] * edge],
                    noise(x, y, 5) * 0.5,
                )
            }
            None => [0, 0, 0, 0],
        }
    })
}
