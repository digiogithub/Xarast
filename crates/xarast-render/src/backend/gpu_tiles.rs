//! The GPU tile cache: CPU-rasterised tiles kept resident on the GPU and
//! composited into a target by a pan or a zoom, behind the `gpu` feature.
//!
//! This is the step XARA-US-0011's decision earns (`docs/memory/render.md`,
//! "The GPU decision"): coverage and paint stay on the deterministic CPU
//! backend, and the GPU does what it is unambiguously good at, which is
//! moving and resampling pixels it already holds. A pan then costs one
//! draw call and the upload of the tiles it exposes, instead of a CPU
//! scroll plus a full-frame upload.
//!
//! * Tiles live in one `Rgba8Unorm` 2D array texture, one layer per tile,
//!   evicted least recently used.
//! * [`GpuTileCache::encode`] draws every placement as one instanced quad.
//!   The fragment shader picks its texel with exactly the expression of
//!   [`crate::compose::source_texel`] and reads it with `textureLoad`: no
//!   sampler, no filtering, so the output is a copy of CPU bytes and the
//!   parity test holds it to [`crate::compose::compose_cpu`] byte for byte.
//! * The device and queue are the caller's, as for [`super::gpu`]. The
//!   target must be `Rgba8Unorm` (never the sRGB variant; see
//!   `research/03 §2.10`) and a render attachment.

use std::collections::HashMap;
use std::sync::Arc;

use crate::backend::BackendError;
use crate::compose::{TexelRect, TileKey, TilePlacement, texel_intersection, texel_rect_is_empty};
use crate::surface::{DeviceRect, Surface};
use crate::tiling::GPU_TILE_SIZE;

use super::gpu::TARGET_FORMAT;

/// How the tile cache is sized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuTileCacheConfig {
    /// Tile edge in pixels; must match the [`crate::TileGrid`] in use.
    pub tile_size: u32,
    /// Tiles resident at once. Clamped to the device's array-layer limit.
    /// A 1080p view needs 40–54 tiles of 256², so the default of 128 holds
    /// the view and a margin of about one screen around it (32 MiB).
    pub capacity: u32,
}

impl Default for GpuTileCacheConfig {
    fn default() -> GpuTileCacheConfig {
        GpuTileCacheConfig {
            tile_size: GPU_TILE_SIZE,
            capacity: 128,
        }
    }
}

/// What one [`GpuTileCache::encode`] drew.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ComposeStats {
    /// Placements drawn.
    pub drawn: u32,
    /// Placements skipped because their tile is not resident.
    pub missing: u32,
}

#[derive(Debug, Clone, Copy)]
struct Slot {
    layer: u32,
    valid: TexelRect,
    used: u64,
}

/// Bytes per instance: origin, inverse scale, rectangle (8 × f32), then
/// the layer (4 × u32, three unused) and the valid texel rectangle
/// (4 × u32).
const INSTANCE_BYTES: u64 = 64;

const SHADER: &str = r"
struct Globals { size: vec2<f32>, pad: vec2<f32> };
@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var atlas: texture_2d_array<f32>;

struct Inst {
    @location(0) origin: vec2<f32>,
    @location(1) inv_scale: vec2<f32>,
    @location(2) rect: vec4<f32>,
    @location(3) info: vec4<u32>,
    @location(4) valid: vec4<u32>,
};

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(flat) origin: vec2<f32>,
    @location(1) @interpolate(flat) inv_scale: vec2<f32>,
    @location(2) @interpolate(flat) info: vec4<u32>,
    @location(3) @interpolate(flat) valid: vec4<u32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Inst) -> VOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let c = corners[vi];
    let p = select(inst.rect.xy, inst.rect.zw, c > vec2<f32>(0.5));
    var out: VOut;
    out.pos = vec4<f32>(p.x / g.size.x * 2.0 - 1.0, 1.0 - p.y / g.size.y * 2.0, 0.0, 1.0);
    out.origin = inst.origin;
    out.inv_scale = inst.inv_scale;
    out.info = inst.info;
    out.valid = inst.valid;
    return out;
}

// The texel rule of `compose::source_texel`: floor((p + 0.5 - origin) * inv).
// `pos.xy` is the pixel centre. One subtraction, one multiplication, in
// this order, or the CPU reference and this shader stop agreeing.
@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let s = floor((in.pos.xy - in.origin) * in.inv_scale);
    let lo = vec2<f32>(f32(in.valid.x), f32(in.valid.y));
    let hi = vec2<f32>(f32(in.valid.z), f32(in.valid.w));
    if (any(s < lo) || any(s >= hi)) {
        discard;
    }
    return textureLoad(atlas, vec2<i32>(s), i32(in.info.x), 0);
}
";

/// CPU-rasterised tiles resident on the GPU, and the pass that composites
/// them.
#[derive(Debug)]
pub struct GpuTileCache {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    cfg: GpuTileCacheConfig,
    atlas: wgpu::Texture,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    globals: wgpu::Buffer,
    instances: wgpu::Buffer,
    slots: HashMap<TileKey, Slot>,
    free: Vec<u32>,
    clock: u64,
}

impl GpuTileCache {
    /// Creates the cache on a device the caller owns.
    ///
    /// # Errors
    ///
    /// [`BackendError::NoAdapter`] when the device cannot hold one tile.
    pub fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        cfg: GpuTileCacheConfig,
    ) -> Result<GpuTileCache, BackendError> {
        let limits = device.limits();
        if cfg.tile_size == 0 || cfg.tile_size > limits.max_texture_dimension_2d {
            return Err(BackendError::NoAdapter(format!(
                "tile size {} is outside the device's 1..={}",
                cfg.tile_size, limits.max_texture_dimension_2d
            )));
        }
        let capacity = cfg
            .capacity
            .clamp(1, limits.max_texture_array_layers.max(1));
        let cfg = GpuTileCacheConfig { capacity, ..cfg };
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("xarast tile atlas"),
            size: wgpu::Extent3d {
                width: cfg.tile_size,
                height: cfg.tile_size,
                depth_or_array_layers: capacity,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_view = atlas.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("xarast tile globals"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("xarast tile instances"),
            size: INSTANCE_BYTES * u64::from(capacity),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("xarast tiles"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("xarast tiles"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("xarast tiles"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("xarast tiles"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attributes = wgpu::vertex_attr_array![
            0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Uint32x4, 4 => Uint32x4
        ];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("xarast tiles"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: INSTANCE_BYTES,
                    step_mode: wgpu::VertexStepMode::Instance,
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
                // Replace: a tile is an opaque piece of the canvas.
                targets: &[Some(wgpu::ColorTargetState {
                    format: TARGET_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Ok(GpuTileCache {
            device,
            queue,
            cfg,
            atlas,
            pipeline,
            bind_group,
            globals,
            instances,
            slots: HashMap::new(),
            free: (0..capacity).rev().collect(),
            clock: 0,
        })
    }

    /// The configuration in force, with the capacity after clamping.
    #[must_use]
    pub const fn config(&self) -> GpuTileCacheConfig {
        self.cfg
    }

    /// Whether a tile is resident.
    #[must_use]
    pub fn contains(&self, key: &TileKey) -> bool {
        self.slots.contains_key(key)
    }

    /// The texels of a resident tile that hold pixels, or `None` when it
    /// is not resident. A presenter uses it to upload only what a tile
    /// lacks, and to start a tile afresh when a new piece would not join
    /// its valid area into a rectangle (see [`GpuTileCache::upload`]).
    #[must_use]
    pub fn valid(&self, key: &TileKey) -> Option<TexelRect> {
        self.slots.get(key).map(|s| s.valid)
    }

    /// Tiles resident now.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether no tile is resident.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// GPU memory the atlas holds, in bytes, resident or not.
    #[must_use]
    pub fn atlas_bytes(&self) -> u64 {
        u64::from(self.cfg.tile_size).pow(2) * 4 * u64::from(self.cfg.capacity)
    }

    /// Forgets one tile; its layer is reused.
    pub fn invalidate(&mut self, key: &TileKey) {
        if let Some(s) = self.slots.remove(key) {
            self.free.push(s.layer);
        }
    }

    /// Forgets every tile, for a new scene or a colour change.
    pub fn clear(&mut self) {
        self.slots.clear();
        self.free = (0..self.cfg.capacity).rev().collect();
    }

    /// Uploads `rect` of `src` into tile `key` with its top-left pixel at
    /// texel `at`.
    ///
    /// A tile new to the cache gets exactly that rectangle as its valid
    /// area. A resident tile's valid area grows to the bounding box of the
    /// old one and the new one, so pieces of one tile must be uploaded so
    /// that their bounding box is covered: a pan's strip completing a
    /// partial tile is, two disjoint corners are not. To start a tile
    /// afresh, [`GpuTileCache::invalidate`] it first.
    ///
    /// When the cache is full the least recently used tile is evicted, so
    /// a caller must not upload more tiles between two composites than
    /// [`GpuTileCacheConfig::capacity`].
    ///
    /// # Errors
    ///
    /// [`BackendError::SurfaceTooLarge`] when `rect` is empty, not inside
    /// `src`, or does not fit in the tile at `at`.
    pub fn upload(
        &mut self,
        key: TileKey,
        src: &Surface,
        rect: DeviceRect,
        at: [u32; 2],
    ) -> Result<(), BackendError> {
        let ts = self.cfg.tile_size;
        if rect.is_empty()
            || rect.intersection(src.bounds()) != rect
            || u64::from(at[0]) + u64::from(rect.width()) > u64::from(ts)
            || u64::from(at[1]) + u64::from(rect.height()) > u64::from(ts)
        {
            return Err(BackendError::SurfaceTooLarge {
                width: rect.width(),
                height: rect.height(),
                max: ts,
            });
        }
        let piece = [at[0], at[1], at[0] + rect.width(), at[1] + rect.height()];
        let (layer, valid) = match self.slots.get(&key) {
            Some(s) => (
                s.layer,
                [
                    s.valid[0].min(piece[0]),
                    s.valid[1].min(piece[1]),
                    s.valid[2].max(piece[2]),
                    s.valid[3].max(piece[3]),
                ],
            ),
            None => match self.free.pop() {
                Some(l) => (l, piece),
                None => (self.evict_lru(), piece),
            },
        };
        let stride = src.width() as usize * 4;
        // `rect` is inside `src`, so both are non-negative.
        let offset = rect.y0 as usize * stride + rect.x0 as usize * 4;
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.atlas,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: at[0],
                    y: at[1],
                    z: layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &src.data()[offset..],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(src.width() * 4),
                rows_per_image: Some(rect.height()),
            },
            wgpu::Extent3d {
                width: rect.width(),
                height: rect.height(),
                depth_or_array_layers: 1,
            },
        );
        self.clock += 1;
        self.slots.insert(
            key,
            Slot {
                layer,
                valid,
                used: self.clock,
            },
        );
        Ok(())
    }

    fn evict_lru(&mut self) -> u32 {
        let oldest = self
            .slots
            .iter()
            .min_by_key(|(k, s)| (s.used, **k))
            .map(|(k, s)| (*k, s.layer));
        match oldest {
            Some((k, layer)) => {
                self.slots.remove(&k);
                layer
            }
            // Unreachable with capacity >= 1: a full cache has a slot.
            None => 0,
        }
    }

    /// Records the composite of `placements` into `target` on `encoder`:
    /// the target is cleared to `backdrop`, then every resident tile is
    /// drawn in order. A placement whose tile is not resident is skipped
    /// and counted, which leaves the backdrop showing where it would be.
    /// At most `capacity` placements are drawn.
    ///
    /// The caller submits the encoder, which is what lets the shell put
    /// this pass in the same submission as its interface pass. The
    /// instance data goes through `Queue::write_buffer`, so encode **once
    /// per submission**: a second encode before the submit would overwrite
    /// the first one's placements.
    ///
    /// # Errors
    ///
    /// [`BackendError::WrongTargetFormat`] for anything but `Rgba8Unorm`,
    /// or a target that is not a render attachment.
    pub fn encode(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        placements: &[TilePlacement],
        backdrop: [u8; 4],
    ) -> Result<ComposeStats, BackendError> {
        if target.format() != TARGET_FORMAT
            || !target
                .usage()
                .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
        {
            return Err(BackendError::WrongTargetFormat);
        }
        let (w, h) = (target.width(), target.height());
        let bounds = DeviceRect::from_size(w, h);
        let mut stats = ComposeStats::default();
        let mut bytes = Vec::with_capacity(placements.len() * INSTANCE_BYTES as usize);
        self.clock += 1;
        for p in placements {
            if stats.drawn >= self.cfg.capacity {
                stats.missing += 1;
                continue;
            }
            let Some(slot) = self.slots.get_mut(&p.key) else {
                stats.missing += 1;
                continue;
            };
            slot.used = self.clock;
            let valid = texel_intersection(p.valid, slot.valid);
            let r = TilePlacement { valid, ..*p }
                .target_rect()
                .intersection(bounds);
            if r.is_empty() || texel_rect_is_empty(valid) {
                continue;
            }
            stats.drawn += 1;
            for v in [p.origin[0], p.origin[1], p.inv_scale[0], p.inv_scale[1]] {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            for v in [r.x0, r.y0, r.x1, r.y1] {
                // f32-ok: target-local pixel bounds, at most the texture limit.
                bytes.extend_from_slice(&(v as f32).to_le_bytes());
            }
            for v in [slot.layer, 0, 0, 0] {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            for v in valid {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
        }
        let mut globals = Vec::with_capacity(16);
        // f32-ok: texture dimensions, at most the device limit.
        for v in [w as f32, h as f32, 0.0, 0.0] {
            globals.extend_from_slice(&v.to_le_bytes());
        }
        self.queue.write_buffer(&self.globals, 0, &globals);
        if !bytes.is_empty() {
            self.queue.write_buffer(&self.instances, 0, &bytes);
        }
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let c = |v: u8| f64::from(v) / 255.0;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("xarast tiles"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: c(backdrop[0]),
                        g: c(backdrop[1]),
                        b: c(backdrop[2]),
                        a: c(backdrop[3]),
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if stats.drawn > 0 {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.instances.slice(..bytes.len() as u64));
            pass.draw(0..6, 0..stats.drawn);
        }
        Ok(stats)
    }

    /// [`GpuTileCache::encode`] on an encoder of its own, submitted.
    ///
    /// # Errors
    ///
    /// As [`GpuTileCache::encode`].
    pub fn compose(
        &mut self,
        target: &wgpu::Texture,
        placements: &[TilePlacement],
        backdrop: [u8; 4],
    ) -> Result<ComposeStats, BackendError> {
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("xarast tiles"),
            });
        let stats = self.encode(&mut enc, target, placements, backdrop)?;
        self.queue.submit([enc.finish()]);
        Ok(stats)
    }
}

/// Creates an `Rgba8Unorm` texture the tile cache can composite into, that
/// can be read back or sampled, and that whole frames can be written into
/// (the path the software tier and the benches' baseline take).
#[must_use]
pub fn create_target(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("xarast composite target"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: TARGET_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Reads an `Rgba8Unorm` texture back into a [`Surface`], waiting for the
/// queue. For tests, benches and screenshots; never on a frame's path.
///
/// # Errors
///
/// [`BackendError::WrongTargetFormat`] for another format, and
/// [`BackendError::NoAdapter`] when the device is lost while mapping.
pub fn read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
) -> Result<Surface, BackendError> {
    if texture.format() != TARGET_FORMAT {
        return Err(BackendError::WrongTargetFormat);
    }
    let (w, h) = (texture.width(), texture.height());
    let row = w * 4;
    let padded = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("xarast read back"),
        size: u64::from(padded) * u64::from(h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    enc.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: None,
            },
        },
        texture.size(),
    );
    queue.submit([enc.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| BackendError::NoAdapter(e.to_string()))?;
    rx.recv()
        .map_err(|e| BackendError::NoAdapter(e.to_string()))?
        .map_err(|e| BackendError::NoAdapter(e.to_string()))?;
    let mut out = Surface::new(w, h);
    {
        let data = buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|e| BackendError::NoAdapter(e.to_string()))?;
        let (row, padded) = (row as usize, padded as usize);
        for (dst, src) in out
            .data_mut()
            .chunks_exact_mut(row)
            .zip(data.chunks_exact(padded))
        {
            dst.copy_from_slice(&src[..row]);
        }
    }
    buffer.unmap();
    Ok(out)
}
