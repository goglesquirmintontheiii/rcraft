//! Biome registry (from `worldgen/biome/*.json`) and the climate-related
//! biome properties worldgen needs (temperature with height adjustment,
//! frozen modifier, carvers, features).

use std::sync::OnceLock;

use ahash::AHashMap;
use rc_core::BiomeId;

use crate::noise::PerlinSimplexNoise;
use crate::rng::Random;

#[derive(Clone, Debug)]
pub struct Biome {
    pub name: String,
    pub temperature: f32,
    pub downfall: f32,
    pub frozen: bool,
    pub has_precipitation: bool,
    pub carvers: Vec<String>,
    /// Placed-feature ids per decoration step (11 steps).
    pub features: Vec<Vec<String>>,
    pub sky_color: u32,
    pub fog_color: u32,
    pub water_color: u32,
    pub water_fog_color: u32,
    pub grass_color: Option<u32>,
    pub foliage_color: Option<u32>,
    pub grass_modifier: String,
}

pub struct BiomeRegistry {
    pub biomes: Vec<Biome>,
    by_name: AHashMap<String, BiomeId>,
    temperature_noise: PerlinSimplexNoise,
    frozen_temperature_noise: PerlinSimplexNoise,
    biome_info_noise: PerlinSimplexNoise,
}

impl std::fmt::Debug for BiomeRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BiomeRegistry({})", self.biomes.len())
    }
}

fn color(v: &serde_json::Value) -> Option<u32> {
    v.as_u64().map(|c| c as u32)
}

impl BiomeRegistry {
    fn load() -> Self {
        let files = rc_data::files();
        let mut biomes = Vec::new();
        let mut by_name = AHashMap::new();
        for name in files.list("worldgen/biome") {
            let j = files.json(&format!("worldgen/biome/{name}.json")).unwrap();
            let key = format!("minecraft:{name}");
            // A holder set: a single id, a list, or (per carving step) either.
            fn holder_list(v: &serde_json::Value) -> Vec<String> {
                match v {
                    serde_json::Value::Array(a) => a
                        .iter()
                        .filter_map(|c| c.as_str().map(str::to_owned))
                        .collect(),
                    serde_json::Value::String(s) => vec![s.clone()],
                    _ => Vec::new(),
                }
            }
            let carvers = match &j["carvers"] {
                serde_json::Value::Object(o) => o.get("air").map(holder_list).unwrap_or_default(),
                other => holder_list(other),
            };
            let features = j["features"]
                .as_array()
                .map(|steps| {
                    steps
                        .iter()
                        .map(|s| match s {
                            serde_json::Value::Array(a) => a
                                .iter()
                                .filter_map(|f| f.as_str().map(str::to_owned))
                                .collect(),
                            serde_json::Value::String(t) => vec![t.clone()],
                            _ => Vec::new(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let fx = &j["effects"];
            let b = Biome {
                name: key.clone(),
                temperature: j["temperature"].as_f64().unwrap_or(0.5) as f32,
                downfall: j["downfall"].as_f64().unwrap_or(0.5) as f32,
                frozen: j["temperature_modifier"].as_str() == Some("frozen"),
                has_precipitation: j["has_precipitation"].as_bool().unwrap_or(true),
                carvers,
                features,
                sky_color: color(&fx["sky_color"]).unwrap_or(0x78A7FF),
                fog_color: color(&fx["fog_color"]).unwrap_or(0xC0D8FF),
                water_color: color(&fx["water_color"]).unwrap_or(0x3F76E4),
                water_fog_color: color(&fx["water_fog_color"]).unwrap_or(0x050533),
                grass_color: color(&fx["grass_color"]),
                foliage_color: color(&fx["foliage_color"]),
                grass_modifier: fx["grass_color_modifier"]
                    .as_str()
                    .unwrap_or("none")
                    .to_owned(),
            };
            by_name.insert(key, BiomeId(biomes.len() as u32));
            biomes.push(b);
        }
        let temperature_noise = PerlinSimplexNoise::new(&mut Random::legacy(1234), &[0]);
        let frozen_temperature_noise =
            PerlinSimplexNoise::new(&mut Random::legacy(3456), &[-2, -1, 0]);
        let biome_info_noise = PerlinSimplexNoise::new(&mut Random::legacy(2345), &[0]);
        Self {
            biomes,
            by_name,
            temperature_noise,
            frozen_temperature_noise,
            biome_info_noise,
        }
    }

    #[must_use]
    pub fn id(&self, name: &str) -> Option<BiomeId> {
        let key = if name.contains(':') {
            name.to_owned()
        } else {
            format!("minecraft:{name}")
        };
        self.by_name.get(&key).copied()
    }

    #[must_use]
    pub fn get(&self, id: BiomeId) -> &Biome {
        &self.biomes[id.index()]
    }

    /// `Biome.getTemperature(pos)` (1.21.1, without the cache).
    #[must_use]
    pub fn temperature_at(&self, id: BiomeId, x: i32, y: i32, z: i32) -> f32 {
        let b = self.get(id);
        let mut f = b.temperature;
        if b.frozen {
            let d = self.frozen_temperature_noise.get_value(
                f64::from(x) * 0.05,
                f64::from(z) * 0.05,
                false,
            ) * 7.0;
            let e = self
                .biome_info_noise
                .get_value(f64::from(x) * 0.2, f64::from(z) * 0.2, false);
            if d + e < 0.3 {
                let g = self.biome_info_noise.get_value(
                    f64::from(x) * 0.09,
                    f64::from(z) * 0.09,
                    false,
                );
                if g < 0.8 {
                    f = 0.2;
                }
            }
        }
        if y > 80 {
            let g = (self.temperature_noise.get_value(
                f64::from(x as f32 / 8.0),
                f64::from(z as f32 / 8.0),
                false,
            ) * 8.0) as f32;
            return f - (g + y as f32 - 80.0) * 0.05 / 40.0;
        }
        f
    }

    #[must_use]
    pub fn cold_enough_to_snow(&self, id: BiomeId, x: i32, y: i32, z: i32) -> bool {
        self.temperature_at(id, x, y, z) < 0.15
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.biomes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.biomes.is_empty()
    }
}

/// The global biome registry.
pub fn registry() -> &'static BiomeRegistry {
    static R: OnceLock<BiomeRegistry> = OnceLock::new();
    R.get_or_init(BiomeRegistry::load)
}
