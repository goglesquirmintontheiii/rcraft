//! GPU device/queue/surface ownership.

use std::sync::Arc;

use anyhow::{Context as _, Result};
use tracing::info;

/// A window surface plus its current configuration.
pub struct SurfaceTarget {
    pub surface: wgpu::Surface<'static>,
    pub config: wgpu::SurfaceConfiguration,
}

/// Everything needed to submit GPU work.
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub surface: Option<SurfaceTarget>,
}

impl std::fmt::Debug for GpuContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuContext")
            .field("adapter", &self.adapter.get_info().name)
            .field("has_surface", &self.surface.is_some())
            .finish()
    }
}

impl GpuContext {
    /// Create a context for a window. `window` must outlive the context, which
    /// is why we take an `Arc` (winit windows are shared this way).
    pub fn new_windowed<W>(window: Arc<W>, width: u32, height: u32) -> Result<Self>
    where
        W: raw_window_handle::HasWindowHandle
            + raw_window_handle::HasDisplayHandle
            + Send
            + Sync
            + 'static,
    {
        let instance = Self::instance();
        let surface = instance
            .create_surface(window)
            .context("failed to create window surface")?;
        let (adapter, device, queue) = Self::adapter_and_device(&instance, Some(&surface))?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            color_space: wgpu::SurfaceColorSpace::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        Ok(Self {
            instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
            surface: Some(SurfaceTarget { surface, config }),
        })
    }

    /// Create a context with no window (CI screenshots, tests, tools).
    pub fn new_headless() -> Result<Self> {
        let instance = Self::instance();
        let (adapter, device, queue) = Self::adapter_and_device(&instance, None)?;
        Ok(Self {
            instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
            surface: None,
        })
    }

    fn instance() -> wgpu::Instance {
        // `WGPU_BACKEND` / `WGPU_POWER_PREF` etc. are honoured, which is how CI
        // forces Vulkan (lavapipe) and users can work around driver bugs.
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
    }

    fn adapter_and_device(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: surface,
            ..Default::default()
        }))
        .context("no compatible GPU adapter found")?;
        let info = adapter.get_info();
        info!(
            adapter = %info.name,
            backend = ?info.backend,
            driver = %info.driver_info,
            "selected GPU adapter"
        );
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("rustcraft"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .context("failed to create GPU device")?;
        Ok((adapter, device, queue))
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if let Some(s) = &mut self.surface {
            s.config.width = width.max(1);
            s.config.height = height.max(1);
            s.surface.configure(&self.device, &s.config);
        }
    }

    /// Record a clear pass into `view`. Placeholder for the real frame graph.
    pub fn clear_pass(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(crate::CLEAR_COLOR),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }

    /// Render one frame to the window surface.
    pub fn render_frame(&mut self) -> Result<()> {
        let Some(target) = &self.surface else {
            return Ok(());
        };
        use wgpu::CurrentSurfaceTexture as Cst;
        let frame = match target.surface.get_current_texture() {
            Cst::Success(f) | Cst::Suboptimal(f) => f,
            Cst::Timeout | Cst::Occluded => return Ok(()),
            Cst::Validation => anyhow::bail!("surface validation error"),
            Cst::Outdated | Cst::Lost => {
                let (w, h) = (target.config.width, target.config.height);
                self.resize(w, h);
                return Ok(());
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        self.clear_pass(&mut encoder, &view);
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        Ok(())
    }
}
