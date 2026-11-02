//! Bitmap fonts: the player's vanilla font providers when available, else a
//! public-domain 8×8 font (font8x8, derived from the IBM PC BIOS glyphs).

use ahash::AHashMap;
use rc_assets::{AssetStack, Image};
use rc_render::gui::{DrawList, TexId};

#[derive(Clone, Copy, Debug)]
pub struct Glyph {
    pub tex: TexId,
    pub uv: [f32; 4],
    /// Size in GUI units.
    pub w: f32,
    pub h: f32,
    /// Vertical offset from the line top (ascent alignment).
    pub y: f32,
    pub advance: f32,
}

#[derive(Debug, Default)]
pub struct Font {
    glyphs: AHashMap<char, Glyph>,
    pub line_height: f32,
}

/// A loaded provider image waiting for a texture id.
pub struct PendingSheet {
    pub image: Image,
    pub chars: Vec<Vec<char>>,
    pub height: f32,
    pub ascent: f32,
}

impl std::fmt::Debug for PendingSheet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PendingSheet({}x{})", self.image.w, self.image.h)
    }
}

/// Collect bitmap providers from the vanilla `default` font.
#[must_use]
pub fn vanilla_sheets(assets: &AssetStack) -> Vec<PendingSheet> {
    let mut out = Vec::new();
    let files = ["font/include/default.json".to_owned()];
    let mut i = 0;
    while i < files.len() {
        let f = files[i].clone();
        i += 1;
        let Some(b) = assets.read(&format!("assets/minecraft/{f}")) else {
            continue;
        };
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b) else {
            continue;
        };
        for p in v["providers"].as_array().into_iter().flatten() {
            if p["type"] != "bitmap" {
                continue;
            }
            let Some(file) = p["file"].as_str() else {
                continue;
            };
            let path = file.strip_prefix("minecraft:").unwrap_or(file);
            let Some(img) = assets
                .read(&format!("assets/minecraft/textures/{path}"))
                .and_then(|b| Image::decode_png(&b))
            else {
                continue;
            };
            let chars: Vec<Vec<char>> = p["chars"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| r.as_str().map(|s| s.chars().collect()))
                .collect();
            out.push(PendingSheet {
                image: img,
                chars,
                height: p["height"].as_f64().unwrap_or(8.0) as f32,
                ascent: p["ascent"].as_f64().unwrap_or(7.0) as f32,
            });
        }
    }
    // ascii first so it wins for shared code points.
    out.sort_by_key(|s| usize::from(!s.chars.first().is_some_and(|r| r.contains(&'!'))));
    out
}

/// The built-in fallback sheet: 16×16 grid of 8×8 glyphs for U+0000..U+00FF.
#[must_use]
pub fn fallback_sheet() -> PendingSheet {
    let mut img = Image::new(128, 128);
    let mut put = |code: usize, rows: &[u8; 8]| {
        let (gx, gy) = ((code % 16) as u32 * 8, (code / 16) as u32 * 8);
        for (y, r) in rows.iter().enumerate() {
            for x in 0..8 {
                if r & (1 << x) != 0 {
                    img.put(gx + x, gy + y as u32, [255, 255, 255, 255]);
                }
            }
        }
    };
    for (i, g) in font8x8::legacy::BASIC_LEGACY.iter().enumerate() {
        put(i, g);
    }
    for (i, g) in font8x8::legacy::LATIN_LEGACY.iter().enumerate() {
        put(0xA0 + i, g);
    }
    let chars = (0..16)
        .map(|r| {
            (0..16)
                .map(|c| char::from_u32(r * 16 + c).unwrap_or('\0'))
                .collect()
        })
        .collect();
    PendingSheet {
        image: img,
        chars,
        height: 8.0,
        ascent: 7.0,
    }
}

impl Font {
    /// Build from sheets already registered as textures.
    #[must_use]
    pub fn build(sheets: &[(PendingSheet, TexId)]) -> Self {
        let mut glyphs = AHashMap::new();
        for (s, tex) in sheets {
            let rows = s.chars.len().max(1) as u32;
            let cols = s.chars.iter().map(Vec::len).max().unwrap_or(1).max(1) as u32;
            let cw = s.image.w / cols;
            let ch = s.image.h / rows;
            if cw == 0 || ch == 0 {
                continue;
            }
            let scale = s.height / ch as f32;
            for (ry, row) in s.chars.iter().enumerate() {
                for (rx, &c) in row.iter().enumerate() {
                    if c == '\0' || glyphs.contains_key(&c) {
                        continue;
                    }
                    let (x0, y0) = (rx as u32 * cw, ry as u32 * ch);
                    // Width = rightmost opaque column + 1.
                    let mut width = 0;
                    for x in 0..cw {
                        if (0..ch).any(|y| s.image.get(x0 + x, y0 + y)[3] > 0) {
                            width = x + 1;
                        }
                    }
                    let w = width as f32 * scale;
                    let advance = if c == ' ' { 4.0 } else { w + 1.0 };
                    glyphs.insert(
                        c,
                        Glyph {
                            tex: *tex,
                            uv: [
                                x0 as f32 / s.image.w as f32,
                                y0 as f32 / s.image.h as f32,
                                (x0 + width) as f32 / s.image.w as f32,
                                (y0 + ch) as f32 / s.image.h as f32,
                            ],
                            w,
                            h: s.height,
                            y: 7.0 - s.ascent,
                            advance,
                        },
                    );
                }
            }
        }
        glyphs.entry(' ').or_insert(Glyph {
            tex: TexId(0),
            uv: [0.0; 4],
            w: 0.0,
            h: 0.0,
            y: 0.0,
            advance: 4.0,
        });
        Self {
            glyphs,
            line_height: 9.0,
        }
    }

    fn glyph(&self, c: char) -> Option<&Glyph> {
        self.glyphs.get(&c).or_else(|| self.glyphs.get(&'?'))
    }

    /// Width of `text` in GUI units (formatting codes ignored).
    #[must_use]
    pub fn width(&self, text: &str) -> f32 {
        let mut w = 0.0;
        let mut it = text.chars();
        while let Some(c) = it.next() {
            if c == '§' {
                it.next();
                continue;
            }
            w += self.glyph(c).map_or(0.0, |g| g.advance);
        }
        (w - 1.0).max(0.0)
    }

    /// Draw `text` at GUI position (`x`,`y`) scaled by `scale` px/unit.
    /// Supports `§` colour codes.
    pub fn draw(
        &self,
        dl: &mut DrawList,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        color: [f32; 4],
        shadow: bool,
    ) {
        if shadow {
            let s = [color[0] * 0.25, color[1] * 0.25, color[2] * 0.25, color[3]];
            self.draw_run(dl, text, x + 1.0, y + 1.0, scale, s, true);
        }
        self.draw_run(dl, text, x, y, scale, color, false);
    }

    fn draw_run(
        &self,
        dl: &mut DrawList,
        text: &str,
        x: f32,
        y: f32,
        scale: f32,
        color: [f32; 4],
        shadow: bool,
    ) {
        let mut cx = x;
        let mut col = color;
        let mut it = text.chars();
        while let Some(c) = it.next() {
            if c == '§' {
                if let Some(code) = it.next() {
                    if let Some(rgb) = format_color(code) {
                        let k = if shadow { 0.25 } else { 1.0 };
                        col = [rgb[0] * k, rgb[1] * k, rgb[2] * k, color[3]];
                    } else if code == 'r' {
                        col = color;
                    }
                }
                continue;
            }
            let Some(g) = self.glyph(c) else { continue };
            if g.w > 0.0 {
                let r = [
                    cx * scale,
                    (y + g.y) * scale,
                    (cx + g.w) * scale,
                    (y + g.y + g.h) * scale,
                ];
                dl.rect_uv(g.tex, r, g.uv, col);
            }
            cx += g.advance;
        }
    }

    /// Split `text` into lines no wider than `max`.
    #[must_use]
    pub fn wrap(&self, text: &str, max: f32) -> Vec<String> {
        let mut out = Vec::new();
        for para in text.split('\n') {
            let mut line = String::new();
            for word in para.split(' ') {
                let cand = if line.is_empty() {
                    word.to_owned()
                } else {
                    format!("{line} {word}")
                };
                if self.width(&cand) > max && !line.is_empty() {
                    out.push(std::mem::take(&mut line));
                    line = word.to_owned();
                } else {
                    line = cand;
                }
            }
            out.push(line);
        }
        out
    }
}

/// Vanilla chat colour codes (publicly documented on minecraft.wiki "Formatting codes").
#[must_use]
pub fn format_color(c: char) -> Option<[f32; 3]> {
    let v: u32 = match c {
        '0' => 0x000000,
        '1' => 0x0000AA,
        '2' => 0x00AA00,
        '3' => 0x00AAAA,
        '4' => 0xAA0000,
        '5' => 0xAA00AA,
        '6' => 0xFFAA00,
        '7' => 0xAAAAAA,
        '8' => 0x555555,
        '9' => 0x5555FF,
        'a' => 0x55FF55,
        'b' => 0x55FFFF,
        'c' => 0xFF5555,
        'd' => 0xFF55FF,
        'e' => 0xFFFF55,
        'f' => 0xFFFFFF,
        _ => return None,
    };
    Some([
        ((v >> 16) & 255) as f32 / 255.0,
        ((v >> 8) & 255) as f32 / 255.0,
        (v & 255) as f32 / 255.0,
    ])
}
