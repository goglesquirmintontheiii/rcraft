//! Bit-exact reimplementations of the game's random number generators.
//!
//! * [`Legacy`]: Java's 48-bit LCG (`java.util.Random` / `LegacyRandomSource`).
//! * [`Xoroshiro`]: Xoroshiro128++ with Stafford-13 seed mixing.
//! * [`Positional`]: per-position / per-name random factories.
//!
//! Source: the algorithms are public (JDK docs, xoshiro paper, minecraft.wiki
//! "Random number generator"); constants verified against cubiomes' test
//! vectors.

use md5::{Digest, Md5};

const MULT: i64 = 0x5_DEEC_E66D;
const MASK48: i64 = (1 << 48) - 1;
const GOLDEN: i64 = 0x9E37_79B9_7F4A_7C15_u64 as i64;
const SILVER: i64 = 0x6A09_E667_F3BC_C909_u64 as i64;

#[inline]
fn mix_stafford13(mut z: i64) -> i64 {
    z = (z ^ ((z as u64) >> 30) as i64).wrapping_mul(0xBF58_476D_1CE4_E5B9_u64 as i64);
    z = (z ^ ((z as u64) >> 27) as i64).wrapping_mul(0x94D0_49BB_1331_11EB_u64 as i64);
    z ^ ((z as u64) >> 31) as i64
}

/// `RandomSupport.upgradeSeedTo128bit`.
#[inline]
#[must_use]
pub fn upgrade_seed(seed: i64) -> (i64, i64) {
    let lo = seed ^ SILVER;
    let hi = lo.wrapping_add(GOLDEN);
    (mix_stafford13(lo), mix_stafford13(hi))
}

/// MD5-based 128-bit seed of a string (`RandomSupport.seedFromHashOf`).
#[must_use]
pub fn seed_from_hash_of(s: &str) -> (i64, i64) {
    let d = Md5::digest(s.as_bytes());
    let lo = i64::from_be_bytes(d[0..8].try_into().unwrap());
    let hi = i64::from_be_bytes(d[8..16].try_into().unwrap());
    (lo, hi)
}

/// Java `String.hashCode()`.
#[must_use]
pub fn java_string_hash(s: &str) -> i32 {
    // Java hashes UTF-16 code units.
    s.encode_utf16()
        .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)))
}

#[derive(Clone, Copy, Debug, Default)]
struct Gaussian {
    next: f64,
    have: bool,
}

/// `java.util.Random`-compatible LCG.
#[derive(Clone, Copy, Debug)]
pub struct Legacy {
    seed: i64,
    gauss: Gaussian,
}

impl Legacy {
    #[must_use]
    pub fn new(seed: i64) -> Self {
        Self {
            seed: (seed ^ MULT) & MASK48,
            gauss: Gaussian::default(),
        }
    }

    pub fn set_seed(&mut self, seed: i64) {
        self.seed = (seed ^ MULT) & MASK48;
        self.gauss = Gaussian::default();
    }

    #[inline]
    pub fn next(&mut self, bits: u32) -> i32 {
        self.seed = (self.seed.wrapping_mul(MULT).wrapping_add(0xB)) & MASK48;
        (self.seed >> (48 - bits)) as i32
    }
}

/// Xoroshiro128++.
#[derive(Clone, Copy, Debug)]
pub struct Xoroshiro {
    lo: i64,
    hi: i64,
    gauss: Gaussian,
}

impl Xoroshiro {
    #[must_use]
    pub fn new(seed: i64) -> Self {
        let (lo, hi) = upgrade_seed(seed);
        Self::from_parts(lo, hi)
    }

    #[must_use]
    pub fn from_parts(mut lo: i64, mut hi: i64) -> Self {
        if lo == 0 && hi == 0 {
            lo = GOLDEN;
            hi = SILVER;
        }
        Self {
            lo,
            hi,
            gauss: Gaussian::default(),
        }
    }

    #[inline]
    pub fn next_long_raw(&mut self) -> i64 {
        let l = self.lo;
        let mut m = self.hi;
        let n = l.wrapping_add(m).rotate_left(17).wrapping_add(l);
        m ^= l;
        self.lo = l.rotate_left(49) ^ m ^ (m << 21);
        self.hi = m.rotate_left(28);
        n
    }
}

/// Any of the game's random sources, with the `RandomSource` API.
///
/// `WorldgenX` is `new WorldgenRandom(new XoroshiroRandomSource(..))`: the
/// legacy bit-based algorithms (`nextInt(bound)`, `nextDouble`, ...) driven by
/// the top bits of Xoroshiro output. Feature decoration uses it.
#[derive(Clone, Copy, Debug)]
pub enum Random {
    Legacy(Legacy),
    Xoroshiro(Xoroshiro),
    WorldgenX(Xoroshiro),
}

impl Random {
    #[must_use]
    pub fn legacy(seed: i64) -> Self {
        Self::Legacy(Legacy::new(seed))
    }

    #[must_use]
    pub fn xoroshiro(seed: i64) -> Self {
        Self::Xoroshiro(Xoroshiro::new(seed))
    }

    #[must_use]
    pub fn xoroshiro_parts(lo: i64, hi: i64) -> Self {
        Self::Xoroshiro(Xoroshiro::from_parts(lo, hi))
    }

    /// `new WorldgenRandom(new XoroshiroRandomSource(seed))`.
    #[must_use]
    pub fn worldgen_xoroshiro(seed: i64) -> Self {
        Self::WorldgenX(Xoroshiro::new(seed))
    }

    pub fn set_seed(&mut self, seed: i64) {
        match self {
            Self::Legacy(r) => r.set_seed(seed),
            Self::Xoroshiro(r) | Self::WorldgenX(r) => *r = Xoroshiro::new(seed),
        }
    }

    /// `next(bits)` of the bit-based (legacy-API) sources.
    #[inline]
    fn bits(&mut self, bits: u32) -> i32 {
        match self {
            Self::Legacy(r) => r.next(bits),
            Self::Xoroshiro(r) | Self::WorldgenX(r) => {
                ((r.next_long_raw() as u64) >> (64 - bits)) as i32
            }
        }
    }

    #[inline]
    pub fn next_int(&mut self) -> i32 {
        match self {
            Self::Xoroshiro(r) => r.next_long_raw() as i32,
            _ => self.bits(32),
        }
    }

    /// `nextInt(bound)`; `bound` must be positive.
    #[inline]
    pub fn next_int_bound(&mut self, bound: i32) -> i32 {
        debug_assert!(bound > 0, "bound must be positive");
        match self {
            Self::Legacy(_) | Self::WorldgenX(_) => {
                if bound & (bound - 1) == 0 {
                    return ((i64::from(bound) * i64::from(self.bits(31))) >> 31) as i32;
                }
                loop {
                    let j = self.bits(31);
                    let k = j % bound;
                    if j.wrapping_sub(k).wrapping_add(bound - 1) >= 0 {
                        return k;
                    }
                }
            }
            Self::Xoroshiro(r) => {
                let bound_u = u64::from(bound as u32);
                let mut l = u64::from(r.next_long_raw() as u32);
                let mut m = l.wrapping_mul(bound_u);
                let mut n = m & 0xFFFF_FFFF;
                if n < bound_u {
                    let i = (!(bound as u32)).wrapping_add(1) % (bound as u32);
                    while n < u64::from(i) {
                        l = u64::from(r.next_long_raw() as u32);
                        m = l.wrapping_mul(bound_u);
                        n = m & 0xFFFF_FFFF;
                    }
                }
                (m >> 32) as i32
            }
        }
    }

    /// `nextIntBetweenInclusive(a, b)`.
    #[inline]
    pub fn next_int_between(&mut self, a: i32, b: i32) -> i32 {
        self.next_int_bound(b - a + 1) + a
    }

    #[inline]
    pub fn next_long(&mut self) -> i64 {
        match self {
            Self::Xoroshiro(r) => r.next_long_raw(),
            _ => {
                let i = self.bits(32);
                let j = self.bits(32);
                (i64::from(i) << 32).wrapping_add(i64::from(j))
            }
        }
    }

    #[inline]
    pub fn next_bool(&mut self) -> bool {
        match self {
            Self::Xoroshiro(r) => r.next_long_raw() & 1 != 0,
            _ => self.bits(1) != 0,
        }
    }

    #[inline]
    pub fn next_float(&mut self) -> f32 {
        match self {
            Self::Xoroshiro(r) => ((r.next_long_raw() as u64) >> 40) as f32 * 5.960_464_5e-8,
            _ => self.bits(24) as f32 * 5.960_464_5e-8,
        }
    }

    #[inline]
    pub fn next_double(&mut self) -> f64 {
        match self {
            Self::Xoroshiro(r) => {
                ((r.next_long_raw() as u64) >> 11) as f64 * 1.110_223_024_625_156_5e-16
            }
            _ => {
                let i = self.bits(26);
                let j = self.bits(27);
                ((i64::from(i) << 27) + i64::from(j)) as f64 * 1.110_223_024_625_156_5e-16
            }
        }
    }

    pub fn next_gaussian(&mut self) -> f64 {
        let g = match self {
            Self::Legacy(r) => r.gauss,
            Self::Xoroshiro(r) | Self::WorldgenX(r) => r.gauss,
        };
        if g.have {
            self.set_gauss(Gaussian {
                next: 0.0,
                have: false,
            });
            return g.next;
        }
        loop {
            let d = 2.0 * self.next_double() - 1.0;
            let e = 2.0 * self.next_double() - 1.0;
            let f = d * d + e * e;
            if f < 1.0 && f != 0.0 {
                let h = (-2.0 * f.ln() / f).sqrt();
                self.set_gauss(Gaussian {
                    next: e * h,
                    have: true,
                });
                return d * h;
            }
        }
    }

    fn set_gauss(&mut self, g: Gaussian) {
        match self {
            Self::Legacy(r) => r.gauss = g,
            Self::Xoroshiro(r) | Self::WorldgenX(r) => r.gauss = g,
        }
    }

    /// `triangle(mode, deviation)`.
    pub fn triangle(&mut self, a: f64, b: f64) -> f64 {
        a + b * (self.next_double() - self.next_double())
    }

    pub fn consume_count(&mut self, n: i32) {
        for _ in 0..n {
            self.next_int();
        }
    }

    #[must_use]
    pub fn fork(&mut self) -> Self {
        match self {
            Self::Legacy(_) => Self::legacy(self.next_long()),
            Self::Xoroshiro(r) | Self::WorldgenX(r) => {
                let lo = r.next_long_raw();
                let hi = r.next_long_raw();
                Self::xoroshiro_parts(lo, hi)
            }
        }
    }

    #[must_use]
    pub fn fork_positional(&mut self) -> Positional {
        match self {
            Self::Legacy(_) => Positional::Legacy(self.next_long()),
            Self::Xoroshiro(r) | Self::WorldgenX(r) => {
                let lo = r.next_long_raw();
                let hi = r.next_long_raw();
                Positional::Xoroshiro(lo, hi)
            }
        }
    }

    // ---- WorldgenRandom seeding helpers -------------------------------------

    /// `WorldgenRandom.setDecorationSeed`; returns the decoration seed.
    pub fn set_decoration_seed(&mut self, world_seed: i64, x: i32, z: i32) -> i64 {
        self.set_seed(world_seed);
        let l = self.next_long() | 1;
        let m = self.next_long() | 1;
        let n =
            (i64::from(x).wrapping_mul(l)).wrapping_add(i64::from(z).wrapping_mul(m)) ^ world_seed;
        self.set_seed(n);
        n
    }

    /// `WorldgenRandom.setFeatureSeed`.
    pub fn set_feature_seed(&mut self, decoration_seed: i64, index: i32, step: i32) {
        let l = decoration_seed
            .wrapping_add(i64::from(index))
            .wrapping_add(i64::from(10_000 * step));
        self.set_seed(l);
    }

    /// `WorldgenRandom.setLargeFeatureSeed`.
    pub fn set_large_feature_seed(&mut self, seed: i64, x: i32, z: i32) {
        self.set_seed(seed);
        let l = self.next_long();
        let m = self.next_long();
        let n = (i64::from(x).wrapping_mul(l)) ^ (i64::from(z).wrapping_mul(m)) ^ seed;
        self.set_seed(n);
    }

    /// `WorldgenRandom.setLargeFeatureWithSalt`.
    pub fn set_large_feature_with_salt(&mut self, seed: i64, x: i32, z: i32, salt: i32) {
        let l = i64::from(x)
            .wrapping_mul(341_873_128_712)
            .wrapping_add(i64::from(z).wrapping_mul(132_897_987_541))
            .wrapping_add(seed)
            .wrapping_add(i64::from(salt));
        self.set_seed(l);
    }
}

/// `PositionalRandomFactory`.
#[derive(Clone, Copy, Debug)]
pub enum Positional {
    Legacy(i64),
    Xoroshiro(i64, i64),
}

impl Positional {
    #[must_use]
    pub fn at(&self, x: i32, y: i32, z: i32) -> Random {
        let l = crate::mth::get_seed(x, y, z);
        match *self {
            Self::Legacy(seed) => Random::legacy(l ^ seed),
            Self::Xoroshiro(lo, hi) => Random::xoroshiro_parts(l ^ lo, hi),
        }
    }

    #[must_use]
    pub fn from_hash_of(&self, name: &str) -> Random {
        match *self {
            Self::Legacy(seed) => Random::legacy(i64::from(java_string_hash(name)) ^ seed),
            Self::Xoroshiro(lo, hi) => {
                let (a, b) = seed_from_hash_of(name);
                Random::xoroshiro_parts(a ^ lo, b ^ hi)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_random_matches_jdk() {
        // new java.util.Random(0): nextInt() = -1155484576, nextInt() = -723955400
        let mut r = Random::legacy(0);
        assert_eq!(r.next_int(), -1_155_484_576);
        assert_eq!(r.next_int(), -723_955_400);
        // new Random(42).nextInt(10) == 0, then 3
        let mut r = Random::legacy(42);
        assert_eq!(r.next_int_bound(10), 0);
        assert_eq!(r.next_int_bound(10), 3);
        // new Random(0).nextLong() == -4962768465676381896
        assert_eq!(Random::legacy(0).next_long(), -4_962_768_465_676_381_896);
        // new Random(0).nextDouble() == 0.730967787376657
        assert_eq!(Random::legacy(0).next_double(), 0.730_967_787_376_657);
    }

    #[test]
    fn string_hash() {
        assert_eq!(
            java_string_hash("octave_0"),
            "octave_0"
                .chars()
                .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(c as i32))
        );
        assert_eq!(java_string_hash("hello"), 99_162_322);
    }

    #[test]
    fn xoroshiro_is_deterministic_and_nonzero() {
        let mut a = Random::xoroshiro(0);
        let mut b = Random::xoroshiro(0);
        for _ in 0..10 {
            assert_eq!(a.next_long(), b.next_long());
        }
        let mut z = Random::xoroshiro_parts(0, 0);
        assert_ne!(z.next_long(), 0);
        for bound in [1, 2, 7, 16, 100, 1 << 30] {
            for _ in 0..100 {
                let v = a.next_int_bound(bound);
                assert!((0..bound).contains(&v));
            }
        }
    }
}
