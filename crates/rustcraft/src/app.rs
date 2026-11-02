//! Window + event loop (winit 0.30 `ApplicationHandler`).

use std::sync::Arc;

use anyhow::Result;
use tracing::{error, info};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub size: (u32, u32),
    pub check_updates: bool,
}

struct App {
    options: Options,
    window: Option<Arc<Window>>,
    gpu: Option<rc_render::GpuContext>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(format!("RustCraft {}", rc_core::version::VERSION))
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.options.size.0,
                self.options.size.1,
            ));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                error!("failed to create window: {e}");
                event_loop.exit();
                return;
            }
        };
        let size = window.inner_size();
        match rc_render::GpuContext::new_windowed(window.clone(), size.width, size.height) {
            Ok(gpu) => self.gpu = Some(gpu),
            Err(e) => {
                error!("failed to initialise GPU: {e:#}");
                event_loop.exit();
                return;
            }
        }
        self.window = Some(window);
        if self.options.check_updates {
            info!(policy = ?rc_update::UpdatePolicy::default(), "update check: not implemented yet (Phase 1)");
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(gpu) = &mut self.gpu
                    && let Err(e) = gpu.render_frame()
                {
                    error!("render error: {e:#}");
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }
}

pub fn run(options: Options) -> Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        options,
        window: None,
        gpu: None,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}
