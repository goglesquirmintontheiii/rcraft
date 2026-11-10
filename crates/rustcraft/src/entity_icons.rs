//! Inventory icons for items the game draws with a block-entity renderer
//! (beds, banners, shulker boxes, heads, shields, the conduit and the
//! decorated pot). Their item models only say `builtin/entity`, so there is no
//! sprite to show: the icon is rasterised once at load time from the entity
//! texture in the player's install and the box geometry in `models.rs` style
//! (community-documented texture layouts).

use rc_assets::{Assets, Image};

use crate::models::{Cube, Model, ModelQuad, Part};

/// Icon edge length in pixels.
pub const SIZE: u32 = 64;

/// Vanilla dye colours (the "texture diffuse colour" of each dye).
pub const DYES: [(&str, u32); 16] = [
    ("white", 0xF9FFFE),
    ("orange", 0xF9801D),
    ("magenta", 0xC74EBD),
    ("light_blue", 0x3AB3DA),
    ("yellow", 0xFED83D),
    ("lime", 0x80C71F),
    ("pink", 0xF38BAA),
    ("gray", 0x474F52),
    ("light_gray", 0x9D9D97),
    ("cyan", 0x169C9C),
    ("purple", 0x8932B8),
    ("blue", 0x3C44AA),
    ("brown", 0x835432),
    ("green", 0x5E7C16),
    ("red", 0xB02E26),
    ("black", 0x1D1D21),
];

fn dye_rgb(name: &str) -> Option<[f32; 3]> {
    DYES.iter().find(|(n, _)| *n == name).map(|&(_, c)| {
        [
            ((c >> 16) & 0xff) as f32 / 255.0,
            ((c >> 8) & 0xff) as f32 / 255.0,
            (c & 0xff) as f32 / 255.0,
        ]
    })
}

fn cube(from: [f32; 3], size: [f32; 3], uv: [f32; 2]) -> Cube {
    Cube {
        from,
        size,
        uv,
        inflate: 0.0,
        mirror: false,
    }
}

fn part(pivot: [f32; 3], rot: [f32; 3], cubes: Vec<Cube>) -> Part {
    Part {
        name: "icon",
        pivot,
        rot,
        cubes,
    }
}

/// How the icon is viewed: (yaw, pitch) in degrees.
#[derive(Clone, Copy)]
struct View {
    yaw: f32,
    pitch: f32,
    /// Fraction of the icon the model's widest extent fills.
    fill: f32,
}

/// The vanilla GUI angle for blocks (30° down, turned 225°).
const BLOCK_VIEW: View = View {
    yaw: 225.0,
    pitch: 30.0,
    fill: 0.98,
};

/// One layer to draw: geometry, its texture and a tint.
struct Layer {
    model: Model,
    tex: Image,
    tint: [f32; 3],
}

/// Icon for `name`, if it is a block-entity item and its texture exists.
#[must_use]
pub fn icon(assets: &Assets, name: &str) -> Option<Image> {
    let png = |p: &str| assets.png(&format!("minecraft:{p}"));
    let white = [1.0; 3];
    let (layers, view) = if name == "shulker_box" || name.ends_with("_shulker_box") {
        let tex = match name.strip_suffix("_shulker_box") {
            Some(c) => png(&format!("entity/shulker/shulker_{c}"))?,
            None => png("entity/shulker/shulker")?,
        };
        (vec![shulker_box(tex)], BLOCK_VIEW)
    } else if let Some(c) = name.strip_suffix("_bed") {
        let tex = png(&format!("entity/bed/{c}"))?;
        (
            vec![Layer {
                model: bed(),
                tex,
                tint: white,
            }],
            View {
                yaw: 160.0,
                pitch: 30.0,
                fill: 1.0,
            },
        )
    } else if let Some(c) = name.strip_suffix("_banner") {
        let tint = dye_rgb(c)?;
        let tex = png("entity/banner_base")?;
        let (pole, flag) = banner();
        (
            vec![
                Layer {
                    model: pole,
                    tex: tex.clone(),
                    tint: white,
                },
                Layer {
                    model: flag,
                    tex: png("entity/banner/base").unwrap_or(tex),
                    tint,
                },
            ],
            View {
                yaw: 200.0,
                pitch: 10.0,
                fill: 0.95,
            },
        )
    } else if name == "shield" {
        let tex = png("entity/shield_base_nopattern")?;
        (
            vec![Layer {
                model: shield(),
                tex,
                tint: white,
            }],
            View {
                yaw: 200.0,
                pitch: 10.0,
                fill: 0.9,
            },
        )
    } else if name == "conduit" {
        let tex = png("entity/conduit/base")?;
        (
            vec![Layer {
                model: Model {
                    tex: [32.0, 16.0],
                    parts: vec![part(
                        [0.0; 3],
                        [0.0; 3],
                        vec![cube([-3.0, -3.0, -3.0], [6.0, 6.0, 6.0], [0.0, 0.0])],
                    )],
                },
                tex,
                tint: white,
            }],
            View {
                fill: 0.6,
                ..BLOCK_VIEW
            },
        )
    } else if let Some(path) = match name {
        "chest" => Some("entity/chest/normal"),
        "trapped_chest" => Some("entity/chest/trapped"),
        "ender_chest" => Some("entity/chest/ender"),
        _ => None,
    } {
        (
            vec![Layer {
                model: chest(),
                tex: png(path)?,
                tint: white,
            }],
            BLOCK_VIEW,
        )
    } else if name == "decorated_pot" {
        decorated_pot(&png)?
    } else {
        let (path, tex_size, extra) = match name {
            "skeleton_skull" => ("entity/skeleton/skeleton", [64.0, 32.0], None),
            "wither_skeleton_skull" => ("entity/skeleton/wither_skeleton", [64.0, 32.0], None),
            "zombie_head" => ("entity/zombie/zombie", [64.0, 64.0], Some([32.0, 0.0])),
            "creeper_head" => ("entity/creeper/creeper", [64.0, 32.0], None),
            "player_head" => ("entity/player/wide/steve", [64.0, 64.0], Some([32.0, 0.0])),
            "piglin_head" => return piglin_head(&png),
            "dragon_head" => return dragon_head(&png),
            _ => return None,
        };
        let tex = png(path)?;
        let mut cubes = vec![cube([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0.0, 0.0])];
        if let Some(uv) = extra {
            let mut hat = cube([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], uv);
            hat.inflate = 0.25;
            cubes.push(hat);
        }
        (
            vec![Layer {
                model: Model {
                    tex: tex_size,
                    parts: vec![part([0.0; 3], [0.0; 3], cubes)],
                },
                tex,
                tint: white,
            }],
            View {
                fill: 0.78,
                ..BLOCK_VIEW
            },
        )
    };
    Some(rasterize(&layers, view))
}

fn chest() -> Model {
    // Base 14×10×14, lid 14×5×14 and the latch, front at -Z. The chest
    // textures are laid out for a model drawn upside down, hence the X flip.
    let flip = [std::f32::consts::PI, 0.0, 0.0];
    Model {
        tex: [64.0, 64.0],
        parts: vec![part(
            [0.0, -8.0, 0.0],
            flip,
            vec![
                cube([-7.0, -8.0, -7.0], [14.0, 10.0, 14.0], [0.0, 19.0]),
                cube([-7.0, 1.0, -7.0], [14.0, 5.0, 14.0], [0.0, 0.0]),
                cube([-1.0, -1.0, 7.0], [2.0, 4.0, 1.0], [0.0, 0.0]),
            ],
        )],
    }
}

fn shulker_box(tex: Image) -> Layer {
    Layer {
        model: Model {
            tex: [64.0, 64.0],
            parts: vec![part(
                [0.0; 3],
                [0.0; 3],
                vec![
                    // Lid (upper 12 px) over the base (lower 8 px).
                    cube([-8.0, -16.0, -8.0], [16.0, 12.0, 16.0], [0.0, 0.0]),
                    cube([-8.0, -8.0, -8.0], [16.0, 8.0, 16.0], [0.0, 28.0]),
                ],
            )],
        },
        tex,
        tint: [1.0; 3],
    }
}

fn bed() -> Model {
    // Head and foot halves are 16×16×6 boxes lying flat (rotated 90° about X),
    // legs 3 px cubes underneath.
    let flat = [-std::f32::consts::FRAC_PI_2, 0.0, 0.0];
    Model {
        tex: [64.0, 64.0],
        parts: vec![
            part(
                [0.0, -3.0, 0.0],
                flat,
                vec![cube([-8.0, -16.0, -6.0], [16.0, 16.0, 6.0], [0.0, 0.0])],
            ),
            part(
                [0.0, -3.0, 0.0],
                flat,
                vec![cube([-8.0, 0.0, -6.0], [16.0, 16.0, 6.0], [0.0, 22.0])],
            ),
            part(
                [0.0; 3],
                [0.0; 3],
                vec![
                    cube([-8.0, -3.0, -16.0], [3.0, 3.0, 3.0], [50.0, 0.0]),
                    cube([5.0, -3.0, -16.0], [3.0, 3.0, 3.0], [50.0, 6.0]),
                    cube([-8.0, -3.0, 13.0], [3.0, 3.0, 3.0], [50.0, 12.0]),
                    cube([5.0, -3.0, 13.0], [3.0, 3.0, 3.0], [50.0, 18.0]),
                ],
            ),
        ],
    }
}

/// (pole + bar, flag)
fn banner() -> (Model, Model) {
    let pole = Model {
        tex: [64.0, 64.0],
        parts: vec![part(
            [0.0; 3],
            [0.0; 3],
            vec![
                cube([-1.0, -42.0, -1.0], [2.0, 42.0, 2.0], [44.0, 0.0]),
                cube([-10.0, -42.0, -2.0], [20.0, 2.0, 2.0], [0.0, 42.0]),
            ],
        )],
    };
    let flag = Model {
        tex: [64.0, 64.0],
        parts: vec![part(
            [0.0; 3],
            [0.0; 3],
            vec![cube([-10.0, -40.0, -3.0], [20.0, 40.0, 1.0], [0.0, 0.0])],
        )],
    };
    (pole, flag)
}

fn shield() -> Model {
    Model {
        tex: [64.0, 64.0],
        parts: vec![part(
            [0.0; 3],
            [0.0; 3],
            vec![
                cube([-6.0, -22.0, -1.0], [12.0, 22.0, 1.0], [0.0, 0.0]),
                cube([-1.0, -14.0, 0.0], [2.0, 6.0, 6.0], [26.0, 0.0]),
            ],
        )],
    }
}

fn piglin_head(png: &dyn Fn(&str) -> Option<Image>) -> Option<Image> {
    let tex = png("entity/piglin/piglin")?;
    let model = Model {
        tex: [64.0, 64.0],
        parts: vec![part(
            [0.0; 3],
            [0.0; 3],
            vec![
                cube([-5.0, -8.0, -4.0], [10.0, 8.0, 8.0], [0.0, 0.0]),
                cube([-2.0, -4.0, -5.0], [4.0, 4.0, 1.0], [31.0, 1.0]),
                cube([2.0, -2.0, -5.0], [1.0, 2.0, 1.0], [2.0, 4.0]),
                cube([-3.0, -2.0, -5.0], [1.0, 2.0, 1.0], [2.0, 0.0]),
            ],
        )],
    };
    Some(rasterize(
        &[Layer {
            model,
            tex,
            tint: [1.0; 3],
        }],
        View {
            fill: 0.85,
            ..BLOCK_VIEW
        },
    ))
}

fn dragon_head(png: &dyn Fn(&str) -> Option<Image>) -> Option<Image> {
    let tex = png("entity/enderdragon/dragon")?;
    let model = Model {
        tex: [256.0, 256.0],
        parts: vec![part(
            [0.0; 3],
            [0.0; 3],
            vec![
                cube([-8.0, -16.0, -6.0], [16.0, 16.0, 16.0], [112.0, 30.0]),
                cube([-6.0, -7.0, -22.0], [12.0, 5.0, 16.0], [176.0, 44.0]),
                cube([-6.0, -2.0, -22.0], [12.0, 4.0, 16.0], [176.0, 65.0]),
            ],
        )],
    };
    Some(rasterize(
        &[Layer {
            model,
            tex,
            tint: [1.0; 3],
        }],
        View {
            fill: 0.95,
            ..BLOCK_VIEW
        },
    ))
}

fn decorated_pot(png: &dyn Fn(&str) -> Option<Image>) -> Option<(Vec<Layer>, View)> {
    let base = png("entity/decorated_pot/decorated_pot_base")?;
    let side = png("entity/decorated_pot/decorated_pot_side").unwrap_or_else(|| base.clone());
    // Neck and rim from the base sheet, the body's four sides from the side
    // sheet (stretched over a 14×16×14 body).
    let neck = Model {
        tex: [32.0, 32.0],
        parts: vec![part(
            [0.0; 3],
            [0.0; 3],
            vec![
                cube([-4.0, -20.0, -4.0], [8.0, 3.0, 8.0], [0.0, 0.0]),
                cube([-3.0, -17.0, -3.0], [6.0, 1.0, 6.0], [0.0, 5.0]),
            ],
        )],
    };
    let body = Model {
        tex: [14.0 * 2.0 + 14.0 * 2.0, 16.0 + 14.0],
        parts: vec![part(
            [0.0; 3],
            [0.0; 3],
            vec![cube([-7.0, -16.0, -7.0], [14.0, 16.0, 14.0], [0.0, 0.0])],
        )],
    };
    // Build a box-UV sheet for the body: every side face shows `side`.
    let (fw, fh) = (14u32, 16u32);
    let mut sheet = Image::new(fw * 4, fh + 14);
    for k in 0..4 {
        for y in 0..fh {
            for x in 0..fw {
                let sx = x * side.w / fw;
                let sy = y * side.h / fh;
                sheet.put(
                    k * fw + x,
                    14 + y,
                    side.get(sx.min(side.w - 1), sy.min(side.h - 1)),
                );
            }
        }
    }
    // Top face: a plain clay colour sampled from the side texture centre.
    let clay = side.get(side.w / 2, side.h / 2);
    for y in 0..14 {
        for x in 14..42 {
            sheet.put(x, y, [clay[0], clay[1], clay[2], 255]);
        }
    }
    Some((
        vec![
            Layer {
                model: neck,
                tex: base,
                tint: [1.0; 3],
            },
            Layer {
                model: body,
                tex: sheet,
                tint: [1.0; 3],
            },
        ],
        View {
            fill: 0.9,
            ..BLOCK_VIEW
        },
    ))
}

/// Orthographic z-buffered rasteriser: model space is Y down, front at -Z.
fn rasterize(layers: &[Layer], view: View) -> Image {
    let n = SIZE as usize;
    let mut out = Image::new(SIZE, SIZE);
    let mut depth = vec![f32::INFINITY; n * n];
    let (sy, cy) = view.yaw.to_radians().sin_cos();
    let (sp, cp) = view.pitch.to_radians().sin_cos();
    // To view space: x right, y up, z towards the viewer (larger = nearer).
    let xf = |p: [f32; 3]| -> [f32; 3] {
        // Y up, front faces the viewer at yaw 180.
        let (x, y, z) = (p[0], -p[1], p[2]);
        let (x1, z1) = (x * cy - z * sy, x * sy + z * cy);
        let (y2, z2) = (y * cp - z1 * sp, y * sp + z1 * cp);
        [x1, y2, z2]
    };
    let mut quads: Vec<(usize, ModelQuad)> = Vec::new();
    for (li, l) in layers.iter().enumerate() {
        let mut q = Vec::new();
        crate::models::build(&l.model, |_| [0.0; 3], &mut q);
        quads.extend(q.into_iter().map(|m| (li, m)));
    }
    let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for (_, q) in &quads {
        for p in q.pos {
            let v = xf(p);
            for k in 0..2 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
    }
    let extent = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-3);
    let scale = n as f32 * view.fill / extent;
    let centre = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
    let to_px = |v: [f32; 3]| -> [f32; 3] {
        [
            n as f32 / 2.0 + (v[0] - centre[0]) * scale,
            n as f32 / 2.0 - (v[1] - centre[1]) * scale,
            v[2],
        ]
    };
    for (li, q) in &quads {
        let l = &layers[*li];
        let nrm = {
            let a = xf([0.0; 3]);
            let b = xf(q.normal);
            [b[0] - a[0], b[1] - a[1], b[2] - a[2]]
        };
        if nrm[2] <= 1e-3 {
            continue; // facing away
        }
        // Vanilla-like GUI shading: top brightest, then front, then sides.
        let up = nrm[1];
        let shade = if up > 0.5 {
            1.0
        } else if up < -0.5 {
            0.5
        } else if nrm[2] > 0.7 {
            0.85
        } else {
            0.65
        };
        let pts = q.pos.map(|p| to_px(xf(p)));
        for tri in [[0usize, 1, 2], [0, 2, 3]] {
            let [a, b, c] = tri.map(|i| pts[i]);
            let [ua, ub, uc] = tri.map(|i| q.uv[i]);
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if area.abs() < 1e-6 {
                continue;
            }
            let minx = a[0].min(b[0]).min(c[0]).floor().max(0.0) as usize;
            let maxx = (a[0].max(b[0]).max(c[0]).ceil() as usize).min(n);
            let miny = a[1].min(b[1]).min(c[1]).floor().max(0.0) as usize;
            let maxy = (a[1].max(b[1]).max(c[1]).ceil() as usize).min(n);
            for py in miny..maxy {
                for px in minx..maxx {
                    let (x, y) = (px as f32 + 0.5, py as f32 + 0.5);
                    let w0 = ((b[0] - x) * (c[1] - y) - (b[1] - y) * (c[0] - x)) / area;
                    let w1 = ((c[0] - x) * (a[1] - y) - (c[1] - y) * (a[0] - x)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let z = -(w0 * a[2] + w1 * b[2] + w2 * c[2]);
                    let di = py * n + px;
                    if z >= depth[di] {
                        continue;
                    }
                    let u = w0 * ua[0] + w1 * ub[0] + w2 * uc[0];
                    let v = w0 * ua[1] + w1 * ub[1] + w2 * uc[1];
                    let tx = ((u * l.tex.w as f32) as u32).min(l.tex.w - 1);
                    let ty = ((v * l.tex.h as f32) as u32).min(l.tex.h - 1);
                    let t = l.tex.get(tx, ty);
                    if t[3] < 26 {
                        continue;
                    }
                    depth[di] = z;
                    let ch = |k: usize| (f32::from(t[k]) * l.tint[k] * shade).min(255.0) as u8;
                    out.put(px as u32, py as u32, [ch(0), ch(1), ch(2), 255]);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn rasterizes_a_textured_cube() {
        let mut tex = rc_assets::Image::new(32, 16);
        tex.px.fill(200);
        let layer = super::Layer {
            model: super::Model {
                tex: [32.0, 16.0],
                parts: vec![super::part(
                    [0.0; 3],
                    [0.0; 3],
                    vec![super::cube([-4.0, -4.0, -4.0], [8.0, 8.0, 8.0], [0.0, 0.0])],
                )],
            },
            tex,
            tint: [1.0; 3],
        };
        let img = super::rasterize(&[layer], super::BLOCK_VIEW);
        let opaque = img.px.chunks(4).filter(|p| p[3] == 255).count();
        // A cube seen from above at an angle covers a good part of the icon.
        assert!(opaque > 1000, "{opaque}");
        assert_eq!(img.get(0, 0)[3], 0);
    }
}
