//! 2D GUI batching: textured, tinted quads in window pixels.
//!
//! The UI crate fills a [`DrawList`] each frame; [`GuiRenderer`] uploads it in
//! one buffer and issues one draw per texture run.

use bytemuck::{Pod, Zeroable};
use rc_assets::Image;

/// Texture handle registered with the GUI renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct TexId(pub u32);

/// The 1×1 white texture, for solid fills.
pub const WHITE: TexId = TexId(0);

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct GuiVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

/// One frame of GUI geometry.
#[derive(Debug, Default)]
pub struct DrawList {
    pub verts: Vec<GuiVertex>,
    /// (texture, first vertex, vertex count)
    pub cmds: Vec<(TexId, u32, u32)>,
    /// Scissor rectangles: (first cmd, rect) — `None` clears.
    pub clips: Vec<(usize, Option<[u32; 4]>)>,
}

impl DrawList {
    pub fn clear(&mut self) {
        self.verts.clear();
        self.cmds.clear();
        self.clips.clear();
    }

    fn begin(&mut self, tex: TexId) {
        let n = self.verts.len() as u32;
        match self.cmds.last_mut() {
            Some((t, s, c)) if *t == tex && *s + *c == n => {}
            _ => self.cmds.push((tex, n, 0)),
        }
    }

    fn end(&mut self, added: u32) {
        if let Some(last) = self.cmds.last_mut() {
            last.2 += added;
        }
    }

    /// Arbitrary quad: points TL, BL, BR, TR.
    pub fn quad_pts(&mut self, tex: TexId, p: [[f32; 2]; 4], uv: [[f32; 2]; 4], color: [f32; 4]) {
        self.begin(tex);
        for i in [0, 1, 2, 0, 2, 3] {
            self.verts.push(GuiVertex {
                pos: p[i],
                uv: uv[i],
                color,
            });
        }
        self.end(6);
    }

    /// Axis-aligned textured rectangle.
    pub fn rect_uv(&mut self, tex: TexId, r: [f32; 4], uv: [f32; 4], color: [f32; 4]) {
        let [x0, y0, x1, y1] = r;
        let [u0, v0, u1, v1] = uv;
        self.quad_pts(
            tex,
            [[x0, y0], [x0, y1], [x1, y1], [x1, y0]],
            [[u0, v0], [u0, v1], [u1, v1], [u1, v0]],
            color,
        );
    }

    /// Solid colour rectangle.
    pub fn fill(&mut self, r: [f32; 4], color: [f32; 4]) {
        self.rect_uv(WHITE, r, [0.0, 0.0, 1.0, 1.0], color);
    }

    /// Vertical gradient.
    pub fn gradient(&mut self, r: [f32; 4], top: [f32; 4], bottom: [f32; 4]) {
        let [x0, y0, x1, y1] = r;
        self.begin(WHITE);
        let v = |x: f32, y: f32, c: [f32; 4]| GuiVertex {
            pos: [x, y],
            uv: [0.5, 0.5],
            color: c,
        };
        let q = [
            v(x0, y0, top),
            v(x0, y1, bottom),
            v(x1, y1, bottom),
            v(x1, y0, top),
        ];
        for i in [0, 1, 2, 0, 2, 3] {
            self.verts.push(q[i]);
        }
        self.end(6);
    }

    /// Start clipping subsequent draws to `r` (window pixels), or stop.
    pub fn clip(&mut self, r: Option<[u32; 4]>) {
        // Force a new command so the clip applies from here.
        self.cmds
            .push((TexId(u32::MAX), self.verts.len() as u32, 0));
        self.clips.push((self.cmds.len() - 1, r));
    }
}

const GUI_WGSL: &str = r"
struct Screen { size: vec4<f32> };
@group(0) @binding(0) var<uniform> screen: Screen;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
struct VIn { @location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32> };
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) color: vec4<f32> };
@vertex fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.clip = vec4<f32>(v.pos.x / screen.size.x * 2.0 - 1.0, 1.0 - v.pos.y / screen.size.y * 2.0, 0.0, 1.0);
    o.uv = v.uv;
    o.color = v.color;
    return o;
}
@fragment fn fs(v: VOut) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, v.uv) * v.color;
    if (t.a < 0.004) { discard; }
    return t;
}
";

struct GuiTex {
    bind: wgpu::BindGroup,
    size: (u32, u32),
}

pub struct GuiRenderer {
    pipeline: wgpu::RenderPipeline,
    bgl_tex: wgpu::BindGroupLayout,
    screen: wgpu::Buffer,
    screen_bind: wgpu::BindGroup,
    textures: Vec<GuiTex>,
    vbuf: wgpu::Buffer,
    nearest: wgpu::Sampler,
    linear: wgpu::Sampler,
}

impl std::fmt::Debug for GuiRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GuiRenderer({} textures)", self.textures.len())
    }
}

impl GuiRenderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gui"),
            source: wgpu::ShaderSource::Wgsl(GUI_WGSL.into()),
        });
        let bgl_screen = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gui-screen"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bgl_tex = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gui-tex"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let screen = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gui-screen"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let screen_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gui-screen"),
            layout: &bgl_screen,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gui"),
            bind_group_layouts: &[Some(&bgl_screen), Some(&bgl_tex)],
            ..Default::default()
        });
        let attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gui"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<GuiVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attrs,
                })],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let nearest = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("gui-nearest"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            ..Default::default()
        });
        let linear = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("gui-linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gui-verts"),
            size: 1 << 20,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut r = Self {
            pipeline,
            bgl_tex,
            screen,
            screen_bind,
            textures: Vec::new(),
            vbuf,
            nearest,
            linear,
        };
        let mut white = Image::new(1, 1);
        white.put(0, 0, [255; 4]);
        r.register(device, queue, &white, false);
        r
    }

    /// Register an image; returns its id.
    pub fn register(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        img: &Image,
        linear: bool,
    ) -> TexId {
        use wgpu::util::DeviceExt as _;
        let tex = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("gui-tex"),
                size: wgpu::Extent3d {
                    width: img.w,
                    height: img.h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &img.px,
        );
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.register_view(device, &view, (img.w, img.h), linear)
    }

    /// Register an existing texture view (e.g. the block atlas).
    pub fn register_view(
        &mut self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        size: (u32, u32),
        linear: bool,
    ) -> TexId {
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gui-tex"),
            layout: &self.bgl_tex,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(if linear {
                        &self.linear
                    } else {
                        &self.nearest
                    }),
                },
            ],
        });
        self.textures.push(GuiTex { bind, size });
        TexId(self.textures.len() as u32 - 1)
    }

    #[must_use]
    pub fn size_of(&self, t: TexId) -> (u32, u32) {
        self.textures.get(t.0 as usize).map_or((1, 1), |t| t.size)
    }

    /// Draw `list` over `target` (loads existing contents).
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
        list: &DrawList,
        clear: Option<wgpu::Color>,
    ) {
        queue.write_buffer(
            &self.screen,
            0,
            bytemuck::cast_slice(&[size.0 as f32, size.1 as f32, 0.0, 0.0]),
        );
        let bytes = bytemuck::cast_slice::<GuiVertex, u8>(&list.verts);
        if bytes.len() as u64 > self.vbuf.size() {
            self.vbuf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gui-verts"),
                size: (bytes.len() as u64).next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.vbuf, 0, bytes);
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("gui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: clear.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if list.verts.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.screen_bind, &[]);
        pass.set_vertex_buffer(0, self.vbuf.slice(..));
        let mut clip_i = 0;
        for (ci, &(tex, start, count)) in list.cmds.iter().enumerate() {
            while clip_i < list.clips.len() && list.clips[clip_i].0 == ci {
                match list.clips[clip_i].1 {
                    Some([x, y, w, h]) => {
                        let x = x.min(size.0);
                        let y = y.min(size.1);
                        pass.set_scissor_rect(
                            x,
                            y,
                            w.min(size.0 - x).max(1),
                            h.min(size.1 - y).max(1),
                        );
                    }
                    None => pass.set_scissor_rect(0, 0, size.0, size.1),
                }
                clip_i += 1;
            }
            if count == 0 {
                continue;
            }
            let Some(t) = self.textures.get(tex.0 as usize) else {
                continue;
            };
            pass.set_bind_group(1, &t.bind, &[]);
            pass.draw(start..start + count, 0..1);
        }
    }
}
