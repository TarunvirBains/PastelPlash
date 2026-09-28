//! The GPU stylization stage: de-light → anisotropic Kuwahara → edge-aware bleeding → palette
//! LUT + accents + temperature + brushstrokes + watercolor finish + lightness ceiling.
//!
//! It is one [`Stage`] rather than several so a texture crosses the bus once each way; the passes
//! inside share textures on the GPU. Per-image parameters (tiling, tint-safety, the de-light
//! field) come from a CPU analysis first (`src/analysis.rs`); accent thresholds come from a
//! histogram built on the GPU.
//!
//! Pass graph (T0..T4 are rgba32float textures of the image or chunk):
//!
//! ```text
//! T0 upload ─delight→ T1 ─tensor→ T2 ─blur_h→ T3 ─blur_v→ T2
//! (T1, T2) ─kuwahara→ T3 ─bleed→ T4 ─finish (T4, T1, T2)→ T0 → readback
//! ```
//!
//! Images larger than the device's texture limit (or `PASTELPLASH_MAX_CHUNK` texels per side) are
//! processed in overlapping chunks; per-image quantities and noise use full-image coordinates, so
//! chunk seams are invisible.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::analysis;
use crate::config::{Category, Config, Style, Treatment};
use crate::gpu::Gpu;
use crate::image::Image;
use crate::lut::Lut3d;
use crate::palette::Mapping;
use crate::pipeline::{FileContext, Stage};

const SHADER: &str = include_str!("shaders/stylize.wgsl");
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
/// Texels per GPU job before chunking (4 GiB of rgba32f across the five textures).
const PIXEL_BUDGET: u64 = 64 * 1024 * 1024;
/// Kuwahara sample-iterations per submitted band, to keep single submissions short.
const KUWAHARA_BAND_BUDGET: f64 = 1.5e9;
/// Concurrent GPU jobs (bounds VRAM; CPU decode/encode still runs on every worker).
const GPU_SLOTS: usize = 2;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, Pod, Zeroable)]
struct Params {
    size_x: i32,
    size_y: i32,
    origin_x: i32,
    origin_y: i32,
    full_x: i32,
    full_y: i32,
    wrap_x: i32,
    wrap_y: i32,
    tile_x: i32,
    tile_y: i32,
    low_w: i32,
    low_h: i32,
    lut_size: i32,
    tint_safe: i32,
    seed: u32,
    _pad0: i32,

    delight_strength: f32,
    delight_min: f32,
    delight_max: f32,
    delight_mean: f32,
    kuw_radius: f32,
    kuw_q: f32,
    kuw_hardness: f32,
    kuw_alpha: f32,
    kuw_zero_cross: f32,
    kuw_strength: f32,
    tensor_sigma: f32,
    edge_dark: f32,
    edge_step: f32,
    bleed: f32,
    bleed_radius: f32,
    bleed_range: f32,
    gran: f32,
    gran_cells_x: f32,
    gran_cells_y: f32,
    gran_valley: f32,
    gran_radius: f32,
    paper: f32,
    paper_cells_x: f32,
    paper_cells_y: f32,
    paper_hl: f32,
    paper_r: f32,
    paper_g: f32,
    paper_b: f32,
    floor_margin: f32,
    ceiling: f32,
    accent_fraction: f32,
    accent_softness: f32,
    accent_radius: f32,
    accent_min_l: f32,
    accent_hue: f32,
    accent_chroma: f32,
    accent_depth: f32,
    temp_strength: f32,
    temp_warm_hue: f32,
    temp_cool_hue: f32,
    temp_sens: f32,
    stroke_strength: f32,
    stroke_chroma: f32,
    stroke_len: f32,
    stroke_step: f32,
    stroke_cells_x: f32,
    stroke_cells_y: f32,
    bloom_cells_x: f32,
    bloom_cells_y: f32,
    ceiling_ref: f32,
    accent_min_depth: f32,
    edge_rel: f32,
    edge_threshold: f32,
    edge_feather: f32,
    _pad2: f32,
    _pad3: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct BandUniform {
    y0: i32,
    y1: i32,
    _pad: [i32; 2],
}

struct Passes {
    delight: wgpu::ComputePipeline,
    tensor: wgpu::ComputePipeline,
    blur_h: wgpu::ComputePipeline,
    blur_v: wgpu::ComputePipeline,
    kuwahara: wgpu::ComputePipeline,
    bleed: wgpu::ComputePipeline,
    accent_hist: wgpu::ComputePipeline,
    accent_threshold: wgpu::ComputePipeline,
    finish: wgpu::ComputePipeline,
}

/// A counting semaphore.
struct Slots {
    free: Mutex<usize>,
    cv: Condvar,
}

impl Slots {
    fn acquire(&self) -> SlotGuard<'_> {
        let mut free = self.free.lock().unwrap();
        while *free == 0 {
            free = self.cv.wait(free).unwrap();
        }
        *free -= 1;
        SlotGuard(self)
    }
}

struct SlotGuard<'a>(&'a Slots);

impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        *self.0.free.lock().unwrap() += 1;
        self.0.cv.notify_one();
    }
}

pub struct Stylize {
    gpu: Gpu,
    layout: wgpu::BindGroupLayout,
    passes: Passes,
    /// Loaded `.cube` from the style (replaces the generated palette).
    external_lut: Option<Arc<wgpu::Buffer>>,
    external_lut_size: i32,
    /// Generated palette LUTs by (lift scale, shadow scale) bits.
    luts: Mutex<HashMap<(u32, u32), Arc<wgpu::Buffer>>>,
    slots: Slots,
    max_side: u32,
}

/// True if the config asks for any stylization at all (otherwise no GPU is opened).
pub fn wanted(config: &Config) -> bool {
    !is_neutral(&config.style)
        || config
            .target
            .categories
            .values()
            .any(|t| t.lightness_ceiling.is_some())
}

fn is_neutral(s: &Style) -> bool {
    let w = &s.watercolor;
    s.kuwahara.radius <= 0.0
        && s.delight.strength <= 0.0
        && s.lut.is_none()
        && !s.palette.enabled
        && s.temperature.chroma <= 0.0
        && s.strokes.strength <= 0.0
        && w.edge_darkening <= 0.0
        && w.bleed <= 0.0
        && w.granulation <= 0.0
        && w.paper_grain <= 0.0
}

fn entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty,
        count: None,
    }
}

fn sampled() -> wgpu::BindingType {
    wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable: false },
        view_dimension: wgpu::TextureViewDimension::D2,
        multisampled: false,
    }
}

fn uniform() -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Uniform,
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

fn storage_ro() -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only: true },
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

impl Stylize {
    pub fn new(config: &Config) -> Result<Self> {
        let max_chunk = std::env::var("PASTELPLASH_MAX_CHUNK")
            .ok()
            .and_then(|v| v.parse::<u32>().ok());
        Self::with_max_chunk(config, max_chunk)
    }

    /// Like [`Stylize::new`], forcing chunked processing for images with a side above
    /// `max_chunk` texels (at least 64; clamped to the device limit).
    pub fn with_max_chunk(config: &Config, max_chunk: Option<u32>) -> Result<Self> {
        let gpu = pollster::block_on(Gpu::new())?;
        let device = &gpu.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("stylize"),
            entries: &[
                entry(0, uniform()),
                entry(1, sampled()),
                entry(2, sampled()),
                entry(
                    3,
                    wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: FORMAT,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                ),
                entry(4, storage_ro()),
                entry(5, storage_ro()),
                entry(6, uniform()),
                entry(7, sampled()),
                entry(
                    8,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stylize"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stylize"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let make = |name: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(name),
                layout: Some(&pl),
                module: &module,
                entry_point: Some(name),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let passes = Passes {
            delight: make("delight"),
            tensor: make("tensor"),
            blur_h: make("blur_h"),
            blur_v: make("blur_v"),
            kuwahara: make("kuwahara"),
            bleed: make("bleed"),
            accent_hist: make("accent_hist"),
            accent_threshold: make("accent_threshold"),
            finish: make("finish"),
        };

        let (external_lut, external_lut_size) = match &config.style.lut {
            Some(path) => {
                let lut = Lut3d::load(path)?;
                (Some(Arc::new(lut_buffer(&gpu, &lut))), lut.size as i32)
            }
            None => (None, 0),
        };

        let limit = gpu.device.limits().max_texture_dimension_2d;
        let max_side = max_chunk.map_or(limit, |v| v.clamp(64, limit));

        Ok(Self {
            gpu,
            layout,
            passes,
            external_lut,
            external_lut_size,
            luts: Mutex::new(HashMap::new()),
            slots: Slots {
                free: Mutex::new(GPU_SLOTS),
                cv: Condvar::new(),
            },
            max_side,
        })
    }

    /// The palette LUT buffer and its size for a category's treatment, if the style has a
    /// palette.
    fn lut(&self, style: &Style, tr: &Treatment) -> Option<(Arc<wgpu::Buffer>, i32)> {
        if let Some(buf) = &self.external_lut {
            return Some((buf.clone(), self.external_lut_size));
        }
        if !style.palette.enabled {
            return None;
        }
        let mut luts = self.luts.lock().unwrap();
        let buf = luts
            .entry((tr.floor_scale.to_bits(), tr.shadow_tint.to_bits()))
            .or_insert_with(|| {
                let lut = Mapping {
                    palette: &style.palette,
                    lift_scale: tr.floor_scale,
                    shadow_scale: tr.shadow_tint,
                }
                .bake();
                Arc::new(lut_buffer(&self.gpu, &lut))
            })
            .clone();
        Some((buf, style.palette.lut_size.clamp(2, 129) as i32))
    }
}

fn lut_buffer(gpu: &Gpu, lut: &Lut3d) -> wgpu::Buffer {
    gpu.device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("lut"),
            contents: bytemuck::cast_slice(&lut.data),
            usage: wgpu::BufferUsages::STORAGE,
        })
}

fn cells(full: u32, size_px: f32) -> f32 {
    (full as f32 / size_px.max(0.5)).round().max(1.0)
}

/// Everything per image that the GPU job needs besides the pixels.
struct Job {
    params: Params,
    lowres: Vec<f32>,
    lut: Option<(Arc<wgpu::Buffer>, i32)>,
    halo: u32,
}

impl Stage for Stylize {
    fn name(&self) -> &str {
        "stylize"
    }

    fn apply(&self, image: &mut Image, ctx: &FileContext) -> Result<()> {
        if matches!(ctx.category, Category::Ui | Category::Skip) {
            return Ok(());
        }
        let t_start = Instant::now();
        let style = &ctx.config.style;
        let tr = ctx.config.target.treatment(ctx.category);
        let (w, h) = (image.width, image.height);
        if w == 0 || h == 0 {
            return Ok(());
        }
        let gm = ((w as f64) * (h as f64)).sqrt() as f32;

        // Size factor: reference texels → texels of this image.
        let source_scale = image
            .source_scale
            .or(ctx.config.pack.source_scale)
            .or(style.scale.source_scale);
        let f = match source_scale {
            Some(s) => s / style.scale.reference_source_scale.max(1e-3),
            None => (gm / style.scale.reference_size.max(1.0)).powf(style.scale.exponent),
        };

        // Analysis.
        let ratios = [
            analysis::seam_ratio(image, false),
            analysis::seam_ratio(image, true),
        ];
        // Pre-rendered backgrounds are whole pictures: never wrap, whatever their edges say.
        let wrap = if ctx.category == Category::Background {
            [false; 2]
        } else {
            ratios.map(|r| r <= style.tiling.threshold)
        };
        let delight_strength = style.delight.strength * tr.delight;
        let temp_strength = style.temperature.chroma * tr.warm_cool;
        let lowres = (delight_strength > 0.0 || temp_strength > 0.0)
            .then(|| analysis::lowres_luminance(image, style.delight.radius * gm, wrap));
        let lut = self.lut(style, &tr);
        let pal = &style.palette;
        let accent_fraction = if lut.is_some() {
            pal.accent_fraction * tr.accent
        } else {
            0.0
        };
        let accent_radius = (pal.accent_radius * f).max(1.0);
        let chroma_p99 = analysis::chroma_p99(image);
        let tint_safe = tr
            .tint_safe
            .or(image.tint_safe)
            .unwrap_or(chroma_p99 < pal.tint_safe_chroma);
        let t_analysis = t_start.elapsed();

        // Parameters.
        let k = &style.kuwahara;
        let radius = if k.radius > 0.0 && k.strength > 0.0 {
            (k.radius * f * tr.radius_scale).clamp(k.min_radius, k.max_radius)
        } else {
            0.0
        };
        let tensor_sigma = (k.tensor_sigma * f).clamp(0.5, 16.0);
        let wc = &style.watercolor;
        let st = &style.strokes;
        let stroke_width = (st.width * f * tr.stroke_scale).max(0.75);
        let stroke_len = (st.length * f * tr.stroke_scale).max(1.0);
        let edge_step = (wc.edge_width * f).max(1.0);
        let bleed_radius = (wc.bleed_radius * f).clamp(1.0, 48.0);
        let gran_px = (wc.granulation_scale * f).max(0.75);
        let paper_px = (wc.paper_scale * f).max(0.75);
        let ceiling = tr.lightness_ceiling.unwrap_or(1.0).min(1.0);
        let temp = &style.temperature;
        let params = Params {
            full_x: w as i32,
            full_y: h as i32,
            tile_x: wrap[0] as i32,
            tile_y: wrap[1] as i32,
            low_w: lowres.as_ref().map_or(0, |l| l.width as i32),
            low_h: lowres.as_ref().map_or(0, |l| l.height as i32),
            lut_size: lut.as_ref().map_or(0, |l| l.1),
            tint_safe: tint_safe as i32,
            seed: wc.seed,
            delight_strength,
            delight_min: style.delight.min_gain,
            delight_max: style.delight.max_gain,
            delight_mean: lowres.as_ref().map_or(0.0, |l| l.mean),
            kuw_radius: radius,
            kuw_q: k.sharpness,
            kuw_hardness: k.hardness,
            kuw_alpha: k.anisotropy.max(1e-3),
            kuw_zero_cross: k.zero_crossing,
            kuw_strength: k.strength,
            tensor_sigma,
            edge_dark: wc.edge_darkening,
            edge_step,
            bleed: wc.bleed,
            bleed_radius,
            bleed_range: wc.bleed_range,
            gran: wc.granulation,
            gran_cells_x: cells(w, gran_px),
            gran_cells_y: cells(h, gran_px),
            gran_valley: wc.granulation_valley.clamp(0.0, 1.0),
            gran_radius: (gran_px * 0.75).max(1.0),
            paper: wc.paper_grain,
            paper_cells_x: cells(w, paper_px),
            paper_cells_y: cells(h, paper_px),
            paper_hl: wc.paper_highlight,
            paper_r: wc.paper_color[0],
            paper_g: wc.paper_color[1],
            paper_b: wc.paper_color[2],
            floor_margin: wc.floor_margin,
            ceiling,
            // Relit textures are scaled down so the palette's top lands on the ceiling
            // (keeps value structure instead of flattening highlights).
            ceiling_ref: if lut.is_some() && style.lut.is_none() {
                style.palette.l_ceiling
            } else {
                1.0
            },
            accent_fraction,
            accent_softness: pal.accent_softness,
            accent_min_depth: pal.accent_min_depth,
            edge_rel: wc.edge_relative,
            edge_threshold: wc.edge_threshold,
            edge_feather: wc.edge_feather,
            accent_radius,
            accent_min_l: pal.accent_min_l,
            accent_hue: pal.accent_hue.to_radians(),
            accent_chroma: pal.accent_chroma,
            accent_depth: tr.accent.clamp(0.0, 1.0),
            temp_strength,
            temp_warm_hue: temp.warm_hue.to_radians(),
            temp_cool_hue: temp.cool_hue.to_radians(),
            temp_sens: temp.sensitivity,
            stroke_strength: st.strength * tr.strokes,
            stroke_chroma: st.chroma,
            stroke_len,
            stroke_step: (stroke_len / 16.0).max(1.0),
            stroke_cells_x: cells(w, stroke_width),
            stroke_cells_y: cells(h, stroke_width),
            bloom_cells_x: cells(w, bleed_radius * 6.0),
            bloom_cells_y: cells(h, bleed_radius * 6.0),
            ..Params::default()
        };
        let reach = [
            edge_step,
            bleed_radius,
            accent_radius,
            (gran_px * 0.75).max(1.0),
        ]
        .into_iter()
        .fold(0.0f32, f32::max);
        let halo = (2.0 * radius + 3.0 * tensor_sigma + reach + stroke_len + 6.0).ceil() as u32;
        let job = Job {
            params,
            lowres: lowres.map_or_else(|| vec![0.0], |l| l.data),
            lut,
            halo,
        };

        let t_gpu = Instant::now();
        let (out, chunks) = self.run(image, &job, wrap)?;
        let t_gpu = t_gpu.elapsed();
        for (dst, src) in image.pixels.iter_mut().zip(out) {
            for c in 0..3 {
                let v = src[c];
                dst[c] = if v.is_finite() {
                    v.clamp(0.0, 1.0)
                } else {
                    dst[c]
                };
            }
        }
        println!(
            "  {}: {w}x{h} {:?} wrap={}{} seam={:.1}/{:.1} tint_safe={} (C99 {:.3}) \
             scale={f:.2} r={radius:.1}{} | analysis {} gpu {}",
            ctx.rel.display(),
            ctx.category,
            if wrap[0] { "u" } else { "-" },
            if wrap[1] { "v" } else { "-" },
            ratios[0].min(99.0),
            ratios[1].min(99.0),
            tint_safe,
            chroma_p99,
            if chunks > 1 {
                format!(" chunks={chunks}")
            } else {
                String::new()
            },
            ms(t_analysis),
            ms(t_gpu),
        );
        Ok(())
    }
}

pub fn ms(d: Duration) -> String {
    format!("{:.0}ms", d.as_secs_f64() * 1000.0)
}

impl Stylize {
    /// Runs the job over the whole image, chunked if needed. Returns output pixels and the
    /// number of chunks.
    fn run(&self, image: &Image, job: &Job, wrap: [bool; 2]) -> Result<(Vec<[f32; 4]>, usize)> {
        let (w, h) = (image.width, image.height);
        let side = self.max_side;
        if w <= side && h <= side && (w as u64) * (h as u64) <= PIXEL_BUDGET {
            let mut params = job.params;
            params.size_x = w as i32;
            params.size_y = h as i32;
            params.wrap_x = wrap[0] as i32;
            params.wrap_y = wrap[1] as i32;
            return Ok((self.run_gpu(&image.pixels, w, h, &params, job)?, 1));
        }

        let budget_side = (PIXEL_BUDGET as f64).sqrt() as u32;
        let chunk = side.min(budget_side);
        if chunk <= 2 * job.halo + 16 {
            bail!("filter reach {} too large for chunk size {chunk}", job.halo);
        }
        let core = chunk - 2 * job.halo;
        let mut out = vec![[0.0f32; 4]; image.pixels.len()];
        let mut count = 0;
        for cy in (0..h).step_by(core as usize) {
            for cx in (0..w).step_by(core as usize) {
                let (cw, ch) = ((w - cx).min(core), (h - cy).min(core));
                let (ox, oy) = (cx as i64 - job.halo as i64, cy as i64 - job.halo as i64);
                let (rw, rh) = (cw + 2 * job.halo, ch + 2 * job.halo);
                let fetch = |v: i64, n: u32, wrap: bool| -> usize {
                    if wrap {
                        v.rem_euclid(n as i64) as usize
                    } else {
                        v.clamp(0, n as i64 - 1) as usize
                    }
                };
                let mut region = Vec::with_capacity((rw * rh) as usize);
                for y in 0..rh as i64 {
                    let sy = fetch(oy + y, h, wrap[1]);
                    for x in 0..rw as i64 {
                        let sx = fetch(ox + x, w, wrap[0]);
                        region.push(image.pixels[sy * w as usize + sx]);
                    }
                }
                let mut params = job.params;
                params.size_x = rw as i32;
                params.size_y = rh as i32;
                params.origin_x = ox as i32;
                params.origin_y = oy as i32;
                let res = self.run_gpu(&region, rw, rh, &params, job)?;
                for y in 0..ch {
                    let src = ((y + job.halo) * rw + job.halo) as usize;
                    let dst = ((cy + y) * w + cx) as usize;
                    out[dst..dst + cw as usize].copy_from_slice(&res[src..src + cw as usize]);
                }
                count += 1;
            }
        }
        Ok((out, count))
    }

    fn run_gpu(
        &self,
        pixels: &[[f32; 4]],
        w: u32,
        h: u32,
        params: &Params,
        job: &Job,
    ) -> Result<Vec<[f32; 4]>> {
        let _slot = self.slots.acquire();
        let device = &self.gpu.device;
        let queue = &self.gpu.queue;
        let extent = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        let tex: Vec<wgpu::Texture> = (0..5)
            .map(|i| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(["t0", "t1", "t2", "t3", "t4"][i]),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: FORMAT,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::STORAGE_BINDING
                        | wgpu::TextureUsages::COPY_DST
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                })
            })
            .collect();
        let views: Vec<wgpu::TextureView> = tex
            .iter()
            .map(|t| t.create_view(&Default::default()))
            .collect();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex[0],
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(pixels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 16),
                rows_per_image: Some(h),
            },
            extent,
        );

        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("params"),
            contents: bytemuck::bytes_of(params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let lowres_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("lowres"),
            contents: bytemuck::cast_slice(&job.lowres),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let dummy_lut;
        let lut_buf: &wgpu::Buffer = match &job.lut {
            Some((b, _)) => b,
            None => {
                dummy_lut = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("no-lut"),
                    contents: bytemuck::cast_slice(&[[0.0f32; 4]]),
                    usage: wgpu::BufferUsages::STORAGE,
                });
                &dummy_lut
            }
        };

        let hist_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("accent-hist"),
            size: 260 * 4,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });

        // Dispatches `pipeline` over rows y0..y1 (an empty range dispatches one workgroup).
        let dispatch = |encoder: &mut wgpu::CommandEncoder,
                        pipeline: &wgpu::ComputePipeline,
                        a: usize,
                        b: usize,
                        c: usize,
                        out: usize,
                        y0: u32,
                        y1: u32| {
            let band = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("band"),
                contents: bytemuck::bytes_of(&BandUniform {
                    y0: y0 as i32,
                    y1: y1 as i32,
                    _pad: [0; 2],
                }),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: params_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&views[a]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&views[b]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&views[out]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: lut_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: lowres_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: band.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: wgpu::BindingResource::TextureView(&views[c]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: hist_buf.as_entire_binding(),
                    },
                ],
            });
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            if y1 > y0 {
                pass.dispatch_workgroups(w.div_ceil(8), (y1 - y0).div_ceil(8), 1);
            } else {
                pass.dispatch_workgroups(1, 1, 1);
            }
        };

        let p = &self.passes;
        let mut buffers = Vec::new();
        let mut enc = device.create_command_encoder(&Default::default());
        dispatch(&mut enc, &p.delight, 0, 0, 0, 1, 0, h);
        let kuwahara_on = params.kuw_radius >= 0.5;
        let tensor_on = kuwahara_on || params.stroke_strength > 0.0;
        if tensor_on {
            dispatch(&mut enc, &p.tensor, 1, 1, 1, 2, 0, h);
            dispatch(&mut enc, &p.blur_h, 2, 2, 2, 3, 0, h);
            dispatch(&mut enc, &p.blur_v, 3, 3, 3, 2, 0, h);
        }
        buffers.push(enc.finish());

        // Kuwahara in row bands, one command buffer each.
        let r = params.kuw_radius as f64;
        let area = (3.0 * r + 1.0).powi(2).max(1.0);
        let rows = ((KUWAHARA_BAND_BUDGET / (w as f64 * area)) as u32).clamp(8, h.max(8));
        let rows = rows.div_ceil(8) * 8;
        let mut y = 0;
        while y < h {
            let y1 = (y + rows).min(h);
            let mut enc = device.create_command_encoder(&Default::default());
            // With the tensor skipped, texB/texC only need to be valid bindings.
            dispatch(
                &mut enc,
                &p.kuwahara,
                1,
                if tensor_on { 2 } else { 1 },
                1,
                3,
                y,
                y1,
            );
            buffers.push(enc.finish());
            y = y1;
        }

        let mut enc = device.create_command_encoder(&Default::default());
        dispatch(&mut enc, &p.bleed, 3, 3, 3, 4, 0, h);
        if params.accent_fraction > 0.0 && params.accent_depth > 0.0 {
            // Histogram of the painted image's accent measure, then its thresholds. (T3 is
            // only a placeholder output here.)
            dispatch(&mut enc, &p.accent_hist, 4, 4, 4, 3, 0, h);
            dispatch(&mut enc, &p.accent_threshold, 4, 4, 4, 3, 0, 0);
        }
        dispatch(
            &mut enc,
            &p.finish,
            4,
            1,
            if tensor_on { 2 } else { 1 },
            0,
            0,
            h,
        );

        let row_bytes = w * 16;
        let padded = row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: padded as u64 * h as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &tex[0],
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(h),
                },
            },
            extent,
        );
        buffers.push(enc.finish());
        queue.submit(buffers);

        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .context("device poll failed")?;
        rx.recv()
            .context("map callback dropped")?
            .context("mapping readback failed")?;
        let mapped = slice
            .get_mapped_range()
            .context("mapped range unavailable")?;
        let mut out = Vec::with_capacity((w * h) as usize);
        for row in 0..h as usize {
            let start = row * padded as usize;
            let bytes = &mapped[start..start + row_bytes as usize];
            out.extend_from_slice(bytemuck::cast_slice::<u8, [f32; 4]>(bytes));
        }
        Ok(out)
    }
}
