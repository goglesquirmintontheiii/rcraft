//! Block-state → model resolution and baking (minecraft.wiki "Model",
//! "Block states" pages; resource-pack format as publicly documented).

use ahash::AHashMap;
use rc_core::BlockStateId;
use rc_data::Blocks;
use serde_json::Value;
use tracing::warn;

use crate::atlas::{self, Animation, Atlas, Image, Sprite};
use crate::source::AssetStack;

/// Face direction. Index order: down, up, north, south, west, east.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Dir {
    Down = 0,
    Up = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

impl Dir {
    pub const ALL: [Dir; 6] = [
        Dir::Down,
        Dir::Up,
        Dir::North,
        Dir::South,
        Dir::West,
        Dir::East,
    ];

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "down" | "bottom" => Self::Down,
            "up" | "top" => Self::Up,
            "north" => Self::North,
            "south" => Self::South,
            "west" => Self::West,
            "east" => Self::East,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn normal(self) -> [i32; 3] {
        match self {
            Self::Down => [0, -1, 0],
            Self::Up => [0, 1, 0],
            Self::North => [0, 0, -1],
            Self::South => [0, 0, 1],
            Self::West => [-1, 0, 0],
            Self::East => [1, 0, 0],
        }
    }

    #[must_use]
    pub fn from_normal(n: [f32; 3]) -> Self {
        let a = [n[0].abs(), n[1].abs(), n[2].abs()];
        if a[1] >= a[0] && a[1] >= a[2] {
            if n[1] < 0.0 { Self::Down } else { Self::Up }
        } else if a[0] >= a[2] {
            if n[0] < 0.0 { Self::West } else { Self::East }
        } else if n[2] < 0.0 {
            Self::North
        } else {
            Self::South
        }
    }

    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }
}

/// A quad ready for meshing: positions in block units (0..1), atlas UVs.
#[derive(Clone, Copy, Debug)]
pub struct BakedQuad {
    pub pos: [[f32; 3]; 4],
    pub uv: [[f32; 2]; 4],
    /// Face direction used for shading and light sampling.
    pub dir: Dir,
    pub cull: Option<Dir>,
    pub tint: i8,
    pub shade: bool,
}

#[derive(Clone, Debug, Default)]
pub struct BakedModel {
    pub quads: Vec<BakedQuad>,
    pub ao: bool,
}

/// What a block state renders as: each part picks one weighted model.
#[derive(Clone, Debug, Default)]
pub struct StateModel {
    pub parts: Vec<Vec<(u32, u32)>>,
}

/// Render pass for a block state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Layer {
    #[default]
    Solid,
    Cutout,
    Translucent,
    /// Not drawn as a model (air, fluids, entity-rendered blocks).
    Invisible,
}

/// How tint index 0 is coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tint {
    #[default]
    None,
    Grass,
    Foliage,
    Water,
    Fixed(u32),
}

/// All baked block models plus the block atlas.
#[derive(Debug)]
pub struct BlockModels {
    pub atlas: Atlas,
    pub models: Vec<BakedModel>,
    pub states: Vec<StateModel>,
    pub layers: Vec<Layer>,
    pub tints: Vec<Tint>,
    pub particle: Vec<Sprite>,
    /// Sprites for fluids: water still/flow, lava still/flow.
    pub fluid: [Sprite; 4],
    /// Block-breaking overlay stages 0..10.
    pub destroy: [Sprite; 10],
}

/// `Mth.getSeed`-style position hash used to pick weighted variants.
#[must_use]
pub fn position_seed(x: i32, y: i32, z: i32) -> i64 {
    let mut l = i64::from(x.wrapping_mul(3_129_871))
        ^ i64::from(z).wrapping_mul(116_129_781)
        ^ i64::from(y);
    l = l
        .wrapping_mul(l)
        .wrapping_mul(42_317_861)
        .wrapping_add(l.wrapping_mul(11));
    l >> 16
}

/// Java `Random` (48-bit LCG), enough for variant selection.
#[derive(Debug)]
pub struct JavaRandom(i64);

impl JavaRandom {
    #[must_use]
    pub fn new(seed: i64) -> Self {
        Self((seed ^ 0x5_DEEC_E66D) & ((1 << 48) - 1))
    }
    fn next(&mut self, bits: u32) -> i32 {
        self.0 = (self.0.wrapping_mul(0x5_DEEC_E66D).wrapping_add(0xB)) & ((1 << 48) - 1);
        (self.0 >> (48 - bits)) as i32
    }
    pub fn next_long(&mut self) -> i64 {
        (i64::from(self.next(32)) << 32).wrapping_add(i64::from(self.next(32)))
    }
}

/// Pick the model index for one part.
#[must_use]
pub fn pick(list: &[(u32, u32)], rng: &mut JavaRandom) -> u32 {
    if list.len() == 1 {
        return list[0].0;
    }
    let total: u32 = list.iter().map(|e| e.1).sum::<u32>().max(1);
    let mut r = ((rng.next_long() as i32).unsigned_abs() % total) as i64;
    for &(m, w) in list {
        r -= i64::from(w);
        if r < 0 {
            return m;
        }
    }
    list[0].0
}

// ---------------------------------------------------------------------------
// Unbaked model resolution
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Face {
    uv: Option<[f32; 4]>,
    texture: String,
    cull: Option<Dir>,
    rotation: u32,
    tint: i8,
}

#[derive(Clone, Debug)]
struct Element {
    from: [f32; 3],
    to: [f32; 3],
    rot: Option<(usize, f32, [f32; 3], bool)>,
    shade: bool,
    faces: Vec<(Dir, Face)>,
}

#[derive(Clone, Debug, Default)]
struct Resolved {
    textures: AHashMap<String, String>,
    elements: Option<Vec<Element>>,
    ao: Option<bool>,
}

fn norm_id(s: &str) -> String {
    if s.contains(':') {
        s.to_owned()
    } else {
        format!("minecraft:{s}")
    }
}

fn arr3(v: &Value) -> Option<[f32; 3]> {
    let a = v.as_array()?;
    Some([
        a.first()?.as_f64()? as f32,
        a.get(1)?.as_f64()? as f32,
        a.get(2)?.as_f64()? as f32,
    ])
}

fn parse_elements(v: &Value) -> Vec<Element> {
    let Some(list) = v.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|e| {
            let from = arr3(e.get("from")?)?;
            let to = arr3(e.get("to")?)?;
            let rot = e.get("rotation").and_then(|r| {
                let axis = match r.get("axis")?.as_str()? {
                    "x" => 0,
                    "y" => 1,
                    _ => 2,
                };
                Some((
                    axis,
                    r.get("angle")?.as_f64()? as f32,
                    arr3(r.get("origin")?)?,
                    r.get("rescale").and_then(Value::as_bool).unwrap_or(false),
                ))
            });
            let faces = e
                .get("faces")
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, f)| {
                            let d = Dir::parse(k)?;
                            let uv = f.get("uv").and_then(|u| {
                                let a = u.as_array()?;
                                Some([
                                    a.first()?.as_f64()? as f32,
                                    a.get(1)?.as_f64()? as f32,
                                    a.get(2)?.as_f64()? as f32,
                                    a.get(3)?.as_f64()? as f32,
                                ])
                            });
                            Some((
                                d,
                                Face {
                                    uv,
                                    texture: f.get("texture")?.as_str()?.to_owned(),
                                    cull: f
                                        .get("cullface")
                                        .and_then(Value::as_str)
                                        .and_then(Dir::parse),
                                    rotation: f.get("rotation").and_then(Value::as_u64).unwrap_or(0)
                                        as u32,
                                    tint: f
                                        .get("tintindex")
                                        .and_then(Value::as_i64)
                                        .map_or(-1, |t| t as i8),
                                },
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(Element {
                from,
                to,
                rot,
                shade: e.get("shade").and_then(Value::as_bool).unwrap_or(true),
                faces,
            })
        })
        .collect()
}

struct Loader<'a> {
    assets: &'a AssetStack,
    cache: AHashMap<String, Option<Resolved>>,
}

impl Loader<'_> {
    fn resolve(&mut self, id: &str, depth: u32) -> Option<Resolved> {
        let id = norm_id(id);
        if let Some(r) = self.cache.get(&id) {
            return r.clone();
        }
        let r = (|| {
            if depth > 32 {
                return None;
            }
            let bytes = self.assets.read_id("models", &id, ".json")?;
            let v: Value = serde_json::from_slice(&bytes).ok()?;
            let mut r = match v.get("parent").and_then(Value::as_str) {
                Some(p) if !p.starts_with("builtin/") => {
                    self.resolve(p, depth + 1).unwrap_or_default()
                }
                _ => Resolved::default(),
            };
            if let Some(t) = v.get("textures").and_then(Value::as_object) {
                for (k, val) in t {
                    if let Some(s) = val.as_str() {
                        r.textures.insert(k.clone(), s.to_owned());
                    }
                }
            }
            if let Some(e) = v.get("elements") {
                r.elements = Some(parse_elements(e));
            }
            if let Some(a) = v.get("ambientocclusion").and_then(Value::as_bool) {
                r.ao = Some(a);
            }
            Some(r)
        })();
        self.cache.insert(id, r.clone());
        r
    }
}

fn resolve_texture(textures: &AHashMap<String, String>, t: &str) -> Option<String> {
    let mut cur = t.to_owned();
    for _ in 0..16 {
        match cur.strip_prefix('#') {
            Some(k) => cur = textures.get(k)?.clone(),
            None => return Some(norm_id(&cur)),
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/// Corners TL, BL, BR, TR of a box face as seen from outside.
fn face_corners(d: Dir, f: [f32; 3], t: [f32; 3]) -> [[f32; 3]; 4] {
    let ([x0, y0, z0], [x1, y1, z1]) = (f, t);
    match d {
        Dir::North => [[x1, y1, z0], [x1, y0, z0], [x0, y0, z0], [x0, y1, z0]],
        Dir::South => [[x0, y1, z1], [x0, y0, z1], [x1, y0, z1], [x1, y1, z1]],
        Dir::West => [[x0, y1, z0], [x0, y0, z0], [x0, y0, z1], [x0, y1, z1]],
        Dir::East => [[x1, y1, z1], [x1, y0, z1], [x1, y0, z0], [x1, y1, z0]],
        Dir::Up => [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
        Dir::Down => [[x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1]],
    }
}

/// Default UV (pixels) for an element face.
fn default_uv(d: Dir, f: [f32; 3], t: [f32; 3]) -> [f32; 4] {
    let ([x0, y0, z0], [x1, y1, z1]) = (f, t);
    match d {
        Dir::Down => [x0, 16.0 - z1, x1, 16.0 - z0],
        Dir::Up => [x0, z0, x1, z1],
        Dir::North => [16.0 - x1, 16.0 - y1, 16.0 - x0, 16.0 - y0],
        Dir::South => [x0, 16.0 - y1, x1, 16.0 - y0],
        Dir::West => [z0, 16.0 - y1, z1, 16.0 - y0],
        Dir::East => [16.0 - z1, 16.0 - y1, 16.0 - z0, 16.0 - y0],
    }
}

fn rotate_axis(
    p: [f32; 3],
    axis: usize,
    angle_deg: f32,
    origin: [f32; 3],
    rescale: bool,
) -> [f32; 3] {
    let a = angle_deg.to_radians();
    let (s, c) = a.sin_cos();
    let mut v = [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
    let (i, j) = match axis {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    };
    let (vi, vj) = (v[i], v[j]);
    v[i] = vi * c - vj * s;
    v[j] = vi * s + vj * c;
    if rescale {
        let k = 1.0 / c.abs().max(1e-4);
        v[i] *= k;
        v[j] *= k;
    }
    [v[0] + origin[0], v[1] + origin[1], v[2] + origin[2]]
}

/// Variant rotation (x then y, multiples of 90°) about the block centre.
fn rot_variant(p: [f32; 3], rx: u32, ry: u32) -> [f32; 3] {
    let [mut x, mut y, mut z] = [p[0] - 0.5, p[1] - 0.5, p[2] - 0.5];
    for _ in 0..(rx / 90) % 4 {
        // north → down
        let (ny, nz) = (z, -y);
        y = ny;
        z = nz;
    }
    for _ in 0..(ry / 90) % 4 {
        // north → east
        let (nx, nz) = (-z, x);
        x = nx;
        z = nz;
    }
    [x + 0.5, y + 0.5, z + 0.5]
}

fn rot_dir(d: Dir, rx: u32, ry: u32) -> Dir {
    let n = d.normal();
    let p = rot_variant(
        [
            n[0] as f32 * 0.5 + 0.5,
            n[1] as f32 * 0.5 + 0.5,
            n[2] as f32 * 0.5 + 0.5,
        ],
        rx,
        ry,
    );
    Dir::from_normal([p[0] - 0.5, p[1] - 0.5, p[2] - 0.5])
}

/// Quad with texture name, before atlas mapping.
struct RawQuad {
    pos: [[f32; 3]; 4],
    uv_px: [[f32; 2]; 4],
    texture: String,
    dir: Dir,
    cull: Option<Dir>,
    tint: i8,
    shade: bool,
}

fn bake_raw(r: &Resolved, rx: u32, ry: u32, uvlock: bool) -> Vec<RawQuad> {
    let mut out = Vec::new();
    let Some(elements) = &r.elements else {
        return out;
    };
    for e in elements {
        for (d, face) in &e.faces {
            // A face's texture is always a variable reference; the `#` is
            // optional (1.21's heavy_core model writes plain "all").
            let reference = if face.texture.starts_with('#') {
                std::borrow::Cow::Borrowed(face.texture.as_str())
            } else {
                std::borrow::Cow::Owned(format!("#{}", face.texture))
            };
            let Some(tex) = resolve_texture(&r.textures, &reference)
                .or_else(|| resolve_texture(&r.textures, &face.texture))
            else {
                continue;
            };
            let mut corners = face_corners(*d, e.from, e.to);
            for c in &mut corners {
                let mut p = [c[0], c[1], c[2]];
                if let Some((axis, angle, origin, rescale)) = e.rot {
                    p = rotate_axis(p, axis, angle, origin, rescale);
                }
                *c = [p[0] / 16.0, p[1] / 16.0, p[2] / 16.0];
            }
            let world: [[f32; 3]; 4] = corners.map(|c| rot_variant(c, rx, ry));
            let dir = rot_dir(*d, rx, ry);
            let (uv, rot) = if uvlock && (rx != 0 || ry != 0) {
                // Recompute UVs from the rotated geometry so the texture
                // stays aligned with the world grid.
                let mut lo = [f32::MAX; 3];
                let mut hi = [f32::MIN; 3];
                for p in &world {
                    for k in 0..3 {
                        lo[k] = lo[k].min(p[k] * 16.0);
                        hi[k] = hi[k].max(p[k] * 16.0);
                    }
                }
                (default_uv(dir, lo, hi), face.rotation)
            } else {
                (
                    face.uv.unwrap_or_else(|| default_uv(*d, e.from, e.to)),
                    face.rotation,
                )
            };
            let base = [
                [uv[0], uv[1]],
                [uv[0], uv[3]],
                [uv[2], uv[3]],
                [uv[2], uv[1]],
            ];
            let k = ((rot / 90) % 4) as usize;
            let uv_px: [[f32; 2]; 4] = std::array::from_fn(|i| base[(i + k) % 4]);
            let (world, uv_px) = if uvlock && (rx != 0 || ry != 0) {
                let w = face_corners(dir, lo_of(&world), hi_of(&world));
                (w, uv_px)
            } else {
                (world, uv_px)
            };
            out.push(RawQuad {
                pos: world,
                uv_px,
                texture: tex,
                dir,
                cull: face.cull.map(|c| rot_dir(c, rx, ry)),
                tint: face.tint,
                shade: e.shade,
            });
        }
    }
    out
}

fn lo_of(p: &[[f32; 3]; 4]) -> [f32; 3] {
    let mut lo = [f32::MAX; 3];
    for q in p {
        for k in 0..3 {
            lo[k] = lo[k].min(q[k]);
        }
    }
    lo
}

fn hi_of(p: &[[f32; 3]; 4]) -> [f32; 3] {
    let mut hi = [f32::MIN; 3];
    for q in p {
        for k in 0..3 {
            hi[k] = hi[k].max(q[k]);
        }
    }
    hi
}

// ---------------------------------------------------------------------------
// Block-state matching
// ---------------------------------------------------------------------------

fn state_props(blocks: &Blocks, s: BlockStateId) -> Vec<(String, String)> {
    let def = blocks.def(s);
    def.props
        .iter()
        .map(|p| {
            (
                p.name.clone(),
                blocks.get(s, &p.name).unwrap_or("").to_owned(),
            )
        })
        .collect()
}

fn matches_key(key: &str, props: &[(String, String)]) -> bool {
    if key.is_empty() || key == "normal" {
        return true;
    }
    key.split(',').all(|kv| {
        let Some((k, v)) = kv.split_once('=') else {
            return false;
        };
        props.iter().any(|(pk, pv)| pk == k && pv == v)
    })
}

fn matches_when(when: &Value, props: &[(String, String)]) -> bool {
    let Some(obj) = when.as_object() else {
        return true;
    };
    if let Some(or) = obj.get("OR").and_then(Value::as_array) {
        return or.iter().any(|w| matches_when(w, props));
    }
    if let Some(and) = obj.get("AND").and_then(Value::as_array) {
        return and.iter().all(|w| matches_when(w, props));
    }
    obj.iter().all(|(k, v)| {
        let want = match v {
            Value::String(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            _ => return false,
        };
        let have = props
            .iter()
            .find(|(pk, _)| pk == k)
            .map(|(_, pv)| pv.as_str());
        let Some(have) = have else { return false };
        if let Some(neg) = want.strip_prefix('!') {
            !neg.split('|').any(|w| w == have)
        } else {
            want.split('|').any(|w| w == have)
        }
    })
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct VariantKey {
    model: String,
    x: u32,
    y: u32,
    uvlock: bool,
}

fn parse_variant(v: &Value) -> Vec<(VariantKey, u32)> {
    let one = |o: &Value| -> Option<(VariantKey, u32)> {
        Some((
            VariantKey {
                model: norm_id(o.get("model")?.as_str()?),
                x: o.get("x").and_then(Value::as_u64).unwrap_or(0) as u32,
                y: o.get("y").and_then(Value::as_u64).unwrap_or(0) as u32,
                uvlock: o.get("uvlock").and_then(Value::as_bool).unwrap_or(false),
            },
            o.get("weight").and_then(Value::as_u64).unwrap_or(1) as u32,
        ))
    };
    match v {
        Value::Array(a) => a.iter().filter_map(one).collect(),
        o => one(o).into_iter().collect(),
    }
}

/// Tint kind for a block, from its name.
#[must_use]
pub fn tint_for(name: &str) -> Tint {
    match name {
        "grass_block" | "short_grass" | "tall_grass" | "fern" | "large_fern" | "sugar_cane"
        | "potted_fern" => Tint::Grass,
        "oak_leaves" | "jungle_leaves" | "acacia_leaves" | "dark_oak_leaves"
        | "mangrove_leaves" | "vine" => Tint::Foliage,
        "birch_leaves" => Tint::Fixed(0x80a755),
        "spruce_leaves" => Tint::Fixed(0x619961),
        "lily_pad" => Tint::Fixed(0x208030),
        "water" | "bubble_column" | "water_cauldron" => Tint::Water,
        "melon_stem" | "pumpkin_stem" | "attached_melon_stem" | "attached_pumpkin_stem" => {
            Tint::Fixed(0xe0c71c)
        }
        "redstone_wire" => Tint::Fixed(0x4b0000),
        _ => Tint::None,
    }
}

fn load_texture(assets: &AssetStack, id: &str) -> Option<(Image, Option<Animation>)> {
    let bytes = assets.read_id("textures", id, ".png")?;
    let img = Image::decode_png(&bytes)?;
    let anim = assets
        .read_id("textures", id, ".png.mcmeta")
        .and_then(|m| serde_json::from_slice::<Value>(&m).ok())
        .and_then(|m| Animation::parse(&m, img.w, img.h));
    Some((img, anim))
}

/// Built-in models for blocks the resource format draws with entity renderers.
fn entity_block(name: &str) -> Option<&'static str> {
    Some(match name {
        "chest" | "trapped_chest" => "entity/chest/normal",
        "ender_chest" => "entity/chest/ender",
        _ => return None,
    })
}

impl BlockModels {
    /// Bake every block state from `assets`.
    #[must_use]
    pub fn load(assets: &AssetStack, blocks: &Blocks, mip_levels: u32) -> Self {
        let mut loader = Loader {
            assets,
            cache: AHashMap::new(),
        };
        let mut variant_ids: AHashMap<VariantKey, u32> = AHashMap::new();
        let mut raw_models: Vec<(Vec<RawQuad>, bool)> = Vec::new();
        let mut states = vec![StateModel::default(); blocks.len()];
        let mut particle_tex = vec![String::new(); blocks.len()];
        let mut textures: AHashMap<String, ()> = AHashMap::new();
        let mut chest_states = Vec::new();

        let mut model_names: Vec<String> = Vec::new();
        let mut intern = |k: VariantKey,
                          loader: &mut Loader<'_>,
                          raw_models: &mut Vec<(Vec<RawQuad>, bool)>,
                          model_names: &mut Vec<String>|
         -> u32 {
            if let Some(&i) = variant_ids.get(&k) {
                return i;
            }
            model_names.push(k.model.clone());
            let r = loader.resolve(&k.model, 0).unwrap_or_default();
            let quads = bake_raw(&r, k.x, k.y, k.uvlock);
            let i = raw_models.len() as u32;
            raw_models.push((quads, r.ao.unwrap_or(true)));
            variant_ids.insert(k, i);
            i
        };

        for def in &blocks.defs {
            if let Some(tex) = entity_block(&def.name) {
                for s in def.first..def.first + def.count {
                    chest_states.push((BlockStateId(s), tex));
                }
                textures.insert(norm_id(tex), ());
                continue;
            }
            let Some(bytes) = assets.read_id("blockstates", &def.name, ".json") else {
                continue;
            };
            let Ok(bs) = serde_json::from_slice::<Value>(&bytes) else {
                warn!(block = %def.name, "bad blockstate json");
                continue;
            };
            for s in def.first..def.first + def.count {
                let sid = BlockStateId(s);
                let props = state_props(blocks, sid);
                let mut parts = Vec::new();
                if let Some(vars) = bs.get("variants").and_then(Value::as_object) {
                    if let Some((_, v)) = vars.iter().find(|(k, _)| matches_key(k, &props)) {
                        parts.push(parse_variant(v));
                    }
                } else if let Some(mp) = bs.get("multipart").and_then(Value::as_array) {
                    for part in mp {
                        let ok = part.get("when").is_none_or(|w| matches_when(w, &props));
                        if ok && let Some(apply) = part.get("apply") {
                            parts.push(parse_variant(apply));
                        }
                    }
                }
                let mut sm = StateModel::default();
                for p in parts {
                    let list: Vec<(u32, u32)> = p
                        .into_iter()
                        .map(|(k, w)| {
                            (intern(k, &mut loader, &mut raw_models, &mut model_names), w)
                        })
                        .collect();
                    if !list.is_empty() {
                        sm.parts.push(list);
                    }
                }
                // Particle texture from the first model.
                if let Some(first) = sm.parts.first().and_then(|p| p.first()) {
                    let m = model_names[first.0 as usize].clone();
                    if let Some(r) = loader.resolve(&m, 0)
                        && let Some(t) = resolve_texture(&r.textures, "#particle")
                    {
                        particle_tex[s as usize] = t;
                    }
                }
                states[s as usize] = sm;
            }
        }
        for (q, _) in &raw_models {
            for quad in q {
                textures.insert(quad.texture.clone(), ());
            }
        }
        for t in particle_tex.iter().filter(|t| !t.is_empty()) {
            textures.insert(t.clone(), ());
        }
        let mut extra: Vec<String> = [
            "block/water_still",
            "block/water_flow",
            "block/lava_still",
            "block/lava_flow",
        ]
        .iter()
        .map(|s| norm_id(s))
        .collect();
        for i in 0..10 {
            extra.push(norm_id(&format!("block/destroy_stage_{i}")));
        }
        for e in &extra {
            textures.insert(e.clone(), ());
        }
        let mut names: Vec<String> = textures.into_keys().collect();
        names.sort();
        let loaded: Vec<(String, Image, Option<Animation>)> = {
            use rayon::prelude::*;
            names
                .par_iter()
                .filter_map(|n| load_texture(assets, n).map(|(i, a)| (n.clone(), i, a)))
                .collect()
        };
        if loaded.len() < names.len() {
            let missing: Vec<&String> = names
                .iter()
                .filter(|n| !loaded.iter().any(|(l, _, _)| l == *n))
                .collect();
            let users: Vec<&String> = raw_models
                .iter()
                .zip(&model_names)
                .filter(|((q, _), _)| q.iter().any(|q| missing.contains(&&q.texture)))
                .map(|(_, n)| n)
                .take(20)
                .collect();
            warn!(?missing, ?users, "block textures not found");
        }
        let atlas = atlas::build(loaded, mip_levels);
        Self::finish(
            atlas,
            raw_models,
            states,
            particle_tex,
            chest_states,
            blocks,
        )
    }

    fn finish(
        atlas: Atlas,
        raw_models: Vec<(Vec<RawQuad>, bool)>,
        mut states: Vec<StateModel>,
        particle_tex: Vec<String>,
        chest_states: Vec<(BlockStateId, &'static str)>,
        blocks: &Blocks,
    ) -> Self {
        let mut model_alpha: Vec<(bool, bool)> = Vec::with_capacity(raw_models.len());
        let mut models: Vec<BakedModel> = raw_models
            .into_iter()
            .map(|(quads, ao)| {
                let mut alpha = (false, false);
                let quads = quads
                    .into_iter()
                    .map(|q| {
                        let s = atlas.sprite(&q.texture);
                        alpha.0 |= s.alpha.0;
                        alpha.1 |= s.alpha.1;
                        BakedQuad {
                            pos: q.pos,
                            uv: q.uv_px.map(|[u, v]| s.map(u, v)),
                            dir: q.dir,
                            cull: q.cull,
                            tint: q.tint,
                            shade: q.shade,
                        }
                    })
                    .collect();
                model_alpha.push(alpha);
                BakedModel { quads, ao }
            })
            .collect();
        // Chests: a 14×14×14 box using the entity texture's lid/base regions.
        let mut particle: Vec<Sprite> = particle_tex.iter().map(|t| atlas.sprite(t)).collect();
        for (sid, tex) in chest_states {
            let s = atlas.sprite(&norm_id(tex));
            let facing = blocks.get(sid, "facing").unwrap_or("north");
            let ry = match facing {
                "east" => 90,
                "south" => 180,
                "west" => 270,
                _ => 0,
            };
            let m = chest_model(s, ry);
            states[sid.0 as usize] = StateModel {
                parts: vec![vec![(models.len() as u32, 1)]],
            };
            particle[sid.0 as usize] = s;
            models.push(m);
            model_alpha.push((false, false));
        }
        let mut layers = vec![Layer::Solid; blocks.len()];
        let mut tints = vec![Tint::None; blocks.len()];
        for def in &blocks.defs {
            let tint = tint_for(&def.name);
            for s in def.first..def.first + def.count {
                let i = s as usize;
                tints[i] = tint;
                let sm = &states[i];
                if sm.parts.is_empty() {
                    layers[i] = Layer::Invisible;
                    continue;
                }
                let mut clear = false;
                let mut partial = false;
                for part in &sm.parts {
                    for &(m, _) in part {
                        let a = model_alpha[m as usize];
                        clear |= a.0;
                        partial |= a.1;
                    }
                }
                layers[i] = if partial {
                    Layer::Translucent
                } else if clear {
                    Layer::Cutout
                } else {
                    Layer::Solid
                };
            }
        }
        for name in [
            "water",
            "lava",
            "air",
            "cave_air",
            "void_air",
            "bubble_column",
        ] {
            if let Some(b) = blocks.index_of(name) {
                let d = &blocks.defs[b as usize];
                for s in d.first..d.first + d.count {
                    layers[s as usize] = Layer::Invisible;
                }
            }
        }
        let fluid = [
            atlas.sprite("minecraft:block/water_still"),
            atlas.sprite("minecraft:block/water_flow"),
            atlas.sprite("minecraft:block/lava_still"),
            atlas.sprite("minecraft:block/lava_flow"),
        ];
        let destroy =
            std::array::from_fn(|i| atlas.sprite(&format!("minecraft:block/destroy_stage_{i}")));
        Self {
            atlas,
            models,
            states,
            layers,
            tints,
            particle,
            fluid,
            destroy,
        }
    }

    /// Models selected for a block at a position.
    pub fn select<'a>(
        &'a self,
        s: BlockStateId,
        x: i32,
        y: i32,
        z: i32,
        out: &mut Vec<&'a BakedModel>,
    ) {
        out.clear();
        let Some(sm) = self.states.get(s.0 as usize) else {
            return;
        };
        let mut rng = JavaRandom::new(position_seed(x, y, z));
        for part in &sm.parts {
            let m = pick(part, &mut rng);
            out.push(&self.models[m as usize]);
        }
    }

    /// Build from already-baked pieces (the procedural fallback pack).
    #[must_use]
    pub fn from_parts(
        atlas: Atlas,
        models: Vec<BakedModel>,
        states: Vec<StateModel>,
        layers: Vec<Layer>,
        tints: Vec<Tint>,
        particle: Vec<Sprite>,
    ) -> Self {
        let fluid = [
            atlas.sprite("water_still"),
            atlas.sprite("water_still"),
            atlas.sprite("lava_still"),
            atlas.sprite("lava_still"),
        ];
        let destroy = std::array::from_fn(|i| atlas.sprite(&format!("destroy_{i}")));
        Self {
            atlas,
            models,
            states,
            layers,
            tints,
            particle,
            fluid,
            destroy,
        }
    }
}

/// A chest approximated as a single 14³ box textured from the entity sheet.
fn chest_model(s: Sprite, ry: u32) -> BakedModel {
    // Entity sheet is 64×64; regions are in sheet pixels.
    let sub = |x0: f32, y0: f32, x1: f32, y1: f32| -> [[f32; 2]; 4] {
        let m = |u: f32, v: f32| {
            [
                s.uv[0] + (s.uv[2] - s.uv[0]) * (u / 64.0),
                s.uv[1] + (s.uv[3] - s.uv[1]) * (v / 64.0),
            ]
        };
        [m(x0, y0), m(x0, y1), m(x1, y1), m(x1, y0)]
    };
    let (f, t) = ([1.0, 0.0, 1.0], [15.0, 14.0, 15.0]);
    let faces = [
        (Dir::Up, sub(14.0, 0.0, 28.0, 14.0)),
        (Dir::Down, sub(28.0, 19.0, 42.0, 33.0)),
        (Dir::North, sub(42.0, 33.0, 56.0, 43.0)),
        (Dir::South, sub(14.0, 33.0, 28.0, 43.0)),
        (Dir::West, sub(0.0, 33.0, 14.0, 43.0)),
        (Dir::East, sub(28.0, 33.0, 42.0, 43.0)),
    ];
    let quads = faces
        .iter()
        .map(|&(d, uv)| {
            let c = face_corners(d, f, t)
                .map(|p| rot_variant([p[0] / 16.0, p[1] / 16.0, p[2] / 16.0], 0, ry));
            BakedQuad {
                pos: c,
                uv,
                dir: rot_dir(d, 0, ry),
                cull: (d == Dir::Down).then_some(Dir::Down),
                tint: -1,
                shade: true,
            }
        })
        .collect();
    BakedModel { quads, ao: true }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_rotation_maps_north_to_east_and_down() {
        assert_eq!(rot_dir(Dir::North, 0, 90), Dir::East);
        assert_eq!(rot_dir(Dir::North, 90, 0), Dir::Down);
        assert_eq!(rot_dir(Dir::Up, 90, 0), Dir::North);
        assert_eq!(rot_dir(Dir::North, 270, 0), Dir::Up);
        assert_eq!(rot_dir(Dir::East, 0, 180), Dir::West);
    }

    #[test]
    fn multipart_conditions() {
        let props = vec![
            ("north".to_owned(), "true".to_owned()),
            ("age".to_owned(), "3".to_owned()),
        ];
        let w: Value = serde_json::json!({"north": "true", "age": "1|3"});
        assert!(matches_when(&w, &props));
        let w: Value = serde_json::json!({"OR": [{"north": "false"}, {"age": "3"}]});
        assert!(matches_when(&w, &props));
        assert!(matches_key("age=3,north=true", &props));
        assert!(!matches_key("age=2", &props));
    }
}
