//! Colours that vanilla applies to layered item sprites (tint indexes):
//! spawn eggs, leather armour, potions, tipped arrows, maps, firework stars.
//! Values from the Minecraft Wiki (spawn egg colour table, dye and potion
//! colour pages); see docs/CLEAN_ROOM.md.

/// (mob, base colour, spots colour) for every 1.21.1 spawn egg.
const SPAWN_EGGS: &[(&str, u32, u32)] = &[
    ("allay", 0x00DAFF, 0x00ADFF),
    ("armadillo", 0xAD716D, 0x824848),
    ("axolotl", 0xFBC1E3, 0xA62D74),
    ("bat", 0x4C3E30, 0x0F0F0F),
    ("bee", 0xEDC343, 0x43241B),
    ("blaze", 0xF6B201, 0xFFF87E),
    ("bogged", 0x8A9C72, 0x314D1B),
    ("breeze", 0xAF94DF, 0x9166DF),
    ("camel", 0xFCC369, 0xCB9337),
    ("cat", 0xEFC88E, 0x957256),
    ("cave_spider", 0x0C424E, 0xA80E0E),
    ("chicken", 0xA1A1A1, 0xFF0000),
    ("cod", 0xC1A76A, 0xE5C48B),
    ("cow", 0x443626, 0xA1A1A1),
    ("creeper", 0x0DA70B, 0x000000),
    ("dolphin", 0x223B4D, 0xF9F9F9),
    ("donkey", 0x534539, 0x867566),
    ("drowned", 0x8FF1D7, 0x799C65),
    ("elder_guardian", 0xCECCBA, 0x747693),
    ("ender_dragon", 0x1C1C1C, 0xE079FA),
    ("enderman", 0x161616, 0x000000),
    ("endermite", 0x161616, 0x6E6E6E),
    ("evoker", 0x959B9B, 0x1E1C1A),
    ("fox", 0xD5B69F, 0xCC6920),
    ("frog", 0xD07444, 0xFFC77C),
    ("ghast", 0xF9F9F9, 0xBCBCBC),
    ("glow_squid", 0x095656, 0x85F1BC),
    ("goat", 0xA5947C, 0x55493E),
    ("guardian", 0x5A8272, 0xF17D30),
    ("hoglin", 0xC66E55, 0x5F6464),
    ("horse", 0xC09E7D, 0xEEE500),
    ("husk", 0x797061, 0xE6CC94),
    ("iron_golem", 0xDBCDC2, 0x74A332),
    ("llama", 0xC09E7D, 0x995F40),
    ("magma_cube", 0x340000, 0xFCFC00),
    ("mooshroom", 0xA00F10, 0xB7B7B7),
    ("mule", 0x1B0200, 0x51331D),
    ("ocelot", 0xEFDE7D, 0x564434),
    ("panda", 0xE7E7E7, 0x1B1B22),
    ("parrot", 0x0DA70B, 0xFF0000),
    ("phantom", 0x43518A, 0x88FF00),
    ("pig", 0xF0A5A2, 0xDB635F),
    ("piglin_brute", 0x592A10, 0xF9F3A4),
    ("piglin", 0x995F40, 0xF9F3A4),
    ("pillager", 0x532F36, 0x959B9B),
    ("polar_bear", 0xEEEEDE, 0xD5D6CD),
    ("pufferfish", 0xF6B201, 0x37C3F2),
    ("rabbit", 0x995F40, 0x734831),
    ("ravager", 0x757470, 0x5B5049),
    ("salmon", 0xA00F10, 0x0E8474),
    ("sheep", 0xE7E7E7, 0xFFB5B5),
    ("shulker", 0x946794, 0x4D3852),
    ("silverfish", 0x6E6E6E, 0x303030),
    ("skeleton_horse", 0x68684F, 0xE5E5D8),
    ("skeleton", 0xC1C1C1, 0x494949),
    ("slime", 0x51A03E, 0x7EBF6E),
    ("sniffer", 0x871E09, 0x25AB70),
    ("snow_golem", 0xD9F2F2, 0x81A4A4),
    ("spider", 0x342D27, 0xA80E0E),
    ("squid", 0x223B4D, 0x708899),
    ("stray", 0x617677, 0xDDEAEA),
    ("strider", 0x9C3436, 0x4D494D),
    ("tadpole", 0x6D533D, 0x160A00),
    ("trader_llama", 0xEAA430, 0x456296),
    ("tropical_fish", 0xEF6915, 0xFFF9EF),
    ("turtle", 0xE7E7E7, 0x00AFAF),
    ("vex", 0x7A90A4, 0xE8EDF1),
    ("villager", 0x563C33, 0xBD8B72),
    ("vindicator", 0x959B9B, 0x275E61),
    ("wandering_trader", 0x456296, 0xEAA430),
    ("warden", 0x0F4649, 0x39D6E0),
    ("witch", 0x340000, 0x51A03E),
    ("wither_skeleton", 0x141414, 0x474D4D),
    ("wither", 0x141414, 0x4D72A0),
    ("wolf", 0xD7D3D3, 0xCEAF96),
    ("zoglin", 0xC66E55, 0xE6E6E6),
    ("zombie_horse", 0x315234, 0x97C284),
    ("zombie", 0x00AFAF, 0x799C65),
    ("zombie_villager", 0x563C33, 0x799C65),
    ("zombified_piglin", 0xEA9393, 0x4C7129),
];

/// Default colour of undyed leather.
const LEATHER: u32 = 0xA06540;
/// Colour of a potion with no effects (water).
const WATER_POTION: u32 = 0x385DC6;
/// Map markings on a filled map.
const MAP_MARKINGS: u32 = 0x46402E;
/// A firework star with no colour.
const FIREWORK_STAR: u32 = 0x8A8A8A;

/// What to do with layer `layer` of item `name`'s sprite.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerTint {
    /// Draw as is.
    None,
    /// Multiply by this RGB colour.
    Rgb(u32),
    /// Not drawn (an overlay that only shows when dyed).
    Hidden,
}

#[must_use]
pub fn layer_tint(name: &str, layer: usize) -> LayerTint {
    if let Some(mob) = name.strip_suffix("_spawn_egg") {
        return SPAWN_EGGS
            .iter()
            .find(|(m, _, _)| *m == mob)
            .map_or(LayerTint::None, |&(_, a, b)| {
                LayerTint::Rgb(if layer == 0 { a } else { b })
            });
    }
    match (name, layer) {
        (
            "leather_helmet"
            | "leather_chestplate"
            | "leather_leggings"
            | "leather_boots"
            | "leather_horse_armor",
            0,
        ) => LayerTint::Rgb(LEATHER),
        ("wolf_armor", 1) => LayerTint::Hidden,
        ("potion" | "splash_potion" | "lingering_potion" | "tipped_arrow", 0) => {
            LayerTint::Rgb(WATER_POTION)
        }
        ("filled_map", 1) => LayerTint::Rgb(MAP_MARKINGS),
        ("firework_star", 1) => LayerTint::Rgb(FIREWORK_STAR),
        _ => LayerTint::None,
    }
}

/// Draw `layers` (already-decoded sprites, bottom first) into one image,
/// applying each layer's tint.
#[must_use]
pub fn compose(name: &str, layers: &[rc_assets::Image]) -> Option<rc_assets::Image> {
    let w = layers.iter().map(|l| l.w).max()?;
    let h = layers.iter().map(|l| l.w).max()?;
    let mut out = rc_assets::Image::new(w, h);
    for (i, l) in layers.iter().enumerate() {
        let tint = match layer_tint(name, i) {
            LayerTint::Hidden => continue,
            LayerTint::None => [255u32; 3],
            LayerTint::Rgb(c) => [(c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff],
        };
        // Animated strips: use the first square frame.
        for y in 0..h {
            for x in 0..w {
                let (sx, sy) = (x * l.w / w, y * l.w / w);
                if sy >= l.h {
                    continue;
                }
                let s = l.get(sx, sy);
                if s[3] == 0 {
                    continue;
                }
                let c = [0, 1, 2].map(|k| (u32::from(s[k]) * tint[k] / 255) as u8);
                let d = out.get(x, y);
                let a = u32::from(s[3]);
                let mix =
                    |k: usize| ((u32::from(c[k]) * a + u32::from(d[k]) * (255 - a)) / 255) as u8;
                out.put(x, y, [mix(0), mix(1), mix(2), d[3].max(s[3])]);
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tints() {
        assert_eq!(layer_tint("pig_spawn_egg", 0), LayerTint::Rgb(0xF0A5A2));
        assert_eq!(layer_tint("pig_spawn_egg", 1), LayerTint::Rgb(0xDB635F));
        assert_eq!(layer_tint("leather_boots", 1), LayerTint::None);
        assert_eq!(layer_tint("wolf_armor", 1), LayerTint::Hidden);
        assert_eq!(SPAWN_EGGS.len(), 80);
    }
}
