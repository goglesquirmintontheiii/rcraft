//! Palette-compressed bit-packed storage, the building block of a section.
//!
//! Layout matches the publicly documented vanilla scheme: entries are packed
//! into `u64` words with a fixed bit width and never straddle a word boundary,
//! so both RCW and Anvil can use the same packed data verbatim.

/// `N` entries of `u32` values, stored through a palette when few distinct
/// values are present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PalettedContainer<const N: usize> {
    palette: Vec<u32>,
    /// Bits per entry. `0` means every entry equals `palette[0]`.
    bits: u8,
    data: Vec<u64>,
}

impl<const N: usize> PalettedContainer<N> {
    /// Width above which we store raw values instead of palette indices.
    pub const DIRECT_BITS: u8 = 16;

    #[must_use]
    pub fn filled(value: u32) -> Self {
        Self {
            palette: vec![value],
            bits: 0,
            data: Vec::new(),
        }
    }

    /// Build from a full list of values (palette in first-seen order).
    #[must_use]
    pub fn from_values(values: &[u32]) -> Self {
        assert_eq!(values.len(), N);
        let first = values[0];
        if values.iter().all(|&v| v == first) {
            return Self::filled(first);
        }
        let mut palette: Vec<u32> = Vec::new();
        let mut idx = vec![0u32; N];
        let mut last = (u32::MAX, 0u32);
        for (i, &v) in values.iter().enumerate() {
            let k = if v == last.0 {
                last.1
            } else {
                let k = match palette.iter().position(|&p| p == v) {
                    Some(k) => k as u32,
                    None => {
                        palette.push(v);
                        palette.len() as u32 - 1
                    }
                };
                last = (v, k);
                k
            };
            idx[i] = k;
        }
        let bits = Self::bits_for_palette(palette.len());
        let mut c = Self {
            palette,
            bits,
            data: vec![0; Self::words_for(bits)],
        };
        for i in 0..N {
            if bits >= Self::DIRECT_BITS {
                c.write_raw(i, values[i]);
            } else {
                c.write_raw(i, idx[i]);
            }
        }
        c
    }

    /// Rebuild from serialized parts; `None` if they are inconsistent.
    #[must_use]
    pub fn from_parts(palette: Vec<u32>, bits: u8, data: Vec<u64>) -> Option<Self> {
        if palette.is_empty() && bits < Self::DIRECT_BITS {
            return None;
        }
        if bits == 0 {
            return (palette.len() == 1 && data.is_empty()).then_some(Self {
                palette,
                bits,
                data,
            });
        }
        if bits > 32 || data.len() != Self::words_for(bits) {
            return None;
        }
        let c = Self {
            palette,
            bits,
            data,
        };
        if bits < Self::DIRECT_BITS {
            let n = c.palette.len() as u32;
            if (0..N).any(|i| c.read_raw(i) >= n) {
                return None;
            }
        }
        Some(c)
    }

    #[inline]
    #[must_use]
    pub fn bits(&self) -> u8 {
        self.bits
    }

    #[inline]
    #[must_use]
    pub fn palette(&self) -> &[u32] {
        &self.palette
    }

    #[inline]
    #[must_use]
    pub fn raw_data(&self) -> &[u64] {
        &self.data
    }

    /// True when every entry is the same value.
    #[inline]
    #[must_use]
    pub fn is_uniform(&self) -> bool {
        self.bits == 0
    }

    #[inline]
    fn entries_per_word(bits: u8) -> usize {
        64 / bits as usize
    }

    #[inline]
    fn words_for(bits: u8) -> usize {
        N.div_ceil(Self::entries_per_word(bits))
    }

    #[inline]
    fn read_raw(&self, i: usize) -> u32 {
        let per = Self::entries_per_word(self.bits);
        let word = self.data[i / per];
        let shift = (i % per) * self.bits as usize;
        ((word >> shift) & ((1u64 << self.bits) - 1)) as u32
    }

    #[inline]
    fn write_raw(&mut self, i: usize, v: u32) {
        let per = Self::entries_per_word(self.bits);
        let shift = (i % per) * self.bits as usize;
        let mask = ((1u64 << self.bits) - 1) << shift;
        let w = &mut self.data[i / per];
        *w = (*w & !mask) | ((u64::from(v) << shift) & mask);
    }

    #[inline]
    #[must_use]
    pub fn get(&self, i: usize) -> u32 {
        debug_assert!(i < N);
        if self.bits == 0 {
            self.palette[0]
        } else if self.bits >= Self::DIRECT_BITS {
            self.read_raw(i)
        } else {
            self.palette[self.read_raw(i) as usize]
        }
    }

    pub fn set(&mut self, i: usize, value: u32) {
        debug_assert!(i < N);
        if self.bits >= Self::DIRECT_BITS {
            self.write_raw(i, value);
            return;
        }
        let idx = match self.palette.iter().position(|&p| p == value) {
            Some(idx) => idx,
            None => {
                self.palette.push(value);
                let needed = Self::bits_for_palette(self.palette.len());
                if needed > self.bits {
                    self.resize(needed);
                }
                self.palette.len() - 1
            }
        };
        if self.bits == 0 {
            // Still uniform (value already palette[0]).
            return;
        }
        if self.bits >= Self::DIRECT_BITS {
            self.write_raw(i, value);
        } else {
            self.write_raw(i, idx as u32);
        }
    }

    /// Minimum bit width for a palette of `len` entries, with vanilla's rule of
    /// starting at 4 bits for block containers of size 4096 and switching to
    /// direct storage past 8 bits.
    #[must_use]
    pub fn bits_for_palette(len: usize) -> u8 {
        if len <= 1 {
            return 0;
        }
        let raw = (usize::BITS - (len - 1).leading_zeros()) as u8;
        let min = if N == 4096 { 4 } else { 1 };
        let b = raw.max(min);
        if N == 4096 && b > 8 {
            Self::DIRECT_BITS
        } else {
            b
        }
    }

    fn resize(&mut self, new_bits: u8) {
        let old: Vec<u32> = (0..N).map(|i| self.get(i)).collect();
        self.bits = new_bits;
        self.data = vec![0; Self::words_for(new_bits)];
        for (i, v) in old.into_iter().enumerate() {
            if new_bits >= Self::DIRECT_BITS {
                self.write_raw(i, v);
            } else {
                let idx = self.palette.iter().position(|&p| p == v).unwrap();
                self.write_raw(i, idx as u32);
            }
        }
    }

    /// Count entries equal to `value` (used for "is this section all air").
    #[must_use]
    pub fn count(&self, value: u32) -> usize {
        if self.bits == 0 {
            return if self.palette[0] == value { N } else { 0 };
        }
        (0..N).filter(|&i| self.get(i) == value).count()
    }

    /// Rebuild the palette dropping unused entries and shrinking the width.
    pub fn optimize(&mut self) {
        let values: Vec<u32> = (0..N).map(|i| self.get(i)).collect();
        let mut palette: Vec<u32> = Vec::new();
        for &v in &values {
            if !palette.contains(&v) {
                palette.push(v);
            }
        }
        let bits = Self::bits_for_palette(palette.len());
        self.palette = palette;
        self.bits = bits;
        self.data = if bits == 0 {
            Vec::new()
        } else {
            vec![0; Self::words_for(bits)]
        };
        if bits != 0 {
            for (i, v) in values.into_iter().enumerate() {
                if bits >= Self::DIRECT_BITS {
                    self.write_raw(i, v);
                } else {
                    let idx = self.palette.iter().position(|&p| p == v).unwrap();
                    self.write_raw(i, idx as u32);
                }
            }
        }
    }
}

impl<const N: usize> Default for PalettedContainer<N> {
    fn default() -> Self {
        Self::filled(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Blocks = PalettedContainer<4096>;
    type Biomes = PalettedContainer<64>;

    #[test]
    fn uniform_by_default() {
        let c = Blocks::default();
        assert!(c.is_uniform());
        assert_eq!(c.get(4095), 0);
        assert_eq!(c.count(0), 4096);
    }

    #[test]
    fn grows_from_uniform_to_4_bits() {
        let mut c = Blocks::filled(1);
        c.set(7, 2);
        assert_eq!(c.bits(), 4);
        assert_eq!(c.get(7), 2);
        assert_eq!(c.get(8), 1);
        assert_eq!(c.raw_data().len(), 4096 / 16);
    }

    #[test]
    fn grows_to_direct_and_back() {
        let mut c = Blocks::default();
        for i in 0..600u32 {
            c.set(i as usize, i + 1);
        }
        assert_eq!(c.bits(), Blocks::DIRECT_BITS);
        for i in 0..600u32 {
            assert_eq!(c.get(i as usize), i + 1);
        }
        assert_eq!(c.get(4000), 0);
        for i in 0..600 {
            c.set(i, 0);
        }
        c.optimize();
        assert!(c.is_uniform());
        assert_eq!(c.get(0), 0);
    }

    #[test]
    fn small_container_uses_minimal_bits() {
        let mut c = Biomes::filled(5);
        c.set(63, 9);
        assert_eq!(c.bits(), 1);
        c.set(1, 11);
        assert_eq!(c.bits(), 2);
        assert_eq!((c.get(0), c.get(1), c.get(63)), (5, 11, 9));
    }

    #[test]
    fn bit_width_rule_matches_vanilla() {
        assert_eq!(Blocks::bits_for_palette(1), 0);
        assert_eq!(Blocks::bits_for_palette(2), 4);
        assert_eq!(Blocks::bits_for_palette(16), 4);
        assert_eq!(Blocks::bits_for_palette(17), 5);
        assert_eq!(Blocks::bits_for_palette(256), 8);
        assert_eq!(Blocks::bits_for_palette(257), Blocks::DIRECT_BITS);
    }
}
