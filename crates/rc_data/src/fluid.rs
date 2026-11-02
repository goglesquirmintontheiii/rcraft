//! Fluid states (water / lava), mirroring vanilla's `FluidState` semantics.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FluidKind {
    Empty = 0,
    Water = 1,
    Lava = 2,
}

/// Packed fluid state: kind, amount (1..=8) and the falling flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FluidState {
    pub kind: FluidKind,
    /// 8 for sources and falling fluid, 1..=7 for flowing.
    pub amount: u8,
    pub falling: bool,
    source: bool,
}

impl FluidState {
    pub const EMPTY: Self = Self {
        kind: FluidKind::Empty,
        amount: 0,
        falling: false,
        source: false,
    };

    #[must_use]
    pub const fn source(kind: FluidKind) -> Self {
        Self {
            kind,
            amount: 8,
            falling: false,
            source: true,
        }
    }

    /// From a liquid block's `level` property (0 = source, 1..7 flowing,
    /// 8..15 falling).
    #[must_use]
    pub const fn from_level(kind: FluidKind, level: u8) -> Self {
        if level == 0 {
            Self::source(kind)
        } else if level >= 8 {
            Self {
                kind,
                amount: 8,
                falling: true,
                source: false,
            }
        } else {
            Self {
                kind,
                amount: 8 - level,
                falling: false,
                source: false,
            }
        }
    }

    #[inline]
    #[must_use]
    pub const fn is_empty(self) -> bool {
        matches!(self.kind, FluidKind::Empty)
    }

    #[inline]
    #[must_use]
    pub const fn is_source(self) -> bool {
        self.source
    }

    #[inline]
    #[must_use]
    pub fn is_water(self) -> bool {
        self.kind == FluidKind::Water
    }

    #[inline]
    #[must_use]
    pub fn is_lava(self) -> bool {
        self.kind == FluidKind::Lava
    }

    /// Fluid surface height in the block (vanilla `getOwnHeight`).
    #[must_use]
    pub fn own_height(self) -> f32 {
        f32::from(self.amount) / 9.0
    }
}
