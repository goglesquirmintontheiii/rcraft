//! Window + event loop (winit 0.30 `ApplicationHandler`).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tracing::{error, info, warn};
use winit::application::ApplicationHandler;
use winit::event::{
    DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Fullscreen, Window, WindowId};

use crate::engine::{Engine, GameKey};
use crate::gfx::Gfx;
use crate::options::Options;

#[derive(Debug, Clone)]
pub struct LaunchOptions {
    pub size: (u32, u32),
    pub check_updates: bool,
    pub game_dir: PathBuf,
    pub assets: Option<PathBuf>,
}

struct App {
    launch: LaunchOptions,
    window: Option<Arc<Window>>,
    gpu: Option<rc_render::GpuContext>,
    engine: Option<Engine>,
    last: Instant,
    grabbed: bool,
    screenshot_requested: bool,
    next_frame: Instant,
}

fn build_gfx(
    gpu: &rc_render::GpuContext,
    opts: &Options,
    assets_override: Option<&std::path::Path>,
) -> Result<Gfx> {
    let path = assets_override
        .map(std::path::Path::to_path_buf)
        .or_else(|| opts.assets_path.as_ref().map(PathBuf::from));
    let assets = rc_assets::Assets::discover(path.as_deref());
    Gfx::new(&gpu.device, &gpu.queue, gpu.color_format(), assets, opts)
}

impl App {
    fn set_grab(&mut self, grab: bool) {
        if self.grabbed == grab {
            return;
        }
        let Some(w) = &self.window else { return };
        if grab {
            let ok = w
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| w.set_cursor_grab(CursorGrabMode::Confined));
            if ok.is_err() {
                warn!("cursor grab unavailable");
            }
            w.set_cursor_visible(false);
        } else {
            let _ = w.set_cursor_grab(CursorGrabMode::None);
            w.set_cursor_visible(true);
            let size = w.inner_size();
            let _ = w.set_cursor_position(winit::dpi::PhysicalPosition::new(
                size.width / 2,
                size.height / 2,
            ));
        }
        self.grabbed = grab;
    }

    fn redraw(&mut self, event_loop: &ActiveEventLoop) {
        let (Some(gpu), Some(engine)) = (&mut self.gpu, &mut self.engine) else {
            return;
        };
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64().min(0.25);
        self.last = now;
        gpu.set_vsync(engine.opts.vsync);
        let Some(target) = &gpu.surface else { return };
        use wgpu::CurrentSurfaceTexture as Cst;
        let frame = match target.surface.get_current_texture() {
            Cst::Success(f) | Cst::Suboptimal(f) => f,
            Cst::Timeout | Cst::Occluded => return,
            Cst::Validation => {
                error!("surface validation error");
                return;
            }
            Cst::Outdated | Cst::Lost => {
                let (w, h) = (target.config.width, target.config.height);
                gpu.resize(w, h);
                return;
            }
        };
        let size = (target.config.width, target.config.height);
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let actions = engine.frame(&gpu.device, &gpu.queue, &view, size, dt);
        if self.screenshot_requested {
            self.screenshot_requested = false;
            match rc_render::screenshot::readback(
                &gpu.device,
                &gpu.queue,
                &frame.texture,
                size.0,
                size.1,
            ) {
                Ok(px) => {
                    let name = format!(
                        "{}.png",
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |d| d.as_millis())
                    );
                    let path = engine.game_dir.join("screenshots").join(name);
                    match rc_render::screenshot::save_png(&px, size.0, size.1, &path) {
                        Ok(()) => {
                            if let Some(g) = engine.game.as_mut() {
                                g.message(format!(
                                    "Saved screenshot as {}",
                                    path.file_name().unwrap_or_default().to_string_lossy()
                                ));
                            }
                        }
                        Err(e) => warn!("screenshot: {e:#}"),
                    }
                }
                Err(e) => warn!("screenshot readback: {e:#}"),
            }
        }
        gpu.queue.present(frame);
        let rebuild = engine.handle(actions);
        if rebuild {
            match build_gfx(gpu, &engine.opts, self.launch.assets.as_deref()) {
                Ok(g) => engine.replace_gfx(g),
                Err(e) => error!("reloading resources: {e:#}"),
            }
        }
        if let Some(fs) = engine.wants_fullscreen.take()
            && let Some(w) = &self.window
        {
            w.set_fullscreen(fs.then_some(Fullscreen::Borderless(None)));
        }
        if engine.quit {
            if engine.game.is_some() {
                engine.close_world();
            }
            engine.save_options();
            event_loop.exit();
            return;
        }
        let grab = engine.wants_grab();
        let cap = engine.opts.frame_cap();
        self.set_grab(grab);
        // Frame pacing: honour the FPS / GPU-usage limit.
        if cap > 0 {
            let period = Duration::from_secs_f64(1.0 / f64::from(cap));
            self.next_frame += period;
            let now = Instant::now();
            if self.next_frame < now {
                self.next_frame = now;
            }
            event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
        } else {
            event_loop.set_control_flow(ControlFlow::Poll);
        }
    }

    fn key(&mut self, code: KeyCode, pressed: bool, text: Option<&str>) {
        let Some(e) = &mut self.engine else { return };
        let typing = matches!(
            e.screens.last(),
            Some(
                crate::screens::Screen::Chat { .. }
                    | crate::screens::Screen::CreateWorld { .. }
                    | crate::screens::Screen::Creative { .. }
            )
        );
        if pressed {
            if let Some(t) = text {
                e.input.typed.push_str(t);
            }
            let uk = match code {
                KeyCode::Backspace => Some(rc_ui::Key::Backspace),
                KeyCode::Delete => Some(rc_ui::Key::Delete),
                KeyCode::Enter | KeyCode::NumpadEnter => Some(rc_ui::Key::Enter),
                KeyCode::Escape => Some(rc_ui::Key::Escape),
                KeyCode::ArrowLeft => Some(rc_ui::Key::Left),
                KeyCode::ArrowRight => Some(rc_ui::Key::Right),
                KeyCode::ArrowUp => Some(rc_ui::Key::Up),
                KeyCode::ArrowDown => Some(rc_ui::Key::Down),
                KeyCode::Home => Some(rc_ui::Key::Home),
                KeyCode::End => Some(rc_ui::Key::End),
                KeyCode::Tab => Some(rc_ui::Key::Tab),
                _ => None,
            };
            if let Some(k) = uk {
                e.input.keys.push(k);
            }
        }
        if matches!(code, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
            e.input.shift = pressed;
        }
        if matches!(code, KeyCode::ControlLeft | KeyCode::ControlRight) {
            e.input.ctrl = pressed;
        }
        // Movement keys are tracked even while typing so releases register.
        let h = &mut e.held;
        match code {
            KeyCode::KeyW if !typing || !pressed => {
                if pressed && !h.forward {
                    e.edges.forward_pressed = true;
                }
                h.forward = pressed;
            }
            KeyCode::KeyS if !typing || !pressed => h.back = pressed,
            KeyCode::KeyA if !typing || !pressed => h.left = pressed,
            KeyCode::KeyD if !typing || !pressed => h.right = pressed,
            KeyCode::Space if !typing || !pressed => {
                if pressed && !h.jump {
                    e.edges.jump_pressed = true;
                }
                h.jump = pressed;
            }
            KeyCode::ShiftLeft => h.sneak = pressed,
            KeyCode::ControlLeft => h.sprint = pressed,
            _ => {}
        }
        if !pressed {
            return;
        }
        match code {
            KeyCode::F2 => self.screenshot_requested = true,
            KeyCode::F3 => e.game_key(GameKey::F3),
            KeyCode::F1 => e.game_key(GameKey::F1),
            KeyCode::F11 => {
                e.opts.fullscreen = !e.opts.fullscreen;
                e.wants_fullscreen = Some(e.opts.fullscreen);
            }
            _ if typing => {}
            KeyCode::Escape => e.game_key(GameKey::Escape),
            KeyCode::KeyE => e.game_key(GameKey::Inventory),
            KeyCode::KeyT => e.game_key(GameKey::Chat(false)),
            KeyCode::Slash => e.game_key(GameKey::Chat(true)),
            KeyCode::KeyQ => e.game_key(GameKey::Drop),
            KeyCode::Digit1 => e.game_key(GameKey::Hotbar(0)),
            KeyCode::Digit2 => e.game_key(GameKey::Hotbar(1)),
            KeyCode::Digit3 => e.game_key(GameKey::Hotbar(2)),
            KeyCode::Digit4 => e.game_key(GameKey::Hotbar(3)),
            KeyCode::Digit5 => e.game_key(GameKey::Hotbar(4)),
            KeyCode::Digit6 => e.game_key(GameKey::Hotbar(5)),
            KeyCode::Digit7 => e.game_key(GameKey::Hotbar(6)),
            KeyCode::Digit8 => e.game_key(GameKey::Hotbar(7)),
            KeyCode::Digit9 => e.game_key(GameKey::Hotbar(8)),
            _ => {}
        }
        // Opening chat must not also type the key that opened it.
        if matches!(code, KeyCode::KeyT | KeyCode::Slash)
            && matches!(e.screens.last(), Some(crate::screens::Screen::Chat { .. }))
            && !typing
        {
            e.input.typed.clear();
        }
        if matches!(code, KeyCode::KeyE) && !typing {
            e.input.typed.clear();
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let opts = Options::load(&self.launch.game_dir.join("options.json"));
        let attrs = Window::default_attributes()
            .with_title(format!("RustCraft {}", rc_core::version::VERSION))
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.launch.size.0,
                self.launch.size.1,
            ))
            .with_min_inner_size(winit::dpi::LogicalSize::new(320, 240));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                error!("failed to create window: {e}");
                event_loop.exit();
                return;
            }
        };
        if opts.fullscreen {
            window.set_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let size = window.inner_size();
        let gpu = match rc_render::GpuContext::new_windowed(window.clone(), size.width, size.height)
        {
            Ok(g) => g,
            Err(e) => {
                error!("failed to initialise GPU: {e:#}");
                event_loop.exit();
                return;
            }
        };
        let info = gpu.adapter.get_info();
        let gfx = match build_gfx(&gpu, &opts, self.launch.assets.as_deref()) {
            Ok(g) => g,
            Err(e) => {
                error!("failed to load resources: {e:#}");
                event_loop.exit();
                return;
            }
        };
        let engine = Engine::new(
            opts,
            self.launch.game_dir.clone(),
            gfx,
            info.name.clone(),
            format!("{:?} {}", info.backend, info.driver_info),
        );
        if self.launch.check_updates && engine.opts.update_policy != rc_update::UpdatePolicy::Off {
            engine.check_updates(false);
        }
        info!("ready");
        self.engine = Some(engine);
        self.gpu = Some(gpu);
        self.window = Some(window);
        self.last = Instant::now();
        self.next_frame = Instant::now();
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event
            && self.grabbed
            && let Some(e) = &mut self.engine
        {
            e.look_delta.0 += delta.0;
            e.look_delta.1 += delta.1;
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                if let Some(e) = &mut self.engine {
                    if e.game.is_some() {
                        e.close_world();
                    }
                    e.save_options();
                }
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::Focused(false) => {
                if let Some(e) = &mut self.engine {
                    e.held = crate::engine::Held::default();
                    if e.game.is_some() && e.screens.is_empty() {
                        e.game_key(GameKey::Escape);
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let pressed = event.state == ElementState::Pressed;
                    let text = event.text.as_ref().map(|t| t.to_string());
                    self.key(
                        code,
                        pressed,
                        text.as_deref()
                            .filter(|t| t.chars().all(|c| !c.is_control())),
                    );
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(e) = &mut self.engine {
                    e.input.mouse_px = (position.x as f32, position.y as f32);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(e) = &mut self.engine {
                    let pressed = state == ElementState::Pressed;
                    match button {
                        MouseButton::Left => {
                            e.input.down = pressed;
                            if pressed {
                                e.input.pressed = true;
                            } else {
                                e.input.released = true;
                            }
                            e.held.attack = pressed && e.screens.is_empty();
                        }
                        MouseButton::Right => {
                            if pressed {
                                e.input.right_pressed = true;
                                if e.screens.is_empty() {
                                    e.edges.use_pressed = true;
                                }
                            }
                            e.held.use_item = pressed && e.screens.is_empty();
                        }
                        MouseButton::Middle if pressed && e.screens.is_empty() => {
                            e.edges.pick = true;
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(e) = &mut self.engine {
                    let d = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(p) => (p.y / 40.0) as f32,
                    };
                    e.input.scroll += d;
                    if e.screens.is_empty() && d != 0.0 {
                        e.scroll_ticks += d.signum() as i32;
                    }
                }
            }
            WindowEvent::RedrawRequested => self.redraw(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _el: &ActiveEventLoop) {
        if let Some(w) = &self.window
            && Instant::now() >= self.next_frame
        {
            w.request_redraw();
        }
    }
}

pub fn run(launch: LaunchOptions) -> Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        launch,
        window: None,
        gpu: None,
        engine: None,
        last: Instant::now(),
        grabbed: false,
        screenshot_requested: false,
        next_frame: Instant::now(),
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}
