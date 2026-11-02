//! RCW on-disk format constants and record framing.
//!
//! Full specification: `docs/WORLD_FORMAT.md`. Phase 0 fixes the magic numbers,
//! header sizes and the checksum so the format can't drift silently; the
//! region reader/writer lands in Phase 1.

pub const REGION_MAGIC: &[u8; 4] = b"RCWR";
pub const CHUNK_MAGIC: &[u8; 4] = b"RCWC";
pub const WORLD_MAGIC: &[u8; 4] = b"RCWW";
pub const FORMAT_VERSION: u16 = 1;
pub const SECTOR_BYTES: usize = 4096;
/// 4 sectors: 8-byte preamble + 1024 × 12-byte chunk table entries (12 296 B), padded.
pub const REGION_HEADER_BYTES: usize = 4 * SECTOR_BYTES;
const _: () = assert!(CHUNKS_PER_REGION * 12 + 8 <= REGION_HEADER_BYTES);
pub const CHUNKS_PER_REGION: usize = 32 * 32;

/// Checksum used for every record.
#[must_use]
pub fn checksum(bytes: &[u8]) -> u64 {
    xxhash_rust::xxh3::xxh3_64(bytes)
}

#[cfg(test)]
mod tests {
    #[test]
    fn checksum_is_stable() {
        assert_eq!(super::checksum(b""), 0x2d06_8005_38d3_94c2);
    }
}
