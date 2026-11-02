//! `.mca` region files: 1024 chunk slots, 4 KiB sectors, per-chunk compression.

use std::io::Read;
use std::path::Path;

use anyhow::{Context as _, Result, bail};

use crate::nbt::{self, Tag};

pub struct RegionFile {
    data: Vec<u8>,
}

impl std::fmt::Debug for RegionFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RegionFile({} bytes)", self.data.len())
    }
}

impl RegionFile {
    pub fn open(path: &Path) -> Result<Self> {
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Ok(Self { data })
    }

    #[must_use]
    pub fn from_bytes(data: Vec<u8>) -> Self {
        Self { data }
    }

    /// Whether slot `(x & 31, z & 31)` holds a chunk.
    #[must_use]
    pub fn has_chunk(&self, x: i32, z: i32) -> bool {
        let i = (((z & 31) << 5) | (x & 31)) as usize * 4;
        self.data.len() >= 4096 && self.data[i..i + 3] != [0, 0, 0]
    }

    /// Indices `(local_x, local_z)` of all present chunks.
    #[must_use]
    pub fn present(&self) -> Vec<(i32, i32)> {
        (0..1024)
            .filter(|&i| self.data.len() >= 4096 && self.data[i * 4..i * 4 + 3] != [0, 0, 0])
            .map(|i| ((i & 31) as i32, (i >> 5) as i32))
            .collect()
    }

    /// Read the raw (decompressed) chunk NBT bytes.
    pub fn read_raw(&self, x: i32, z: i32) -> Result<Option<Vec<u8>>> {
        let i = (((z & 31) << 5) | (x & 31)) as usize * 4;
        if self.data.len() < 8192 {
            return Ok(None);
        }
        let off = (u32::from(self.data[i]) << 16)
            | (u32::from(self.data[i + 1]) << 8)
            | u32::from(self.data[i + 2]);
        if off == 0 {
            return Ok(None);
        }
        let o = off as usize * 4096;
        if o + 5 > self.data.len() {
            bail!("chunk offset out of file");
        }
        let len = u32::from_be_bytes(self.data[o..o + 4].try_into()?) as usize;
        let comp = self.data[o + 4];
        let body = self
            .data
            .get(o + 5..o + 4 + len)
            .context("truncated chunk")?;
        let mut out = Vec::new();
        match comp & 0x7f {
            1 => {
                flate2::read::GzDecoder::new(body).read_to_end(&mut out)?;
            }
            2 => {
                flate2::read::ZlibDecoder::new(body).read_to_end(&mut out)?;
            }
            3 => out.extend_from_slice(body),
            c => bail!("unsupported chunk compression {c}"),
        }
        Ok(Some(out))
    }

    pub fn read_chunk(&self, x: i32, z: i32) -> Result<Option<Tag>> {
        match self.read_raw(x, z)? {
            Some(b) => Ok(Some(nbt::parse(&b)?)),
            None => Ok(None),
        }
    }
}
