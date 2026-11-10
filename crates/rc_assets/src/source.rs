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
    /// The launcher's hashed object store (`assets/objects`), keyed by
    /// `assets/<namespace>/<path>` through an asset index.
    Objects {
        index: PathBuf,
        files: AHashMap<String, PathBuf>,
    },
}

impl std::fmt::Debug for AssetSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dir(p) => write!(f, "Dir({})", p.display()),
            Self::Zip { path, .. } => write!(f, "Zip({})", path.display()),
            Self::Objects { index, files } => {
                write!(f, "Objects({}, {} files)", index.display(), files.len())
            }
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

    /// Open the launcher asset index that belongs to a client jar, for the
    /// vanilla launcher layout and for Prism/PolyMC/MultiMC.
    #[must_use]
    pub fn objects_for_jar(jar: &Path) -> Option<Self> {
        let (id, assets) = asset_index_for_jar(jar)?;
        let index = assets.join("indexes").join(format!("{id}.json"));
        let idx: serde_json::Value = serde_json::from_slice(&std::fs::read(&index).ok()?).ok()?;
        let objects = assets.join("objects");
        let mut files = AHashMap::new();
        for (k, o) in idx["objects"].as_object()? {
            let Some(h) = o["hash"].as_str() else {
                continue;
            };
            if h.len() < 2 {
                continue;
            }
            files.insert(format!("assets/{k}"), objects.join(&h[..2]).join(h));
        }
        Some(Self::Objects { index, files })
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
            Self::Objects { files, .. } => std::fs::read(files.get(path)?).ok(),
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
            Self::Objects { files, .. } => {
                let mut v: Vec<String> = files
                    .keys()
                    .filter(|n| n.starts_with(prefix))
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

    /// An independent copy (zips reopened) for use on another thread.
    #[must_use]
    pub fn reopen(&self) -> Self {
        let sources = self
            .sources
            .iter()
            .filter_map(|s| match s {
                AssetSource::Dir(p) => Some(AssetSource::Dir(p.clone())),
                AssetSource::Zip { path, .. } => AssetSource::open(path).ok(),
                AssetSource::Objects { index, files } => Some(AssetSource::Objects {
                    index: index.clone(),
                    files: files.clone(),
                }),
            })
            .collect();
        Self { sources }
    }

    /// `minecraft:block/stone` → `assets/minecraft/<kind>/block/stone<ext>`.
    #[must_use]
    pub fn read_id(&self, kind: &str, id: &str, ext: &str) -> Option<Vec<u8>> {
        let (ns, p) = id.split_once(':').unwrap_or(("minecraft", id));
        self.read(&format!("assets/{ns}/{kind}/{p}{ext}"))
    }
}

/// A directory the player chose in Options → "Minecraft Directory". It is
/// searched before the auto-detected locations.
static CUSTOM_DIR: parking_lot::RwLock<Option<PathBuf>> = parking_lot::RwLock::new(None);

/// Set (or clear) the player's chosen Minecraft directory.
pub fn set_custom_minecraft_dir(dir: Option<PathBuf>) {
    *CUSTOM_DIR.write() = dir.filter(|d| !d.as_os_str().is_empty());
}

/// The player's chosen Minecraft directory, if any.
#[must_use]
pub fn custom_minecraft_dir() -> Option<PathBuf> {
    CUSTOM_DIR.read().clone()
}

/// Candidate `.minecraft` directories for this platform (vanilla launcher
/// layout). The player's chosen directory comes first.
#[must_use]
pub fn minecraft_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(c) = custom_minecraft_dir() {
        v.push(c.clone());
        // A Prism/MultiMC instance folder holds `minecraft/` or `.minecraft/`.
        v.push(c.join(".minecraft"));
        v.push(c.join("minecraft"));
    }
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

/// Data directories of MultiMC-family launchers (Prism Launcher, PolyMC,
/// MultiMC). These keep the client jar under `libraries/`, version metadata
/// under `meta/`, a shared `assets/` store, and instances elsewhere.
#[must_use]
pub fn launcher_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(c) = custom_minecraft_dir() {
        v.push(c);
    }
    const NAMES: [&str; 3] = ["PrismLauncher", "PolyMC", "multimc"];
    if let Some(appdata) = std::env::var_os("APPDATA") {
        for n in NAMES {
            v.push(PathBuf::from(&appdata).join(n));
        }
    }
    if let Some(base) = directories::BaseDirs::new() {
        let home = base.home_dir();
        let data = base.data_dir();
        for n in NAMES {
            v.push(data.join(n));
            v.push(home.join("Library/Application Support").join(n));
        }
        v.push(home.join(".var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher"));
        v.push(home.join(".var/app/org.polymc.PolyMC/data/PolyMC"));
        v.push(home.join(".var/app/org.multimc.MultiMC/data/multimc"));
    }
    v.retain(|d| d.join("libraries").is_dir() || d.join("instances").is_dir());
    v.dedup();
    v
}

/// Read a `key=value` from a launcher's INI-style config file.
fn ini_value(file: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| v.trim().to_owned())
    })
}

/// The instances folder of a MultiMC-family launcher (`InstanceDir` in its
/// config may be relative to the data directory, or absolute).
fn launcher_instances_dir(data: &Path) -> PathBuf {
    for cfg in ["prismlauncher.cfg", "polymc.cfg", "multimc.cfg"] {
        if let Some(v) = ini_value(&data.join(cfg), "InstanceDir").filter(|v| !v.is_empty()) {
            let p = PathBuf::from(v);
            return if p.is_absolute() { p } else { data.join(p) };
        }
    }
    data.join("instances")
}

/// Game directories (`.minecraft` equivalents) of every launcher instance.
#[must_use]
pub fn launcher_instance_game_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for data in launcher_dirs() {
        let Ok(rd) = std::fs::read_dir(launcher_instances_dir(&data)) else {
            continue;
        };
        for e in rd.flatten() {
            for sub in ["minecraft", ".minecraft"] {
                let g = e.path().join(sub);
                if g.is_dir() {
                    out.push(g);
                }
            }
        }
    }
    out
}

/// Every directory whose `saves/` may hold Java worlds.
#[must_use]
pub fn game_dirs() -> Vec<PathBuf> {
    let mut v = minecraft_dirs();
    v.extend(launcher_instance_game_dirs());
    v.dedup();
    v
}

/// Pick the newest 1.21.x entry (or exactly 1.21.1) from `(version, jar)` pairs.
fn pick_version(mut found: Vec<(String, PathBuf)>) -> Option<PathBuf> {
    if let Some(i) = found.iter().position(|(v, _)| v == "1.21.1") {
        return Some(found.swap_remove(i).1);
    }
    found.retain(|(v, _)| v.starts_with("1.21"));
    found.sort();
    found.pop().map(|(_, j)| j)
}

/// Client jar in a vanilla-layout `.minecraft` directory.
fn vanilla_layout_jar(dir: &Path) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir.join("versions")).ok()?;
    pick_version(
        rd.flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let jar = e.path().join(format!("{name}.jar"));
                jar.is_file().then_some((name, jar))
            })
            .collect(),
    )
}

/// Client jar in a MultiMC-family data directory
/// (`libraries/com/mojang/minecraft/<v>/minecraft-<v>-client.jar`).
fn launcher_layout_jar(data: &Path) -> Option<PathBuf> {
    let base = data.join("libraries/com/mojang/minecraft");
    let rd = std::fs::read_dir(&base).ok()?;
    pick_version(
        rd.flatten()
            .filter_map(|e| {
                let v = e.file_name().to_string_lossy().into_owned();
                let jar = e.path().join(format!("minecraft-{v}-client.jar"));
                jar.is_file().then_some((v, jar))
            })
            .collect(),
    )
}

/// Find the player's own 1.21.1 client jar (or the closest 1.21.x), in the
/// player's chosen directory, the vanilla launcher, or Prism/PolyMC/MultiMC.
#[must_use]
pub fn find_vanilla_jar() -> Option<PathBuf> {
    let mut found = Vec::new();
    for dir in minecraft_dirs() {
        if let Some(j) = vanilla_layout_jar(&dir) {
            found.push(j);
        }
    }
    for dir in launcher_dirs() {
        if let Some(j) = launcher_layout_jar(&dir) {
            found.push(j);
        }
    }
    // Prefer an exact 1.21.1 from any launcher over a newer 1.21.x.
    found
        .iter()
        .find(|j| jar_version(j).as_deref() == Some("1.21.1"))
        .or_else(|| found.first())
        .cloned()
}

/// Version id of a client jar found by [`find_vanilla_jar`].
fn jar_version(jar: &Path) -> Option<String> {
    Some(jar.parent()?.file_name()?.to_string_lossy().into_owned())
}

/// The asset index id and the assets root (holding `indexes/` and
/// `objects/`) that belong to a client jar, for either launcher layout.
fn asset_index_for_jar(jar: &Path) -> Option<(String, PathBuf)> {
    let vdir = jar.parent()?;
    let version = vdir.file_name()?.to_string_lossy().into_owned();
    // Vanilla: versions/<v>/<v>.jar beside versions/<v>/<v>.json.
    let vjson = vdir.join(format!("{version}.json"));
    if let Some(id) = std::fs::read(&vjson)
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| v["assetIndex"]["id"].as_str().map(str::to_owned))
    {
        return Some((id, vdir.parent()?.parent()?.join("assets")));
    }
    // MultiMC family: <data>/libraries/com/mojang/minecraft/<v>/…-client.jar
    // with metadata in <data>/meta/net.minecraft/<v>.json.
    let data = vdir.ancestors().nth(5)?;
    let meta = data
        .join("meta/net.minecraft")
        .join(format!("{version}.json"));
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(meta).ok()?).ok()?;
    let id = v["assetIndex"]["id"].as_str()?.to_owned();
    Some((id, data.join("assets")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_prism_layout_jar_and_index() {
        let tmp = std::env::temp_dir().join(format!("rc_prism_{}", std::process::id()));
        let data = tmp.join("PrismLauncher");
        let jar_dir = data.join("libraries/com/mojang/minecraft/1.21.1");
        std::fs::create_dir_all(&jar_dir).unwrap();
        std::fs::write(jar_dir.join("minecraft-1.21.1-client.jar"), b"x").unwrap();
        std::fs::create_dir_all(data.join("meta/net.minecraft")).unwrap();
        std::fs::write(
            data.join("meta/net.minecraft/1.21.1.json"),
            br#"{"assetIndex":{"id":"17"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(data.join("assets/indexes")).unwrap();
        std::fs::write(
            data.join("assets/indexes/17.json"),
            br#"{"objects":{"minecraft/sounds.json":{"hash":"abcdef"}}}"#,
        )
        .unwrap();
        let inst = tmp.join("Elsewhere/Instances/My Pack/minecraft/saves");
        std::fs::create_dir_all(&inst).unwrap();
        std::fs::write(
            data.join("prismlauncher.cfg"),
            format!(
                "[General]\nInstanceDir={}\n",
                tmp.join("Elsewhere/Instances").display()
            ),
        )
        .unwrap();

        let jar = launcher_layout_jar(&data).unwrap();
        assert!(jar.ends_with("minecraft-1.21.1-client.jar"));
        let (id, assets) = asset_index_for_jar(&jar).unwrap();
        assert_eq!(id, "17");
        assert_eq!(assets, data.join("assets"));
        let Some(AssetSource::Objects { files, .. }) = AssetSource::objects_for_jar(&jar) else {
            panic!("no object index");
        };
        assert!(files.contains_key("assets/minecraft/sounds.json"));
        assert_eq!(
            launcher_instances_dir(&data),
            tmp.join("Elsewhere/Instances")
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn version_pick_prefers_exact() {
        let v = |s: &str| (s.to_owned(), PathBuf::from(s));
        assert_eq!(
            pick_version(vec![v("1.21.4"), v("1.21.1"), v("1.20.6")]),
            Some(PathBuf::from("1.21.1"))
        );
        assert_eq!(
            pick_version(vec![v("1.21.4"), v("1.21.3"), v("1.20.6")]),
            Some(PathBuf::from("1.21.4"))
        );
        assert_eq!(pick_version(vec![v("1.20.6")]), None);
    }
}
