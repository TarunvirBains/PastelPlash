//! Planning a stylize job on the CPU: the image's facts ([`ImageFacts`]), then each stage's
//! parameters, assembled into the uniform block, plus the LUT to bind and the filter reach.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;

use super::facts::ImageFacts;
use super::params::Params;

mod abstraction;
mod accent;
mod bleed;
mod delight;
mod grouping;
mod kuwahara;
mod palette;
mod speck;
mod strokes;
mod temperature;
mod terracotta;
mod tint;
mod value;
mod watercolor;

use crate::config::{Config, Palette, Style, Treatment};
use crate::image::Image;
use crate::lut::Lut3d;
use crate::palette::Mapping;
use crate::pipeline::FileContext;

/// Palette LUT cache key: the palette fingerprint and the bits of the treatment's lift, shadow,
/// hue, warmth, chroma-floor, dark-chroma and reference scales.
pub(super) type LutKey = (u64, [u32; 7]);

/// Which palette LUT a job binds.
#[derive(Debug, Clone)]
pub enum LutSpec {
    /// The style's `.cube` (replaces the generated palette).
    External(Arc<Lut3d>),
    /// Generated from the (mood's) palette under a category treatment.
    Palette {
        key: LutKey,
        palette: Box<Palette>,
        treatment: Treatment,
    },
}

impl LutSpec {
    /// Entries per axis.
    pub fn size(&self) -> i32 {
        match self {
            Self::External(lut) => lut.size as i32,
            Self::Palette { palette, .. } => palette.lut_size.clamp(2, 129) as i32,
        }
    }

    /// The table (baked on the CPU for a generated palette).
    pub fn bake(&self) -> Lut3d {
        match self {
            Self::External(lut) => (**lut).clone(),
            Self::Palette {
                palette, treatment, ..
            } => Mapping::new(palette, treatment).bake(),
        }
    }
}

/// Everything per image that the GPU job needs besides the pixels, derived on the CPU.
#[derive(Debug, Clone)]
pub struct Plan {
    pub(super) params: Params,
    /// Low-resolution luminance field (de-light, temperature, contrast pivot); `[0.0]` if unused.
    pub lowres: Vec<f32>,
    pub lut: Option<LutSpec>,
    /// Filter reach in texels: the overlap between chunks.
    pub halo: u32,
    /// The pull toward the reference water tone this image gets (the treatment's, or 0 for
    /// pale water).
    pub reference: f32,
    /// Axes that wrap (seamless tiling).
    pub wrap: [bool; 2],
    pub(super) note: Note,
}

impl Plan {
    /// The uniform block as uploaded (size, origin and wrap are set per chunk by the runner).
    pub fn params_bytes(&self) -> &[u8] {
        bytemuck::bytes_of(&self.params)
    }
}

/// What the per-file log line reports about a plan.
#[derive(Debug, Clone, Default)]
pub(super) struct Note {
    pub ratios: [f32; 2],
    pub tint_safe: bool,
    pub chroma_p99: f32,
    pub scale: f32,
    pub radius: f32,
    pub spread: f32,
    pub busy: f32,
    pub speckle: f32,
    pub marks_scale: f32,
    pub grouping: String,
    pub analysis: Duration,
}

/// Derives [`Plan`]s on the CPU (no GPU needed).
pub struct Planner {
    /// Loaded `.cube` from the style (replaces the generated palette).
    pub(super) external_lut: Option<Arc<Lut3d>>,
}

/// Noise cells across `full` texels for a feature of `size_px` texels.
fn cells(full: u32, size_px: f32) -> f32 {
    (full as f32 / size_px.max(0.5)).round().max(1.0)
}

impl Planner {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            external_lut: match &config.style.lut {
                Some(path) => Some(Arc::new(Lut3d::load(path)?)),
                None => None,
            },
        })
    }

    /// The plan for one file in `style` (the file's mood already applied); `None` when the stage
    /// leaves the file alone (UI, skip, empty images).
    pub fn plan(&self, image: &Image, ctx: &FileContext, style: &Style) -> Option<Plan> {
        if !ctx.category.is_stylized() {
            return None;
        }
        let t_start = Instant::now();
        let mut tr = ctx.config.target.treatment(ctx.category);
        if image.width == 0 || image.height == 0 {
            return None;
        }
        // Pale water (falls, foam, rapids) keeps its own tone: the reference lean is for pooled
        // water seen over depth.
        let mut pale_note = "";
        let pale = style.palette.water.pale_body;
        if tr.reference > 0.0
            && pale < 1.0
            && crate::palette::water_body_l(&ctx.source.unwrap_or(image).pixels) >= pale
        {
            tr.reference = 0.0;
            pale_note = " pale-water";
        }
        let facts = ImageFacts::analyze(image, ctx, style, &tr);
        if facts.effect_like {
            // An unnamed effect (a soft gray glow): its gray is light, not paint.
            println!(
                "  {}: effect-like (radial glow): untouched",
                ctx.rel.display()
            );
            return None;
        }

        // Stage plans, in dependency order: the low-res field serves de-light, temperature and
        // the adaptive-contrast pivot; the busy gate (value spread) drives adaptive contrast,
        // abstraction and grouping; abstraction's busy weight shapes the marks and strokes.
        let delight = delight::plan(style, &tr);
        let temperature = temperature::plan(style, &tr);
        let contrast_on = value::contrast_on(style, &tr);
        let lowres = (delight.strength > 0.0 || temperature.strength > 0.0 || contrast_on)
            .then(|| facts.lowres(image, style));
        let lut = palette::lut(self.external_lut.as_ref(), style, &tr);
        let accent = accent::plan(style, &tr, &facts, lut.is_some());
        let (marks_scale, speckle) = kuwahara::marks(image, style, ctx, &facts);
        let abstraction_on = abstraction::on(style, &tr, ctx);
        let grouping_on = grouping::on(style, &tr, ctx);
        let (spread, gate) = if contrast_on || abstraction_on || grouping_on {
            value::spread_gate(image, ctx, style, &facts)
        } else {
            (0.0, 0.0)
        };
        let value = value::plan(style, &tr, &facts, contrast_on, spread, gate);
        let abstraction = abstraction::plan(image, style, &tr, &facts, abstraction_on, gate);
        let grouping = grouping::plan(
            // The value masses of an enlarged texture are its source's.
            ctx.source.unwrap_or(image),
            style,
            &tr,
            &facts,
            grouping_on,
            gate,
            lowres.as_ref(),
            &delight,
        );
        let busy = abstraction.busy;
        let kuwahara = kuwahara::plan(style, &tr, &facts, marks_scale, busy);
        let brushwork = ctx.config.pack.brushwork_for(ctx.rel);
        let strokes = strokes::plan(style, &tr, &facts, busy, brushwork);
        let bleed = bleed::plan(style, &facts);
        let watercolor = watercolor::plan(style, &tr, &facts);
        let tint_safe = tint::plan(image, &tr, &facts);
        let speck = speck::plan(style, ctx, &facts);
        let terracotta = terracotta::plan(image, style, ctx, facts.tint_safe);

        let mut params = Params {
            full_x: facts.w as i32,
            full_y: facts.h as i32,
            tile_x: facts.wrap[0] as i32,
            tile_y: facts.wrap[1] as i32,
            low_w: lowres.as_ref().map_or(0, |l| l.width as i32),
            low_h: lowres.as_ref().map_or(0, |l| l.height as i32),
            ..Params::default()
        };
        palette::write(&mut params, lut.as_ref(), &facts, &tr);
        palette::write_cast(&mut params, style, &tr, lut.is_some());
        let dark_reach = palette::write_dark_floor(&mut params, lut.as_ref(), &tr, &facts);
        delight.write(&mut params, lowres.as_ref());
        grouping.write(&mut params, style);
        kuwahara.write(&mut params);
        abstraction.write(&mut params);
        bleed.write(&mut params, &facts);
        accent.write(&mut params);
        temperature.write(&mut params);
        strokes.write(&mut params, &facts);
        value.write(&mut params);
        watercolor.write(&mut params, style, &facts);
        tint_safe.write(&mut params);
        speck.write(&mut params);
        terracotta.write(&mut params, style);

        // Filter reach: how far a texel's result depends on its neighbors (chunk overlap).
        let reach = [
            watercolor.edge_step,
            bleed.radius,
            accent.radius,
            watercolor.gran_radius(),
            value.r_coarse,
            1.5 * speck.radius,
            speck.clip_radius,
            3.2 * speck.thin_radius,
            dark_reach,
        ]
        .into_iter()
        .fold(0.0f32, f32::max);
        let halo = (2.0 * (kuwahara.radius + abstraction.radius_coarse)
            + 3.0 * kuwahara.tensor_sigma
            + reach
            + strokes.len * if tint_safe.on() { 3.0 } else { 1.0 }
            + abstraction.highlight_radius * busy.ceil()
            + if grouping.amount > 0.0 {
                grouping.radius + 1.0
            } else {
                0.0
            }
            + 6.0)
            .ceil() as u32;
        Some(Plan {
            params,
            lowres: lowres.map_or_else(|| vec![0.0], |l| l.data),
            lut,
            halo,
            reference: tr.reference,
            wrap: facts.wrap,
            note: Note {
                ratios: facts.seam_ratios,
                tint_safe: facts.tint_safe,
                chroma_p99: facts.chroma_p99,
                scale: facts.scale,
                radius: kuwahara.radius,
                spread,
                busy,
                speckle,
                marks_scale,
                grouping: grouping.note + &terracotta.note + pale_note,
                analysis: t_start.elapsed(),
            },
        })
    }
}
