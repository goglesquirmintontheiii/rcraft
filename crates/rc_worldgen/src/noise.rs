//! Noise primitives: improved Perlin, octave Perlin, "normal" (double
//! Perlin) noise, blended noise, simplex and octave simplex — all with the
//! vanilla construction order of random draws and evaluation order.

use crate::mth::{self, floor, lfloor, smoothstep};
use crate::rng::{Positional, Random};

const GRADIENT: [[i32; 3]; 16] = [
    [1, 1, 0],
    [-1, 1, 0],
    [1, -1, 0],
    [-1, -1, 0],
    [1, 0, 1],
    [-1, 0, 1],
    [1, 0, -1],
    [-1, 0, -1],
    [0, 1, 1],
    [0, -1, 1],
    [0, 1, -1],
    [0, -1, -1],
    [1, 1, 0],
    [0, -1, 1],
    [-1, 1, 0],
    [0, -1, -1],
];

#[inline]
fn dot(g: [i32; 3], x: f64, y: f64, z: f64) -> f64 {
    f64::from(g[0]) * x + f64::from(g[1]) * y + f64::from(g[2]) * z
}

#[inline]
fn grad_dot(hash: i32, x: f64, y: f64, z: f64) -> f64 {
    dot(GRADIENT[(hash & 15) as usize], x, y, z)
}

/// Single-octave improved Perlin noise (`ImprovedNoise`).
#[derive(Clone, Debug)]
pub struct ImprovedNoise {
    p: [u8; 256],
    pub xo: f64,
    pub yo: f64,
    pub zo: f64,
}

impl ImprovedNoise {
    #[must_use]
    pub fn new(r: &mut Random) -> Self {
        let xo = r.next_double() * 256.0;
        let yo = r.next_double() * 256.0;
        let zo = r.next_double() * 256.0;
        let mut p = [0u8; 256];
        for (i, v) in p.iter_mut().enumerate() {
            *v = i as u8;
        }
        for i in 0..256 {
            let j = r.next_int_bound(256 - i as i32) as usize;
            p.swap(i, i + j);
        }
        Self { p, xo, yo, zo }
    }

    #[inline]
    fn p(&self, i: i32) -> i32 {
        i32::from(self.p[(i & 255) as usize])
    }

    #[inline]
    #[must_use]
    pub fn noise(&self, x: f64, y: f64, z: f64) -> f64 {
        self.noise_scaled(x, y, z, 0.0, 0.0)
    }

    /// `noise(x, y, z, yScale, yMax)`.
    #[must_use]
    pub fn noise_scaled(&self, x: f64, y: f64, z: f64, y_scale: f64, y_max: f64) -> f64 {
        let i = x + self.xo;
        let j = y + self.yo;
        let k = z + self.zo;
        let l = floor(i);
        let m = floor(j);
        let n = floor(k);
        let o = i - f64::from(l);
        let p = j - f64::from(m);
        let q = k - f64::from(n);
        let t = if y_scale != 0.0 {
            let r = if y_max >= 0.0 && y_max < p { y_max } else { p };
            f64::from(floor(r / y_scale + f64::from(1.0e-7f32))) * y_scale
        } else {
            0.0
        };
        self.sample_and_lerp(l, m, n, o, p - t, q, p)
    }

    #[allow(clippy::too_many_arguments)]
    fn sample_and_lerp(
        &self,
        gx: i32,
        gy: i32,
        gz: i32,
        dx: f64,
        wdy: f64,
        dz: f64,
        dy: f64,
    ) -> f64 {
        let i = self.p(gx);
        let j = self.p(gx.wrapping_add(1));
        let k = self.p(i.wrapping_add(gy));
        let l = self.p(i.wrapping_add(gy).wrapping_add(1));
        let m = self.p(j.wrapping_add(gy));
        let n = self.p(j.wrapping_add(gy).wrapping_add(1));
        let d = grad_dot(self.p(k.wrapping_add(gz)), dx, wdy, dz);
        let e = grad_dot(self.p(m.wrapping_add(gz)), dx - 1.0, wdy, dz);
        let f = grad_dot(self.p(l.wrapping_add(gz)), dx, wdy - 1.0, dz);
        let g = grad_dot(self.p(n.wrapping_add(gz)), dx - 1.0, wdy - 1.0, dz);
        let h = grad_dot(
            self.p(k.wrapping_add(gz).wrapping_add(1)),
            dx,
            wdy,
            dz - 1.0,
        );
        let o = grad_dot(
            self.p(m.wrapping_add(gz).wrapping_add(1)),
            dx - 1.0,
            wdy,
            dz - 1.0,
        );
        let q = grad_dot(
            self.p(l.wrapping_add(gz).wrapping_add(1)),
            dx,
            wdy - 1.0,
            dz - 1.0,
        );
        let r = grad_dot(
            self.p(n.wrapping_add(gz).wrapping_add(1)),
            dx - 1.0,
            wdy - 1.0,
            dz - 1.0,
        );
        let s = smoothstep(dx);
        let t = smoothstep(dy);
        let u = smoothstep(dz);
        mth::lerp3(s, t, u, d, e, f, g, h, o, q, r)
    }
}

/// `PerlinNoise.wrap`.
#[inline]
#[must_use]
pub fn wrap(d: f64) -> f64 {
    d - (lfloor(d / 3.355_443_2e7 + 0.5) as f64) * 3.355_443_2e7
}

/// Octave Perlin noise (`PerlinNoise`).
#[derive(Clone, Debug)]
pub struct PerlinNoise {
    levels: Vec<Option<ImprovedNoise>>,
    amplitudes: Vec<f64>,
    lowest_freq_input_factor: f64,
    lowest_freq_value_factor: f64,
}

impl PerlinNoise {
    /// Modern (positional) initialisation, `PerlinNoise.create`.
    #[must_use]
    pub fn new_modern(r: &mut Random, first_octave: i32, amplitudes: &[f64]) -> Self {
        let factory = r.fork_positional();
        let levels = amplitudes
            .iter()
            .enumerate()
            .map(|(k, &a)| {
                (a != 0.0).then(|| {
                    let mut rr =
                        factory.from_hash_of(&format!("octave_{}", first_octave + k as i32));
                    ImprovedNoise::new(&mut rr)
                })
            })
            .collect();
        Self::finish(levels, first_octave, amplitudes)
    }

    /// Legacy initialisation (octave 0 first, then descending), used by
    /// blended noise and the legacy nether biome noise.
    #[must_use]
    pub fn new_legacy(r: &mut Random, first_octave: i32, amplitudes: &[f64]) -> Self {
        let n = amplitudes.len();
        let j = -first_octave;
        let mut levels: Vec<Option<ImprovedNoise>> = vec![None; n];
        let base = ImprovedNoise::new(r);
        if j >= 0 && (j as usize) < n && amplitudes[j as usize] != 0.0 {
            levels[j as usize] = Some(base);
        }
        let mut k = j - 1;
        while k >= 0 {
            if (k as usize) < n {
                if amplitudes[k as usize] != 0.0 {
                    levels[k as usize] = Some(ImprovedNoise::new(r));
                } else {
                    r.consume_count(262);
                }
            } else {
                r.consume_count(262);
            }
            k -= 1;
        }
        Self::finish(levels, first_octave, amplitudes)
    }

    fn finish(levels: Vec<Option<ImprovedNoise>>, first_octave: i32, amplitudes: &[f64]) -> Self {
        let n = amplitudes.len() as i32;
        Self {
            levels,
            amplitudes: amplitudes.to_vec(),
            lowest_freq_input_factor: 2f64.powi(first_octave),
            lowest_freq_value_factor: 2f64.powi(n - 1) / (2f64.powi(n) - 1.0),
        }
    }

    #[must_use]
    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        let mut d = 0.0;
        let mut e = self.lowest_freq_input_factor;
        let mut f = self.lowest_freq_value_factor;
        for (i, lvl) in self.levels.iter().enumerate() {
            if let Some(n) = lvl {
                let g = n.noise_scaled(wrap(x * e), wrap(y * e), wrap(z * e), 0.0, 0.0);
                d += self.amplitudes[i] * g * f;
            }
            e *= 2.0;
            f /= 2.0;
        }
        d
    }

    /// Value with `useFixedY` (y taken as `-yo` of each octave), as used by the
    /// surface noises' 2-D samples in older code paths.
    #[must_use]
    pub fn get_value_full(
        &self,
        x: f64,
        y: f64,
        z: f64,
        y_scale: f64,
        y_max: f64,
        fixed_y: bool,
    ) -> f64 {
        let mut d = 0.0;
        let mut e = self.lowest_freq_input_factor;
        let mut f = self.lowest_freq_value_factor;
        for (i, lvl) in self.levels.iter().enumerate() {
            if let Some(n) = lvl {
                let yy = if fixed_y { -n.yo } else { wrap(y * e) };
                let g = n.noise_scaled(wrap(x * e), yy, wrap(z * e), y_scale * e, y_max * e);
                d += self.amplitudes[i] * g * f;
            }
            e *= 2.0;
            f /= 2.0;
        }
        d
    }

    /// `getOctaveNoise(i)`: index 0 is the highest-frequency octave.
    #[must_use]
    pub fn octave(&self, i: usize) -> Option<&ImprovedNoise> {
        let n = self.levels.len();
        if i >= n {
            None
        } else {
            self.levels[n - 1 - i].as_ref()
        }
    }

    fn edge_value(&self, d: f64) -> f64 {
        let mut e = 0.0;
        let mut f = self.lowest_freq_value_factor;
        for (i, lvl) in self.levels.iter().enumerate() {
            if lvl.is_some() {
                e += self.amplitudes[i] * d * f;
            }
            f /= 2.0;
        }
        e
    }

    #[must_use]
    pub fn max_value(&self) -> f64 {
        self.edge_value(2.0)
    }
}

/// Noise parameters from `worldgen/noise/*.json`.
#[derive(Clone, Debug)]
pub struct NoiseParams {
    pub first_octave: i32,
    pub amplitudes: Vec<f64>,
}

impl NoiseParams {
    #[must_use]
    pub fn from_json(v: &serde_json::Value) -> Self {
        Self {
            first_octave: v["firstOctave"].as_i64().unwrap_or(0) as i32,
            amplitudes: v["amplitudes"]
                .as_array()
                .map(|a| a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect())
                .unwrap_or_default(),
        }
    }
}

/// `NormalNoise` — two offset octave noises summed.
#[derive(Clone, Debug)]
pub struct NormalNoise {
    first: PerlinNoise,
    second: PerlinNoise,
    value_factor: f64,
    max_value: f64,
}

const INPUT_FACTOR: f64 = 1.018_126_888_217_522_7;

impl NormalNoise {
    #[must_use]
    pub fn new(r: &mut Random, p: &NoiseParams) -> Self {
        let first = PerlinNoise::new_modern(r, p.first_octave, &p.amplitudes);
        let second = PerlinNoise::new_modern(r, p.first_octave, &p.amplitudes);
        Self::finish(first, second, &p.amplitudes)
    }

    #[must_use]
    pub fn new_legacy_nether_biome(r: &mut Random, p: &NoiseParams) -> Self {
        let first = PerlinNoise::new_legacy(r, p.first_octave, &p.amplitudes);
        let second = PerlinNoise::new_legacy(r, p.first_octave, &p.amplitudes);
        Self::finish(first, second, &p.amplitudes)
    }

    fn finish(first: PerlinNoise, second: PerlinNoise, amps: &[f64]) -> Self {
        let mut lo = i32::MAX;
        let mut hi = i32::MIN;
        for (i, &a) in amps.iter().enumerate() {
            if a != 0.0 {
                lo = lo.min(i as i32);
                hi = hi.max(i as i32);
            }
        }
        // Java int arithmetic: wraps for the all-zero legacy offset noise.
        let expected_deviation = 0.1 * (1.0 + 1.0 / f64::from(hi.wrapping_sub(lo).wrapping_add(1)));
        let value_factor = 0.166_666_666_666_666_66 / expected_deviation;
        let max_value = (first.max_value() + second.max_value()) * value_factor;
        Self {
            first,
            second,
            value_factor,
            max_value,
        }
    }

    #[inline]
    #[must_use]
    pub fn get_value(&self, x: f64, y: f64, z: f64) -> f64 {
        let d = x * INPUT_FACTOR;
        let e = y * INPUT_FACTOR;
        let f = z * INPUT_FACTOR;
        (self.first.get_value(x, y, z) + self.second.get_value(d, e, f)) * self.value_factor
    }

    #[must_use]
    pub fn max_value(&self) -> f64 {
        self.max_value
    }
}

/// `BlendedNoise` (`minecraft:old_blended_noise`).
#[derive(Clone, Debug)]
pub struct BlendedNoise {
    min_limit: PerlinNoise,
    max_limit: PerlinNoise,
    main: PerlinNoise,
    xz_multiplier: f64,
    y_multiplier: f64,
    xz_factor: f64,
    y_factor: f64,
    smear_scale_multiplier: f64,
}

impl BlendedNoise {
    #[must_use]
    pub fn new(
        r: &mut Random,
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear: f64,
    ) -> Self {
        let amps16 = [1.0; 16];
        let amps8 = [1.0; 8];
        let min_limit = PerlinNoise::new_legacy(r, -15, &amps16);
        let max_limit = PerlinNoise::new_legacy(r, -15, &amps16);
        let main = PerlinNoise::new_legacy(r, -7, &amps8);
        Self {
            min_limit,
            max_limit,
            main,
            xz_multiplier: 684.412 * xz_scale,
            y_multiplier: 684.412 * y_scale,
            xz_factor,
            y_factor,
            smear_scale_multiplier: smear,
        }
    }

    #[must_use]
    pub fn compute(&self, x: i32, y: i32, z: i32) -> f64 {
        let d = f64::from(x) * self.xz_multiplier;
        let e = f64::from(y) * self.y_multiplier;
        let f = f64::from(z) * self.xz_multiplier;
        let g = d / self.xz_factor;
        let h = e / self.y_factor;
        let l = f / self.xz_factor;
        let m = self.y_multiplier * self.smear_scale_multiplier;
        let n = m / self.y_factor;
        let mut o = 0.0;
        let mut p = 0.0;
        let mut q = 0.0;
        let mut r = 1.0;
        for s in 0..8 {
            if let Some(noise) = self.main.octave(s) {
                q += noise.noise_scaled(wrap(g * r), wrap(h * r), wrap(l * r), n * r, h * r) / r;
            }
            r /= 2.0;
        }
        let t = (q / 10.0 + 1.0) / 2.0;
        let bl2 = t >= 1.0;
        let bl3 = t <= 0.0;
        r = 1.0;
        for s in 0..16 {
            let u = wrap(d * r);
            let v = wrap(e * r);
            let w = wrap(f * r);
            let x = m * r;
            if !bl2 && let Some(n1) = self.min_limit.octave(s) {
                o += n1.noise_scaled(u, v, w, x, e * r) / r;
            }
            if !bl3 && let Some(n2) = self.max_limit.octave(s) {
                p += n2.noise_scaled(u, v, w, x, e * r) / r;
            }
            r /= 2.0;
        }
        mth::clamped_lerp(o / 512.0, p / 512.0, t) / 128.0
    }
}

// ---- Simplex ---------------------------------------------------------------

const SQRT_3: f64 = 1.732_050_807_568_877_2;
const F2: f64 = 0.5 * (SQRT_3 - 1.0);
const G2: f64 = (3.0 - SQRT_3) / 6.0;

/// `SimplexNoise`.
#[derive(Clone, Debug)]
pub struct SimplexNoise {
    p: [i32; 512],
    pub xo: f64,
    pub yo: f64,
    pub zo: f64,
}

impl SimplexNoise {
    #[must_use]
    pub fn new(r: &mut Random) -> Self {
        let xo = r.next_double() * 256.0;
        let yo = r.next_double() * 256.0;
        let zo = r.next_double() * 256.0;
        let mut p = [0i32; 512];
        for (i, v) in p.iter_mut().take(256).enumerate() {
            *v = i as i32;
        }
        for i in 0..256 {
            let j = r.next_int_bound(256 - i as i32) as usize;
            p.swap(i, j + i);
        }
        Self { p, xo, yo, zo }
    }

    #[inline]
    fn p(&self, i: i32) -> i32 {
        self.p[(i & 255) as usize]
    }

    fn corner(i: i32, x: f64, y: f64, z: f64, falloff: f64) -> f64 {
        let mut f = falloff - x * x - y * y - z * z;
        if f < 0.0 {
            0.0
        } else {
            f *= f;
            f * f * dot(GRADIENT[i as usize], x, y, z)
        }
    }

    /// 2-D simplex value.
    #[must_use]
    pub fn get_value(&self, x: f64, y: f64) -> f64 {
        let d = (x + y) * F2;
        let i = floor(x + d);
        let j = floor(y + d);
        let e = f64::from(i.wrapping_add(j)) * G2;
        let f = f64::from(i) - e;
        let g = f64::from(j) - e;
        let h = x - f;
        let k = y - g;
        let (l, m) = if h > k { (1, 0) } else { (0, 1) };
        let n = h - f64::from(l) + G2;
        let o = k - f64::from(m) + G2;
        let q = h - 1.0 + 2.0 * G2;
        let r = k - 1.0 + 2.0 * G2;
        let s = i & 255;
        let t = j & 255;
        let u = self.p(s + self.p(t)) % 12;
        let v = self.p(s + l + self.p(t + m)) % 12;
        let w = self.p(s + 1 + self.p(t + 1)) % 12;
        let z = Self::corner(u, h, k, 0.0, 0.5);
        let aa = Self::corner(v, n, o, 0.0, 0.5);
        let ab = Self::corner(w, q, r, 0.0, 0.5);
        70.0 * (z + aa + ab)
    }
}

/// `PerlinSimplexNoise` (octave simplex), used by biome temperature noise.
#[derive(Clone, Debug)]
pub struct PerlinSimplexNoise {
    levels: Vec<Option<SimplexNoise>>,
    highest_freq_input_factor: f64,
    highest_freq_value_factor: f64,
}

impl PerlinSimplexNoise {
    /// `octaves` must be sorted ascending and contain only values <= 0.
    #[must_use]
    pub fn new(r: &mut Random, octaves: &[i32]) -> Self {
        let i = -octaves[0];
        let j = *octaves.last().unwrap();
        let k = i + j + 1;
        assert!(j <= 0, "positive simplex octaves are not used by 1.21.1");
        let base = SimplexNoise::new(r);
        let mut levels: Vec<Option<SimplexNoise>> = (0..k).map(|_| None).collect();
        if j >= 0 && j < k && octaves.contains(&0) {
            levels[0] = Some(base);
        }
        for m in (j + 1)..k {
            if m >= 0 && octaves.contains(&(j - m)) {
                levels[m as usize] = Some(SimplexNoise::new(r));
            } else {
                r.consume_count(262);
            }
        }
        Self {
            levels,
            highest_freq_input_factor: 2f64.powi(j),
            highest_freq_value_factor: 1.0 / (2f64.powi(k) - 1.0),
        }
    }

    #[must_use]
    pub fn get_value(&self, x: f64, y: f64, use_offset: bool) -> f64 {
        let mut d = 0.0;
        let mut e = self.highest_freq_input_factor;
        let mut f = self.highest_freq_value_factor;
        for lvl in &self.levels {
            if let Some(s) = lvl {
                let (ox, oy) = if use_offset { (s.xo, s.yo) } else { (0.0, 0.0) };
                d += s.get_value(x * e + ox, y * e + oy) * f;
            }
            e /= 2.0;
            f *= 2.0;
        }
        d
    }
}

/// Builder for noises keyed by resource name, as `RandomState` does.
#[derive(Clone, Copy, Debug)]
pub struct NoiseFactory {
    pub random: Positional,
}

impl NoiseFactory {
    #[must_use]
    pub fn normal(&self, key: &str, params: &NoiseParams) -> NormalNoise {
        let mut r = self.random.from_hash_of(key);
        NormalNoise::new(&mut r, params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn improved_noise_is_continuous_and_bounded() {
        let mut r = Random::xoroshiro(1);
        let n = ImprovedNoise::new(&mut r);
        for i in 0..1000 {
            let x = f64::from(i) * 0.137;
            let v = n.noise(x, x * 0.5, -x);
            assert!(v.abs() <= 1.1, "{v}");
        }
    }

    #[test]
    fn simplex_bounded() {
        let mut r = Random::legacy(1234);
        let s = PerlinSimplexNoise::new(&mut r, &[0]);
        for i in 0..1000 {
            let v = s.get_value(f64::from(i) * 0.3, f64::from(i) * -0.7, false);
            assert!(v.abs() <= 1.1);
        }
    }
}
