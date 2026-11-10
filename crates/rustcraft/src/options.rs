//! Player options, persisted as `options.json` in the game directory.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Graphics {
    Fast,
    Fancy,
}

/// Performance preset applied by the "Preset" button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    Potato,
    Low,
    Medium,
    High,
    Ultra,
}

impl Preset {
    pub const ALL: [Preset; 5] = [
        Self::Potato,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::Ultra,
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Potato => "Potato",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Ultra => "Ultra",
        }
    }
}

/// Largest selectable chunk render distance (full-detail chunks; LOD
/// rendering later extends the view to 1024 chunks).
pub const MAX_RENDER_DISTANCE: i32 = 128;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub render_distance: i32,
    pub fov: f32,
    /// 0 = auto.
    pub gui_scale: u32,
    pub smooth_lighting: bool,
    pub graphics: Graphics,
    pub clouds: bool,
    /// 0 = unlimited.
    pub max_fps: u32,
    pub vsync: bool,
    pub brightness: f32,
    pub view_bobbing: bool,
    pub mouse_sensitivity: f32,
    pub invert_mouse: bool,
    pub fullscreen: bool,
    pub mipmap_levels: u32,
    pub anisotropy: u16,
    // Resource limits (applied live where possible; worker threads on world load).
    /// Maximum memory for chunk caches, in MiB.
    pub max_memory_mb: u32,
    /// Worker threads for generation/lighting/meshing; 0 = auto.
    pub cpu_threads: u32,
    /// Percentage of GPU time the game may use (frame cap relative to refresh).
    pub gpu_limit_percent: u32,
    pub update_policy: rc_update::UpdatePolicy,
    /// Explicit path to a Minecraft jar or assets folder.
    pub assets_path: Option<String>,
    /// Minecraft directory chosen by the player (a `.minecraft`, a Prism /
    /// MultiMC data folder, or an instance folder); `None` = auto-detect.
    pub minecraft_dir: Option<String>,
    pub show_fps: bool,
    pub preset: Preset,
    /// Volume per sound category (vanilla order, see `rc_audio::Category::ALL`).
    pub volumes: [f32; 8],
}

impl Default for Options {
    fn default() -> Self {
        Self {
            render_distance: 10,
            fov: 70.0,
            gui_scale: 0,
            smooth_lighting: true,
            graphics: Graphics::Fancy,
            clouds: true,
            max_fps: 120,
            vsync: true,
            brightness: 0.5,
            view_bobbing: true,
            mouse_sensitivity: 0.5,
            invert_mouse: false,
            fullscreen: false,
            mipmap_levels: 4,
            anisotropy: 1,
            max_memory_mb: default_memory_mb(),
            cpu_threads: 0,
            gpu_limit_percent: 100,
            update_policy: rc_update::UpdatePolicy::Ask,
            assets_path: None,
            minecraft_dir: None,
            show_fps: false,
            preset: Preset::High,
            volumes: [1.0, 0.5, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        }
    }
}

/// Logical CPUs available.
#[must_use]
pub fn cpu_count() -> usize {
    std::thread::available_parallelism().map_or(2, std::num::NonZero::get)
}

fn default_memory_mb() -> u32 {
    2048
}

impl Options {
    pub fn load(path: &Path) -> Self {
        let mut o: Self = std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        o.sanitize();
        o
    }

    /// Clamp every value into its supported range, so a hand-edited file or
    /// one written by another version can never crash startup.
    pub fn sanitize(&mut self) {
        let d = Self::default();
        let finite = |v: f32, lo: f32, hi: f32, def: f32| {
            if v.is_finite() { v.clamp(lo, hi) } else { def }
        };
        self.render_distance = self.render_distance.clamp(2, MAX_RENDER_DISTANCE);
        self.fov = finite(self.fov, 30.0, 110.0, d.fov);
        self.gui_scale = self.gui_scale.min(8);
        self.brightness = finite(self.brightness, 0.0, 1.0, d.brightness);
        self.mouse_sensitivity = finite(self.mouse_sensitivity, 0.0, 1.0, d.mouse_sensitivity);
        self.mipmap_levels = self.mipmap_levels.min(4);
        self.anisotropy = 1;
        self.max_memory_mb = self.max_memory_mb.max(256);
        self.gpu_limit_percent = self.gpu_limit_percent.clamp(10, 100);
        for v in &mut self.volumes {
            *v = finite(*v, 0.0, 1.0, 1.0);
        }
    }

    /// Tell the asset finder about the player's chosen Minecraft directory.
    pub fn apply_minecraft_dir(&self) {
        rc_assets::source::set_custom_minecraft_dir(
            self.minecraft_dir
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(std::path::PathBuf::from),
        );
    }

    pub fn save(&self, path: &Path) {
        if let Ok(s) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(path, s);
        }
    }

    /// Worker thread count after applying the limit: leave headroom for the
    /// render thread and the OS so the machine never saturates.
    #[must_use]
    pub fn worker_threads(&self) -> usize {
        let n = cpu_count();
        if self.cpu_threads == 0 {
            n.saturating_sub(2).clamp(1, 32)
        } else {
            (self.cpu_threads as usize).clamp(1, n)
        }
    }

    /// Render distance after the memory limit.
    #[must_use]
    pub fn effective_render_distance(&self) -> i32 {
        // ~380 MiB is fixed cost (GPU driver, assets, atlas, worldgen
        // caches); loaded chunks with meshes and worldgen in flight cost
        // ~0.15 MiB each (measured with `--scene bench`, 0.2.1).
        let budget_chunks = ((f64::from(self.max_memory_mb) - 380.0).max(16.0) / 0.15) as i32;
        let mut rd = self.render_distance.clamp(2, MAX_RENDER_DISTANCE);
        while rd > 2 && (2 * rd + 7).pow(2) > budget_chunks {
            rd -= 1;
        }
        rd
    }

    /// Frame cap from max_fps and the GPU limit (0 = uncapped).
    #[must_use]
    pub fn frame_cap(&self) -> u32 {
        let base = if self.max_fps == 0 { 240 } else { self.max_fps };
        if self.gpu_limit_percent >= 100 && self.max_fps == 0 {
            0
        } else {
            (base * self.gpu_limit_percent.clamp(10, 100) / 100).max(10)
        }
    }

    pub fn apply_preset(&mut self, p: Preset) {
        self.preset = p;
        match p {
            Preset::Potato => {
                self.render_distance = 3;
                self.graphics = Graphics::Fast;
                self.smooth_lighting = false;
                self.clouds = false;
                self.mipmap_levels = 0;
                self.max_fps = 60;
                self.max_memory_mb = 512;
            }
            Preset::Low => {
                self.render_distance = 6;
                self.graphics = Graphics::Fast;
                self.smooth_lighting = true;
                self.clouds = false;
                self.mipmap_levels = 2;
                self.max_memory_mb = 1024;
            }
            Preset::Medium => {
                self.render_distance = 10;
                self.graphics = Graphics::Fancy;
                self.smooth_lighting = true;
                self.clouds = true;
                self.mipmap_levels = 4;
                self.max_memory_mb = 2048;
            }
            Preset::High => {
                self.render_distance = 16;
                self.graphics = Graphics::Fancy;
                self.smooth_lighting = true;
                self.clouds = true;
                self.mipmap_levels = 4;
                self.max_memory_mb = 4096;
            }
            Preset::Ultra => {
                self.render_distance = 32;
                self.graphics = Graphics::Fancy;
                self.smooth_lighting = true;
                self.clouds = true;
                self.mipmap_levels = 4;
                self.max_memory_mb = 12288;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_limit_caps_render_distance() {
        let mut o = Options {
            render_distance: 32,
            max_memory_mb: 512,
            ..Options::default()
        };
        assert!(o.effective_render_distance() <= 12);
        o.max_memory_mb = 32768;
        assert_eq!(o.effective_render_distance(), 32);
        o.render_distance = 500;
        o.max_memory_mb = u32::MAX / 2;
        assert_eq!(o.effective_render_distance(), MAX_RENDER_DISTANCE);
    }
}
