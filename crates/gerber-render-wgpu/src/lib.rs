//! Headless `wgpu` renderer for backend-neutral Gerber render plans.
//!
//! The renderer performs geometry conversion and Lyon tessellation on the CPU,
//! uploads one immutable mesh, renders into an RGBA8 texture, and reads the
//! result back without requiring a window or surface.
//!
use std::ops::Range;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc;

use bytemuck::{Pod, Zeroable};
use lyon_path::Path;
use lyon_path::math::{Point, point};
use lyon_tessellation::geometry_builder::{
    BuffersBuilder, FillVertexConstructor, StrokeVertexConstructor, VertexBuffers,
};
use lyon_tessellation::{
    FillOptions, FillTessellator, FillVertex, LineCap, LineJoin, StrokeOptions, StrokeTessellator,
    StrokeVertex,
};
use thiserror::Error;
use wgpu::util::DeviceExt;

use gerber_render_plan::{
    ArcDirection, Polarity, RenderBounds, RenderGeometry, RenderOperation, RenderPlan, RenderPoint,
    RenderPolygon,
};

const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const BYTES_PER_PIXEL: u32 = 4;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 512 * 1024 * 1024;
const DEFAULT_MAX_VERTICES: usize = 4_000_000;
const DEFAULT_MAX_INDICES: usize = 12_000_000;
const MAX_ARC_SEGMENTS: usize = 262_144;
const ARC_RADIUS_ABSOLUTE_TOLERANCE_IU: f64 = 5.0;
const ARC_RADIUS_RELATIVE_TOLERANCE: f64 = 1.0e-5;

const SHADER_SOURCE: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(input.position, 0.0, 1.0);
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
"#;

/// An unpremultiplied 8-bit RGBA color.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn to_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    fn to_unorm_array(self) -> [f32; 4] {
        let scale = 1.0 / 255.0;
        [
            self.r as f32 * scale,
            self.g as f32 * scale,
            self.b as f32 * scale,
            self.a as f32 * scale,
        ]
    }

    fn to_wgpu_color(self) -> wgpu::Color {
        let scale = 1.0 / 255.0;
        wgpu::Color {
            r: self.r as f64 * scale,
            g: self.g as f64 * scale,
            b: self.b as f64 * scale,
            a: self.a as f64 * scale,
        }
    }
}

impl Default for Rgba8 {
    fn default() -> Self {
        Self::new(0, 0, 0, 0)
    }
}

/// Immutable renderer settings.
#[derive(Clone, Debug)]
pub struct RendererConfig {
    pub width: u32,
    pub height: u32,
    pub foreground: Rgba8,
    pub background: Rgba8,
    /// Requested sample count. The selected adapter must support this count for
    /// `Rgba8Unorm`; common values are 1 and 4.
    pub msaa_samples: u32,
    /// Empty border, in output pixels, retained around the plan bounding box.
    pub padding_px: f32,
    /// Maximum screen-space sagitta error used when flattening analytic arcs.
    pub arc_error_px: f32,
    /// Lyon's screen-space tessellation tolerance.
    pub tessellation_tolerance_px: f32,
    pub power_preference: wgpu::PowerPreference,
    pub force_fallback_adapter: bool,
    /// Hard allocation guard for the tightly packed returned RGBA bytes.
    pub max_output_bytes: usize,
    /// Hard CPU/GPU mesh guards for untrusted or unexpectedly complex input.
    pub max_vertices: usize,
    pub max_indices: usize,
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            width: 1600,
            height: 1200,
            foreground: Rgba8::new(214, 170, 63, 255),
            background: Rgba8::new(16, 20, 24, 255),
            msaa_samples: 4,
            padding_px: 16.0,
            arc_error_px: 0.25,
            tessellation_tolerance_px: 0.1,
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES,
            max_vertices: DEFAULT_MAX_VERTICES,
            max_indices: DEFAULT_MAX_INDICES,
        }
    }
}

/// Tightly packed, top-to-bottom RGBA8 pixels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl RenderedImage {
    pub fn row(&self, y: u32) -> Option<&[u8]> {
        if y >= self.height {
            return None;
        }

        let row_bytes = usize::try_from(self.width).ok()?.checked_mul(4)?;
        let start = usize::try_from(y).ok()?.checked_mul(row_bytes)?;
        let end = start.checked_add(row_bytes)?;
        self.rgba.get(start..end)
    }
}

/// Errors are returned for configuration, geometry, GPU, and readback failures.
#[derive(Debug, Error)]
pub enum RenderError {
    #[error("output dimensions must be non-zero (received {width}x{height})")]
    ZeroDimensions { width: u32, height: u32 },

    #[error("renderer setting {name} must be finite and {requirement} (received {value})")]
    InvalidFloatSetting {
        name: &'static str,
        requirement: &'static str,
        value: f32,
    },

    #[error("padding {padding_px}px leaves no drawable area in a {width}x{height} output")]
    PaddingConsumesOutput {
        padding_px: f32,
        width: u32,
        height: u32,
    },

    #[error("renderer limit {name} must be greater than zero")]
    ZeroLimit { name: &'static str },

    #[error("requested output requires {required} bytes, exceeding configured limit {limit} bytes")]
    OutputTooLarge { required: usize, limit: usize },

    #[error("arithmetic overflow while calculating {context}")]
    ArithmeticOverflow { context: &'static str },

    #[error("wgpu was compiled without a native graphics backend")]
    NoCompiledBackend,

    #[error("no compatible headless GPU adapter was found")]
    AdapterUnavailable(#[source] wgpu::RequestAdapterError),

    #[error("failed to create wgpu device")]
    DeviceRequest(#[source] wgpu::RequestDeviceError),

    #[error(
        "requested {width}x{height} texture exceeds adapter limit {max_dimension} per dimension"
    )]
    TextureDimensionUnsupported {
        width: u32,
        height: u32,
        max_dimension: u32,
    },

    #[error("sample count {requested} is unsupported for RGBA8 on adapter {adapter}")]
    UnsupportedSampleCount { requested: u32, adapter: String },

    #[error("wgpu reported an error during {stage}: {message}")]
    GpuFailure {
        stage: &'static str,
        message: String,
    },

    #[error("non-empty render plan has no bounding box")]
    MissingPlanBounds,

    #[error(
        "render operations are not in ascending draw order: {previous} is followed by {current}"
    )]
    DrawOrderRegression { previous: u64, current: u64 },

    #[error("draw operation {draw_order} has invalid geometry: {message}")]
    InvalidGeometry { draw_order: u64, message: String },

    #[error("Lyon tessellation failed for draw operation {draw_order}: {message}")]
    Tessellation { draw_order: u64, message: String },

    #[error(
        "tessellated mesh exceeds configured limits: {vertices}/{max_vertices} vertices, \
         {indices}/{max_indices} indices"
    )]
    MeshTooLarge {
        vertices: usize,
        indices: usize,
        max_vertices: usize,
        max_indices: usize,
    },

    #[error("GPU polling failed")]
    DevicePoll(#[source] wgpu::PollError),

    #[error("GPU readback mapping failed")]
    BufferMap(#[source] wgpu::BufferAsyncError),

    #[error("GPU readback callback terminated without a result")]
    ReadbackCallbackClosed,

    #[cfg(target_arch = "wasm32")]
    #[error("native blocking readback is not available on wasm32")]
    ReadbackUnsupportedOnWasm,
}

/// Reusable headless renderer. Each `render` call creates transient output,
/// multisample, upload, and readback resources, so calls may use independent
/// render plans without rebuilding the device or pipeline.
pub struct OffscreenRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    config: RendererConfig,
    adapter_info: wgpu::AdapterInfo,
}

impl OffscreenRenderer {
    /// Select a headless adapter and asynchronously initialize the device.
    pub async fn new(config: RendererConfig) -> Result<Self, RenderError> {
        validate_config(&config)?;

        if wgpu::Instance::enabled_backend_features().is_empty() {
            return Err(RenderError::NoCompiledBackend);
        }

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: config.power_preference,
                force_fallback_adapter: config.force_fallback_adapter,
                compatible_surface: None,
            })
            .await
            .map_err(RenderError::AdapterUnavailable)?;
        let adapter_info = adapter.get_info();
        let adapter_limits = adapter.limits();
        let requested_dimension = config.width.max(config.height);

        if requested_dimension > adapter_limits.max_texture_dimension_2d {
            return Err(RenderError::TextureDimensionUnsupported {
                width: config.width,
                height: config.height,
                max_dimension: adapter_limits.max_texture_dimension_2d,
            });
        }

        let format_features = adapter.get_texture_format_features(OUTPUT_FORMAT);
        if !format_features
            .flags
            .sample_count_supported(config.msaa_samples)
        {
            return Err(RenderError::UnsupportedSampleCount {
                requested: config.msaa_samples,
                adapter: adapter_label(&adapter_info),
            });
        }

        let required_limits = wgpu::Limits {
            max_texture_dimension_2d: requested_dimension,
            ..wgpu::Limits::default()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("gerber-offscreen-device"),
                required_features: wgpu::Features::empty(),
                required_limits,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(RenderError::DeviceRequest)?;

        let pipeline_error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gerber-offscreen-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SOURCE.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gerber-offscreen-pipeline-layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gerber-offscreen-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[GpuVertex::layout()],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: config.msaa_samples,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: OUTPUT_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        if let Some(error) = pipeline_error_scope.pop().await {
            return Err(RenderError::GpuFailure {
                stage: "pipeline creation",
                message: error.to_string(),
            });
        }

        Ok(Self {
            device,
            queue,
            pipeline,
            config,
            adapter_info,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_blocking(config: RendererConfig) -> Result<Self, RenderError> {
        pollster::block_on(Self::new(config))
    }

    pub fn config(&self) -> &RendererConfig {
        &self.config
    }

    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    /// Tessellate, render, and asynchronously read back one plan.
    pub async fn render(&self, plan: &RenderPlan) -> Result<RenderedImage, RenderError> {
        let mesh = tessellate_plan(plan, &self.config)?;
        let extent = wgpu::Extent3d {
            width: self.config.width,
            height: self.config.height,
            depth_or_array_layers: 1,
        };

        let validation_scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let out_of_memory_scope = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);

        let output_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gerber-offscreen-output"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let output_view = output_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let msaa_texture = (self.config.msaa_samples > 1).then(|| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("gerber-offscreen-msaa"),
                size: extent,
                mip_level_count: 1,
                sample_count: self.config.msaa_samples,
                dimension: wgpu::TextureDimension::D2,
                format: OUTPUT_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
        });
        let msaa_view = msaa_texture
            .as_ref()
            .map(|texture| texture.create_view(&wgpu::TextureViewDescriptor::default()));

        let vertex_buffer = (!mesh.vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("gerber-offscreen-vertices"),
                    contents: bytemuck::cast_slice(&mesh.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let index_buffer = (!mesh.indices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("gerber-offscreen-indices"),
                    contents: bytemuck::cast_slice(&mesh.indices),
                    usage: wgpu::BufferUsages::INDEX,
                })
        });

        let row_layout = ReadbackLayout::new(&self.config)?;
        let readback_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gerber-offscreen-readback"),
            size: row_layout.buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("gerber-offscreen-command-encoder"),
            });

        {
            let color_view = msaa_view.as_ref().unwrap_or(&output_view);
            let resolve_target = msaa_view.as_ref().map(|_| &output_view);
            let color_attachment = Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                depth_slice: None,
                resolve_target,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(self.config.background.to_wgpu_color()),
                    store: if resolve_target.is_some() {
                        wgpu::StoreOp::Discard
                    } else {
                        wgpu::StoreOp::Store
                    },
                },
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("gerber-offscreen-render-pass"),
                color_attachments: &[color_attachment],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            if let (Some(vertices), Some(indices)) = (&vertex_buffer, &index_buffer) {
                pass.set_pipeline(&self.pipeline);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);

                for draw in &mesh.draws {
                    pass.draw_indexed(draw.index_range.clone(), 0, 0..1);
                }
            }
        }

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &output_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_layout.padded_bytes_per_row),
                    rows_per_image: Some(self.config.height),
                },
            },
            extent,
        );

        self.queue.submit([encoder.finish()]);

        let out_of_memory_error = out_of_memory_scope.pop().await;
        let validation_error = validation_scope.pop().await;
        if let Some(error) = out_of_memory_error.or(validation_error) {
            return Err(RenderError::GpuFailure {
                stage: "rendering",
                message: error.to_string(),
            });
        }

        let rgba = read_buffer_rgba(
            &self.device,
            &readback_buffer,
            self.config.height,
            row_layout,
        )
        .await?;

        Ok(RenderedImage {
            width: self.config.width,
            height: self.config.height,
            rgba,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn render_blocking(&self, plan: &RenderPlan) -> Result<RenderedImage, RenderError> {
        pollster::block_on(self.render(plan))
    }
}

/// Convenience entry point for one-shot rendering.
pub async fn render_plan_rgba(
    plan: &RenderPlan,
    config: RendererConfig,
) -> Result<RenderedImage, RenderError> {
    OffscreenRenderer::new(config).await?.render(plan).await
}

#[cfg(not(target_arch = "wasm32"))]
pub fn render_plan_rgba_blocking(
    plan: &RenderPlan,
    config: RendererConfig,
) -> Result<RenderedImage, RenderError> {
    pollster::block_on(render_plan_rgba(plan, config))
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct GpuVertex {
    position: [f32; 2],
    color: [f32; 4],
}

impl GpuVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

struct Mesh {
    vertices: Vec<GpuVertex>,
    indices: Vec<u32>,
    draws: Vec<Draw>,
}

struct Draw {
    index_range: Range<u32>,
}

struct VertexCtor {
    color: [f32; 4],
    target_width: f32,
    target_height: f32,
}

impl VertexCtor {
    fn vertex(&self, position: Point) -> GpuVertex {
        GpuVertex {
            position: [
                position.x * 2.0 / self.target_width - 1.0,
                1.0 - position.y * 2.0 / self.target_height,
            ],
            color: self.color,
        }
    }
}

impl FillVertexConstructor<GpuVertex> for VertexCtor {
    fn new_vertex(&mut self, vertex: FillVertex<'_>) -> GpuVertex {
        self.vertex(vertex.position())
    }
}

impl StrokeVertexConstructor<GpuVertex> for VertexCtor {
    fn new_vertex(&mut self, vertex: StrokeVertex<'_, '_>) -> GpuVertex {
        self.vertex(vertex.position())
    }
}

#[derive(Clone, Copy)]
struct ViewTransform {
    scale: f64,
    offset_x: f64,
    offset_y: f64,
}

impl ViewTransform {
    fn from_plan(plan: &RenderPlan, config: &RendererConfig) -> Result<Option<Self>, RenderError> {
        let Some(bbox) = plan.bbox else {
            if plan.operations.is_empty() {
                return Ok(None);
            }
            return Err(RenderError::MissingPlanBounds);
        };

        let (min_x, min_y, max_x, max_y) = normalized_extents(bbox);
        let world_width = (max_x - min_x).max(1) as f64;
        let world_height = (max_y - min_y).max(1) as f64;
        let padding = config.padding_px as f64;
        let available_width = config.width as f64 - padding * 2.0;
        let available_height = config.height as f64 - padding * 2.0;
        let scale = (available_width / world_width).min(available_height / world_height);
        let drawn_width = (max_x - min_x) as f64 * scale;
        let drawn_height = (max_y - min_y) as f64 * scale;
        let left = (config.width as f64 - drawn_width) * 0.5;
        let top = (config.height as f64 - drawn_height) * 0.5;

        Ok(Some(Self {
            scale,
            offset_x: left - min_x as f64 * scale,
            offset_y: top - min_y as f64 * scale,
        }))
    }

    fn point(self, value: RenderPoint) -> Point {
        self.point_f64(value.x as f64, value.y as f64)
    }

    fn point_f64(self, x: f64, y: f64) -> Point {
        point(
            (x * self.scale + self.offset_x) as f32,
            (y * self.scale + self.offset_y) as f32,
        )
    }

    fn width(self, world_width: i32) -> f32 {
        (world_width as f64 * self.scale) as f32
    }
}

fn validate_config(config: &RendererConfig) -> Result<(), RenderError> {
    if config.width == 0 || config.height == 0 {
        return Err(RenderError::ZeroDimensions {
            width: config.width,
            height: config.height,
        });
    }

    validate_positive_finite("arc_error_px", config.arc_error_px)?;
    validate_positive_finite(
        "tessellation_tolerance_px",
        config.tessellation_tolerance_px,
    )?;

    if !config.padding_px.is_finite() || config.padding_px < 0.0 {
        return Err(RenderError::InvalidFloatSetting {
            name: "padding_px",
            requirement: "non-negative",
            value: config.padding_px,
        });
    }

    if config.padding_px * 2.0 >= config.width as f32
        || config.padding_px * 2.0 >= config.height as f32
    {
        return Err(RenderError::PaddingConsumesOutput {
            padding_px: config.padding_px,
            width: config.width,
            height: config.height,
        });
    }

    for (name, value) in [
        ("max_output_bytes", config.max_output_bytes),
        ("max_vertices", config.max_vertices),
        ("max_indices", config.max_indices),
    ] {
        if value == 0 {
            return Err(RenderError::ZeroLimit { name });
        }
    }

    let output_bytes = usize::try_from(config.width)
        .ok()
        .and_then(|width| width.checked_mul(BYTES_PER_PIXEL as usize))
        .and_then(|row| {
            usize::try_from(config.height)
                .ok()
                .and_then(|height| row.checked_mul(height))
        })
        .ok_or(RenderError::ArithmeticOverflow {
            context: "output byte size",
        })?;

    if output_bytes > config.max_output_bytes {
        return Err(RenderError::OutputTooLarge {
            required: output_bytes,
            limit: config.max_output_bytes,
        });
    }

    Ok(())
}

fn validate_positive_finite(name: &'static str, value: f32) -> Result<(), RenderError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(RenderError::InvalidFloatSetting {
            name,
            requirement: "greater than zero",
            value,
        });
    }
    Ok(())
}

fn tessellate_plan(plan: &RenderPlan, config: &RendererConfig) -> Result<Mesh, RenderError> {
    let Some(transform) = ViewTransform::from_plan(plan, config)? else {
        return Ok(Mesh {
            vertices: Vec::new(),
            indices: Vec::new(),
            draws: Vec::new(),
        });
    };
    let mut buffers = VertexBuffers::<GpuVertex, u32>::new();
    let mut draws = Vec::with_capacity(plan.operations.len());
    let mut previous_order = None;

    for operation in &plan.operations {
        if let Some(previous) = previous_order
            && operation.draw_order < previous
        {
            return Err(RenderError::DrawOrderRegression {
                previous,
                current: operation.draw_order,
            });
        }
        previous_order = Some(operation.draw_order);

        let first_index =
            u32::try_from(buffers.indices.len()).map_err(|_| RenderError::MeshTooLarge {
                vertices: buffers.vertices.len(),
                indices: buffers.indices.len(),
                max_vertices: config.max_vertices,
                max_indices: config.max_indices,
            })?;
        tessellate_operation(operation, transform, config, &mut buffers)?;
        enforce_mesh_limits(&buffers, config)?;
        let end_index =
            u32::try_from(buffers.indices.len()).map_err(|_| RenderError::MeshTooLarge {
                vertices: buffers.vertices.len(),
                indices: buffers.indices.len(),
                max_vertices: config.max_vertices,
                max_indices: config.max_indices,
            })?;

        if end_index > first_index {
            draws.push(Draw {
                index_range: first_index..end_index,
            });
        }
    }

    Ok(Mesh {
        vertices: buffers.vertices,
        indices: buffers.indices,
        draws,
    })
}

/// Match the current render-plan primitive variants in one deliberately small
/// adapter layer. GPU resource code below this point has no plan dependencies.
fn tessellate_operation(
    operation: &RenderOperation,
    transform: ViewTransform,
    config: &RendererConfig,
    buffers: &mut VertexBuffers<GpuVertex, u32>,
) -> Result<(), RenderError> {
    let color = match operation.effective_polarity {
        Polarity::Positive => config.foreground,
        Polarity::Negative => config.background,
    }
    .to_unorm_array();
    let ctor = || VertexCtor {
        color,
        target_width: config.width as f32,
        target_height: config.height as f32,
    };

    match &operation.geometry {
        RenderGeometry::FillPath(fill) => {
            tessellate_fill(
                &fill.polygons,
                operation.draw_order,
                transform,
                config,
                buffers,
                ctor,
            )?;
        }
        RenderGeometry::StrokeLine(line) => {
            let width = validate_stroke_width(line.width, transform, operation.draw_order)?;
            let transformed_start = transform.point(line.start);
            let transformed_end = transform.point(line.end);

            if line.start == line.end {
                tessellate_point_stroke(
                    transformed_start,
                    width,
                    operation.draw_order,
                    config,
                    buffers,
                    ctor(),
                )?;
            } else {
                let mut builder = Path::builder();
                builder.begin(transformed_start);
                builder.line_to(transformed_end);
                builder.end(false);
                tessellate_stroke_path(
                    &builder.build(),
                    width,
                    operation.draw_order,
                    config,
                    buffers,
                    ctor(),
                )?;
            }
        }
        RenderGeometry::StrokeArc(plan_arc) => {
            let width = validate_stroke_width(plan_arc.width, transform, operation.draw_order)?;
            let arc = ArcGeometry {
                start: plan_arc.start,
                end: plan_arc.end,
                center: plan_arc.center,
                direction: plan_arc.direction,
                full_circle: plan_arc.full_circle,
            };
            let path = flattened_arc_path(arc, operation.draw_order, transform, config)?;
            tessellate_stroke_path(&path, width, operation.draw_order, config, buffers, ctor())?;
        }
    }

    Ok(())
}

fn tessellate_fill<F>(
    polygons: &[RenderPolygon],
    draw_order: u64,
    transform: ViewTransform,
    config: &RendererConfig,
    buffers: &mut VertexBuffers<GpuVertex, u32>,
    mut ctor: F,
) -> Result<(), RenderError>
where
    F: FnMut() -> VertexCtor,
{
    let mut tessellator = FillTessellator::new();
    let options = FillOptions::even_odd().with_tolerance(config.tessellation_tolerance_px);

    for polygon in polygons {
        let mut builder = Path::builder();
        let has_outline = append_closed_ring(&mut builder, &polygon.outline, transform);

        for hole in &polygon.holes {
            append_closed_ring(&mut builder, hole, transform);
        }

        if !has_outline {
            continue;
        }

        let path = builder.build();
        tessellator
            .tessellate_path(
                path.as_slice(),
                &options,
                &mut BuffersBuilder::new(buffers, ctor()),
            )
            .map_err(|error| RenderError::Tessellation {
                draw_order,
                message: error.to_string(),
            })?;
    }

    Ok(())
}

fn append_closed_ring(
    builder: &mut lyon_path::path::Builder,
    points: &[RenderPoint],
    transform: ViewTransform,
) -> bool {
    let mut unique = Vec::with_capacity(points.len());

    for value in points.iter().copied() {
        if unique.last().copied() != Some(value) {
            unique.push(value);
        }
    }

    while unique.len() > 1 && unique.first() == unique.last() {
        unique.pop();
    }

    if unique.len() < 3 {
        return false;
    }

    builder.begin(transform.point(unique[0]));
    for value in unique.iter().copied().skip(1) {
        builder.line_to(transform.point(value));
    }
    builder.end(true);
    true
}

fn tessellate_stroke_path(
    path: &Path,
    width: f32,
    draw_order: u64,
    config: &RendererConfig,
    buffers: &mut VertexBuffers<GpuVertex, u32>,
    ctor: VertexCtor,
) -> Result<(), RenderError> {
    let options = StrokeOptions::default()
        .with_line_width(width)
        .with_line_cap(LineCap::Round)
        .with_line_join(LineJoin::Round)
        .with_tolerance(config.tessellation_tolerance_px);

    StrokeTessellator::new()
        .tessellate_path(
            path.as_slice(),
            &options,
            &mut BuffersBuilder::new(buffers, ctor),
        )
        .map_err(|error| RenderError::Tessellation {
            draw_order,
            message: error.to_string(),
        })
}

fn tessellate_point_stroke(
    center: Point,
    width: f32,
    draw_order: u64,
    config: &RendererConfig,
    buffers: &mut VertexBuffers<GpuVertex, u32>,
    ctor: VertexCtor,
) -> Result<(), RenderError> {
    FillTessellator::new()
        .tessellate_circle(
            center,
            width * 0.5,
            &FillOptions::even_odd().with_tolerance(config.tessellation_tolerance_px),
            &mut BuffersBuilder::new(buffers, ctor),
        )
        .map_err(|error| RenderError::Tessellation {
            draw_order,
            message: error.to_string(),
        })
}

fn validate_stroke_width(
    width: i32,
    transform: ViewTransform,
    draw_order: u64,
) -> Result<f32, RenderError> {
    if width <= 0 {
        return Err(RenderError::InvalidGeometry {
            draw_order,
            message: format!("stroke width must be positive, received {width}"),
        });
    }

    let pixels = transform.width(width);
    if !pixels.is_finite() || pixels <= 0.0 {
        return Err(RenderError::InvalidGeometry {
            draw_order,
            message: format!("stroke width {width} maps to invalid pixel width {pixels}"),
        });
    }
    Ok(pixels)
}

#[derive(Clone, Copy, Debug)]
struct ArcGeometry {
    start: RenderPoint,
    end: RenderPoint,
    center: RenderPoint,
    direction: ArcDirection,
    full_circle: bool,
}

fn flattened_arc_path(
    arc: ArcGeometry,
    draw_order: u64,
    transform: ViewTransform,
    config: &RendererConfig,
) -> Result<Path, RenderError> {
    let center_x = arc.center.x as f64;
    let center_y = arc.center.y as f64;
    let start_dx = arc.start.x as f64 - center_x;
    let start_dy = arc.start.y as f64 - center_y;
    let end_dx = arc.end.x as f64 - center_x;
    let end_dy = arc.end.y as f64 - center_y;
    let radius = start_dx.hypot(start_dy);

    if !radius.is_finite() || radius <= 0.0 {
        if arc.start == arc.end && !arc.full_circle {
            let mut builder = Path::builder();
            builder.begin(transform.point(arc.start));
            builder.end(false);
            return Ok(builder.build());
        }
        return Err(RenderError::InvalidGeometry {
            draw_order,
            message: "arc start coincides with its center".to_string(),
        });
    }

    let end_radius = end_dx.hypot(end_dy);
    let radius_mismatch = (radius - end_radius).abs();
    let radius_tolerance = radius.mul_add(
        ARC_RADIUS_RELATIVE_TOLERANCE,
        ARC_RADIUS_ABSOLUTE_TOLERANCE_IU,
    );
    if !arc.full_circle && radius_mismatch > radius_tolerance {
        return Err(RenderError::InvalidGeometry {
            draw_order,
            message: format!(
                "arc endpoint radii differ by {radius_mismatch:.3} world units \
                 (start {radius:.3}, end {end_radius:.3}, tolerance {radius_tolerance:.3})"
            ),
        });
    }

    let start_angle = start_dy.atan2(start_dx);
    let end_angle = end_dy.atan2(end_dx);
    let sweep = arc_sweep(start_angle, end_angle, arc.direction, arc.full_circle);
    let radius_px = radius * transform.scale;
    let segment_count = adaptive_arc_segment_count(
        radius_px,
        sweep.abs(),
        config.arc_error_px as f64,
        draw_order,
    )?;
    let mut builder = Path::builder();
    builder.begin(transform.point(arc.start));

    if arc.full_circle {
        for index in 1..segment_count {
            let angle = start_angle + sweep * index as f64 / segment_count as f64;
            builder.line_to(transform.point_f64(
                center_x + radius * angle.cos(),
                center_y + radius * angle.sin(),
            ));
        }
        builder.end(true);
    } else {
        for index in 1..=segment_count {
            if index == segment_count {
                builder.line_to(transform.point(arc.end));
            } else {
                let angle = start_angle + sweep * index as f64 / segment_count as f64;
                builder.line_to(transform.point_f64(
                    center_x + radius * angle.cos(),
                    center_y + radius * angle.sin(),
                ));
            }
        }
        builder.end(false);
    }

    Ok(builder.build())
}

fn arc_sweep(start_angle: f64, end_angle: f64, direction: ArcDirection, full_circle: bool) -> f64 {
    if full_circle {
        return match direction {
            ArcDirection::Clockwise => -std::f64::consts::TAU,
            ArcDirection::CounterClockwise => std::f64::consts::TAU,
        };
    }

    let mut sweep = end_angle - start_angle;
    match direction {
        ArcDirection::CounterClockwise => {
            while sweep <= 0.0 {
                sweep += std::f64::consts::TAU;
            }
        }
        ArcDirection::Clockwise => {
            while sweep >= 0.0 {
                sweep -= std::f64::consts::TAU;
            }
        }
    }
    sweep
}

fn adaptive_arc_segment_count(
    radius_px: f64,
    sweep: f64,
    max_error_px: f64,
    draw_order: u64,
) -> Result<usize, RenderError> {
    if !radius_px.is_finite() || radius_px <= 0.0 || !sweep.is_finite() || sweep <= 0.0 {
        return Err(RenderError::InvalidGeometry {
            draw_order,
            message: format!("invalid screen-space arc radius {radius_px} or sweep {sweep}"),
        });
    }

    let relative_error = (max_error_px / radius_px).clamp(f64::EPSILON, 1.0);
    let max_step = (2.0 * (1.0 - relative_error).acos()).min(std::f64::consts::FRAC_PI_4);
    let count = (sweep / max_step).ceil().max(1.0);

    if count > MAX_ARC_SEGMENTS as f64 {
        return Err(RenderError::InvalidGeometry {
            draw_order,
            message: format!(
                "arc requires {count:.0} segments to satisfy {max_error_px}px error limit; \
                 maximum is {MAX_ARC_SEGMENTS}"
            ),
        });
    }

    Ok(count as usize)
}

fn enforce_mesh_limits(
    buffers: &VertexBuffers<GpuVertex, u32>,
    config: &RendererConfig,
) -> Result<(), RenderError> {
    if buffers.vertices.len() > config.max_vertices || buffers.indices.len() > config.max_indices {
        return Err(RenderError::MeshTooLarge {
            vertices: buffers.vertices.len(),
            indices: buffers.indices.len(),
            max_vertices: config.max_vertices,
            max_indices: config.max_indices,
        });
    }
    Ok(())
}

fn normalized_extents(bbox: RenderBounds) -> (i64, i64, i64, i64) {
    let x0 = bbox.origin.x as i64;
    let y0 = bbox.origin.y as i64;
    let x1 = x0 + bbox.size.x as i64;
    let y1 = y0 + bbox.size.y as i64;
    (x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1))
}

fn adapter_label(info: &wgpu::AdapterInfo) -> String {
    if info.name.is_empty() {
        format!("{:?}", info.backend)
    } else {
        format!("{} ({:?})", info.name, info.backend)
    }
}

#[derive(Clone, Copy)]
struct ReadbackLayout {
    unpadded_bytes_per_row: u32,
    padded_bytes_per_row: u32,
    buffer_size: u64,
    output_size: usize,
}

impl ReadbackLayout {
    fn new(config: &RendererConfig) -> Result<Self, RenderError> {
        let unpadded_bytes_per_row =
            config
                .width
                .checked_mul(BYTES_PER_PIXEL)
                .ok_or(RenderError::ArithmeticOverflow {
                    context: "readback row size",
                })?;
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row
            .checked_add(alignment - 1)
            .map(|value| value / alignment * alignment)
            .ok_or(RenderError::ArithmeticOverflow {
                context: "padded readback row size",
            })?;
        let buffer_size = u64::from(padded_bytes_per_row)
            .checked_mul(u64::from(config.height))
            .ok_or(RenderError::ArithmeticOverflow {
                context: "readback buffer size",
            })?;
        let output_size = usize::try_from(unpadded_bytes_per_row)
            .ok()
            .and_then(|row| {
                usize::try_from(config.height)
                    .ok()
                    .and_then(|height| row.checked_mul(height))
            })
            .ok_or(RenderError::ArithmeticOverflow {
                context: "returned RGBA size",
            })?;

        if output_size > config.max_output_bytes {
            return Err(RenderError::OutputTooLarge {
                required: output_size,
                limit: config.max_output_bytes,
            });
        }

        Ok(Self {
            unpadded_bytes_per_row,
            padded_bytes_per_row,
            buffer_size,
            output_size,
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn read_buffer_rgba(
    device: &wgpu::Device,
    buffer: &wgpu::Buffer,
    height: u32,
    layout: ReadbackLayout,
) -> Result<Vec<u8>, RenderError> {
    let slice = buffer.slice(..);
    let (sender, receiver) = mpsc::sync_channel(1);
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(RenderError::DevicePoll)?;
    receiver
        .recv()
        .map_err(|_| RenderError::ReadbackCallbackClosed)?
        .map_err(RenderError::BufferMap)?;

    let mapped = slice.get_mapped_range();
    let padded_row = usize::try_from(layout.padded_bytes_per_row).map_err(|_| {
        RenderError::ArithmeticOverflow {
            context: "mapped padded row size",
        }
    })?;
    let unpadded_row = usize::try_from(layout.unpadded_bytes_per_row).map_err(|_| {
        RenderError::ArithmeticOverflow {
            context: "mapped output row size",
        }
    })?;
    let mut rgba = Vec::with_capacity(layout.output_size);

    for row in 0..usize::try_from(height).map_err(|_| RenderError::ArithmeticOverflow {
        context: "readback row count",
    })? {
        let start = row
            .checked_mul(padded_row)
            .ok_or(RenderError::ArithmeticOverflow {
                context: "mapped row offset",
            })?;
        let end = start
            .checked_add(unpadded_row)
            .ok_or(RenderError::ArithmeticOverflow {
                context: "mapped row end",
            })?;
        let bytes = mapped
            .get(start..end)
            .ok_or(RenderError::ArithmeticOverflow {
                context: "mapped row bounds",
            })?;
        rgba.extend_from_slice(bytes);
    }

    drop(mapped);
    buffer.unmap();
    Ok(rgba)
}

#[cfg(target_arch = "wasm32")]
async fn read_buffer_rgba(
    _device: &wgpu::Device,
    _buffer: &wgpu::Buffer,
    _height: u32,
    _layout: ReadbackLayout,
) -> Result<Vec<u8>, RenderError> {
    Err(RenderError::ReadbackUnsupportedOnWasm)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_arc_segments_tighten_with_error_bound() {
        let coarse =
            adaptive_arc_segment_count(100.0, std::f64::consts::TAU, 1.0, 0).unwrap_or_default();
        let fine =
            adaptive_arc_segment_count(100.0, std::f64::consts::TAU, 0.1, 0).unwrap_or_default();
        assert!(fine > coarse);
    }

    #[test]
    fn readback_rows_are_aligned_to_wgpu_requirement() {
        let config = RendererConfig {
            width: 65,
            height: 2,
            padding_px: 0.0,
            ..RendererConfig::default()
        };
        let layout = ReadbackLayout::new(&config).unwrap_or(ReadbackLayout {
            unpadded_bytes_per_row: 0,
            padded_bytes_per_row: 0,
            buffer_size: 0,
            output_size: 0,
        });

        assert_eq!(layout.unpadded_bytes_per_row, 260);
        assert_eq!(
            layout.padded_bytes_per_row % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT,
            0
        );
    }

    #[test]
    fn rendered_image_rows_are_tightly_packed() {
        let image = RenderedImage {
            width: 2,
            height: 2,
            rgba: (0..16).collect(),
        };

        assert_eq!(image.row(0), Some(&[0, 1, 2, 3, 4, 5, 6, 7][..]));
        assert_eq!(image.row(1), Some(&[8, 9, 10, 11, 12, 13, 14, 15][..]));
        assert_eq!(image.row(2), None);
    }

    #[test]
    fn validation_rejects_padding_that_consumes_the_output() {
        let config = RendererConfig {
            width: 32,
            height: 24,
            padding_px: 12.0,
            ..RendererConfig::default()
        };

        assert!(matches!(
            validate_config(&config),
            Err(RenderError::PaddingConsumesOutput { .. })
        ));
    }

    #[test]
    fn arc_radius_quantization_within_five_iu_is_accepted() {
        let arc = ArcGeometry {
            start: RenderPoint::new(23_114, 0),
            end: RenderPoint::new(0, 23_115),
            center: RenderPoint::new(0, 0),
            direction: ArcDirection::CounterClockwise,
            full_circle: false,
        };
        let transform = ViewTransform {
            scale: 0.01,
            offset_x: 0.0,
            offset_y: 0.0,
        };

        assert!(flattened_arc_path(arc, 0, transform, &RendererConfig::default()).is_ok());
    }
}
