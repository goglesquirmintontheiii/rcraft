//! Density functions: compiled from vanilla JSON into an arena of nodes and
//! evaluated with the same semantics the game's `NoiseChunk` gives them.
//!
//! Evaluation modes (see [`Mode`]):
//! * `Direct` – a bare `SinglePointContext` outside any chunk (biome source,
//!   spawn search). Markers are transparent.
//! * `ChunkPoint` – a single point evaluated through a chunk's wrapped router
//!   (aquifer, surface). `flat_cache` snaps to the quart grid when the point is
//!   inside the chunk's cache range; `interpolated` is computed directly.
//! * `Cell` – per-block evaluation during terrain fill. `interpolated` reads
//!   trilinear interpolation of the chunk's corner grid, either in `lerp3`
//!   order (x, y, z — used when filling `cache_all_in_cell`, i.e. final density)
//!   or sequential y → x → z order (the interpolator's running value, used by
//!   the ore-vein functions).

use ahash::AHashMap;
use serde_json::Value;

use crate::mth;
use crate::noise::{BlendedNoise, NoiseParams, NormalNoise, SimplexNoise};
use crate::rng::{Positional, Random};

pub type Id = u32;

#[derive(Clone, Debug)]
pub enum SplineValue {
    Const(f32),
    Spline(Box<Spline>),
}

#[derive(Clone, Debug)]
pub struct Spline {
    pub coordinate: Id,
    pub locations: Vec<f32>,
    pub values: Vec<SplineValue>,
    pub derivatives: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rarity {
    Type1,
    Type2,
}

#[derive(Clone, Debug)]
pub enum Node {
    Const(f64),
    Add(Id, Id),
    Mul(Id, Id),
    Min(Id, Id),
    Max(Id, Id),
    Abs(Id),
    Square(Id),
    Cube(Id),
    HalfNegative(Id),
    QuarterNegative(Id),
    Squeeze(Id),
    Clamp {
        input: Id,
        min: f64,
        max: f64,
    },
    YClampedGradient {
        from_y: i32,
        to_y: i32,
        from_value: f64,
        to_value: f64,
    },
    RangeChoice {
        input: Id,
        min: f64,
        max: f64,
        in_range: Id,
        out_range: Id,
    },
    Noise {
        noise: u32,
        xz_scale: f64,
        y_scale: f64,
    },
    ShiftedNoise {
        noise: u32,
        xz_scale: f64,
        y_scale: f64,
        shift_x: Id,
        shift_y: Id,
        shift_z: Id,
    },
    ShiftA(u32),
    ShiftB(u32),
    Shift(u32),
    WeirdScaled {
        input: Id,
        noise: u32,
        rarity: Rarity,
    },
    Interpolated(Id),
    FlatCache(Id),
    /// `cache_2d`, `cache_once`, `cache_all_in_cell`: value-transparent.
    Cache(Id),
    BlendAlpha,
    BlendOffset,
    BlendDensity(Id),
    Beardifier,
    OldBlended(u32),
    EndIslands,
    Spline(Box<Spline>),
}

/// Compiled density-function graph plus the noises it samples, for one world
/// seed and one noise-settings preset.
pub struct Graph {
    pub nodes: Vec<Node>,
    pub noises: Vec<NormalNoise>,
    noise_ids: AHashMap<String, u32>,
    pub blended: Vec<BlendedNoise>,
    pub end_islands: Option<SimplexNoise>,
    named: AHashMap<String, Id>,
    /// Ids of all `interpolated` nodes, in creation order.
    pub interpolated: Vec<Id>,
    /// Ids of all `flat_cache` nodes.
    pub flat_caches: Vec<Id>,
    /// Dense index of each interpolated / flat-cache node (or u32::MAX).
    pub slot: Vec<u32>,
    random: Positional,
    seed: i64,
    legacy: bool,
}

impl std::fmt::Debug for Graph {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Graph({} nodes, {} noises)",
            self.nodes.len(),
            self.noises.len()
        )
    }
}

fn num(v: &Value, k: &str) -> f64 {
    v[k].as_f64()
        .unwrap_or_else(|| panic!("missing number {k} in {v}"))
}

impl Graph {
    #[must_use]
    pub fn new(seed: i64, legacy: bool, random: Positional) -> Self {
        Self {
            nodes: Vec::new(),
            noises: Vec::new(),
            noise_ids: AHashMap::new(),
            blended: Vec::new(),
            end_islands: None,
            named: AHashMap::new(),
            interpolated: Vec::new(),
            flat_caches: Vec::new(),
            slot: Vec::new(),
            random,
            seed,
            legacy,
        }
    }

    fn push(&mut self, n: Node) -> Id {
        let id = self.nodes.len() as Id;
        let slot = match &n {
            Node::Interpolated(_) => {
                self.interpolated.push(id);
                (self.interpolated.len() - 1) as u32
            }
            Node::FlatCache(_) => {
                self.flat_caches.push(id);
                (self.flat_caches.len() - 1) as u32
            }
            _ => u32::MAX,
        };
        self.nodes.push(n);
        self.slot.push(slot);
        id
    }

    fn noise(&mut self, v: &Value) -> u32 {
        let key = match v {
            Value::String(s) => {
                if s.contains(':') {
                    s.clone()
                } else {
                    format!("minecraft:{s}")
                }
            }
            other => panic!("inline noise parameters are not used by vanilla: {other}"),
        };
        if let Some(&i) = self.noise_ids.get(&key) {
            return i;
        }
        let params = NoiseParams::from_json(
            &rc_data::files()
                .registry_json("worldgen/noise", &key)
                .unwrap_or_else(|| panic!("noise {key}")),
        );
        let n = if self.legacy && key == "minecraft:temperature" {
            let p = NoiseParams {
                first_octave: -7,
                amplitudes: vec![1.0, 1.0],
            };
            NormalNoise::new_legacy_nether_biome(&mut Random::legacy(self.seed), &p)
        } else if self.legacy && key == "minecraft:vegetation" {
            let p = NoiseParams {
                first_octave: -7,
                amplitudes: vec![1.0, 1.0],
            };
            NormalNoise::new_legacy_nether_biome(&mut Random::legacy(self.seed.wrapping_add(1)), &p)
        } else if self.legacy && key == "minecraft:offset" {
            let p = NoiseParams {
                first_octave: 0,
                amplitudes: vec![0.0],
            };
            NormalNoise::new(&mut self.random.from_hash_of(&key), &p)
        } else {
            NormalNoise::new(&mut self.random.from_hash_of(&key), &params)
        };
        let i = self.noises.len() as u32;
        self.noises.push(n);
        self.noise_ids.insert(key, i);
        i
    }

    /// Normal noise by key, created on demand (surface rules use this).
    pub fn noise_by_key(&mut self, key: &str) -> u32 {
        self.noise(&Value::String(key.to_owned()))
    }

    /// Compile a JSON density function (number, reference or object).
    pub fn compile(&mut self, v: &Value) -> Id {
        match v {
            Value::Number(n) => self.push(Node::Const(n.as_f64().unwrap())),
            Value::String(s) => self.reference(s),
            Value::Object(_) => self.compile_obj(v),
            other => panic!("bad density function {other}"),
        }
    }

    /// Resolve a named density function (`minecraft:overworld/continents`).
    pub fn reference(&mut self, name: &str) -> Id {
        let key = if name.contains(':') {
            name.to_owned()
        } else {
            format!("minecraft:{name}")
        };
        if let Some(&id) = self.named.get(&key) {
            return id;
        }
        let j = rc_data::files()
            .registry_json("worldgen/density_function", &key)
            .unwrap_or_else(|| panic!("density function {key}"));
        let id = self.compile(&j);
        self.named.insert(key, id);
        id
    }

    fn compile_obj(&mut self, v: &Value) -> Id {
        let t = v["type"].as_str().unwrap();
        let t = t.strip_prefix("minecraft:").unwrap_or(t);
        let arg = |g: &mut Self, k: &str| g.compile(&v[k]);
        let node = match t {
            "constant" => Node::Const(num(v, "argument")),
            "add" | "mul" | "min" | "max" => {
                let a = arg(self, "argument1");
                let b = arg(self, "argument2");
                match t {
                    "add" => Node::Add(a, b),
                    "mul" => Node::Mul(a, b),
                    "min" => Node::Min(a, b),
                    _ => Node::Max(a, b),
                }
            }
            "abs" => Node::Abs(arg(self, "argument")),
            "square" => Node::Square(arg(self, "argument")),
            "cube" => Node::Cube(arg(self, "argument")),
            "half_negative" => Node::HalfNegative(arg(self, "argument")),
            "quarter_negative" => Node::QuarterNegative(arg(self, "argument")),
            "squeeze" => Node::Squeeze(arg(self, "argument")),
            "clamp" => Node::Clamp {
                input: arg(self, "input"),
                min: num(v, "min"),
                max: num(v, "max"),
            },
            "y_clamped_gradient" => Node::YClampedGradient {
                from_y: num(v, "from_y") as i32,
                to_y: num(v, "to_y") as i32,
                from_value: num(v, "from_value"),
                to_value: num(v, "to_value"),
            },
            "range_choice" => Node::RangeChoice {
                input: arg(self, "input"),
                min: num(v, "min_inclusive"),
                max: num(v, "max_exclusive"),
                in_range: arg(self, "when_in_range"),
                out_range: arg(self, "when_out_of_range"),
            },
            "noise" => Node::Noise {
                noise: self.noise(&v["noise"]),
                xz_scale: num(v, "xz_scale"),
                y_scale: num(v, "y_scale"),
            },
            "shifted_noise" => {
                let shift_x = arg(self, "shift_x");
                let shift_y = arg(self, "shift_y");
                let shift_z = arg(self, "shift_z");
                Node::ShiftedNoise {
                    noise: self.noise(&v["noise"]),
                    xz_scale: num(v, "xz_scale"),
                    y_scale: num(v, "y_scale"),
                    shift_x,
                    shift_y,
                    shift_z,
                }
            }
            "shift_a" => Node::ShiftA(self.noise(&v["argument"])),
            "shift_b" => Node::ShiftB(self.noise(&v["argument"])),
            "shift" => Node::Shift(self.noise(&v["argument"])),
            "weird_scaled_sampler" => Node::WeirdScaled {
                input: arg(self, "input"),
                noise: self.noise(&v["noise"]),
                rarity: if v["rarity_value_mapper"].as_str() == Some("type_1") {
                    Rarity::Type1
                } else {
                    Rarity::Type2
                },
            },
            "interpolated" => Node::Interpolated(arg(self, "argument")),
            "flat_cache" => Node::FlatCache(arg(self, "argument")),
            "cache_2d" | "cache_once" | "cache_all_in_cell" => Node::Cache(arg(self, "argument")),
            "blend_alpha" => Node::BlendAlpha,
            "blend_offset" => Node::BlendOffset,
            "blend_density" => Node::BlendDensity(arg(self, "argument")),
            "beardifier" => Node::Beardifier,
            "old_blended_noise" => {
                let mut r = if self.legacy {
                    Random::legacy(self.seed)
                } else {
                    self.random.from_hash_of("minecraft:terrain")
                };
                let b = BlendedNoise::new(
                    &mut r,
                    num(v, "xz_scale"),
                    num(v, "y_scale"),
                    num(v, "xz_factor"),
                    num(v, "y_factor"),
                    num(v, "smear_scale_multiplier"),
                );
                self.blended.push(b);
                Node::OldBlended((self.blended.len() - 1) as u32)
            }
            "end_islands" => {
                if self.end_islands.is_none() {
                    let mut r = Random::legacy(self.seed);
                    r.consume_count(17_292);
                    self.end_islands = Some(SimplexNoise::new(&mut r));
                }
                Node::EndIslands
            }
            "spline" => Node::Spline(Box::new(self.compile_spline(&v["spline"]))),
            other => panic!("unsupported density function type {other}"),
        };
        self.push(node)
    }

    fn compile_spline(&mut self, v: &Value) -> Spline {
        let coordinate = self.compile(&v["coordinate"]);
        let mut locations = Vec::new();
        let mut values = Vec::new();
        let mut derivatives = Vec::new();
        for p in v["points"].as_array().unwrap() {
            locations.push(p["location"].as_f64().unwrap() as f32);
            derivatives.push(p["derivative"].as_f64().unwrap() as f32);
            let val = &p["value"];
            values.push(if let Some(n) = val.as_f64() {
                SplineValue::Const(n as f32)
            } else {
                SplineValue::Spline(Box::new(self.compile_spline(val)))
            });
        }
        Spline {
            coordinate,
            locations,
            values,
            derivatives,
        }
    }
}

// ---- Evaluation --------------------------------------------------------------

/// Interpolation order for `interpolated` nodes in [`Mode::Cell`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LerpOrder {
    /// `Mth.lerp3` (x, then y, then z) — cell-cache fill.
    Lerp3,
    /// Running interpolator value (y, then x, then z).
    Sequential,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Direct,
    ChunkPoint,
    Cell(LerpOrder),
}

/// Per-chunk state for `flat_cache` and `interpolated` nodes.
pub struct ChunkCache {
    pub first_quart_x: i32,
    pub first_quart_z: i32,
    /// `[slot][qx * 5 + qz]`, NaN when not yet computed.
    flat: Vec<[f64; 25]>,
    /// Corner grids for interpolated nodes, `[slot][(cx * (ny + 1) + cy) * (nz + 1) + cz]`.
    pub grids: Vec<Vec<f64>>,
    pub cells_xz: i32,
    pub cells_y: i32,
    pub cell_width: i32,
    pub cell_height: i32,
    pub min_cell_y: i32,
    pub chunk_min_x: i32,
    pub chunk_min_z: i32,
    // Current cell & position inside it (Cell mode).
    pub cell_x: i32,
    pub cell_y: i32,
    pub cell_z: i32,
    pub in_x: i32,
    pub in_y: i32,
    pub in_z: i32,
}

impl ChunkCache {
    #[must_use]
    pub fn new(
        g: &Graph,
        chunk_min_x: i32,
        chunk_min_z: i32,
        cell_width: i32,
        cell_height: i32,
        min_y: i32,
        height: i32,
    ) -> Self {
        Self {
            first_quart_x: chunk_min_x >> 2,
            first_quart_z: chunk_min_z >> 2,
            flat: vec![[f64::NAN; 25]; g.flat_caches.len()],
            grids: vec![Vec::new(); g.interpolated.len()],
            cells_xz: 16 / cell_width,
            cells_y: height / cell_height,
            cell_width,
            cell_height,
            min_cell_y: mth::floor_div(min_y, cell_height),
            chunk_min_x,
            chunk_min_z,
            cell_x: 0,
            cell_y: 0,
            cell_z: 0,
            in_x: 0,
            in_y: 0,
            in_z: 0,
        }
    }

    /// Fill all interpolated corner grids.
    pub fn fill_grids(&mut self, g: &Graph) {
        let (nx, ny, nz) = (self.cells_xz + 1, self.cells_y + 1, self.cells_xz + 1);
        for (slot, &id) in g.interpolated.iter().enumerate() {
            let Node::Interpolated(inner) = g.nodes[id as usize] else {
                unreachable!()
            };
            let mut grid = Vec::with_capacity((nx * ny * nz) as usize);
            for cx in 0..nx {
                let x = self.chunk_min_x + cx * self.cell_width;
                for cy in 0..ny {
                    let y = (self.min_cell_y + cy) * self.cell_height;
                    for cz in 0..nz {
                        let z = self.chunk_min_z + cz * self.cell_width;
                        grid.push(g.eval(inner, x, y, z, Mode::ChunkPoint, Some(self)));
                    }
                }
            }
            self.grids[slot] = grid;
        }
    }

    #[inline]
    fn corner(&self, slot: usize, dx: i32, dy: i32, dz: i32) -> f64 {
        let ny = self.cells_y + 1;
        let nz = self.cells_xz + 1;
        let i = ((self.cell_x + dx) * ny + (self.cell_y + dy)) * nz + (self.cell_z + dz);
        self.grids[slot][i as usize]
    }

    #[inline]
    fn interpolate(&self, slot: usize, order: LerpOrder) -> f64 {
        let fx = f64::from(self.in_x) / f64::from(self.cell_width);
        let fy = f64::from(self.in_y) / f64::from(self.cell_height);
        let fz = f64::from(self.in_z) / f64::from(self.cell_width);
        let n000 = self.corner(slot, 0, 0, 0);
        let n100 = self.corner(slot, 1, 0, 0);
        let n010 = self.corner(slot, 0, 1, 0);
        let n110 = self.corner(slot, 1, 1, 0);
        let n001 = self.corner(slot, 0, 0, 1);
        let n101 = self.corner(slot, 1, 0, 1);
        let n011 = self.corner(slot, 0, 1, 1);
        let n111 = self.corner(slot, 1, 1, 1);
        match order {
            LerpOrder::Lerp3 => {
                mth::lerp3(fx, fy, fz, n000, n100, n010, n110, n001, n101, n011, n111)
            }
            LerpOrder::Sequential => {
                let xz00 = mth::lerp(fy, n000, n010);
                let xz10 = mth::lerp(fy, n100, n110);
                let xz01 = mth::lerp(fy, n001, n011);
                let xz11 = mth::lerp(fy, n101, n111);
                let z0 = mth::lerp(fx, xz00, xz10);
                let z1 = mth::lerp(fx, xz01, xz11);
                mth::lerp(fz, z0, z1)
            }
        }
    }
}

impl Graph {
    /// Evaluate node `id` at block position `(x, y, z)`.
    #[must_use]
    pub fn eval(&self, id: Id, x: i32, y: i32, z: i32, mode: Mode, cc: Option<&ChunkCache>) -> f64 {
        match &self.nodes[id as usize] {
            Node::Const(c) => *c,
            Node::Add(a, b) => self.eval(*a, x, y, z, mode, cc) + self.eval(*b, x, y, z, mode, cc),
            Node::Mul(a, b) => {
                let d = self.eval(*a, x, y, z, mode, cc);
                if d == 0.0 {
                    0.0
                } else {
                    d * self.eval(*b, x, y, z, mode, cc)
                }
            }
            Node::Min(a, b) => self
                .eval(*a, x, y, z, mode, cc)
                .min(self.eval(*b, x, y, z, mode, cc)),
            Node::Max(a, b) => self
                .eval(*a, x, y, z, mode, cc)
                .max(self.eval(*b, x, y, z, mode, cc)),
            Node::Abs(a) => self.eval(*a, x, y, z, mode, cc).abs(),
            Node::Square(a) => {
                let d = self.eval(*a, x, y, z, mode, cc);
                d * d
            }
            Node::Cube(a) => {
                let d = self.eval(*a, x, y, z, mode, cc);
                d * d * d
            }
            Node::HalfNegative(a) => {
                let d = self.eval(*a, x, y, z, mode, cc);
                if d > 0.0 { d } else { d * 0.5 }
            }
            Node::QuarterNegative(a) => {
                let d = self.eval(*a, x, y, z, mode, cc);
                if d > 0.0 { d } else { d * 0.25 }
            }
            Node::Squeeze(a) => {
                let e = mth::clamp(self.eval(*a, x, y, z, mode, cc), -1.0, 1.0);
                e / 2.0 - e * e * e / 24.0
            }
            Node::Clamp { input, min, max } => {
                mth::clamp(self.eval(*input, x, y, z, mode, cc), *min, *max)
            }
            Node::YClampedGradient {
                from_y,
                to_y,
                from_value,
                to_value,
            } => mth::clamped_map(
                f64::from(y),
                f64::from(*from_y),
                f64::from(*to_y),
                *from_value,
                *to_value,
            ),
            Node::RangeChoice {
                input,
                min,
                max,
                in_range,
                out_range,
            } => {
                let d = self.eval(*input, x, y, z, mode, cc);
                if d >= *min && d < *max {
                    self.eval(*in_range, x, y, z, mode, cc)
                } else {
                    self.eval(*out_range, x, y, z, mode, cc)
                }
            }
            Node::Noise {
                noise,
                xz_scale,
                y_scale,
            } => self.noises[*noise as usize].get_value(
                f64::from(x) * xz_scale,
                f64::from(y) * y_scale,
                f64::from(z) * xz_scale,
            ),
            Node::ShiftedNoise {
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => {
                let d = f64::from(x) * xz_scale + self.eval(*shift_x, x, y, z, mode, cc);
                let e = f64::from(y) * y_scale + self.eval(*shift_y, x, y, z, mode, cc);
                let f = f64::from(z) * xz_scale + self.eval(*shift_z, x, y, z, mode, cc);
                self.noises[*noise as usize].get_value(d, e, f)
            }
            Node::ShiftA(n) => {
                self.noises[*n as usize].get_value(f64::from(x) * 0.25, 0.0, f64::from(z) * 0.25)
                    * 4.0
            }
            Node::ShiftB(n) => {
                self.noises[*n as usize].get_value(f64::from(z) * 0.25, f64::from(x) * 0.25, 0.0)
                    * 4.0
            }
            Node::Shift(n) => {
                self.noises[*n as usize].get_value(
                    f64::from(x) * 0.25,
                    f64::from(y) * 0.25,
                    f64::from(z) * 0.25,
                ) * 4.0
            }
            Node::WeirdScaled {
                input,
                noise,
                rarity,
            } => {
                let d = self.eval(*input, x, y, z, mode, cc);
                let e = match rarity {
                    Rarity::Type1 => {
                        if d < -0.5 {
                            0.75
                        } else if d < 0.0 {
                            1.0
                        } else if d < 0.5 {
                            1.5
                        } else {
                            2.0
                        }
                    }
                    Rarity::Type2 => {
                        if d < -0.75 {
                            0.5
                        } else if d < -0.5 {
                            0.75
                        } else if d < 0.5 {
                            1.0
                        } else if d < 0.75 {
                            2.0
                        } else {
                            3.0
                        }
                    }
                };
                e * self.noises[*noise as usize]
                    .get_value(f64::from(x) / e, f64::from(y) / e, f64::from(z) / e)
                    .abs()
            }
            Node::Interpolated(inner) => match (mode, cc) {
                (Mode::Cell(order), Some(c)) => {
                    c.interpolate(self.slot[id as usize] as usize, order)
                }
                _ => self.eval(*inner, x, y, z, mode, cc),
            },
            Node::FlatCache(inner) => match (mode, cc) {
                (Mode::ChunkPoint | Mode::Cell(_), Some(c)) => {
                    let qx = (x >> 2) - c.first_quart_x;
                    let qz = (z >> 2) - c.first_quart_z;
                    if (0..5).contains(&qx) && (0..5).contains(&qz) {
                        let bx = (c.first_quart_x + qx) << 2;
                        let bz = (c.first_quart_z + qz) << 2;
                        let slot = self.slot[id as usize] as usize;
                        let cached = c.flat[slot][(qx * 5 + qz) as usize];
                        if cached.is_nan() {
                            self.eval(*inner, bx, 0, bz, Mode::ChunkPoint, cc)
                        } else {
                            cached
                        }
                    } else {
                        self.eval(*inner, x, y, z, Mode::ChunkPoint, cc)
                    }
                }
                _ => self.eval(*inner, x, y, z, mode, cc),
            },
            Node::Cache(inner) => self.eval(*inner, x, y, z, mode, cc),
            Node::BlendAlpha => 1.0,
            Node::BlendOffset | Node::Beardifier => 0.0,
            Node::BlendDensity(inner) => self.eval(*inner, x, y, z, mode, cc),
            Node::OldBlended(b) => self.blended[*b as usize].compute(x, y, z),
            Node::EndIslands => end_islands(self.end_islands.as_ref().unwrap(), x, z),
            Node::Spline(s) => f64::from(self.spline(s, x, y, z, mode, cc)),
        }
    }

    fn spline(
        &self,
        s: &Spline,
        x: i32,
        y: i32,
        z: i32,
        mode: Mode,
        cc: Option<&ChunkCache>,
    ) -> f32 {
        let f = self.eval(s.coordinate, x, y, z, mode, cc) as f32;
        // Mth.binarySearch(0, n, i -> f < loc[i]) - 1
        let n = s.locations.len();
        let i = s.locations.partition_point(|&l| !(f < l)) as isize - 1;
        let last = n - 1;
        let val = |k: usize| -> f32 {
            match &s.values[k] {
                SplineValue::Const(c) => *c,
                SplineValue::Spline(sp) => self.spline(sp, x, y, z, mode, cc),
            }
        };
        let linear_extend = |value: f32, k: usize| -> f32 {
            let g = s.derivatives[k];
            if g == 0.0 {
                value
            } else {
                value + g * (f - s.locations[k])
            }
        };
        if i < 0 {
            return linear_extend(val(0), 0);
        }
        let i = i as usize;
        if i == last {
            return linear_extend(val(last), last);
        }
        let g = s.locations[i];
        let h = s.locations[i + 1];
        let k = (f - g) / (h - g);
        let l = s.derivatives[i];
        let m = s.derivatives[i + 1];
        let nv = val(i);
        let o = val(i + 1);
        let p = l * (h - g) - (o - nv);
        let q = -m * (h - g) + (o - nv);
        mth::lerp_f(k, nv, o) + k * (1.0 - k) * mth::lerp_f(k, p, q)
    }

    /// Precompute a chunk's flat-cache grids (vanilla fills them eagerly).
    pub fn fill_flat(&self, cc: &mut ChunkCache) {
        for (slot, &id) in self.flat_caches.iter().enumerate() {
            let Node::FlatCache(inner) = self.nodes[id as usize] else {
                unreachable!()
            };
            let mut vals = [0.0; 25];
            for qx in 0..5 {
                for qz in 0..5 {
                    let bx = (cc.first_quart_x + qx) << 2;
                    let bz = (cc.first_quart_z + qz) << 2;
                    vals[(qx * 5 + qz) as usize] =
                        self.eval(inner, bx, 0, bz, Mode::ChunkPoint, Some(cc));
                }
            }
            cc.flat[slot] = vals;
        }
    }
}

/// `EndIslandDensityFunction`.
fn end_islands(noise: &SimplexNoise, block_x: i32, block_z: i32) -> f64 {
    let x = block_x / 8;
    let z = block_z / 8;
    (f64::from(end_height(noise, x, z)) - 8.0) / 128.0
}

/// `EndIslandDensityFunction.getHeightValue`.
#[must_use]
pub fn end_height(noise: &SimplexNoise, x: i32, z: i32) -> f32 {
    let i = x / 2;
    let j = z / 2;
    let k = x % 2;
    let l = z % 2;
    let mut f = 100.0 - mth::sqrt_f(x.wrapping_mul(x).wrapping_add(z.wrapping_mul(z)) as f32) * 8.0;
    f = mth::clamp_f(f, -100.0, 80.0);
    for m in -12..=12 {
        for n in -12..=12 {
            let o = i64::from(i + m);
            let p = i64::from(j + n);
            if o * o + p * p > 4096 && noise.get_value(o as f64, p as f64) < f64::from(-0.9f32) {
                let g = ((o as f32).abs() * 3439.0 + (p as f32).abs() * 147.0) % 13.0 + 9.0;
                let h = (k - m * 2) as f32;
                let q = (l - n * 2) as f32;
                let mut r = 100.0 - mth::sqrt_f(h * h + q * q) * g;
                r = mth::clamp_f(r, -100.0, 80.0);
                f = f.max(r);
            }
        }
    }
    f
}
