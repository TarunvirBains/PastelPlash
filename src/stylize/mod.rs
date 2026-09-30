//! The GPU stylization stage: de-light → anisotropic Kuwahara → edge-aware bleeding → palette
//! LUT + accents + temperature + brushstrokes + watercolor finish + lightness ceiling.
//!
//! It is one [`Stage`] rather than several so a texture crosses the bus once each way; the passes
//! inside share textures on the GPU. Per file: the [`Planner`] analyzes the image on the CPU
//! (`facts.rs`) and plans every stage (`plan/`), the runner executes the passes (`runner.rs`,
//! chunked above the device limit), then exposure is restored and the result written back.
//! Accent thresholds come from a histogram built on the GPU. The pass graph is in
//! ARCHITECTURE.md.

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

/// The WGSL of every pass: shared declarations, then one file per stage, concatenated in this
/// order (the order fixes the generated code, so keep it).
pub const SHADER: &str = concat!(
    include_str!("../shaders/common/bindings.wgsl"),
    "\n",
    include_str!("../shaders/common/addr.wgsl"),
    "\n",
    include_str!("../shaders/common/color.wgsl"),
    "\n",
    include_str!("../shaders/common/noise.wgsl"),
    "\n",
    include_str!("../shaders/common/lowres.wgsl"),
    "\n",
    include_str!("../shaders/stages/delight.wgsl"),
    "\n",
    include_str!("../shaders/stages/group.wgsl"),
    "\n",
    include_str!("../shaders/stages/tensor.wgsl"),
    "\n",
    include_str!("../shaders/stages/kuwahara.wgsl"),
    "\n",
    include_str!("../shaders/stages/bleed.wgsl"),
    "\n",
    include_str!("../shaders/stages/palette.wgsl"),
    "\n",
    include_str!("../shaders/stages/strokes.wgsl"),
    "\n",
    include_str!("../shaders/stages/accent.wgsl"),
    "\n",
    include_str!("../shaders/stages/finish.wgsl"),
);

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
        // Emissive materials keep their own light: no mood reaches them.
        let base = crate::config::Mood::default();
        let mood = if ctx.category.is_emissive() {
            &base
        } else {
            &ctx.mood
        };
        let style = self.cache.style_for(&ctx.config.style, mood)?;
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
        let (mut out, chunks, t_wait) = self.runner.run(image, &job, wrap)?;
        let t_gpu = t_gpu.elapsed();
        let t_post = Instant::now();
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
            // A mood's moonlight cast dims the room on purpose: restore the source's exposure as
            // the cast would map it (the same scaling above the palette floor).
            let target = crate::palette::cast_exposure(
                &style.palette,
                tr.cast,
                crate::exposure::mean_l(&image.pixels),
            );
            let before = crate::exposure::mean_l(&out);
            let curve = crate::exposure::preserve_mean(&mut out, target, tr.exposure.protect);
            exp_note = format!(
                " exposure L {target:.3}: {before:.3}->{:.3} (k {:.3})",
                crate::exposure::mean_l(&out),
                curve.k
            );
        }
        // Water: the body leans toward the reference lightness (as the mood dims it); caustic
        // highlights stay.
        let wt = &style.palette.water;
        let lean_pull = wt.lightness_pull * plan.reference;
        if lean_pull > 0.0 && plan.lut.is_some() {
            let mut target = crate::palette::cast_exposure(&style.palette, tr.cast, wt.lightness);
            if plan.note.tint_safe {
                target =
                    target.max(crate::palette::water_body_l(&out) - wt.tint_safe_max_darkening);
            }
            let lean = crate::palette::lean_water_lightness(&mut out, target, lean_pull);
            exp_note += &format!(
                " water body L {:.3}{:+.3} (target {:.3})",
                lean.body, lean.shift, lean.target
            );
        }
        // Write back. No new clipping (as in `finish`, after the exposure curve too): a channel the
        // source had inside 8-bit 1..254 stays there.
        let (lo, hi) = (1.0 / 255.0, 254.0 / 255.0);
        for (dst, src) in image.pixels.iter_mut().zip(out) {
            for c in 0..3 {
                let v = src[c];
                dst[c] = if !v.is_finite() {
                    dst[c]
                } else if (lo..=hi).contains(&dst[c]) {
                    v.clamp(lo, hi)
                } else {
                    v.clamp(0.0, 1.0)
                };
            }
        }
        let n = &plan.note;
        crate::log::detail!(
            "  {}: {w}x{h} {:?} mood={} wrap={}{} seam={:.1}/{:.1} tint_safe={} (C99 {:.3}) \
             scale={:.2} r={:.1} spread={:.4} busy={:.2} speckle={:.2} marks={:.2}{}{exp_note}{} | analysis {} gpu {} (slot wait {}) post {}",
            ctx.rel.display(),
            ctx.category,
            mood,
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
            ms(t_wait),
            ms(t_post.elapsed()),
        );
        Ok(())
    }
}
