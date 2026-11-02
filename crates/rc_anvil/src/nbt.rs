//! Named Binary Tag reader (big-endian, Java Edition).

use std::io::Read;

use anyhow::{Result, bail};

#[derive(Clone, Debug, PartialEq)]
pub enum Tag {
    End,
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(Vec<Tag>),
    Compound(Vec<(String, Tag)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Tag> {
        match self {
            Tag::Compound(v) => v.iter().find(|(k, _)| k == key).map(|(_, t)| t),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        if let Tag::String(s) = self {
            Some(s)
        } else {
            None
        }
    }

    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Tag::Byte(v) => Some(i64::from(v)),
            Tag::Short(v) => Some(i64::from(v)),
            Tag::Int(v) => Some(i64::from(v)),
            Tag::Long(v) => Some(v),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_list(&self) -> &[Tag] {
        if let Tag::List(v) = self { v } else { &[] }
    }

    #[must_use]
    pub fn as_long_array(&self) -> &[i64] {
        if let Tag::LongArray(v) = self { v } else { &[] }
    }

    #[must_use]
    pub fn compound(&self) -> &[(String, Tag)] {
        if let Tag::Compound(v) = self { v } else { &[] }
    }
}

struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        if self.p + n > self.b.len() {
            bail!("unexpected end of NBT data");
        }
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into()?))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into()?))
    }
    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into()?))
    }
    fn string(&mut self) -> Result<String> {
        let n = self.i16()? as u16 as usize;
        // Java "modified UTF-8"; vanilla block/biome names are ASCII.
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn len(&mut self) -> Result<usize> {
        let n = self.i32()?;
        if n < 0 {
            return Ok(0);
        }
        Ok(n as usize)
    }
    fn payload(&mut self, t: u8, depth: u32) -> Result<Tag> {
        if depth > 512 {
            bail!("NBT nesting too deep");
        }
        Ok(match t {
            0 => Tag::End,
            1 => Tag::Byte(self.u8()? as i8),
            2 => Tag::Short(self.i16()?),
            3 => Tag::Int(self.i32()?),
            4 => Tag::Long(self.i64()?),
            5 => Tag::Float(f32::from_bits(self.i32()? as u32)),
            6 => Tag::Double(f64::from_bits(self.i64()? as u64)),
            7 => {
                let n = self.len()?;
                Tag::ByteArray(self.take(n)?.iter().map(|&b| b as i8).collect())
            }
            8 => Tag::String(self.string()?),
            9 => {
                let et = self.u8()?;
                let n = self.len()?;
                let mut v = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    v.push(self.payload(et, depth + 1)?);
                }
                Tag::List(v)
            }
            10 => {
                let mut v = Vec::new();
                loop {
                    let tt = self.u8()?;
                    if tt == 0 {
                        break;
                    }
                    let k = self.string()?;
                    v.push((k, self.payload(tt, depth + 1)?));
                }
                Tag::Compound(v)
            }
            11 => {
                let n = self.len()?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(self.i32()?);
                }
                Tag::IntArray(v)
            }
            12 => {
                let n = self.len()?;
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(self.i64()?);
                }
                Tag::LongArray(v)
            }
            _ => bail!("unknown NBT tag type {t}"),
        })
    }
}

/// Parse an uncompressed root tag (named compound).
pub fn parse(bytes: &[u8]) -> Result<Tag> {
    let mut r = Reader { b: bytes, p: 0 };
    let t = r.u8()?;
    let _name = r.string()?;
    r.payload(t, 0)
}

/// Parse a possibly gzip/zlib-compressed root tag (e.g. `level.dat`).
pub fn parse_compressed(bytes: &[u8]) -> Result<Tag> {
    let mut out = Vec::new();
    if bytes.starts_with(&[0x1f, 0x8b]) {
        flate2::read::GzDecoder::new(bytes).read_to_end(&mut out)?;
        parse(&out)
    } else if bytes.first() == Some(&0x78) {
        flate2::read::ZlibDecoder::new(bytes).read_to_end(&mut out)?;
        parse(&out)
    } else {
        parse(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_small_compound() {
        // { "a": Int(5), "s": String("hi"), "l": List<Byte>[1,2] }
        let mut b = vec![10, 0, 0];
        b.extend([3, 0, 1, b'a', 0, 0, 0, 5]);
        b.extend([8, 0, 1, b's', 0, 2, b'h', b'i']);
        b.extend([9, 0, 1, b'l', 1, 0, 0, 0, 2, 1, 2]);
        b.push(0);
        let t = parse(&b).unwrap();
        assert_eq!(t.get("a").and_then(Tag::as_i64), Some(5));
        assert_eq!(t.get("s").and_then(Tag::as_str), Some("hi"));
        assert_eq!(t.get("l").unwrap().as_list().len(), 2);
    }
}
