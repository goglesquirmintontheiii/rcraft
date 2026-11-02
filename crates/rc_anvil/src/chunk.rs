//! Decoding of 1.18+ chunk NBT (data version 3955 for 1.21.1).

use anyhow::{Context as _, Result};

use crate::nbt::Tag;

/// A block state in Anvil's string form: `name` plus sorted properties.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AnvilState {
    pub name: String,
    pub props: Vec<(String, String)>,
}

impl AnvilState {
    /// `minecraft:oak_log[axis=y]`
    #[must_use]
    pub fn to_state_string(&self) -> String {
        if self.props.is_empty() {
            self.name.clone()
        } else {
            let p: Vec<String> = self.props.iter().map(|(k, v)| format!("{k}={v}")).collect();
            format!("{}[{}]", self.name, p.join(","))
        }
    }
}

#[derive(Clone, Debug)]
pub struct AnvilSection {
    pub y: i8,
    pub block_palette: Vec<AnvilState>,
    /// 4096 palette indices (y<<8 | z<<4 | x).
    pub blocks: Vec<u16>,
    pub biome_palette: Vec<String>,
    /// 64 palette indices (y<<4 | z<<2 | x) at quart resolution.
    pub biomes: Vec<u16>,
}

#[derive(Clone, Debug)]
pub struct AnvilChunk {
    pub x: i32,
    pub z: i32,
    pub status: String,
    pub data_version: i32,
    pub sections: Vec<AnvilSection>,
    pub heightmaps: Vec<(String, Vec<i64>)>,
}

/// Unpack vanilla's no-straddle long arrays.
fn unpack(data: &[i64], bits: usize, count: usize) -> Vec<u16> {
    let mut out = Vec::with_capacity(count);
    if bits == 0 {
        out.resize(count, 0);
        return out;
    }
    let per = 64 / bits;
    let mask = (1u64 << bits) - 1;
    for i in 0..count {
        let w = data.get(i / per).copied().unwrap_or(0) as u64;
        out.push(((w >> ((i % per) * bits)) & mask) as u16);
    }
    out
}

fn bits_for(len: usize, min: usize) -> usize {
    if len <= 1 {
        return 0;
    }
    let b = (usize::BITS - (len - 1).leading_zeros()) as usize;
    b.max(min)
}

impl AnvilChunk {
    pub fn from_nbt(t: &Tag) -> Result<Self> {
        let x = t.get("xPos").and_then(Tag::as_i64).context("xPos")? as i32;
        let z = t.get("zPos").and_then(Tag::as_i64).context("zPos")? as i32;
        let status = t
            .get("Status")
            .and_then(Tag::as_str)
            .unwrap_or("")
            .to_owned();
        let data_version = t.get("DataVersion").and_then(Tag::as_i64).unwrap_or(0) as i32;
        let mut sections = Vec::new();
        for s in t.get("sections").map(Tag::as_list).unwrap_or(&[]) {
            let y = s.get("Y").and_then(Tag::as_i64).unwrap_or(0) as i8;
            let Some(bs) = s.get("block_states") else {
                continue;
            };
            let block_palette: Vec<AnvilState> = bs
                .get("palette")
                .map(Tag::as_list)
                .unwrap_or(&[])
                .iter()
                .map(|p| {
                    let name = p
                        .get("Name")
                        .and_then(Tag::as_str)
                        .unwrap_or("minecraft:air")
                        .to_owned();
                    let mut props: Vec<(String, String)> = p
                        .get("Properties")
                        .map(Tag::compound)
                        .unwrap_or(&[])
                        .iter()
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_owned()))
                        .collect();
                    props.sort();
                    AnvilState { name, props }
                })
                .collect();
            let bits = bits_for(block_palette.len(), 4);
            let blocks = unpack(
                bs.get("data").map(Tag::as_long_array).unwrap_or(&[]),
                bits,
                4096,
            );
            let (biome_palette, biomes) = match s.get("biomes") {
                Some(b) => {
                    let pal: Vec<String> = b
                        .get("palette")
                        .map(Tag::as_list)
                        .unwrap_or(&[])
                        .iter()
                        .map(|v| v.as_str().unwrap_or("").to_owned())
                        .collect();
                    let bits = bits_for(pal.len(), 1);
                    let data = unpack(
                        b.get("data").map(Tag::as_long_array).unwrap_or(&[]),
                        bits,
                        64,
                    );
                    (pal, data)
                }
                None => (Vec::new(), vec![0; 64]),
            };
            sections.push(AnvilSection {
                y,
                block_palette,
                blocks,
                biome_palette,
                biomes,
            });
        }
        let heightmaps = t
            .get("Heightmaps")
            .map(Tag::compound)
            .unwrap_or(&[])
            .iter()
            .map(|(k, v)| (k.clone(), v.as_long_array().to_vec()))
            .collect();
        Ok(Self {
            x,
            z,
            status,
            data_version,
            sections,
            heightmaps,
        })
    }

    #[must_use]
    pub fn section(&self, y: i8) -> Option<&AnvilSection> {
        self.sections.iter().find(|s| s.y == y)
    }

    /// Block at chunk-local x/z (0..16) and world y.
    #[must_use]
    pub fn block(&self, x: usize, y: i32, z: usize) -> Option<&AnvilState> {
        let s = self.section((y >> 4) as i8)?;
        let i = (((y & 15) as usize) << 8) | (z << 4) | x;
        s.block_palette.get(s.blocks[i] as usize)
    }

    /// Biome at quart coords (x, z in 0..4; quart y absolute).
    #[must_use]
    pub fn biome(&self, qx: usize, qy: i32, qz: usize) -> Option<&str> {
        let s = self.section((qy >> 2) as i8)?;
        let i = (((qy & 3) as usize) << 4) | (qz << 2) | qx;
        s.biome_palette
            .get(s.biomes[i] as usize)
            .map(String::as_str)
    }
}

fn pack(indices: &[u16], bits: usize) -> Vec<i64> {
    if bits == 0 {
        return Vec::new();
    }
    let per = 64 / bits;
    let mut out = vec![0i64; indices.len().div_ceil(per)];
    for (i, &v) in indices.iter().enumerate() {
        out[i / per] |= (i64::from(v) & ((1i64 << bits) - 1)) << ((i % per) * bits);
    }
    out
}

fn state_tag(s: &AnvilState) -> Tag {
    let mut v = vec![("Name".to_owned(), Tag::String(s.name.clone()))];
    if !s.props.is_empty() {
        v.push((
            "Properties".to_owned(),
            Tag::Compound(
                s.props
                    .iter()
                    .map(|(k, val)| (k.clone(), Tag::String(val.clone())))
                    .collect(),
            ),
        ));
    }
    Tag::Compound(v)
}

impl AnvilChunk {
    /// Encode as 1.21.1 chunk NBT (status full, light to be recomputed).
    #[must_use]
    pub fn to_nbt(&self) -> Tag {
        let sections: Vec<Tag> = self
            .sections
            .iter()
            .map(|s| {
                let bits = bits_for(s.block_palette.len(), 4);
                let mut bs = vec![(
                    "palette".to_owned(),
                    Tag::List(s.block_palette.iter().map(state_tag).collect()),
                )];
                if bits > 0 {
                    bs.push(("data".to_owned(), Tag::LongArray(pack(&s.blocks, bits))));
                }
                let bbits = bits_for(s.biome_palette.len(), 1);
                let mut bio = vec![(
                    "palette".to_owned(),
                    Tag::List(
                        s.biome_palette
                            .iter()
                            .map(|b| Tag::String(b.clone()))
                            .collect(),
                    ),
                )];
                if bbits > 0 {
                    bio.push(("data".to_owned(), Tag::LongArray(pack(&s.biomes, bbits))));
                }
                Tag::Compound(vec![
                    ("Y".to_owned(), Tag::Byte(s.y)),
                    ("block_states".to_owned(), Tag::Compound(bs)),
                    ("biomes".to_owned(), Tag::Compound(bio)),
                ])
            })
            .collect();
        Tag::Compound(vec![
            ("DataVersion".to_owned(), Tag::Int(self.data_version)),
            ("xPos".to_owned(), Tag::Int(self.x)),
            ("zPos".to_owned(), Tag::Int(self.z)),
            ("yPos".to_owned(), Tag::Int(-4)),
            ("Status".to_owned(), Tag::String(self.status.clone())),
            ("LastUpdate".to_owned(), Tag::Long(0)),
            ("InhabitedTime".to_owned(), Tag::Long(0)),
            ("isLightOn".to_owned(), Tag::Byte(0)),
            ("sections".to_owned(), Tag::List(sections)),
            ("block_entities".to_owned(), Tag::List(Vec::new())),
            ("Heightmaps".to_owned(), Tag::Compound(Vec::new())),
            ("fluid_ticks".to_owned(), Tag::List(Vec::new())),
            ("block_ticks".to_owned(), Tag::List(Vec::new())),
            ("PostProcessing".to_owned(), Tag::List(Vec::new())),
            (
                "structures".to_owned(),
                Tag::Compound(vec![
                    ("References".to_owned(), Tag::Compound(Vec::new())),
                    ("starts".to_owned(), Tag::Compound(Vec::new())),
                ]),
            ),
        ])
    }
}

#[cfg(test)]
mod write_tests {
    use super::*;

    #[test]
    fn chunk_round_trip() {
        let pal = vec![
            AnvilState {
                name: "minecraft:air".into(),
                props: vec![],
            },
            AnvilState {
                name: "minecraft:oak_log".into(),
                props: vec![("axis".into(), "y".into())],
            },
        ];
        let blocks: Vec<u16> = (0..4096).map(|i| (i % 3 == 0) as u16).collect();
        let c = AnvilChunk {
            x: 3,
            z: -2,
            status: "minecraft:full".into(),
            data_version: 3955,
            sections: vec![AnvilSection {
                y: 4,
                block_palette: pal,
                blocks: blocks.clone(),
                biome_palette: vec!["minecraft:plains".into()],
                biomes: vec![0; 64],
            }],
            heightmaps: vec![],
        };
        let back = AnvilChunk::from_nbt(&c.to_nbt()).unwrap();
        assert_eq!(back.sections[0].blocks, blocks);
        assert_eq!(back.block(0, 64, 0).unwrap().name, "minecraft:oak_log");
    }
}
