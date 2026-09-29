//! Planning a stylize job on the CPU: the image's facts ([`ImageFacts`]), then each stage's
//! parameters, assembled into the uniform block, plus the LUT to bind and the filter reach.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;

use super::facts::ImageFacts;
use super::params::Params;
use crate::config::{Config, Mood, Palette, Style, Treatment};
use crate::grouping;
use crate::image::Image;
use crate::lut::Lut3d;
use crate::palette::{Mapping, smoothstep};
use crate::pipeline::FileContext;

/// Palette LUT cache key: mood key and the bits of the treatment's lift, shadow and hue scales.
pub(super) type LutKey = (String, [u32; 4]);

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

    /// The palette LUT for a mood and category treatment, if the style has a palette.
    fn lut(&self, style: &Style, mood: &Mood, tr: &Treatment) -> Option<LutSpec> {
        if let Some(lut) = &self.external_lut {
            return Some(LutSpec::External(lut.clone()));
        }
        if !style.palette.enabled {
            return None;
        }
        let key = if mood.is_base() {
            String::new()
        } else {
            mood.key()
        };
        Some(LutSpec::Palette {
            key: (
                key,
                [tr.floor_scale, tr.shadow_tint, tr.hue, tr.warmth].map(f32::to_bits),
            ),
            palette: Box::new(style.palette.clone()),
            treatment: tr.clone(),
        })
    }

    /// The plan for one file in `style` (the file's mood already applied); `None` when the stage
    /// leaves the file alone (UI, skip, empty images).
    pub fn plan(&self, image: &Image, ctx: &FileContext, style: &Style) -> Option<Plan> {
        if !ctx.category.is_stylized() {
            return None;
        }
        let t_start = Instant::now();
        let tr = ctx.config.target.treatment(ctx.category);
        if image.width == 0 || image.height == 0 {
            return None;
        }
        let facts = ImageFacts::analyze(image, ctx, style, &tr);
        let (w, h, gm, f, wrap) = (facts.w, facts.h, facts.gm, facts.scale, facts.wrap);

        let delight_strength = style.delight.strength * tr.delight;
        let temp_strength = style.temperature.chroma * tr.warm_cool;
        // The low-res luminance field also serves as the adaptive-contrast pivot.
        let contrast_on = style.contrast.strength > 0.0 && tr.value_contrast > 0.0;
        let lowres = (delight_strength > 0.0 || temp_strength > 0.0 || contrast_on)
            .then(|| facts.lowres(image, style));
        let lut = self.lut(style, &ctx.mood, &tr);
        let pal = &style.palette;
        let accent_fraction = if lut.is_some() {
            pal.accent_fraction * tr.accent
        } else {
            0.0
        };
        let accent_radius = (pal.accent_radius * f).max(1.0);
        let tint_safe = facts.tint_safe;

        // Parameters.
        let k = &style.kuwahara;
        // Paint-mark size: the style's marks.size (or kuwahara.radius), per category and per
        // pack-map rule (e.g. larger dabs on ground textures that tile many times).
        let mark = style.marks.size.unwrap_or(k.radius);
        let mut marks_scale = ctx.config.pack.marks_scale_for(ctx.rel);
        let mut speckle = 0.0;
        let mk = &style.marks;
        if mk.tiling_multiplier != 1.0 && (wrap[0] || wrap[1]) {
            let fine = facts.l_std(image, (3.0 * f).max(1.0));
            let mid = facts.l_std(image, (12.0 * f).max(2.0));
            speckle = fine / mid.max(1e-6);
            let w = smoothstep(mk.speckle[0], mk.speckle[1], speckle);
            marks_scale *= 1.0 + (mk.tiling_multiplier - 1.0) * w;
        }
        let radius = if mark > 0.0 && k.strength > 0.0 {
            (mark * f * tr.radius_scale * marks_scale).clamp(k.min_radius, k.max_radius)
        } else {
            0.0
        };
        let tensor_sigma = (k.tensor_sigma * f).clamp(0.5, 16.0);
        let wc = &style.watercolor;
        let st = &style.strokes;
        let vc = &style.value_contrast;
        // Adaptive contrast: busy textures above the trigger spread (bark, cliffs) get their
        // compression raised at every scale, including the groove scale, toward the target
        // spread; textures below the trigger (the ground) keep the base amounts.
        let ct = &style.contrast;
        let r_mid = (vc.radius_mid * f).max(2.0);
        let ab = &style.abstraction;
        let abstraction_on = ab.strength > 0.0
            && tr.value_contrast > 0.0
            && ctx.config.pack.abstraction_allowed(ctx.rel);
        // Soft value grouping: world and background textures only (never actors, which the cel
        // shader bands, nor UI), unless the pack map opts the file out.
        let gr = &style.grouping;
        let grouping_on = gr.strength > 0.0
            && tr.grouping > 0.0
            && ctx.category.may_group()
            && ctx.config.pack.grouping_allowed(ctx.rel);
        let (spread, gate) = if contrast_on || abstraction_on || grouping_on {
            let s = facts.l_std(image, r_mid);
            (
                s,
                smoothstep(ct.trigger_spread * 0.85, ct.trigger_spread * 1.15, s),
            )
        } else {
            (0.0, 0.0)
        };
        let adapt = if contrast_on {
            let need = (1.0 - ct.target_spread / spread.max(1e-6)).max(0.0);
            (ct.strength * need * gate).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Busy weight for the design-like abstraction (photographic, high-contrast textures).
        let busy = if abstraction_on {
            (ab.strength * gate * tr.value_contrast.min(1.0) * tr.abstraction).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mut grp = if grouping_on {
            (gr.strength * gate * tr.grouping).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let mut grp_note = String::new();
        let masses = if grp > 0.0 {
            let field = lowres.as_ref().filter(|_| delight_strength > 0.0).map(|l| {
                grouping::DelightField {
                    field: l,
                    strength: delight_strength,
                    min_gain: style.delight.min_gain,
                    max_gain: style.delight.max_gain,
                }
            });
            match grouping::value_masses(image, field.as_ref(), wrap, gr) {
                Ok(m) => {
                    let lch: Vec<String> = (0..m.count)
                        .map(|k| {
                            let [a, b] = m.ab[k];
                            format!(
                                "{:.2}/{:.3}/{:.0}@{:.0}%",
                                m.l[k],
                                a.hypot(b),
                                b.atan2(a).to_degrees().rem_euclid(360.0),
                                m.share[k] * 100.0
                            )
                        })
                        .collect();
                    grp_note = format!(
                        " grp={grp:.2} masses={} [{}] expl={:.2}/{:.2}",
                        m.count,
                        lch.join(" "),
                        m.explained,
                        m.explained2
                    );
                    Some(m)
                }
                Err(skip) => {
                    grp_note = format!(" grp=skip({skip:?})");
                    grp = 0.0;
                    None
                }
            }
        } else {
            None
        };
        let grp_radius = (gr.radius * gm).max(1.0);
        let grp_sigma = masses.as_ref().map_or(0.0, |m| {
            let gap = (0..m.count - 1)
                .map(|i| m.l[i + 1] - m.l[i])
                .fold(f32::MAX, f32::min);
            (gr.softness * 0.5 * gap).max(1e-3)
        });
        let mean_lab = if busy > 0.0 {
            crate::report::mean_oklab(image)
        } else {
            [0.5, 0.0, 0.0]
        };
        let tensor_sigma =
            (tensor_sigma * (1.0 + busy * (ab.flow_scale - 1.0).max(0.0))).clamp(0.5, 32.0);
        let radius_coarse = if busy > 0.0 {
            (ab.radius * f)
                .max(ab.min_frac * gm)
                .clamp(k.min_radius, k.max_radius)
        } else {
            0.0
        };
        // Fine grit is compressed harder; the rest of the goal is reached by scaling the whole
        // texture's light/dark amplitude around its mean (`amp`): every shape, groove and edge
        // stays where it is, only its value contrast shrinks.
        let vc_fine_a = vc.fine + (0.9 - vc.fine).max(0.0) * adapt;
        let vc_mid = vc.mid;
        let vc_coarse = vc.coarse;
        // adapt = strength · (1 − target/spread): at full strength, amp = target/spread.
        let amp = 1.0 - adapt;
        let stroke_width = (st.width * f * tr.stroke_scale).max(0.75);
        let stroke_len =
            (st.length * f * tr.stroke_scale * (1.0 + busy * (ab.stroke_scale - 1.0))).max(1.0);
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
            lut_size: lut.as_ref().map_or(0, LutSpec::size),
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
            smear: st.smear * tr.strokes,
            vc_fine: vc_fine_a * tr.value_contrast,
            vc_mid: vc_mid * tr.value_contrast,
            vc_coarse: vc_coarse * tr.value_contrast,
            vc_chroma: vc.chroma,
            vc_r_fine: (vc.radius_fine * f).max(1.0),
            vc_r_mid: (vc.radius_mid * f).max(2.0),
            vc_r_coarse: (vc.radius_coarse * f).max(4.0),
            vc_range: vc.range,
            amp: 1.0 - (1.0 - amp) * tr.value_contrast.min(1.0),
            busy,
            kuw_radius_coarse: radius_coarse,
            edge_coarse_step: (wc.edge_width * f * ab.edge_scale).max(2.0),
            edge_soften: ab.edge_soften,
            highlight_calm: ab.highlight_calm,
            highlight_radius: (ab.highlight_radius * f)
                .max(2.0 * ab.min_frac * gm)
                .max(2.0),
            chroma_retain: ab.chroma_retain,
            mean_l: mean_lab[0],
            mean_a: mean_lab[1],
            mean_b: mean_lab[2],
            spread,
            pivot_r: (style.contrast.pattern_radius * gm).max(2.0),
            grp,
            grp_sigma,
            grp_radius,
            grp_range: gr.range,
            grp_color: gr.color,
            grp_family: gr.color_family,
            grp_stroke: gr.stroke_value,
            grp_sal0: gr.salient[0],
            grp_sal1: gr.salient[1],
            grp_count: masses.as_ref().map_or(0.0, |m| m.count as f32),
            grp_l: masses.as_ref().map_or([0.0; 4], |m| m.l),
            grp_a: masses.as_ref().map_or([0.0; 4], |m| m.ab.map(|v| v[0])),
            grp_b: masses.as_ref().map_or([0.0; 4], |m| m.ab.map(|v| v[1])),
            paper: wc.paper_grain * tr.paper,
            paper_tint: wc.paper_tint * tr.paper,
            paper_cells_x: cells(w, paper_px),
            paper_cells_y: cells(h, paper_px),
            paper_hl: wc.paper_highlight,
            paper_r: wc.paper_color[0],
            paper_g: wc.paper_color[1],
            paper_b: wc.paper_color[2],
            floor_margin: wc.floor_margin,
            ceiling,
            ceiling_knee: tr.ceiling_knee,
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
            params.vc_r_coarse,
        ]
        .into_iter()
        .fold(0.0f32, f32::max);
        let halo = (2.0 * (radius + radius_coarse)
            + 3.0 * tensor_sigma
            + reach
            + stroke_len
            + (ab.highlight_radius * f)
                .max(2.0 * ab.min_frac * gm)
                .max(2.0)
                * busy.ceil()
            + if grp > 0.0 { grp_radius + 1.0 } else { 0.0 }
            + 6.0)
            .ceil() as u32;
        Some(Plan {
            params,
            lowres: lowres.map_or_else(|| vec![0.0], |l| l.data),
            lut,
            halo,
            wrap,
            note: Note {
                ratios: facts.seam_ratios,
                tint_safe,
                chroma_p99: facts.chroma_p99,
                scale: f,
                radius,
                spread,
                busy,
                speckle,
                marks_scale,
                grouping: grp_note,
                analysis: t_start.elapsed(),
            },
        })
    }
}
