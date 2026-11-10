//! Audio: sound events from the player's own `sounds.json`, decoded on a
//! background thread, mixed by rodio, with distance attenuation and stereo
//! panning relative to the listener.

#![forbid(unsafe_code)]

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use ahash::AHashMap;
use parking_lot::Mutex;
use rand::Rng as _;
use rc_assets::AssetStack;
use rodio::Source as _;
use tracing::{info, warn};

/// Volume categories (vanilla's sound sources).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Master,
    Music,
    Blocks,
    Weather,
    Hostile,
    Neutral,
    Players,
    Ambient,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Master,
        Category::Music,
        Category::Blocks,
        Category::Weather,
        Category::Hostile,
        Category::Neutral,
        Category::Players,
        Category::Ambient,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Master => "Master Volume",
            Self::Music => "Music",
            Self::Blocks => "Blocks",
            Self::Weather => "Weather",
            Self::Hostile => "Hostile Creatures",
            Self::Neutral => "Friendly Creatures",
            Self::Players => "Players",
            Self::Ambient => "Ambient/Environment",
        }
    }
}

#[derive(Clone, Debug)]
struct Variant {
    file: String,
    volume: f32,
    pitch: f32,
    weight: u32,
    /// Reference to another event instead of a file.
    event: bool,
    attenuation: f32,
    /// Long sounds (music, records) are streamed, never decoded whole.
    stream: bool,
}

type Pcm = Arc<(u16, u32, Arc<[f32]>)>;

struct Request {
    file: String,
    stream: bool,
    volume: f32,
    pitch: f32,
    pan: (f32, f32),
}

/// The sound system. Cheap to call from the game thread: `play` only
/// queues work.
pub struct Audio {
    events: AHashMap<String, Vec<Variant>>,
    tx: Option<Sender<Request>>,
    listener: Mutex<([f64; 3], f32)>,
    pub volumes: Mutex<AHashMap<Category, f32>>,
    music_playing: Arc<Mutex<bool>>,
}

impl std::fmt::Debug for Audio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Audio({} events)", self.events.len())
    }
}

fn parse_events(v: &serde_json::Value) -> AHashMap<String, Vec<Variant>> {
    let mut out = AHashMap::new();
    let Some(obj) = v.as_object() else { return out };
    for (k, e) in obj {
        let list = e["sounds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| match s {
                serde_json::Value::String(n) => Some(Variant {
                    file: n.clone(),
                    volume: 1.0,
                    pitch: 1.0,
                    weight: 1,
                    event: false,
                    attenuation: 16.0,
                    stream: false,
                }),
                serde_json::Value::Object(o) => Some(Variant {
                    file: o.get("name")?.as_str()?.to_owned(),
                    volume: o
                        .get("volume")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(1.0) as f32,
                    pitch: o
                        .get("pitch")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(1.0) as f32,
                    weight: o
                        .get("weight")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(1) as u32,
                    event: o.get("type").and_then(serde_json::Value::as_str) == Some("event"),
                    attenuation: o
                        .get("attenuation_distance")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(16.0) as f32,
                    stream: o.get("stream").and_then(serde_json::Value::as_bool) == Some(true),
                }),
                _ => None,
            })
            .collect();
        out.insert(format!("minecraft:{k}"), list);
        out.insert(k.clone(), out[&format!("minecraft:{k}")].clone());
    }
    out
}

fn decode(bytes: Vec<u8>) -> Option<Pcm> {
    let d = rodio::Decoder::new(std::io::Cursor::new(bytes)).ok()?;
    let ch = d.channels();
    let rate = d.sample_rate();
    let samples: Vec<f32> = d.collect();
    Some(Arc::new((ch, rate, samples.into())))
}

/// Decoded samples kept for short sounds: 8 M samples = 32 MB.
const CACHE_SAMPLES: usize = 8 * 1024 * 1024;

/// Plays cached samples without copying them.
struct Shared {
    data: Arc<[f32]>,
    pos: usize,
    channels: u16,
    rate: u32,
}

impl Shared {
    fn new(pcm: &Pcm) -> Self {
        Self {
            data: pcm.2.clone(),
            pos: 0,
            channels: pcm.0,
            rate: pcm.1,
        }
    }
}

impl Iterator for Shared {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let v = self.data.get(self.pos).copied();
        self.pos += 1;
        v
    }
}

impl rodio::Source for Shared {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.data.len().saturating_sub(self.pos)).filter(|n| *n > 0)
    }

    fn channels(&self) -> u16 {
        self.channels
    }

    fn sample_rate(&self) -> u32 {
        self.rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        let frames = self.data.len() / usize::from(self.channels.max(1));
        Some(std::time::Duration::from_secs_f64(
            frames as f64 / f64::from(self.rate.max(1)),
        ))
    }
}

fn worker(rx: Receiver<Request>, files: Arc<AssetStack>) {
    let stream = match rodio::OutputStreamBuilder::open_default_stream() {
        Ok(mut s) => {
            s.log_on_drop(false);
            s
        }
        Err(e) => {
            warn!("no audio output device: {e}");
            // Drain requests so senders never block.
            while rx.recv().is_ok() {}
            return;
        }
    };
    info!("audio output opened");
    let mut cache: AHashMap<String, Option<Pcm>> = AHashMap::new();
    let mut cached_samples = 0usize;
    while let Ok(r) = rx.recv() {
        let (ns, p) = r.file.split_once(':').unwrap_or(("minecraft", &r.file));
        let path = format!("assets/{ns}/sounds/{p}.ogg");
        if r.stream {
            // Music: decode while playing from the compressed bytes (a few
            // MB) instead of holding ~90 MB of samples per track (issue #2).
            let Some(bytes) = files.read(&path) else {
                continue;
            };
            let Ok(d) = rodio::Decoder::new(std::io::Cursor::new(bytes)) else {
                continue;
            };
            stream.mixer().add(d.amplify(r.volume.clamp(0.0, 4.0)));
            continue;
        }
        if !cache.contains_key(&r.file) {
            let pcm = files.read(&path).and_then(decode);
            cached_samples += pcm.as_ref().map_or(0, |p| p.2.len());
            // Keep the decoded short-sound cache bounded (~32 MB).
            if cached_samples > CACHE_SAMPLES {
                cache.clear();
                cached_samples = pcm.as_ref().map_or(0, |p| p.2.len());
            }
            cache.insert(r.file.clone(), pcm);
        }
        let Some(pcm) = cache.get(&r.file).cloned().flatten() else {
            continue;
        };
        let ch = pcm.0;
        let src = Shared::new(&pcm)
            .speed(r.pitch.clamp(0.5, 2.0))
            .amplify(r.volume.clamp(0.0, 4.0));
        if ch == 1 {
            let out_ch = stream.config().channel_count();
            let mut vols = vec![r.pan.0; usize::from(out_ch.max(1))];
            if vols.len() >= 2 {
                vols[1] = r.pan.1;
            }
            stream
                .mixer()
                .add(rodio::source::ChannelVolume::new(src, vols));
        } else {
            stream.mixer().add(src);
        }
    }
}

impl Audio {
    /// Start the audio thread. With no `sounds.json` (fallback pack) the
    /// system is silent but still usable.
    #[must_use]
    pub fn new(assets: Arc<AssetStack>) -> Self {
        let events = assets
            .read("assets/minecraft/sounds.json")
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .map(|v| parse_events(&v))
            .unwrap_or_default();
        let tx = if events.is_empty() {
            info!("no sounds.json found; audio disabled");
            None
        } else {
            let (tx, rx) = channel();
            let files = assets.clone();
            std::thread::Builder::new()
                .name("rc-audio".into())
                .spawn(move || worker(rx, files))
                .ok()
                .map(|_| tx)
        };
        let volumes = Category::ALL.iter().map(|c| (*c, 1.0)).collect();
        Self {
            events,
            tx,
            listener: Mutex::new(([0.0; 3], 0.0)),
            volumes: Mutex::new(volumes),
            music_playing: Arc::new(Mutex::new(false)),
        }
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        self.tx.is_some()
    }

    #[must_use]
    pub fn has_event(&self, event: &str) -> bool {
        self.events.contains_key(event)
    }

    pub fn set_listener(&self, pos: [f64; 3], yaw: f32) {
        *self.listener.lock() = (pos, yaw);
    }

    fn pick(&self, event: &str, depth: u32) -> Option<Variant> {
        let list = self.events.get(event)?;
        let total: u32 = list.iter().map(|v| v.weight).sum();
        if total == 0 {
            return None;
        }
        let mut r = rand::rng().random_range(0..total);
        let v = list.iter().find(|v| {
            if r < v.weight {
                true
            } else {
                r -= v.weight;
                false
            }
        })?;
        if v.event && depth < 4 {
            return self.pick(&v.file, depth + 1).map(|mut x| {
                x.volume *= v.volume;
                x.pitch *= v.pitch;
                x
            });
        }
        Some(v.clone())
    }

    /// Play `event` at `pos` (None = non-positional, e.g. UI).
    pub fn play(&self, event: &str, pos: Option<[f64; 3]>, volume: f32, pitch: f32, cat: Category) {
        let Some(tx) = &self.tx else { return };
        let Some(v) = self.pick(event, 0) else { return };
        let vols = self.volumes.lock();
        let master = vols.get(&Category::Master).copied().unwrap_or(1.0);
        let catv = if cat == Category::Master {
            1.0
        } else {
            vols.get(&cat).copied().unwrap_or(1.0)
        };
        drop(vols);
        let mut vol = volume * v.volume * master * catv;
        let mut pan = (1.0, 1.0);
        if let Some(p) = pos {
            let (l, yaw) = *self.listener.lock();
            let d = [p[0] - l[0], p[1] - l[1], p[2] - l[2]];
            let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() as f32;
            let range = v.attenuation * volume.max(1.0);
            if dist > range {
                return;
            }
            vol *= (1.0 - dist / range).max(0.0);
            // Right vector for yaw (0 = +Z): (-cos, 0, -sin).
            let (sy, cy) = f64::from(yaw).to_radians().sin_cos();
            let right = -d[0] * cy - d[2] * sy;
            let side = if dist > 0.01 {
                (right as f32 / dist).clamp(-1.0, 1.0)
            } else {
                0.0
            };
            pan = ((1.0 - side * 0.6).min(1.0), (1.0 + side * 0.6).min(1.0));
        }
        if vol <= 0.001 {
            return;
        }
        let _ = tx.send(Request {
            stream: v.stream || cat == Category::Music,
            file: v.file,
            volume: vol,
            pitch: pitch * v.pitch,
            pan,
        });
    }

    /// Start a random track of `event` if none is playing (music).
    pub fn play_music(&self, event: &str) {
        let mut playing = self.music_playing.lock();
        if *playing {
            return;
        }
        *playing = true;
        drop(playing);
        self.play(event, None, 1.0, 1.0, Category::Music);
        // Tracks are long; allow another one after a while.
        let flag = self.music_playing.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(300));
            *flag.lock() = false;
        });
    }
}

/// Sound group (`block.<group>.*` events) for a block name.
#[must_use]
pub fn block_sound_group(name: &str, has: impl Fn(&str) -> bool) -> String {
    if has(&format!("block.{name}.break")) {
        return name.to_owned();
    }
    let n = name;
    let g = if n.ends_with("leaves")
        || n.contains("grass")
        || n.contains("fern")
        || n.contains("sapling")
        || n.contains("flower")
        || n.contains("tulip")
        || n == "poppy"
        || n == "dandelion"
        || n.contains("orchid")
        || n.contains("bush")
        || n.contains("vine")
        || n == "lily_pad"
        || n.contains("seagrass")
        || n.contains("kelp")
        || n.contains("mushroom") && !n.contains("block")
        || n == "sugar_cane"
        || n == "hay_block"
        || n.contains("dried_kelp")
        || n == "sponge"
        || n == "wet_sponge"
        || n.contains("allium")
        || n.contains("bluet")
        || n.contains("daisy")
        || n.contains("cornflower")
        || n.contains("lily_of")
        || n.contains("lilac")
        || n.contains("peony")
        || n.contains("rose")
        || n == "sunflower"
        || n == "tnt"
        || n == "mycelium"
        || n == "podzol"
    {
        "grass"
    } else if n.contains("log")
        || n.contains("wood")
        || n.contains("planks")
        || n.contains("fence") && !n.contains("nether")
        || n.contains("door")
        || n.contains("chest")
        || n.contains("crafting")
        || n.contains("bookshelf")
        || n.contains("barrel")
        || n.contains("sign")
        || n.contains("stem")
        || n == "note_block"
        || n.contains("loom")
        || n.contains("_table")
        || n == "composter"
        || n.contains("campfire")
        || n == "jukebox"
        || n == "ladder"
        || n.contains("bed")
        || n.contains("button")
        || n.contains("pressure_plate") && !n.contains("stone")
    {
        "wood"
    } else if n.contains("sand") && !n.contains("sandstone") || n.contains("concrete_powder") {
        "sand"
    } else if n.contains("gravel")
        || n.contains("dirt")
        || n == "farmland"
        || n == "clay"
        || n.contains("path")
    {
        "gravel"
    } else if n.contains("glass")
        || n.contains("ice")
        || n.contains("lantern") && false
        || n == "beacon"
        || n.contains("sea_lantern")
    {
        "glass"
    } else if n.contains("wool") || n.contains("carpet") {
        "wool"
    } else if n.contains("snow") {
        "snow"
    } else if n.contains("iron") && n.contains("block")
        || n.contains("gold_block")
        || n.contains("diamond_block")
        || n.contains("emerald_block")
        || n.contains("hopper")
        || n.contains("cauldron")
        || n.contains("rail")
        || n.contains("iron_bars")
        || n.contains("iron_door")
        || n.contains("iron_trapdoor")
    {
        "metal"
    } else {
        "stone"
    };
    g.to_owned()
}
