//! World rendering: terrain sections (solid, cutout, translucent), sky,
//! clouds, block outline and the breaking overlay.

use ahash::AHashMap;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use rc_assets::{Atlas, Image};
use wgpu::util::DeviceExt as _;

use crate::mesh::{SectionMesh, Vertex};

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

const TERRAIN_WGSL: &str = r"
struct Globals {
    view_proj: mat4x4<f32>,
    inv_view_proj: mat4x4<f32>,
    fog_color: vec4<f32>,
    sky_color: vec4<f32>,
    // x: fog start, y: fog end, z: time of day (0..1), w: underwater
    fog: vec4<f32>,
    // xyz: sun direction, w: star brightness
    sun: vec4<f32>,
    // x: cloud offset, y: camera y, z: moon phase, w: cloud height
    misc: vec4<f32>,
};
@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_s: sampler;
@group(0) @binding(3) var lightmap: texture_2d<f32>;
@group(0) @binding(4) var lightmap_s: sampler;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) light: vec4<f32>,
    @location(4) offset: vec3<f32>,
};
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) lm: vec2<f32>,
    @location(3) dist: f32,
};

@vertex
fn vs(v: VIn) -> VOut {
    var o: VOut;
    let world = v.pos + v.offset;
    o.clip = g.view_proj * vec4<f32>(world, 1.0);
    o.uv = v.uv;
    o.color = v.color;
    let l = v.light.xy * 255.0;
    o.lm = vec2<f32>((l.y + 8.0) / 256.0, (l.x + 8.0) / 256.0);
    o.dist = length(world.xz);
    return o;
}

fn shade(v: VOut, tex: vec4<f32>) -> vec4<f32> {
    let lm = textureSample(lightmap, lightmap_s, v.lm).rgb;
    var c = tex.rgb * v.color.rgb * lm;
    let f = clamp((v.dist - g.fog.x) / max(g.fog.y - g.fog.x, 0.001), 0.0, 1.0);
    c = mix(c, g.fog_color.rgb, f);
    return vec4<f32>(c, tex.a * v.color.a);
}

@fragment
fn fs_solid(v: VOut) -> @location(0) vec4<f32> {
    let tex = textureSample(atlas, atlas_s, v.uv);
    return vec4<f32>(shade(v, tex).rgb, 1.0);
}

@fragment
fn fs_cutout(v: VOut) -> @location(0) vec4<f32> {
    let tex = textureSample(atlas, atlas_s, v.uv);
    if (tex.a < 0.5) { discard; }
    return vec4<f32>(shade(v, tex).rgb, 1.0);
}

@fragment
fn fs_translucent(v: VOut) -> @location(0) vec4<f32> {
    let tex = textureSample(atlas, atlas_s, v.uv);
    return shade(v, tex);
}

// ---- sky ----
struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_sky(@builtin(vertex_index) i: u32) -> SkyOut {
    var p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -3.0), vec2<f32>(3.0, 1.0), vec2<f32>(-1.0, 1.0));
    var o: SkyOut;
    o.clip = vec4<f32>(p[i], 0.0, 1.0);
    o.ndc = p[i];
    return o;
}

fn hash3(p: vec3<f32>) -> f32 {
    let q = fract(p * 0.1031);
    let r = q + dot(q, q.yzx + 33.33);
    return fract((r.x + r.y) * r.z);
}

@fragment
fn fs_sky(v: SkyOut) -> @location(0) vec4<f32> {
    let far = g.inv_view_proj * vec4<f32>(v.ndc, 1.0, 1.0);
    let dir = normalize(far.xyz / far.w);
    if (g.fog.w > 0.5) {
        return vec4<f32>(g.fog_color.rgb, 1.0);
    }
    let up = clamp(dir.y, -1.0, 1.0);
    var c = mix(g.fog_color.rgb, g.sky_color.rgb, smoothstep(-0.05, 0.35, up));
    if (up < -0.05) {
        c = mix(c, g.sky_color.rgb * 0.2, smoothstep(-0.05, -0.4, up));
    }
    // Stars at night.
    if (g.sun.w > 0.0 && up > 0.0) {
        let cell = floor(dir * 180.0);
        let h = hash3(cell);
        if (h > 0.9975) {
            c += vec3<f32>(g.sun.w * (h - 0.9975) * 400.0);
        }
    }
    return vec4<f32>(c, 1.0);
}

// ---- sun / moon / clouds (textured quads in world space) ----
struct QIn {
    @location(0) pos: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};
struct QOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) dist: f32,
};
@group(1) @binding(0) var qtex: texture_2d<f32>;
@group(1) @binding(1) var qtex_s: sampler;

@vertex
fn vs_quad(v: QIn) -> QOut {
    var o: QOut;
    o.clip = g.view_proj * vec4<f32>(v.pos, 1.0);
    o.uv = v.uv;
    o.color = v.color;
    o.dist = length(v.pos.xz);
    return o;
}

@fragment
fn fs_quad_add(v: QOut) -> @location(0) vec4<f32> {
    let t = textureSample(qtex, qtex_s, v.uv);
    return vec4<f32>(t.rgb * v.color.rgb * v.color.a, 1.0);
}

@fragment
fn fs_quad_blend(v: QOut) -> @location(0) vec4<f32> {
    let t = textureSample(qtex, qtex_s, v.uv) * v.color;
    if (t.a < 0.01) { discard; }
    let f = clamp((v.dist - g.fog.x) / max(g.fog.y - g.fog.x, 0.001), 0.0, 1.0);
    return vec4<f32>(mix(t.rgb, g.fog_color.rgb, f), t.a);
}

// ---- lines ----
struct LOut {
    @builtin(position) clip: vec4<f32>,
};
@vertex
fn vs_line(@location(0) pos: vec3<f32>) -> LOut {
    var o: LOut;
    o.clip = g.view_proj * vec4<f32>(pos, 1.0);
    o.clip.z -= 0.0004 * o.clip.w;
    return o;
}
@fragment
fn fs_line() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 0.45);
}
";

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    inv_view_proj: [[f32; 4]; 4],
    fog_color: [f32; 4],
    sky_color: [f32; 4],
    fog: [f32; 4],
    sun: [f32; 4],
    misc: [f32; 4],
}

/// Vertex for sky/cloud/overlay quads.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct QuadVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

/// Per-frame camera and environment.
#[derive(Clone, Copy, Debug)]
pub struct FrameParams {
    /// Camera position in world space (f64 for precision far from origin).
    pub cam_pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub fov_y_deg: f32,
    pub aspect: f32,
    pub render_distance_blocks: f32,
    pub sky_color: [f32; 3],
    pub fog_color: [f32; 3],
    /// 0..1 where 0 is sunrise, 0.25 noon.
    pub time_of_day: f32,
    pub underwater: bool,
    pub cloud_offset: f32,
    pub clouds: bool,
    /// Selected block outline boxes in world space.
    pub outline: Option<([i32; 3], &'static [[f64; 6]])>,
}

/// Owned outline boxes for the selected block.
#[derive(Clone, Debug, Default)]
pub struct Outline {
    pub pos: [i32; 3],
    pub boxes: Vec<[f64; 6]>,
}

struct GpuSection {
    origin: [i32; 3],
    layers: [Option<(wgpu::Buffer, u32)>; 3],
}

/// Statistics for the debug overlay.
#[derive(Clone, Copy, Debug, Default)]
pub struct RenderStats {
    pub sections_total: usize,
    pub sections_drawn: usize,
    pub quads_drawn: usize,
}

pub struct TerrainRenderer {
    globals: wgpu::Buffer,
    bind: wgpu::BindGroup,
    bgl_tex: wgpu::BindGroupLayout,
    solid: wgpu::RenderPipeline,
    cutout: wgpu::RenderPipeline,
    translucent: wgpu::RenderPipeline,
    sky: wgpu::RenderPipeline,
    quad_add: wgpu::RenderPipeline,
    quad_blend: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    atlas_tex: wgpu::Texture,
    lightmap_tex: wgpu::Texture,
    index: wgpu::Buffer,
    index_quads: u32,
    instance: wgpu::Buffer,
    instance_cap: usize,
    instance_data: Vec<[f32; 4]>,
    draw_list: Vec<(usize, u32, f32)>,
    sections: AHashMap<[i32; 3], GpuSection>,
    sun_bind: Option<wgpu::BindGroup>,
    moon_bind: Option<wgpu::BindGroup>,
    cloud_bind: Option<wgpu::BindGroup>,
    cloud_img: Option<Image>,
    quad_buf: wgpu::Buffer,
    line_buf: wgpu::Buffer,
    depth: Option<(wgpu::Texture, wgpu::TextureView, u32, u32)>,
    pub stats: RenderStats,
    color_format: wgpu::TextureFormat,
    /// Breaking overlay quads (world space), filled by the game.
    pub overlay: Vec<QuadVertex>,
    overlay_bind: wgpu::BindGroup,
}

impl std::fmt::Debug for TerrainRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TerrainRenderer({} sections)", self.sections.len())
    }
}

fn texture_from_image(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    img: &Image,
    label: &str,
) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
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
    (tex, view)
}

impl TerrainRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        color_format: wgpu::TextureFormat,
        atlas: &Atlas,
        sun: Option<&Image>,
        moon: Option<&Image>,
        clouds: Option<&Image>,
        anisotropy: u16,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terrain"),
            source: wgpu::ShaderSource::Wgsl(TERRAIN_WGSL.into()),
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let levels = atlas.mips.len() as u32;
        let base = &atlas.mips[0];
        let atlas_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("block-atlas"),
            size: wgpu::Extent3d {
                width: base.w,
                height: base.h,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (l, m) in atlas.mips.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &atlas_tex,
                    mip_level: l as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &m.px,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(m.w * 4),
                    rows_per_image: Some(m.h),
                },
                wgpu::Extent3d {
                    width: m.w,
                    height: m.h,
                    depth_or_array_layers: 1,
                },
            );
        }
        let atlas_view = atlas_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let atlas_s = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: if anisotropy > 1 {
                wgpu::FilterMode::Linear
            } else {
                wgpu::FilterMode::Nearest
            },
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: anisotropy.max(1),
            ..Default::default()
        });
        let lightmap_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lightmap"),
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let lm_view = lightmap_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let lm_s = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lightmap"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let tex_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let samp_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_entry(1),
                samp_entry(2),
                tex_entry(3),
                samp_entry(4),
            ],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&atlas_s),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&lm_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&lm_s),
                },
            ],
        });
        let bgl_tex = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quad-tex"),
            entries: &[tex_entry(0), samp_entry(1)],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terrain"),
            bind_group_layouts: &[Some(&bgl)],
            ..Default::default()
        });
        let layout_q = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quad"),
            bind_group_layouts: &[Some(&bgl), Some(&bgl_tex)],
            ..Default::default()
        });
        let vattrs =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Unorm8x4, 3 => Unorm8x4];
        let iattrs = wgpu::vertex_attr_array![4 => Float32x3];
        let vbufs = [
            Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &vattrs,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: 16,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &iattrs,
            }),
        ];
        let depth = |write: bool, cmp: wgpu::CompareFunction| {
            Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(write),
                depth_compare: Some(cmp),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            })
        };
        let make = |fs: &str, blend: Option<wgpu::BlendState>, write: bool, cull: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &vbufs,
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: cull.then_some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: depth(write, wgpu::CompareFunction::LessEqual),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let solid = make("fs_solid", None, true, true);
        let cutout = make("fs_cutout", None, true, false);
        let translucent = make(
            "fs_translucent",
            Some(wgpu::BlendState::ALPHA_BLENDING),
            false,
            false,
        );
        let sky = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_sky"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth(false, wgpu::CompareFunction::Always),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_sky"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let qattrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4];
        let qbuf = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<QuadVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &qattrs,
        })];
        let make_q = |fs: &str, blend: wgpu::BlendState, depth_test: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(&layout_q),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_quad"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &qbuf,
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: depth(
                    false,
                    if depth_test {
                        wgpu::CompareFunction::LessEqual
                    } else {
                        wgpu::CompareFunction::Always
                    },
                ),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: color_format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::OVER,
        };
        let quad_add = make_q("fs_quad_add", additive, false);
        let quad_blend = make_q("fs_quad_blend", wgpu::BlendState::ALPHA_BLENDING, true);
        let lattrs = wgpu::vertex_attr_array![0 => Float32x3];
        let lines = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("lines"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_line"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 12,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &lattrs,
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: depth(false, wgpu::CompareFunction::LessEqual),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_line"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let mk_bind = |img: Option<&Image>, label: &str, linear: bool| {
            img.map(|img| {
                let (_, view) = texture_from_image(device, queue, img, label);
                let s = device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some(label),
                    address_mode_u: wgpu::AddressMode::Repeat,
                    address_mode_v: wgpu::AddressMode::Repeat,
                    mag_filter: if linear {
                        wgpu::FilterMode::Linear
                    } else {
                        wgpu::FilterMode::Nearest
                    },
                    ..Default::default()
                });
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(label),
                    layout: &bgl_tex,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&s),
                        },
                    ],
                })
            })
        };
        let sun_bind = mk_bind(sun, "sun", false);
        let moon_bind = mk_bind(moon, "moon", false);
        let cloud_bind = mk_bind(clouds, "clouds", false);
        let overlay_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("overlay"),
            layout: &bgl_tex,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&atlas_s),
                },
            ],
        });
        let quad_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sky-quads"),
            size: 64 * 1024,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let line_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lines"),
            size: 16 * 1024,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let instance_cap = 4096;
        let instance = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("section-offsets"),
            size: (instance_cap * 16) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut r = Self {
            globals,
            bind,
            bgl_tex,
            solid,
            cutout,
            translucent,
            sky,
            quad_add,
            quad_blend,
            lines,
            atlas_tex,
            lightmap_tex,
            index: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("quad-index"),
                size: 4,
                usage: wgpu::BufferUsages::INDEX,
                mapped_at_creation: false,
            }),
            index_quads: 0,
            instance,
            instance_cap,
            instance_data: Vec::with_capacity(instance_cap),
            draw_list: Vec::with_capacity(instance_cap * 3),
            sections: AHashMap::new(),
            sun_bind,
            moon_bind,
            cloud_bind,
            cloud_img: clouds.cloned(),
            quad_buf,
            line_buf,
            depth: None,
            stats: RenderStats::default(),
            color_format,
            overlay: Vec::new(),
            overlay_bind,
        };
        r.ensure_index(device, 1 << 16);
        r
    }

    fn ensure_index(&mut self, device: &wgpu::Device, quads: u32) {
        if quads <= self.index_quads {
            return;
        }
        let n = quads.next_power_of_two();
        let mut idx = Vec::with_capacity(n as usize * 6);
        for q in 0..n {
            let b = q * 4;
            idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        self.index = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad-index"),
            contents: bytemuck::cast_slice(&idx),
            usage: wgpu::BufferUsages::INDEX,
        });
        self.index_quads = n;
    }

    /// A view of the block atlas (for GUI item icons).
    #[must_use]
    pub fn atlas_view(&self) -> wgpu::TextureView {
        self.atlas_tex.create_view(&wgpu::TextureViewDescriptor {
            base_mip_level: 0,
            mip_level_count: Some(1),
            ..Default::default()
        })
    }

    #[must_use]
    pub fn color_format(&self) -> wgpu::TextureFormat {
        self.color_format
    }

    /// Upload (or replace) the mesh of a section at world-space `origin`.
    pub fn upload(&mut self, device: &wgpu::Device, origin: [i32; 3], mesh: &SectionMesh) {
        if mesh.is_empty() {
            self.sections.remove(&origin);
            return;
        }
        let mut most = 0;
        let mk = |v: &[Vertex], label: &str| {
            (!v.is_empty()).then(|| {
                (
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(label),
                        contents: bytemuck::cast_slice(v),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
                    (v.len() / 4) as u32,
                )
            })
        };
        let layers = [
            mk(&mesh.solid, "solid"),
            mk(&mesh.cutout, "cutout"),
            mk(&mesh.translucent, "translucent"),
        ];
        for (_, q) in layers.iter().flatten() {
            most = most.max(*q);
        }
        self.ensure_index(device, most);
        self.sections.insert(origin, GpuSection { origin, layers });
    }

    /// Drop all sections of a chunk column.
    pub fn remove_chunk(&mut self, cx: i32, cz: i32) {
        self.sections
            .retain(|k, _| !(k[0] == cx * 16 && k[2] == cz * 16));
    }

    pub fn clear(&mut self) {
        self.sections.clear();
    }

    #[must_use]
    pub fn section_count(&self) -> usize {
        self.sections.len()
    }

    /// Replace the animated sprites' pixels (water, lava, fire…).
    pub fn update_animations(&self, queue: &wgpu::Queue, atlas: &Atlas, tick: u64) {
        for a in &atlas.animated {
            let f = a.frame_at(tick);
            let prev = a.frame_at(tick.wrapping_sub(1));
            if f == prev && tick != 0 {
                continue;
            }
            let img = a.frame(f);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: a.sprite.x,
                        y: a.sprite.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &img.px,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(img.w * 4),
                    rows_per_image: Some(img.h),
                },
                wgpu::Extent3d {
                    width: img.w.min(a.sprite.w),
                    height: img.h.min(a.sprite.h),
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    /// Upload the 16×16 lightmap (x = block light, y = sky light).
    pub fn update_lightmap(
        &self,
        queue: &wgpu::Queue,
        daylight: f32,
        gamma: f32,
        night_vision: bool,
    ) {
        let mut px = [0u8; 16 * 16 * 4];
        let curve = |l: f32| {
            let f = l / 15.0;
            f / (4.0 - 3.0 * f)
        };
        for sky in 0..16 {
            for blk in 0..16 {
                let s = curve(sky as f32) * daylight.max(0.05);
                let b = curve(blk as f32);
                let mut c = [
                    s * (daylight * 0.65 + 0.35) + b,
                    s * (daylight * 0.65 + 0.35) + b * ((b * 0.6 + 0.4) * 0.6 + 0.4),
                    s + b * (b * b * 0.6 + 0.4),
                ];
                for v in &mut c {
                    *v = v.clamp(0.0, 1.0);
                    let lift = 1.0 - (1.0 - *v).powi(4);
                    *v = *v * (1.0 - gamma) + lift * gamma;
                    *v = (*v * 0.96 + 0.03).clamp(0.0, 1.0);
                    if night_vision {
                        *v = v.max(0.7);
                    }
                }
                let i = (sky * 16 + blk) * 4;
                px[i] = (c[0] * 255.0) as u8;
                px[i + 1] = (c[1] * 255.0) as u8;
                px[i + 2] = (c[2] * 255.0) as u8;
                px[i + 3] = 255;
            }
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.lightmap_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &px,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(64),
                rows_per_image: Some(16),
            },
            wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
        );
    }

    fn depth_view(&mut self, device: &wgpu::Device, w: u32, h: u32) -> wgpu::TextureView {
        if !matches!(&self.depth, Some((_, _, dw, dh)) if *dw == w && *dh == h) {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("depth"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let v = t.create_view(&wgpu::TextureViewDescriptor::default());
            self.depth = Some((t, v, w, h));
        }
        self.depth.as_ref().expect("just created").1.clone()
    }

    /// View and projection matrices for the frame.
    #[must_use]
    pub fn matrices(p: &FrameParams) -> (Mat4, Vec3) {
        let (sy, cy) = p.yaw.to_radians().sin_cos();
        let (sp, cp) = p.pitch.to_radians().sin_cos();
        // Minecraft yaw: 0 = +Z (south), 90 = -X (west).
        let forward = Vec3::new(-sy * cp, -sp, cy * cp);
        let view = look_to(forward, Vec3::Y);
        let far = (p.render_distance_blocks * 1.5).max(256.0) + 256.0;
        let proj = perspective(p.fov_y_deg.to_radians(), p.aspect.max(0.01), 0.05, far);
        (proj * view, forward)
    }

    /// Render sky, terrain and overlays into `target`.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: (u32, u32),
        p: &FrameParams,
        outline: Option<&Outline>,
    ) {
        let depth = self.depth_view(device, size.0, size.1);
        let (vp, forward) = Self::matrices(p);
        let angle = p.time_of_day * std::f32::consts::TAU;
        let sun_dir = Vec3::new(angle.cos(), angle.sin(), 0.0);
        let night = (-sun_dir.y * 2.0 + 0.2).clamp(0.0, 1.0);
        let rd = p.render_distance_blocks;
        let (fog_start, fog_end) = if p.underwater {
            (0.0, 24.0)
        } else {
            (rd * 0.82, rd)
        };
        let g = Globals {
            view_proj: vp.to_cols_array_2d(),
            inv_view_proj: vp.inverse().to_cols_array_2d(),
            fog_color: [p.fog_color[0], p.fog_color[1], p.fog_color[2], 1.0],
            sky_color: [p.sky_color[0], p.sky_color[1], p.sky_color[2], 1.0],
            fog: [
                fog_start,
                fog_end,
                p.time_of_day,
                if p.underwater { 1.0 } else { 0.0 },
            ],
            sun: [sun_dir.x, sun_dir.y, sun_dir.z, night],
            misc: [p.cloud_offset, p.cam_pos[1] as f32, 0.0, 192.0],
        };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&g));

        // Frustum-cull sections and build the per-section offset list.
        let planes = frustum_planes(vp);
        let cam = p.cam_pos;
        self.instance_data.clear();
        self.draw_list.clear();
        let max_d2 = (rd + 24.0) * (rd + 24.0);
        let mut quads = 0usize;
        for (key, s) in &self.sections {
            let off = [
                (f64::from(s.origin[0]) - cam[0]) as f32,
                (f64::from(s.origin[1]) - cam[1]) as f32,
                (f64::from(s.origin[2]) - cam[2]) as f32,
            ];
            let c = Vec3::new(off[0] + 8.0, off[1] + 8.0, off[2] + 8.0);
            if c.x * c.x + c.z * c.z > max_d2 {
                continue;
            }
            if !sphere_visible(&planes, c, 14.0) {
                continue;
            }
            let _ = key;
            let inst = self.instance_data.len() as u32;
            self.instance_data.push([off[0], off[1], off[2], 0.0]);
            self.draw_list.push((0, inst, c.length_squared()));
            for (_, q) in s.layers.iter().flatten() {
                quads += *q as usize;
            }
        }
        self.stats = RenderStats {
            sections_total: self.sections.len(),
            sections_drawn: self.draw_list.len(),
            quads_drawn: quads,
        };
        if self.instance_data.len() > self.instance_cap {
            self.instance_cap = self.instance_data.len().next_power_of_two();
            self.instance = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("section-offsets"),
                size: (self.instance_cap * 16) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !self.instance_data.is_empty() {
            queue.write_buffer(&self.instance, 0, bytemuck::cast_slice(&self.instance_data));
        }
        // Sections by key in draw order: front-to-back for opaque.
        let mut order: Vec<(f32, u32, [i32; 3])> = Vec::with_capacity(self.draw_list.len());
        {
            let mut i = 0;
            for s in self.sections.values() {
                let off = [
                    (f64::from(s.origin[0]) - cam[0]) as f32,
                    (f64::from(s.origin[1]) - cam[1]) as f32,
                    (f64::from(s.origin[2]) - cam[2]) as f32,
                ];
                let c = Vec3::new(off[0] + 8.0, off[1] + 8.0, off[2] + 8.0);
                if c.x * c.x + c.z * c.z > max_d2 || !sphere_visible(&planes, c, 14.0) {
                    continue;
                }
                order.push((c.length_squared(), i, s.origin));
                i += 1;
            }
        }
        order.sort_by(|a, b| a.0.total_cmp(&b.0));

        // Sky quads (sun, moon) and clouds, camera-relative.
        let mut qv: Vec<QuadVertex> = Vec::with_capacity(64);
        let push_quad =
            |qv: &mut Vec<QuadVertex>, c: Vec3, a: Vec3, b: Vec3, uv: [f32; 4], col: [f32; 4]| {
                let p0 = c - a - b;
                let p1 = c - a + b;
                let p2 = c + a + b;
                let p3 = c + a - b;
                let v = |p: Vec3, u: f32, w: f32| QuadVertex {
                    pos: p.to_array(),
                    uv: [u, w],
                    color: col,
                };
                qv.extend_from_slice(&[
                    v(p0, uv[0], uv[1]),
                    v(p1, uv[0], uv[3]),
                    v(p2, uv[2], uv[3]),
                    v(p0, uv[0], uv[1]),
                    v(p2, uv[2], uv[3]),
                    v(p3, uv[2], uv[1]),
                ]);
            };
        let up_axis = Vec3::Z;
        let side = sun_dir.cross(up_axis).normalize();
        if !p.underwater {
            push_quad(
                &mut qv,
                sun_dir * 100.0,
                side * 30.0,
                up_axis * 30.0,
                [0.0, 0.0, 1.0, 1.0],
                [1.0, 1.0, 1.0, 1.0],
            );
            push_quad(
                &mut qv,
                -sun_dir * 100.0,
                -side * 20.0,
                up_axis * 20.0,
                [0.0, 0.0, 0.25, 0.5],
                [1.0, 1.0, 1.0, 1.0],
            );
        }
        let sky_quads = qv.len() as u32;
        let mut cloud_quads = 0;
        if p.clouds && self.cloud_img.is_some() && !p.underwater {
            let h = 192.33 - cam[1] as f32;
            let size = (rd * 2.0).max(256.0) + 256.0;
            let scale = 12.0 * 256.0;
            let ox = ((cam[0] as f32 + p.cloud_offset) / scale).rem_euclid(1.0);
            let oz = ((cam[2] as f32) / scale).rem_euclid(1.0);
            let k = size / scale;
            let bright = (1.0 - night * 0.85).max(0.15);
            push_quad(
                &mut qv,
                Vec3::new(0.0, h, 0.0),
                Vec3::new(size, 0.0, 0.0),
                Vec3::new(0.0, 0.0, size),
                [ox - k, oz - k, ox + k, oz + k],
                [bright, bright, bright, 0.8],
            );
            cloud_quads = qv.len() as u32 - sky_quads;
        }
        let overlay_start = qv.len() as u32;
        qv.extend(self.overlay.iter().map(|v| QuadVertex {
            pos: [
                (f64::from(v.pos[0]) - cam[0]) as f32,
                (f64::from(v.pos[1]) - cam[1]) as f32,
                (f64::from(v.pos[2]) - cam[2]) as f32,
            ],
            uv: v.uv,
            color: v.color,
        }));
        let overlay_n = qv.len() as u32 - overlay_start;
        let qbytes = bytemuck::cast_slice::<QuadVertex, u8>(&qv);
        if qbytes.len() as u64 > self.quad_buf.size() {
            self.quad_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("sky-quads"),
                size: (qbytes.len() as u64).next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        queue.write_buffer(&self.quad_buf, 0, qbytes);

        // Outline lines.
        let mut lines: Vec<[f32; 3]> = Vec::new();
        if let Some(o) = outline {
            for b in &o.boxes {
                let e = 0.002;
                let lo = [
                    (f64::from(o.pos[0]) + b[0] - cam[0]) as f32 - e,
                    (f64::from(o.pos[1]) + b[1] - cam[1]) as f32 - e,
                    (f64::from(o.pos[2]) + b[2] - cam[2]) as f32 - e,
                ];
                let hi = [
                    (f64::from(o.pos[0]) + b[3] - cam[0]) as f32 + e,
                    (f64::from(o.pos[1]) + b[4] - cam[1]) as f32 + e,
                    (f64::from(o.pos[2]) + b[5] - cam[2]) as f32 + e,
                ];
                let c = |i: u32| {
                    [
                        if i & 1 != 0 { hi[0] } else { lo[0] },
                        if i & 2 != 0 { hi[1] } else { lo[1] },
                        if i & 4 != 0 { hi[2] } else { lo[2] },
                    ]
                };
                for (a, bb) in [
                    (0, 1),
                    (2, 3),
                    (4, 5),
                    (6, 7),
                    (0, 2),
                    (1, 3),
                    (4, 6),
                    (5, 7),
                    (0, 4),
                    (1, 5),
                    (2, 6),
                    (3, 7),
                ] {
                    lines.push(c(a));
                    lines.push(c(bb));
                }
            }
        }
        let lbytes = bytemuck::cast_slice::<[f32; 3], u8>(&lines);
        if lbytes.len() as u64 > self.line_buf.size() {
            self.line_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("lines"),
                size: (lbytes.len() as u64).next_power_of_two(),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        if !lines.is_empty() {
            queue.write_buffer(&self.line_buf, 0, lbytes);
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.bind, &[]);
        pass.set_pipeline(&self.sky);
        pass.draw(0..3, 0..1);
        if sky_quads > 0 {
            pass.set_pipeline(&self.quad_add);
            pass.set_vertex_buffer(0, self.quad_buf.slice(..));
            if let Some(b) = &self.sun_bind {
                pass.set_bind_group(1, b, &[]);
                pass.draw(0..6, 0..1);
            }
            if let Some(b) = &self.moon_bind {
                pass.set_bind_group(1, b, &[]);
                pass.draw(6..12, 0..1);
            }
        }
        pass.set_index_buffer(self.index.slice(..), wgpu::IndexFormat::Uint32);
        pass.set_vertex_buffer(1, self.instance.slice(..));
        for (li, pipe) in [(0usize, &self.solid), (1, &self.cutout)] {
            pass.set_pipeline(pipe);
            for (_, inst, origin) in &order {
                let s = &self.sections[origin];
                if let Some((buf, q)) = &s.layers[li] {
                    pass.set_vertex_buffer(0, buf.slice(..));
                    pass.draw_indexed(0..q * 6, 0, *inst..inst + 1);
                }
            }
        }
        if !lines.is_empty() {
            pass.set_pipeline(&self.lines);
            pass.set_vertex_buffer(0, self.line_buf.slice(..));
            pass.draw(0..lines.len() as u32, 0..1);
        }
        if cloud_quads > 0
            && let Some(b) = &self.cloud_bind
        {
            pass.set_pipeline(&self.quad_blend);
            pass.set_bind_group(1, b, &[]);
            pass.set_vertex_buffer(0, self.quad_buf.slice(..));
            pass.draw(sky_quads..sky_quads + cloud_quads, 0..1);
        }
        pass.set_pipeline(&self.translucent);
        pass.set_index_buffer(self.index.slice(..), wgpu::IndexFormat::Uint32);
        pass.set_vertex_buffer(1, self.instance.slice(..));
        for (_, inst, origin) in order.iter().rev() {
            let s = &self.sections[origin];
            if let Some((buf, q)) = &s.layers[2] {
                pass.set_vertex_buffer(0, buf.slice(..));
                pass.draw_indexed(0..q * 6, 0, *inst..inst + 1);
            }
        }
        if overlay_n > 0 {
            pass.set_pipeline(&self.quad_blend);
            pass.set_bind_group(1, &self.overlay_bind, &[]);
            pass.set_vertex_buffer(0, self.quad_buf.slice(..));
            pass.draw(overlay_start..overlay_start + overlay_n, 0..1);
        }
        let _ = (forward, &self.bgl_tex);
    }
}

/// Right-handed view matrix looking along `f` from the origin.
#[must_use]
pub fn look_to(f: Vec3, up: Vec3) -> Mat4 {
    let f = f.normalize();
    let s = f.cross(up).normalize();
    let u = s.cross(f);
    Mat4::from_cols(
        Vec4::new(s.x, u.x, -f.x, 0.0),
        Vec4::new(s.y, u.y, -f.y, 0.0),
        Vec4::new(s.z, u.z, -f.z, 0.0),
        Vec4::W,
    )
}

/// Right-handed perspective with a 0..1 depth range.
#[must_use]
pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let f = 1.0 / (fov_y * 0.5).tan();
    Mat4::from_cols(
        Vec4::new(f / aspect, 0.0, 0.0, 0.0),
        Vec4::new(0.0, f, 0.0, 0.0),
        Vec4::new(0.0, 0.0, far / (near - far), -1.0),
        Vec4::new(0.0, 0.0, near * far / (near - far), 0.0),
    )
}

fn frustum_planes(m: Mat4) -> [Vec4; 6] {
    let r0 = m.row(0);
    let r1 = m.row(1);
    let r2 = m.row(2);
    let r3 = m.row(3);
    let n = |p: Vec4| p / p.truncate().length();
    [
        n(r3 + r0),
        n(r3 - r0),
        n(r3 + r1),
        n(r3 - r1),
        n(r2),
        n(r3 - r2),
    ]
}

fn sphere_visible(planes: &[Vec4; 6], c: Vec3, r: f32) -> bool {
    planes.iter().all(|p| p.truncate().dot(c) + p.w >= -r)
}
