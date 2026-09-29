//! Paint marks: the structure tensor and the anisotropic Kuwahara pass (`tensor`, `blur_h`,
//! `blur_v`, `kuwahara`).

use crate::config::{Style, Treatment};
use crate::image::Image;
use crate::palette::smoothstep;
use crate::pipeline::FileContext;
use crate::stylize::facts::ImageFacts;
use crate::stylize::params::Params;

/// Paint-mark scale for a file: the pack map's marks rule, times the style's tiling multiplier
/// as far as the texture tiles and is speckled. Returns (scale, speckle ratio or 0).
pub(super) fn marks(
    image: &Image,
    style: &Style,
    ctx: &FileContext,
    facts: &ImageFacts,
) -> (f32, f32) {
    let (f, wrap) = (facts.scale, facts.wrap);
    let mut marks_scale = ctx.config.pack.marks_scale_for(ctx.rel);
    let mut speckle = 0.0;
    let mk = &style.marks;
    // Fluids are not ground grit: their marks stay small (caustics would turn into cells).
    if mk.tiling_multiplier != 1.0 && (wrap[0] || wrap[1]) && !ctx.category.is_fluid() {
        let fine = facts.l_std(image, (3.0 * f).max(1.0));
        let mid = facts.l_std(image, (12.0 * f).max(2.0));
        speckle = fine / mid.max(1e-6);
        let w = smoothstep(mk.speckle[0], mk.speckle[1], speckle);
        marks_scale *= 1.0 + (mk.tiling_multiplier - 1.0) * w;
    }
    (marks_scale, speckle)
}

pub(super) struct Kuwahara {
    /// Filter radius in texels (0 = off).
    pub radius: f32,
    /// Structure-tensor smoothing sigma in texels.
    pub tensor_sigma: f32,
    sharpness: f32,
    hardness: f32,
    anisotropy: f32,
    zero_crossing: f32,
    strength: f32,
}

/// Busy textures (`busy` from the abstraction) follow a smoother, coarser structure.
pub(super) fn plan(
    style: &Style,
    tr: &Treatment,
    facts: &ImageFacts,
    marks_scale: f32,
    busy: f32,
) -> Kuwahara {
    let f = facts.scale;
    let k = &style.kuwahara;
    // Paint-mark size: the style's marks.size (or kuwahara.radius), per category and per
    // pack-map rule (e.g. larger dabs on ground textures that tile many times).
    let mark = style.marks.size.unwrap_or(k.radius);
    let radius = if mark > 0.0 && k.strength > 0.0 {
        (mark * f * tr.radius_scale * marks_scale).clamp(k.min_radius, k.max_radius)
    } else {
        0.0
    };
    let tensor_sigma = (k.tensor_sigma * f).clamp(0.5, 16.0);
    let flow = style.abstraction.flow_scale;
    Kuwahara {
        radius,
        tensor_sigma: (tensor_sigma * (1.0 + busy * (flow - 1.0).max(0.0))).clamp(0.5, 32.0),
        sharpness: k.sharpness,
        hardness: k.hardness,
        anisotropy: k.anisotropy.max(1e-3),
        zero_crossing: k.zero_crossing,
        strength: k.strength,
    }
}

impl Kuwahara {
    pub fn write(&self, p: &mut Params) {
        p.kuw_radius = self.radius;
        p.kuw_q = self.sharpness;
        p.kuw_hardness = self.hardness;
        p.kuw_alpha = self.anisotropy;
        p.kuw_zero_cross = self.zero_crossing;
        p.kuw_strength = self.strength;
        p.tensor_sigma = self.tensor_sigma;
    }
}
