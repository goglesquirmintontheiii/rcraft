//! Int / float / height providers (value samplers used by carvers and
//! features), with vanilla's random-draw order.

use serde_json::Value;

use crate::rng::Random;
use crate::surface::Anchor;

fn f(v: &Value, k: &str) -> f32 {
    v[k].as_f64()
        .unwrap_or_else(|| panic!("missing {k} in {v}")) as f32
}

fn i(v: &Value, k: &str) -> i32 {
    v[k].as_i64()
        .unwrap_or_else(|| panic!("missing {k} in {v}")) as i32
}

/// `Mth.normal`.
#[inline]
fn normal(r: &mut Random, mean: f32, dev: f32) -> f32 {
    mean + r.next_gaussian() as f32 * dev
}

/// `Mth.nextInt(random, a, b)`.
#[inline]
fn mth_next_int(r: &mut Random, a: i32, b: i32) -> i32 {
    if a >= b {
        a
    } else {
        r.next_int_bound(b - a + 1) + a
    }
}

#[derive(Clone, Debug)]
pub enum FloatProvider {
    Constant(f32),
    Uniform {
        min: f32,
        max: f32,
    },
    ClampedNormal {
        mean: f32,
        deviation: f32,
        min: f32,
        max: f32,
    },
    Trapezoid {
        min: f32,
        max: f32,
        plateau: f32,
    },
}

impl FloatProvider {
    #[must_use]
    pub fn from_json(v: &Value) -> Self {
        if let Some(n) = v.as_f64() {
            return Self::Constant(n as f32);
        }
        match v["type"].as_str().unwrap().trim_start_matches("minecraft:") {
            "constant" => Self::Constant(f(v, "value")),
            "uniform" => Self::Uniform {
                min: f(v, "min_inclusive"),
                max: f(v, "max_exclusive"),
            },
            "clamped_normal" => Self::ClampedNormal {
                mean: f(v, "mean"),
                deviation: f(v, "deviation"),
                min: f(v, "min"),
                max: f(v, "max"),
            },
            "trapezoid" => Self::Trapezoid {
                min: f(v, "min"),
                max: f(v, "max"),
                plateau: f(v, "plateau"),
            },
            t => panic!("unknown float provider {t}"),
        }
    }

    pub fn sample(&self, r: &mut Random) -> f32 {
        match *self {
            Self::Constant(c) => c,
            Self::Uniform { min, max } => r.next_float() * (max - min) + min,
            Self::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            } => normal(r, mean, deviation).clamp(min, max),
            Self::Trapezoid { min, max, plateau } => {
                let ff = max - min;
                let g = (ff - plateau) / 2.0;
                let h = ff - g;
                min + r.next_float() * h + r.next_float() * g
            }
        }
    }
}

#[derive(Clone, Debug)]
pub enum IntProvider {
    Constant(i32),
    Uniform {
        min: i32,
        max: i32,
    },
    BiasedToBottom {
        min: i32,
        max: i32,
    },
    Clamped {
        source: Box<IntProvider>,
        min: i32,
        max: i32,
    },
    ClampedNormal {
        mean: f32,
        deviation: f32,
        min: i32,
        max: i32,
    },
    Weighted(Vec<(IntProvider, i32)>),
}

impl IntProvider {
    #[must_use]
    pub fn from_json(v: &Value) -> Self {
        if let Some(n) = v.as_i64() {
            return Self::Constant(n as i32);
        }
        match v["type"].as_str().unwrap().trim_start_matches("minecraft:") {
            "constant" => Self::Constant(i(v, "value")),
            "uniform" => Self::Uniform {
                min: i(v, "min_inclusive"),
                max: i(v, "max_inclusive"),
            },
            "biased_to_bottom" => Self::BiasedToBottom {
                min: i(v, "min_inclusive"),
                max: i(v, "max_inclusive"),
            },
            "clamped" => Self::Clamped {
                source: Box::new(Self::from_json(&v["source"])),
                min: i(v, "min_inclusive"),
                max: i(v, "max_inclusive"),
            },
            "clamped_normal" => Self::ClampedNormal {
                mean: f(v, "mean"),
                deviation: f(v, "deviation"),
                min: i(v, "min_inclusive"),
                max: i(v, "max_inclusive"),
            },
            "weighted_list" => Self::Weighted(
                v["distribution"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| (Self::from_json(&e["data"]), i(e, "weight")))
                    .collect(),
            ),
            t => panic!("unknown int provider {t}"),
        }
    }

    pub fn sample(&self, r: &mut Random) -> i32 {
        match self {
            Self::Constant(c) => *c,
            Self::Uniform { min, max } => r.next_int_between(*min, *max),
            Self::BiasedToBottom { min, max } => {
                let inner = r.next_int_bound(max - min + 1) + 1;
                min + r.next_int_bound(inner)
            }
            Self::Clamped { source, min, max } => source.sample(r).clamp(*min, *max),
            Self::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            } => {
                let d = f64::from(normal(r, *mean, *deviation));
                d.clamp(f64::from(*min), f64::from(*max)) as i32
            }
            Self::Weighted(list) => {
                let total: i32 = list.iter().map(|e| e.1).sum();
                let mut k = r.next_int_bound(total);
                for (p, w) in list {
                    k -= w;
                    if k < 0 {
                        return p.sample(r);
                    }
                }
                unreachable!()
            }
        }
    }

    #[must_use]
    pub fn max_value(&self) -> i32 {
        match self {
            Self::Constant(c) => *c,
            Self::Uniform { max, .. }
            | Self::BiasedToBottom { max, .. }
            | Self::Clamped { max, .. } => *max,
            Self::ClampedNormal { max, .. } => *max,
            Self::Weighted(l) => l.iter().map(|e| e.0.max_value()).max().unwrap_or(0),
        }
    }

    #[must_use]
    pub fn min_value(&self) -> i32 {
        match self {
            Self::Constant(c) => *c,
            Self::Uniform { min, .. }
            | Self::BiasedToBottom { min, .. }
            | Self::Clamped { min, .. } => *min,
            Self::ClampedNormal { min, .. } => *min,
            Self::Weighted(l) => l.iter().map(|e| e.0.min_value()).min().unwrap_or(0),
        }
    }
}

#[derive(Clone, Debug)]
pub enum HeightProvider {
    Constant(Anchor),
    Uniform {
        min: Anchor,
        max: Anchor,
    },
    BiasedToBottom {
        min: Anchor,
        max: Anchor,
        inner: i32,
    },
    VeryBiasedToBottom {
        min: Anchor,
        max: Anchor,
        inner: i32,
    },
    Trapezoid {
        min: Anchor,
        max: Anchor,
        plateau: i32,
    },
    Weighted(Vec<(HeightProvider, i32)>),
}

impl HeightProvider {
    #[must_use]
    pub fn from_json(v: &Value) -> Self {
        let Some(t) = v.get("type").and_then(Value::as_str) else {
            return Self::Constant(Anchor::from_json(v));
        };
        let a = |k: &str| Anchor::from_json(&v[k]);
        match t.trim_start_matches("minecraft:") {
            "constant" => Self::Constant(a("value")),
            "uniform" => Self::Uniform {
                min: a("min_inclusive"),
                max: a("max_inclusive"),
            },
            "biased_to_bottom" => Self::BiasedToBottom {
                min: a("min_inclusive"),
                max: a("max_inclusive"),
                inner: v["inner"].as_i64().unwrap_or(1) as i32,
            },
            "very_biased_to_bottom" => Self::VeryBiasedToBottom {
                min: a("min_inclusive"),
                max: a("max_inclusive"),
                inner: v["inner"].as_i64().unwrap_or(1) as i32,
            },
            "trapezoid" => Self::Trapezoid {
                min: a("min_inclusive"),
                max: a("max_inclusive"),
                plateau: v["plateau"].as_i64().unwrap_or(0) as i32,
            },
            "weighted_list" => Self::Weighted(
                v["distribution"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| (Self::from_json(&e["data"]), i(e, "weight")))
                    .collect(),
            ),
            t => panic!("unknown height provider {t}"),
        }
    }

    pub fn sample(&self, r: &mut Random, min_y: i32, height: i32) -> i32 {
        let res = |a: &Anchor| a.resolve(min_y, height);
        match self {
            Self::Constant(a) => res(a),
            Self::Uniform { min, max } => {
                let (i, j) = (res(min), res(max));
                if i > j { i } else { r.next_int_between(i, j) }
            }
            Self::BiasedToBottom { min, max, inner } => {
                let (i, j) = (res(min), res(max));
                if j - i - inner + 1 <= 0 {
                    return i;
                }
                let k = r.next_int_bound(j - i - inner + 1);
                r.next_int_bound(k + inner) + i
            }
            Self::VeryBiasedToBottom { min, max, inner } => {
                let (i, j) = (res(min), res(max));
                if j - i - inner + 1 <= 0 {
                    return i;
                }
                let k = mth_next_int(r, i + inner, j);
                let l = mth_next_int(r, i, k - 1);
                mth_next_int(r, i, l - 1 + inner)
            }
            Self::Trapezoid { min, max, plateau } => {
                let (i, j) = (res(min), res(max));
                if i > j {
                    return i;
                }
                let k = j - i;
                if *plateau >= k {
                    return r.next_int_between(i, j);
                }
                let l = (k - plateau) / 2;
                let m = k - l;
                i + r.next_int_between(0, m) + r.next_int_between(0, l)
            }
            Self::Weighted(list) => {
                let total: i32 = list.iter().map(|e| e.1).sum();
                let mut k = r.next_int_bound(total);
                for (p, w) in list {
                    k -= w;
                    if k < 0 {
                        return p.sample(r, min_y, height);
                    }
                }
                unreachable!()
            }
        }
    }
}
