//! Value-contrast compression, fixed and adaptive (in `finish`).

use crate::config::{Style, Treatment};
use crate::image::Image;
use crate::palette::smoothstep;
use crate::pipeline::FileContext;
use crate::stylize::facts::ImageFacts;
use crate::stylize::params::Params;

/// Whether adaptive contrast applies (it also needs the low-res field as its pivot).
pub(super) fn contrast_on(style: &Style, tr: &Treatment) -> bool {
    style.contrast.strength > 0.0 && tr.value_contrast > 0.0
}

/// The texture's value spread (median L std over mid-scale windows) and the busy gate: how far
/// the spread is above the style's trigger (feathered ±15%). Shared by adaptive contrast,
/// abstraction and grouping.
pub(super) fn spread_gate(
    image: &Image,
    ctx: &FileContext,
    style: &Style,
    facts: &ImageFacts,
) -> (f32, f32) {
    let r_mid = (style.value_contrast.radius_mid * facts.scale).max(2.0 * facts.upscale);
    let ct = &style.contrast;
    let s = facts.l_std(image, ctx, r_mid);
    (
        s,
        smoothstep(ct.trigger_spread * 0.85, ct.trigger_spread * 1.15, s),
    )
}

pub(super) struct Value {
    fine: f32,
    mid: f32,
    coarse: f32,
    chroma: f32,
    r_fine: f32,
    r_mid: f32,
    /// Coarse ring radius in texels (also part of the filter reach).
    pub r_coarse: f32,
    range: f32,
    amp: f32,
    spread: f32,
    pivot_r: f32,
}

/// Adaptive contrast: busy textures above the trigger spread (bark, cliffs) get their
/// compression raised at every scale, including the groove scale, toward the target spread;
/// textures below the trigger (the ground) keep the base amounts.
pub(super) fn plan(
    style: &Style,
    tr: &Treatment,
    facts: &ImageFacts,
    contrast_on: bool,
    spread: f32,
    gate: f32,
) -> Value {
    let (f, gm, u) = (facts.scale, facts.gm, facts.upscale);
    let vc = &style.value_contrast;
    let ct = &style.contrast;
    let adapt = if contrast_on {
        let need = (1.0 - ct.target_spread / spread.max(1e-6)).max(0.0);
        (ct.strength * need * gate).clamp(0.0, 1.0)
    } else {
        0.0
    };
    // Fine grit is compressed harder; the rest of the goal is reached by scaling the whole
    // texture's light/dark amplitude around its mean (`amp`): every shape, groove and edge
    // stays where it is, only its value contrast shrinks.
    let vc_fine_a = vc.fine + (0.9 - vc.fine).max(0.0) * adapt;
    // adapt = strength · (1 − target/spread): at full strength, amp = target/spread.
    let amp = 1.0 - adapt;
    Value {
        fine: vc_fine_a * tr.value_contrast,
        mid: vc.mid * tr.value_contrast,
        coarse: vc.coarse * tr.value_contrast,
        chroma: vc.chroma,
        r_fine: (vc.radius_fine * f).max(u),
        r_mid: (vc.radius_mid * f).max(2.0 * u),
        r_coarse: (vc.radius_coarse * f).max(4.0 * u),
        range: vc.range,
        amp: 1.0 - (1.0 - amp) * tr.value_contrast.min(1.0),
        spread,
        pivot_r: (ct.pattern_radius * gm).max(2.0 * u),
    }
}

impl Value {
    pub fn write(&self, p: &mut Params) {
        p.vc_fine = self.fine;
        p.vc_mid = self.mid;
        p.vc_coarse = self.coarse;
        p.vc_chroma = self.chroma;
        p.vc_r_fine = self.r_fine;
        p.vc_r_mid = self.r_mid;
        p.vc_r_coarse = self.r_coarse;
        p.vc_range = self.range;
        p.amp = self.amp;
        p.spread = self.spread;
        p.pivot_r = self.pivot_r;
    }
}
