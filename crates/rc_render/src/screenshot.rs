//! Offscreen rendering and PNG capture.
//!
//! Used by F2 in game and by `rustcraft --screenshot` / `cargo xtask screenshot`
//! for headless CI captures.

use std::path::Path;

use anyhow::{Context as _, Result};

use crate::{GpuContext, OFFSCREEN_FORMAT};

/// Render one frame into an offscreen texture of `width`×`height` and return
/// tightly packed RGBA8 pixels.
pub fn render_to_rgba(ctx: &GpuContext, width: u32, height: u32) -> Result<Vec<u8>> {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = ctx.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("screenshot"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OFFSCREEN_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    // Rows must be 256-byte aligned for buffer copies.
    let unpadded = width as usize * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
    let padded = unpadded.div_ceil(align) * align;
    let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("screenshot-readback"),
        size: (padded * height as usize) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("screenshot"),
        });
    ctx.clear_pass(&mut encoder, &view);
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded as u32),
                rows_per_image: Some(height),
            },
        },
        size,
    );
    ctx.queue.submit([encoder.finish()]);

    let (tx, rx) = std::sync::mpsc::channel();
    buffer.map_async(wgpu::MapMode::Read, .., move |r| {
        let _ = tx.send(r);
    });
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .context("device poll failed")?;
    rx.recv().context("map callback dropped")??;

    let data = buffer
        .get_mapped_range(..)
        .context("mapping readback buffer")?;
    let mut out = Vec::with_capacity(unpadded * height as usize);
    for row in data.chunks(padded) {
        out.extend_from_slice(&row[..unpadded]);
    }
    drop(data);
    buffer.unmap();
    Ok(out)
}

/// Render and save a PNG.
pub fn capture_png(ctx: &GpuContext, width: u32, height: u32, path: &Path) -> Result<()> {
    let rgba = render_to_rgba(ctx, width, height)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    image::save_buffer(path, &rgba, width, height, image::ExtendedColorType::Rgba8)
        .with_context(|| format!("writing {}", path.display()))
}

/// Copy a texture (RGBA8 or BGRA8, `COPY_SRC`) back to tightly packed RGBA8.
pub fn readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    let unpadded = width as usize * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
    let padded = unpadded.div_ceil(align) * align;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (padded * height as usize) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("readback"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded as u32),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.map_async(wgpu::MapMode::Read, .., move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .context("device poll failed")?;
    rx.recv().context("map callback dropped")??;
    let data = buffer
        .get_mapped_range(..)
        .context("mapping readback buffer")?;
    let bgra = matches!(
        texture.format(),
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    );
    let mut out = Vec::with_capacity(unpadded * height as usize);
    for row in data.chunks(padded) {
        let row = &row[..unpadded];
        if bgra {
            for px in row.chunks(4) {
                out.extend_from_slice(&[px[2], px[1], px[0], 255]);
            }
        } else {
            for px in row.chunks(4) {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
    }
    drop(data);
    buffer.unmap();
    Ok(out)
}

/// Save RGBA8 pixels as PNG.
pub fn save_png(rgba: &[u8], width: u32, height: u32, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    image::save_buffer(path, rgba, width, height, image::ExtendedColorType::Rgba8)
        .with_context(|| format!("writing {}", path.display()))
}
