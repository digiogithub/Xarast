//! Frame composition: the canvas pass, then the interface pass, into one
//! swapchain texture with one command encoder (`phase-05 §U5.1`).
//!
//! # Why the shell paints `egui` itself
//!
//! `egui-wgpu` 0.33 is built against `wgpu` 27; this workspace is on
//! `wgpu` 30, and two `wgpu`s in one binary would mean two devices that
//! cannot share a texture. What `egui` actually hands a renderer is small —
//! triangle meshes in logical points, a clip rectangle per mesh and a
//! texture delta per frame — so drawing it is one pipeline and a texture
//! table, written here against the `wgpu` the shell already owns.
//!
//! # Colour
//!
//! Both passes carry **encoded** sRGB bytes: the CPU canvas is
//! premultiplied non-linear sRGB (`xarast-render` invariant 2) and `egui`'s
//! vertex colours and textures are premultiplied sRGB too. The swapchain
//! is therefore a *non*-sRGB format, the textures are `Rgba8Unorm`, and no
//! stage converts anything; an sRGB swapchain would encode already-encoded
//! values a second time and wash everything out. Blending is premultiplied
//! (`One, OneMinusSrcAlpha`) in encoded space, which is what `egui` expects.
//!
//! # One pipeline for both passes
//!
//! The canvas is drawn as a single textured quad through the interface
//! pipeline, with a uniform that says "coordinates are device pixels"
//! instead of "coordinates are points", and a nearest-neighbour sampler so
//! that the document's pixels reach the screen one to one.

use std::collections::HashMap;

use wgpu::util::DeviceExt;

/// A canvas image to show, in device pixels.
#[derive(Debug, Clone)]
pub struct CanvasFrame {
    /// Where its top-left corner goes, in window device pixels.
    pub origin: (i32, i32),
    /// The pixels: premultiplied RGBA8, non-linear sRGB.
    pub surface: xarast_render::Surface,
}

/// One frame of interface output, as `egui` produced it.
#[derive(Debug, Clone, Default)]
pub struct UiFrame {
    /// Tessellated meshes, in logical points.
    pub primitives: Vec<egui::ClippedPrimitive>,
    /// Texture uploads and frees for this frame.
    pub textures: egui::TexturesDelta,
    /// Device pixels per logical point for this frame.
    pub pixels_per_point: f32,
}

const SHADER: &str = r"
struct Screen { size: vec2<f32>, pad: vec2<f32> };
@group(0) @binding(0) var<uniform> screen: Screen;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>,
           @location(2) color: vec4<f32>) -> VOut {
    var o: VOut;
    o.pos = vec4<f32>(2.0 * pos.x / screen.size.x - 1.0,
                      1.0 - 2.0 * pos.y / screen.size.y, 0.0, 1.0);
    o.uv = uv;
    o.color = color;
    return o;
}

@fragment
fn fs_main(i: VOut) -> @location(0) vec4<f32> {
    return i.color * textureSample(tex, samp, i.uv);
}
";

/// Bytes per vertex: position and uv as `f32` pairs, colour as four bytes.
const VERTEX_STRIDE: u64 = 20;

#[derive(Debug)]
struct GpuTexture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    size: [u32; 2],
}

/// The pipelines, textures and retained output of the two passes.
#[derive(Debug)]
pub(crate) struct Painter {
    pipeline: wgpu::RenderPipeline,
    screen_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    linear: wgpu::Sampler,
    nearest: wgpu::Sampler,
    textures: HashMap<egui::TextureId, GpuTexture>,
    canvas: Option<(GpuTexture, (i32, i32))>,
    ui: Vec<egui::ClippedPrimitive>,
    ppp: f32,
}

impl Painter {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Painter {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("xarast paint"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let screen_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("screen"),
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
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("xarast paint"),
            bind_group_layouts: &[Some(&screen_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Unorm8x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("xarast paint"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: VERTEX_STRIDE,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = |filter| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: None,
                mag_filter: filter,
                min_filter: filter,
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                ..Default::default()
            })
        };
        Painter {
            pipeline,
            screen_layout,
            texture_layout,
            linear: sampler(wgpu::FilterMode::Linear),
            nearest: sampler(wgpu::FilterMode::Nearest),
            textures: HashMap::new(),
            canvas: None,
            ui: Vec::new(),
            ppp: 1.0,
        }
    }

    fn make_texture(
        &self,
        device: &wgpu::Device,
        size: [u32; 2],
        sampler: &wgpu::Sampler,
    ) -> GpuTexture {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: size[0].max(1),
                height: size[1].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Encoded bytes in, encoded bytes out: never an sRGB format.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        GpuTexture {
            texture,
            bind_group,
            size,
        }
    }

    fn write(
        queue: &wgpu::Queue,
        tex: &wgpu::Texture,
        origin: [u32; 2],
        size: [u32; 2],
        rgba: &[u8],
    ) {
        if size[0] == 0 || size[1] == 0 {
            return;
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: tex,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: origin[0],
                    y: origin[1],
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size[0] * 4),
                rows_per_image: Some(size[1]),
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
    }

    /// Uploads a new canvas image. The texture is reused while the size
    /// holds, which is every frame of a pan.
    pub(crate) fn set_canvas(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &CanvasFrame,
    ) {
        let size = [frame.surface.width(), frame.surface.height()];
        let reuse = self.canvas.as_ref().is_some_and(|(t, _)| t.size == size);
        if !reuse {
            let t = self.make_texture(device, size, &self.nearest);
            self.canvas = Some((t, frame.origin));
        }
        if let Some((t, origin)) = self.canvas.as_mut() {
            *origin = frame.origin;
            Self::write(queue, &t.texture, [0, 0], size, frame.surface.data());
        }
    }

    /// Moves the canvas without new pixels, when the layout moved it.
    pub(crate) fn move_canvas(&mut self, origin: (i32, i32)) {
        if let Some((_, o)) = self.canvas.as_mut() {
            *o = origin;
        }
    }

    /// Forgets the canvas image: the next frame shows the backdrop.
    pub(crate) fn clear_canvas(&mut self) {
        self.canvas = None;
    }

    /// Applies an interface frame's texture uploads and keeps its meshes
    /// for every present until the next one. Frees are applied at once:
    /// `egui` only frees a texture its new meshes no longer use.
    pub(crate) fn set_ui(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: UiFrame) {
        for (id, delta) in &frame.textures.set {
            let egui::ImageData::Color(image) = &delta.image;
            let size = [
                u32::try_from(image.size[0]).unwrap_or(0),
                u32::try_from(image.size[1]).unwrap_or(0),
            ];
            let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            match delta.pos {
                None => {
                    let t = self.make_texture(device, size, &self.linear);
                    Self::write(queue, &t.texture, [0, 0], size, &bytes);
                    self.textures.insert(*id, t);
                }
                Some(pos) => {
                    if let Some(t) = self.textures.get(id) {
                        let origin = [
                            u32::try_from(pos[0]).unwrap_or(0),
                            u32::try_from(pos[1]).unwrap_or(0),
                        ];
                        // A patch outside the texture is an egui bug we
                        // would rather skip than hand to validation.
                        if origin[0] + size[0] <= t.size[0] && origin[1] + size[1] <= t.size[1] {
                            Self::write(queue, &t.texture, origin, size, &bytes);
                        }
                    }
                }
            }
        }
        for id in &frame.textures.free {
            self.textures.remove(id);
        }
        self.ui = frame.primitives;
        self.ppp = frame.pixels_per_point.max(0.01);
    }

    fn screen(&self, device: &wgpu::Device, w: f32, h: f32) -> wgpu::BindGroup {
        let data: Vec<u8> = [w, h, 0.0, 0.0]
            .iter()
            .flat_map(|v| v.to_ne_bytes())
            .collect();
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &data,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.screen_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        })
    }

    /// Records both passes into `encoder`, clearing `view` to `clear`.
    pub(crate) fn draw(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        target: [u32; 2],
        clear: wgpu::Color,
    ) {
        let (tw, th) = (target[0] as f32, target[1] as f32);

        // Geometry first, so that the pass borrows only finished buffers.
        let mut verts: Vec<u8> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        // (index range, texture, scissor, is_canvas)
        let mut draws: Vec<(std::ops::Range<u32>, &wgpu::BindGroup, [u32; 4], bool)> = Vec::new();
        let push_vertex = |verts: &mut Vec<u8>, pos: [f32; 2], uv: [f32; 2], c: [u8; 4]| {
            for v in [pos[0], pos[1], uv[0], uv[1]] {
                verts.extend_from_slice(&v.to_ne_bytes());
            }
            verts.extend_from_slice(&c);
        };
        let vertex_count = |verts: &Vec<u8>| (verts.len() as u64 / VERTEX_STRIDE) as u32;

        if let Some((tex, (ox, oy))) = &self.canvas {
            let base = vertex_count(&verts);
            let (x0, y0) = (*ox as f32, *oy as f32);
            let (x1, y1) = (x0 + tex.size[0] as f32, y0 + tex.size[1] as f32);
            for (p, uv) in [
                ([x0, y0], [0.0, 0.0]),
                ([x1, y0], [1.0, 0.0]),
                ([x1, y1], [1.0, 1.0]),
                ([x0, y1], [0.0, 1.0]),
            ] {
                push_vertex(&mut verts, p, uv, [255, 255, 255, 255]);
            }
            let start = indices.len() as u32;
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            draws.push((
                start..indices.len() as u32,
                &tex.bind_group,
                [0, 0, target[0], target[1]],
                true,
            ));
        }

        for prim in &self.ui {
            let egui::epaint::Primitive::Mesh(mesh) = &prim.primitive else {
                continue;
            };
            let Some(tex) = self.textures.get(&mesh.texture_id) else {
                continue;
            };
            let Some(scissor) = scissor(prim.clip_rect, self.ppp, target) else {
                continue;
            };
            let base = vertex_count(&verts);
            for v in &mesh.vertices {
                push_vertex(
                    &mut verts,
                    [v.pos.x, v.pos.y],
                    [v.uv.x, v.uv.y],
                    v.color.to_array(),
                );
            }
            let start = indices.len() as u32;
            indices.extend(mesh.indices.iter().map(|i| i + base));
            draws.push((start..indices.len() as u32, &tex.bind_group, scissor, false));
        }

        let px_screen = self.screen(device, tw, th);
        let pt_screen = self.screen(device, tw / self.ppp, th / self.ppp);
        let buffers = (!indices.is_empty()).then(|| {
            let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("xarast vertices"),
                contents: &verts,
                usage: wgpu::BufferUsages::VERTEX,
            });
            let ib_bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_ne_bytes()).collect();
            let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("xarast indices"),
                contents: &ib_bytes,
                usage: wgpu::BufferUsages::INDEX,
            });
            (vb, ib)
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("canvas + interface"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let Some((vb, ib)) = &buffers else { return };
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, vb.slice(..));
        pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
        for (range, bind_group, [x, y, w, h], is_canvas) in draws {
            pass.set_bind_group(0, if is_canvas { &px_screen } else { &pt_screen }, &[]);
            pass.set_bind_group(1, bind_group, &[]);
            pass.set_scissor_rect(x, y, w, h);
            pass.draw_indexed(range, 0, 0..1);
        }
    }
}

/// A clip rectangle in points, as a scissor in device pixels clamped to
/// the target. `None` when nothing of it is visible.
fn scissor(clip: egui::Rect, ppp: f32, target: [u32; 2]) -> Option<[u32; 4]> {
    let clamp = |v: f32, max: u32| (v.round().max(0.0) as u32).min(max);
    let x0 = clamp(clip.min.x * ppp, target[0]);
    let y0 = clamp(clip.min.y * ppp, target[1]);
    let x1 = clamp(clip.max.x * ppp, target[0]);
    let y1 = clamp(clip.max.y * ppp, target[1]);
    (x1 > x0 && y1 > y0).then(|| [x0, y0, x1 - x0, y1 - y0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scissors_are_clamped_to_the_target_and_empty_ones_dropped() {
        let r = egui::Rect::from_min_max(egui::pos2(-10.0, 5.0), egui::pos2(50.0, 2000.0));
        assert_eq!(scissor(r, 2.0, [80, 600]), Some([0, 10, 80, 590]));
        let off = egui::Rect::from_min_max(egui::pos2(100.0, 0.0), egui::pos2(120.0, 10.0));
        assert_eq!(scissor(off, 1.0, [80, 600]), None);
    }

    #[test]
    fn a_fractional_scale_rounds_the_scissor_to_whole_pixels() {
        let r = egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(20.0, 20.0));
        assert_eq!(scissor(r, 1.25, [100, 100]), Some([13, 13, 12, 12]));
    }
}
