//! Packs `data/vanilla/**` into one zstd blob embedded in the binary.

use std::io::Write;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, base: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, base, out);
        } else if p.extension().is_some_and(|e| e == "json") {
            let rel = p
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, std::fs::read(&p).unwrap()));
        }
    }
}

fn main() {
    let root =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../data/vanilla");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    let mut raw = Vec::new();
    raw.extend_from_slice(&(files.len() as u32).to_le_bytes());
    for (name, data) in &files {
        raw.extend_from_slice(&(name.len() as u16).to_le_bytes());
        raw.extend_from_slice(name.as_bytes());
        raw.extend_from_slice(&(data.len() as u32).to_le_bytes());
        raw.extend_from_slice(data);
    }
    let compressed = zstd::encode_all(&raw[..], 19).unwrap();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("vanilla.bin.zst");
    std::fs::File::create(out)
        .unwrap()
        .write_all(&compressed)
        .unwrap();
}
