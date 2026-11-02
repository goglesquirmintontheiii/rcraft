//! Texture atlas packing, mipmaps and sprite animation.

use ahash::AHashMap;

/// An RGBA8 image.
#[derive(Clone, Debug)]
pub struct Image {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Image {
    #[must_use]
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            px: vec![0; (w * h * 4) as usize],
        }
    }

    pub fn decode_png(bytes: &[u8]) -> Option<Self> {
        let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?;
        let rgba = img.to_rgba8();
        Some(Self {
            w: rgba.width(),
            h: rgba.height(),
            px: rgba.into_raw(),
        })
    }

    #[inline]
    #[must_use]
    pub fn get(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.w + x) * 4) as usize;
        [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]
    }

    #[inline]
    pub fn put(&mut self, x: u32, y: u32, c: [u8; 4]) {
        let i = ((y * self.w + x) * 4) as usize;
        self.px[i..i + 4].copy_from_slice(&c);
    }

    /// Copy a `w`×`h` block from `src` at (`sx`,`sy`) to (`dx`,`dy`).
    pub fn blit(&mut self, src: &Image, sx: u32, sy: u32, w: u32, h: u32, dx: u32, dy: u32) {
        for y in 0..h {
            let s = ((sy + y) * src.w + sx) as usize * 4;
            let d = ((dy + y) * self.w + dx) as usize * 4;
            self.px[d..d + w as usize * 4].copy_from_slice(&src.px[s..s + w as usize * 4]);
        }
    }

    /// Alpha classification: (has fully transparent, has partial alpha).
    #[must_use]
    pub fn alpha_kind(&self) -> (bool, bool) {
        let mut clear = false;
        let mut partial = false;
        for a in self.px.iter().skip(3).step_by(4) {
            match *a {
                0 => clear = true,
                255 => {}
                _ => partial = true,
            }
        }
        (clear, partial)
    }

    /// Half-size image, alpha-weighted colour average.
    #[must_use]
    pub fn downsample(&self) -> Self {
        let (w, h) = ((self.w / 2).max(1), (self.h / 2).max(1));
        let mut out = Self::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0u32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let c = self.get((x * 2 + dx).min(self.w - 1), (y * 2 + dy).min(self.h - 1));
                    let a = u32::from(c[3]);
                    acc[0] += u32::from(c[0]) * a;
                    acc[1] += u32::from(c[1]) * a;
                    acc[2] += u32::from(c[2]) * a;
                    acc[3] += a;
                }
                let c = match (
                    acc[0].checked_div(acc[3]),
                    acc[1].checked_div(acc[3]),
                    acc[2].checked_div(acc[3]),
                ) {
                    (Some(r), Some(g), Some(b)) => [r as u8, g as u8, b as u8, (acc[3] / 4) as u8],
                    _ => [0, 0, 0, 0],
                };
                out.put(x, y, c);
            }
        }
        out
    }
}

/// Animation metadata from a `.png.mcmeta`.
#[derive(Clone, Debug, Default)]
pub struct Animation {
    /// (frame index, ticks) in play order.
    pub frames: Vec<(u32, u32)>,
    pub frame_w: u32,
    pub frame_h: u32,
}

impl Animation {
    /// Parse `animation` from mcmeta JSON for an image of `w`×`h`.
    #[must_use]
    pub fn parse(meta: &serde_json::Value, w: u32, h: u32) -> Option<Self> {
        let a = meta.get("animation")?;
        let fw = a
            .get("width")
            .and_then(serde_json::Value::as_u64)
            .map_or(w, |v| v as u32);
        let fh = a
            .get("height")
            .and_then(serde_json::Value::as_u64)
            .map_or(fw.min(w), |v| v as u32);
        if fw == 0 || fh == 0 {
            return None;
        }
        let frametime = a
            .get("frametime")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(1)
            .max(1) as u32;
        let count = (w / fw) * (h / fh);
        let frames: Vec<(u32, u32)> = match a.get("frames").and_then(|f| f.as_array()) {
            Some(list) => list
                .iter()
                .filter_map(|f| match f {
                    serde_json::Value::Number(n) => Some((n.as_u64()? as u32, frametime)),
                    serde_json::Value::Object(o) => Some((
                        o.get("index")?.as_u64()? as u32,
                        o.get("time")
                            .and_then(serde_json::Value::as_u64)
                            .map_or(frametime, |t| t as u32),
                    )),
                    _ => None,
                })
                .filter(|f| f.0 < count)
                .collect(),
            None => (0..count).map(|i| (i, frametime)).collect(),
        };
        (frames.len() > 1).then_some(Self {
            frames,
            frame_w: fw,
            frame_h: fh,
        })
    }
}

/// A packed sprite.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sprite {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// Normalised UV rectangle.
    pub uv: [f32; 4],
    /// (has fully transparent texels, has translucent texels)
    pub alpha: (bool, bool),
}

impl Sprite {
    /// Map a 0..16 model UV into atlas space.
    #[inline]
    #[must_use]
    pub fn map(&self, u: f32, v: f32) -> [f32; 2] {
        [
            self.uv[0] + (self.uv[2] - self.uv[0]) * (u / 16.0),
            self.uv[1] + (self.uv[3] - self.uv[1]) * (v / 16.0),
        ]
    }
}

/// An animated sprite's source frames.
#[derive(Clone, Debug)]
pub struct AnimatedSprite {
    pub sprite: Sprite,
    pub sheet: Image,
    pub anim: Animation,
    /// Total ticks in one loop.
    pub period: u32,
}

impl AnimatedSprite {
    /// Frame index to show at game tick `tick`.
    #[must_use]
    pub fn frame_at(&self, tick: u64) -> u32 {
        let mut t = (tick % u64::from(self.period.max(1))) as u32;
        for &(f, d) in &self.anim.frames {
            if t < d {
                return f;
            }
            t -= d;
        }
        self.anim.frames[0].0
    }

    /// Pixels of frame `f`.
    #[must_use]
    pub fn frame(&self, f: u32) -> Image {
        let per_row = (self.sheet.w / self.anim.frame_w).max(1);
        let (fx, fy) = (f % per_row, f / per_row);
        let mut img = Image::new(self.anim.frame_w, self.anim.frame_h);
        img.blit(
            &self.sheet,
            fx * self.anim.frame_w,
            fy * self.anim.frame_h,
            self.anim.frame_w,
            self.anim.frame_h,
            0,
            0,
        );
        img
    }
}

/// A finished atlas with its mip chain.
#[derive(Debug)]
pub struct Atlas {
    pub mips: Vec<Image>,
    pub sprites: AHashMap<String, Sprite>,
    pub animated: Vec<AnimatedSprite>,
    pub missing: Sprite,
}

impl Atlas {
    #[must_use]
    pub fn sprite(&self, name: &str) -> Sprite {
        self.sprites.get(name).copied().unwrap_or(self.missing)
    }
}

/// Magenta/black checkerboard, the conventional missing texture.
#[must_use]
pub fn missing_texture() -> Image {
    let mut img = Image::new(16, 16);
    for y in 0..16 {
        for x in 0..16 {
            let on = (x < 8) ^ (y < 8);
            img.put(
                x,
                y,
                if on {
                    [248, 0, 248, 255]
                } else {
                    [0, 0, 0, 255]
                },
            );
        }
    }
    img
}

/// Pack `(name, image, animation)` entries. `mip_levels` counts level 0.
#[must_use]
pub fn build(mut entries: Vec<(String, Image, Option<Animation>)>, mip_levels: u32) -> Atlas {
    entries.push(("__missing".into(), missing_texture(), None));
    // Frame size is what gets packed.
    let mut items: Vec<(String, Image, Option<(Image, Animation)>)> = entries
        .into_iter()
        .map(|(n, img, anim)| match anim {
            Some(a) => {
                let mut first = Image::new(a.frame_w, a.frame_h);
                let f0 = a.frames[0].0;
                let per_row = (img.w / a.frame_w).max(1);
                first.blit(
                    &img,
                    (f0 % per_row) * a.frame_w,
                    (f0 / per_row) * a.frame_h,
                    a.frame_w,
                    a.frame_h,
                    0,
                    0,
                );
                (n, first, Some((img, a)))
            }
            None => {
                // Non-animated but taller-than-wide strips use the top square.
                if img.h > img.w && img.h % img.w == 0 {
                    let mut sq = Image::new(img.w, img.w);
                    sq.blit(&img, 0, 0, img.w, img.w, 0, 0);
                    (n, sq, None)
                } else {
                    (n, img, None)
                }
            }
        })
        .collect();
    items.sort_by(|a, b| {
        b.1.h
            .cmp(&a.1.h)
            .then(b.1.w.cmp(&a.1.w))
            .then(a.0.cmp(&b.0))
    });
    let area: u64 = items
        .iter()
        .map(|i| u64::from(i.1.w) * u64::from(i.1.h))
        .sum();
    let mut width = 256u32;
    while u64::from(width) * u64::from(width) < area * 2 && width < 8192 {
        width *= 2;
    }
    let max_w = items.iter().map(|i| i.1.w).max().unwrap_or(16);
    width = width.max(max_w.next_power_of_two());
    // Shelf packing.
    let mut placed = Vec::with_capacity(items.len());
    let (mut x, mut y, mut shelf_h) = (0u32, 0u32, 0u32);
    for it in &items {
        let (w, h) = (it.1.w, it.1.h);
        if x + w > width {
            x = 0;
            y += shelf_h;
            shelf_h = 0;
        }
        placed.push((x, y));
        x += w;
        shelf_h = shelf_h.max(h);
    }
    let height = (y + shelf_h).next_power_of_two().max(16);
    let mut img = Image::new(width, height);
    let mut sprites = AHashMap::new();
    let mut animated = Vec::new();
    for ((name, im, anim), &(px, py)) in items.into_iter().zip(&placed) {
        img.blit(&im, 0, 0, im.w, im.h, px, py);
        let s = Sprite {
            x: px,
            y: py,
            w: im.w,
            h: im.h,
            uv: [
                px as f32 / width as f32,
                py as f32 / height as f32,
                (px + im.w) as f32 / width as f32,
                (py + im.h) as f32 / height as f32,
            ],
            alpha: im.alpha_kind(),
        };
        if let Some((sheet, a)) = anim {
            let period = a.frames.iter().map(|f| f.1).sum();
            animated.push(AnimatedSprite {
                sprite: s,
                sheet,
                anim: a,
                period,
            });
        }
        sprites.insert(name, s);
    }
    let missing = sprites["__missing"];
    let mut mips = vec![img];
    for _ in 1..mip_levels {
        let next = mips.last().expect("non-empty").downsample();
        mips.push(next);
    }
    Atlas {
        mips,
        sprites,
        animated,
        missing,
    }
}
