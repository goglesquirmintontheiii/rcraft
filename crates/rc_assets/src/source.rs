//! Where asset files come from: a directory, a zip/jar, or nothing.
//!
//! Vanilla assets are never shipped. The player points RustCraft at their own
//! Minecraft install (auto-detected) and the client jar is read in place.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use ahash::AHashMap;
use parking_lot::Mutex;

/// One layer of assets (a resource pack, the vanilla jar, …).
pub enum AssetSource {
    Dir(PathBuf),
    Zip {
        path: PathBuf,
        archive: Mutex<zip::ZipArchive<File>>,
        names: AHashMap<String, usize>,
    },
}

impl std::fmt::Debug for AssetSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dir(p) => write!(f, "Dir({})", p.display()),
            Self::Zip { path, .. } => write!(f, "Zip({})", path.display()),
        }
    }
}

impl AssetSource {
    /// Open a directory containing `assets/` or a zip/jar containing it.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if path.is_dir() {
            return Ok(Self::Dir(path.to_owned()));
        }
        let archive = zip::ZipArchive::new(File::open(path)?)?;
        let mut names = AHashMap::new();
        for i in 0..archive.len() {
            if let Some(Ok(n)) = archive.name_for_index(i) {
                names.insert(n.to_string(), i);
            }
        }
        Ok(Self::Zip {
            path: path.to_owned(),
            archive: Mutex::new(archive),
            names,
        })
    }

    /// Read `path` (relative, `/`-separated, e.g. `assets/minecraft/…`).
    #[must_use]
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        match self {
            Self::Dir(root) => std::fs::read(root.join(path)).ok(),
            Self::Zip { archive, names, .. } => {
                let &i = names.get(path)?;
                let mut a = archive.lock();
                let mut f = a.by_index(i).ok()?;
                let mut v = Vec::with_capacity(f.size() as usize);
                f.read_to_end(&mut v).ok()?;
                Some(v)
            }
        }
    }

    /// File paths under `prefix` (recursive).
    #[must_use]
    pub fn list(&self, prefix: &str) -> Vec<String> {
        match self {
            Self::Dir(root) => {
                let mut out = Vec::new();
                let base = root.join(prefix);
                let mut stack = vec![base];
                while let Some(d) = stack.pop() {
                    let Ok(rd) = std::fs::read_dir(&d) else {
                        continue;
                    };
                    for e in rd.flatten() {
                        let p = e.path();
                        if p.is_dir() {
                            stack.push(p);
                        } else if let Ok(rel) = p.strip_prefix(root) {
                            out.push(rel.to_string_lossy().replace('\\', "/"));
                        }
                    }
                }
                out.sort();
                out
            }
            Self::Zip { names, .. } => {
                let mut v: Vec<String> = names
                    .keys()
                    .filter(|n| n.starts_with(prefix) && !n.ends_with('/'))
                    .cloned()
                    .collect();
                v.sort();
                v
            }
        }
    }
}

/// Ordered stack of sources; earlier entries override later ones.
#[derive(Debug, Default)]
pub struct AssetStack {
    pub sources: Vec<AssetSource>,
}

impl AssetStack {
    #[must_use]
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.sources.iter().find_map(|s| s.read(path))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// `minecraft:block/stone` → `assets/minecraft/<kind>/block/stone<ext>`.
    #[must_use]
    pub fn read_id(&self, kind: &str, id: &str, ext: &str) -> Option<Vec<u8>> {
        let (ns, p) = id.split_once(':').unwrap_or(("minecraft", id));
        self.read(&format!("assets/{ns}/{kind}/{p}{ext}"))
    }
}

/// Candidate `.minecraft` directories for this platform.
#[must_use]
pub fn minecraft_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        v.push(PathBuf::from(appdata).join(".minecraft"));
    }
    if let Some(base) = directories::BaseDirs::new() {
        let home = base.home_dir();
        v.push(home.join(".minecraft"));
        v.push(home.join("Library/Application Support/minecraft"));
        v.push(home.join(".var/app/com.mojang.Minecraft/.minecraft"));
    }
    v
}

/// Find the player's own 1.21.1 client jar (or the closest 1.21.x).
#[must_use]
pub fn find_vanilla_jar() -> Option<PathBuf> {
    for dir in minecraft_dirs() {
        let exact = dir.join("versions/1.21.1/1.21.1.jar");
        if exact.is_file() {
            return Some(exact);
        }
        let Ok(rd) = std::fs::read_dir(dir.join("versions")) else {
            continue;
        };
        let mut found: Vec<PathBuf> = rd
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let jar = e.path().join(format!("{name}.jar"));
                (name.starts_with("1.21") && jar.is_file()).then_some(jar)
            })
            .collect();
        found.sort();
        if let Some(j) = found.pop() {
            return Some(j);
        }
    }
    None
}
