//! Running a stylize job on the GPU: the passes, the bind group layout, chunking for images
//! larger than the device allows, and the concurrency slots that bound VRAM.

use std::sync::{Arc, Condvar, Mutex};

use anyhow::{Context, Result, bail};
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use super::SHADER;
use super::params::Params;
use crate::gpu::Gpu;
use crate::image::Image;
use crate::lut::Lut3d;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
/// Texels per GPU job before chunking (4 GiB of rgba32f across the five textures).
const PIXEL_BUDGET: u64 = 64 * 1024 * 1024;
/// Kuwahara sample-iterations per submitted band, to keep single submissions short.
const KUWAHARA_BAND_BUDGET: f64 = 1.5e9;
/// Concurrent GPU jobs (bounds VRAM; CPU decode/encode still runs on every worker).
const GPU_SLOTS: usize = 2;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct BandUniform {
    y0: i32,
    y1: i32,
    _pad: [i32; 2],
}

struct Passes {
    delight: wgpu::ComputePipeline,
    group: wgpu::ComputePipeline,
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

/// Everything per image that the GPU job needs besides the pixels.
pub(super) struct Job {
    pub params: Params,
    pub lowres: Vec<f32>,
    pub lut: Option<Arc<wgpu::Buffer>>,
    pub halo: u32,
}

/// The GPU side of the stage: the device, the compiled passes, and running a job over an image
/// (in chunks when it is larger than the device allows).
pub(super) struct Runner {
    gpu: Gpu,
    layout: wgpu::BindGroupLayout,
    passes: Passes,
    slots: Slots,
    max_side: u32,
}

impl Runner {
    /// Opens the GPU and compiles the passes; images with a side above `max_chunk` texels (at
    /// least 64; clamped to the device limit) are processed in chunks.
    pub fn new(max_chunk: Option<u32>) -> Result<Self> {
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
                entry(9, sampled()),
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
            group: make("group"),
            tensor: make("tensor"),
            blur_h: make("blur_h"),
            blur_v: make("blur_v"),
            kuwahara: make("kuwahara"),
            bleed: make("bleed"),
            accent_hist: make("accent_hist"),
            accent_threshold: make("accent_threshold"),
            finish: make("finish"),
        };
        let limit = gpu.device.limits().max_texture_dimension_2d;
        let max_side = max_chunk.map_or(limit, |v| v.clamp(64, limit));
        Ok(Self {
            gpu,
            layout,
            passes,
            slots: Slots {
                free: Mutex::new(GPU_SLOTS),
                cv: Condvar::new(),
            },
            max_side,
        })
    }

    /// Uploads a LUT as a storage buffer.
    pub fn upload_lut(&self, lut: &Lut3d) -> wgpu::Buffer {
        self.gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("lut"),
                contents: bytemuck::cast_slice(&lut.data),
                usage: wgpu::BufferUsages::STORAGE,
            })
    }
    /// Runs the job over the whole image, chunked if needed. Returns output pixels and the
    /// number of chunks.
    pub fn run(&self, image: &Image, job: &Job, wrap: [bool; 2]) -> Result<(Vec<[f32; 4]>, usize)> {
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
            Some(b) => b,
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

        // Pass variant for the next dispatch (band._a in the shader; 1 = coarse Kuwahara).
        let variant = std::cell::Cell::new(0i32);
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
                    _pad: [variant.get(), 0],
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
                    // The upload itself (T0 is never written): the original texels.
                    wgpu::BindGroupEntry {
                        binding: 9,
                        resource: wgpu::BindingResource::TextureView(&views[0]),
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
        if params.grp > 0.0 && params.grp_count >= 2.0 {
            // Soft value grouping (T1 → T3), which then stands in for the de-lit source.
            dispatch(&mut enc, &p.group, 1, 1, 1, 3, 0, h);
            enc.copy_texture_to_texture(tex[3].as_image_copy(), tex[1].as_image_copy(), extent);
        }
        let kuwahara_on = params.kuw_radius >= 0.5;
        let tensor_on = kuwahara_on || params.stroke_strength > 0.0 || params.smear > 0.0;
        if tensor_on {
            dispatch(&mut enc, &p.tensor, 1, 1, 1, 2, 0, h);
            dispatch(&mut enc, &p.blur_h, 2, 2, 2, 3, 0, h);
            dispatch(&mut enc, &p.blur_v, 3, 3, 3, 2, 0, h);
        }
        buffers.push(enc.finish());

        // Kuwahara in row bands, one command buffer each. Busy textures first get a large-scale
        // abstraction pass (T1 → T4), which the regular pass then paints (T4 → T3).
        let coarse = params.busy > 0.0 && params.kuw_radius_coarse >= 0.5 && tensor_on;
        let passes: &[(i32, usize, f32)] = if coarse {
            &[(1, 1, params.kuw_radius_coarse), (0, 4, params.kuw_radius)]
        } else {
            &[(0, 1, params.kuw_radius)]
        };
        for &(var, input, r) in passes {
            let out = if var == 1 { 4 } else { 3 };
            // Samples per texel (the shader strides the disc from radius 64: see kuwahara.wgsl).
            let stride = (r / 32.0).floor().max(1.0) as f64;
            let area = (3.0 * r as f64 / stride + 1.0).powi(2).max(1.0);
            let rows = ((KUWAHARA_BAND_BUDGET / (w as f64 * area)) as u32).clamp(8, h.max(8));
            let rows = rows.div_ceil(8) * 8;
            let mut y = 0;
            while y < h {
                let y1 = (y + rows).min(h);
                let mut enc = device.create_command_encoder(&Default::default());
                variant.set(var);
                // With the tensor skipped, texB/texC only need to be valid bindings.
                dispatch(
                    &mut enc,
                    &p.kuwahara,
                    input,
                    if tensor_on { 2 } else { 1 },
                    1,
                    out,
                    y,
                    y1,
                );
                variant.set(0);
                buffers.push(enc.finish());
                y = y1;
            }
        }

        let mut enc = device.create_command_encoder(&Default::default());
        dispatch(&mut enc, &p.bleed, 3, 3, 3, 4, 0, h);
        if params.accent_fraction > 0.0 && params.accent_depth > 0.0 {
            // Histogram of the painted image's accent measure, then its thresholds. (T3 is
            // only a placeholder output here.)
            dispatch(&mut enc, &p.accent_hist, 4, 4, 4, 3, 0, h);
            dispatch(&mut enc, &p.accent_threshold, 4, 4, 4, 3, 0, 0);
        }
        // The finish writes T3 (T0 stays the original for texD).
        dispatch(
            &mut enc,
            &p.finish,
            4,
            1,
            if tensor_on { 2 } else { 1 },
            3,
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
                texture: &tex[3],
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
        let submission = queue.submit(buffers);

        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        // Wait for this job only: another slot's job, submitted later, keeps running meanwhile.
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
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
