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

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;

use crate::config::{Config, Style};
use crate::image::Image;
use crate::pipeline::{FileContext, Stage};
use crate::util::ms;

mod cache;
mod facts;
mod params;
mod plan;
mod runner;

use cache::Cache;
pub use plan::{LutSpec, Plan, Planner};
use runner::{Job, Runner};

const SHADER: &str = include_str!("../shaders/stylize.wgsl");

pub struct Stylize {
    runner: Runner,
    planner: Planner,
    cache: Cache,
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
        && s.strokes.smear <= 0.0
        && s.value_contrast.fine <= 0.0
        && s.value_contrast.mid <= 0.0
        && s.value_contrast.coarse <= 0.0
        && w.edge_darkening <= 0.0
        && w.bleed <= 0.0
        && w.granulation <= 0.0
        && w.paper_grain <= 0.0
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
        let runner = Runner::new(max_chunk)?;
        let planner = Planner::new(config)?;
        let external_lut = planner
            .external_lut
            .as_ref()
            .map(|lut| Arc::new(runner.upload_lut(lut)));
        Ok(Self {
            runner,
            planner,
            cache: Cache::new(external_lut),
        })
    }
}

impl Stage for Stylize {
    fn name(&self) -> &str {
        "stylize"
    }

    fn apply(&self, image: &mut Image, ctx: &FileContext) -> Result<()> {
        if !ctx.category.is_stylized() {
            return Ok(());
        }
        let style = self.cache.style_for(&ctx.config.style, &ctx.mood)?;
        let Some(plan) = self.planner.plan(image, ctx, &style) else {
            return Ok(());
        };
        let tr = ctx.config.target.treatment(ctx.category);
        let (w, h) = (image.width, image.height);
        let wrap = plan.wrap;
        let job = Job {
            params: plan.params,
            lowres: plan.lowres,
            lut: plan
                .lut
                .as_ref()
                .map(|spec| self.cache.lut(spec, &self.runner)),
            halo: plan.halo,
        };

        let t_gpu = Instant::now();
        let (mut out, chunks) = self.runner.run(image, &job, wrap)?;
        let t_gpu = t_gpu.elapsed();
        // Exposure: restore the source's mean lightness with a monotone tone curve (the murk lift
        // stays; mids and lights come down).
        let mut exp_note = String::new();
        if tr.exposure.preserve_mean {
            for p in out.iter_mut() {
                for c in p.iter_mut().take(3) {
                    if !c.is_finite() {
                        *c = 0.0;
                    }
                }
                p[3] = p[3].clamp(0.0, 1.0);
            }
            let target = crate::exposure::mean_l(&image.pixels);
            let before = crate::exposure::mean_l(&out);
            let curve = crate::exposure::preserve_mean(&mut out, target, tr.exposure.protect);
            exp_note = format!(
                " exposure L {target:.3}: {before:.3}->{:.3} (k {:.3})",
                crate::exposure::mean_l(&out),
                curve.k
            );
        }
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
        let n = &plan.note;
        println!(
            "  {}: {w}x{h} {:?} mood={} wrap={}{} seam={:.1}/{:.1} tint_safe={} (C99 {:.3}) \
             scale={:.2} r={:.1} spread={:.4} busy={:.2} speckle={:.2} marks={:.2}{}{exp_note}{} | analysis {} gpu {}",
            ctx.rel.display(),
            ctx.category,
            ctx.mood,
            if wrap[0] { "u" } else { "-" },
            if wrap[1] { "v" } else { "-" },
            n.ratios[0].min(99.0),
            n.ratios[1].min(99.0),
            n.tint_safe,
            n.chroma_p99,
            n.scale,
            n.radius,
            n.spread,
            n.busy,
            n.speckle,
            n.marks_scale,
            n.grouping,
            if chunks > 1 {
                format!(" chunks={chunks}")
            } else {
                String::new()
            },
            ms(n.analysis),
            ms(t_gpu),
        );
        Ok(())
    }
}
