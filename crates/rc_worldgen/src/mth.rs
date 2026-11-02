//! Math helpers with vanilla's exact semantics (rounding, lookup-table trig,
//! operation order). Every function here is used on a seed-exact path, so
//! "simplifying" any of them changes world generation.

use std::sync::OnceLock;

/// `Mth.floor(double)`: floor to i32 with Java's saturating cast.
#[inline]
#[must_use]
pub fn floor(d: f64) -> i32 {
    let i = d as i32;
    if d < f64::from(i) {
        i.wrapping_sub(1)
    } else {
        i
    }
}

/// `Mth.lfloor(double)`.
#[inline]
#[must_use]
pub fn lfloor(d: f64) -> i64 {
    let i = d as i64;
    if d < i as f64 { i - 1 } else { i }
}

/// `Mth.ceil(double)`.
#[inline]
#[must_use]
pub fn ceil(d: f64) -> i32 {
    let i = d as i32;
    if d > f64::from(i) { i + 1 } else { i }
}

#[inline]
#[must_use]
pub fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + t * (b - a)
}

#[inline]
#[must_use]
pub fn lerp_f(t: f32, a: f32, b: f32) -> f32 {
    a + t * (b - a)
}

#[inline]
#[must_use]
pub fn lerp2(dx: f64, dy: f64, a: f64, b: f64, c: f64, d: f64) -> f64 {
    lerp(dy, lerp(dx, a, b), lerp(dx, c, d))
}

#[inline]
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn lerp3(
    dx: f64,
    dy: f64,
    dz: f64,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
    g: f64,
    h: f64,
) -> f64 {
    lerp(dz, lerp2(dx, dy, a, b, c, d), lerp2(dx, dy, e, f, g, h))
}

#[inline]
#[must_use]
pub fn inverse_lerp(d: f64, a: f64, b: f64) -> f64 {
    (d - a) / (b - a)
}

#[inline]
#[must_use]
pub fn clamped_lerp(a: f64, b: f64, t: f64) -> f64 {
    if t < 0.0 {
        a
    } else if t > 1.0 {
        b
    } else {
        lerp(t, a, b)
    }
}

/// `Mth.map`.
#[inline]
#[must_use]
pub fn map(d: f64, a: f64, b: f64, c: f64, e: f64) -> f64 {
    lerp(inverse_lerp(d, a, b), c, e)
}

/// `Mth.clampedMap`.
#[inline]
#[must_use]
pub fn clamped_map(d: f64, a: f64, b: f64, c: f64, e: f64) -> f64 {
    clamped_lerp(c, e, inverse_lerp(d, a, b))
}

#[inline]
#[must_use]
pub fn clamp(d: f64, lo: f64, hi: f64) -> f64 {
    if d < lo {
        lo
    } else if d > hi {
        hi
    } else {
        d
    }
}

#[inline]
#[must_use]
pub fn clamp_f(d: f32, lo: f32, hi: f32) -> f32 {
    if d < lo {
        lo
    } else if d > hi {
        hi
    } else {
        d
    }
}

#[inline]
#[must_use]
pub fn clamp_i(d: i32, lo: i32, hi: i32) -> i32 {
    if d < lo {
        lo
    } else if d > hi {
        hi
    } else {
        d
    }
}

/// Perlin fade curve `6t^5 - 15t^4 + 10t^3`.
#[inline]
#[must_use]
pub fn smoothstep(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// `Mth.getSeed(x, y, z)` — positional seed hash.
#[inline]
#[must_use]
pub fn get_seed(x: i32, y: i32, z: i32) -> i64 {
    let mut l = i64::from(x.wrapping_mul(3_129_871))
        ^ (i64::from(z)).wrapping_mul(116_129_781)
        ^ i64::from(y);
    l = l
        .wrapping_mul(l)
        .wrapping_mul(42_317_861)
        .wrapping_add(l.wrapping_mul(11));
    l >> 16
}

/// `Mth.quantize(d, step)`.
#[inline]
#[must_use]
pub fn quantize(d: f64, step: i32) -> i32 {
    floor(d / f64::from(step)) * step
}

fn sin_table() -> &'static [f32; 65536] {
    static T: OnceLock<Box<[f32; 65536]>> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = Box::new([0f32; 65536]);
        for (i, v) in t.iter_mut().enumerate() {
            *v = (i as f64 * std::f64::consts::PI * 2.0 / 65536.0).sin() as f32;
        }
        t
    })
}

/// `Mth.sin(float)` — 65536-entry lookup table.
#[inline]
#[must_use]
pub fn sin(f: f32) -> f32 {
    sin_table()[((f * 10430.378) as i32 & 65535) as usize]
}

/// `Mth.cos(float)`.
#[inline]
#[must_use]
pub fn cos(f: f32) -> f32 {
    sin_table()[((f * 10430.378 + 16384.0) as i32 & 65535) as usize]
}

/// `Mth.sqrt(float)`.
#[inline]
#[must_use]
pub fn sqrt_f(f: f32) -> f32 {
    f.sqrt()
}

/// Java `Math.round(double)` → long.
#[inline]
#[must_use]
pub fn round(d: f64) -> i64 {
    lfloor(d + 0.5)
}

/// Java `floorDiv` for i32.
#[inline]
#[must_use]
pub fn floor_div(a: i32, b: i32) -> i32 {
    a.div_euclid(b) - i32::from(b < 0 && a.rem_euclid(b) != 0)
}

/// Java `floorMod` for i64.
#[inline]
#[must_use]
pub fn floor_mod_l(a: i64, b: i64) -> i64 {
    ((a % b) + b) % b
}

/// `Mth.square`.
#[inline]
#[must_use]
pub fn square(d: f64) -> f64 {
    d * d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_matches_java() {
        assert_eq!(floor(-0.5), -1);
        assert_eq!(floor(-1.0), -1);
        assert_eq!(floor(2.9), 2);
        assert_eq!(floor_div(-1, 16), -1);
        assert_eq!(floor_div(-16, 16), -1);
        assert_eq!(floor_div(-17, 16), -2);
        assert_eq!(floor_div(15, 16), 0);
    }

    #[test]
    fn trig_table() {
        assert_eq!(sin(0.0), 0.0);
        assert_eq!(sin(std::f32::consts::FRAC_PI_2), 1.0);
        assert!((cos(0.0) - 1.0).abs() < 1e-6);
    }
}
