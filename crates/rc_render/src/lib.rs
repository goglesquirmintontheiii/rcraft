//! wgpu renderer.
//!
//! Phase 0: GPU context creation (windowed or headless), a clear pass, and
//! offscreen screenshot capture. Phase 1 adds chunk meshing, the texture
//! atlas, lighting and all world passes.

#![forbid(unsafe_code)]
// Renderer/UI entry points take many plain parameters by design.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

pub mod context;
pub mod gui;
pub mod mesh;
pub mod screenshot;
pub mod terrain;

pub use context::{GpuContext, SurfaceTarget};

/// The sky-blue colour vanilla shows before any world is loaded, used as the
/// clear colour until the real sky renderer exists.
pub const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 0.47,
    g: 0.65,
    b: 1.0,
    a: 1.0,
};

/// Format used for offscreen render targets and screenshots.
pub const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
