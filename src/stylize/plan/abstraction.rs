//! Design-like abstraction of busy, photographic textures: the coarse Kuwahara pass, and in
//! `finish` coarse wet edges, glare calming and chroma retention.

use crate::config::{Style, Treatment};
use crate::image::Image;
use crate::pipeline::FileContext;
use crate::stylize::facts::ImageFacts;
use crate::stylize::params::Params;

/// Whether the file may be abstracted (the pack map can opt it out).
pub(super) fn on(style: &Style, tr: &Treatment, ctx: &FileContext) -> bool {
    style.abstraction.strength > 0.0
        && tr.value_contrast > 0.0
        && ctx.config.pack.abstraction_allowed(ctx.rel)
}

pub(super) struct Abstraction {
    /// Busy weight 0..1 (photographic, high-contrast textures).
    pub busy: f32,
    /// Coarse Kuwahara radius in texels (0 when not busy).
    pub radius_coarse: f32,
    /// Glare-calming ring radius in texels.
    pub highlight_radius: f32,
    mean_lab: [f32; 3],
    edge_coarse_step: f32,
    edge_soften: f32,
    highlight_calm: f32,
    chroma_retain: f32,
}

pub(super) fn plan(
    image: &Image,
    style: &Style,
    tr: &Treatment,
    facts: &ImageFacts,
    on: bool,
    gate: f32,
) -> Abstraction {
    let (f, gm, u) = (facts.scale, facts.gm, facts.upscale);
    let ab = &style.abstraction;
    let k = &style.kuwahara;
    let busy = if on {
        (ab.strength * gate * tr.value_contrast.min(1.0) * tr.abstraction).clamp(0.0, 1.0)
    } else {
        0.0
    };
    Abstraction {
        busy,
        radius_coarse: if busy > 0.0 {
            (ab.radius * f)
                .max(ab.min_frac * gm)
                .clamp(k.min_radius * u, k.max_radius * u)
        } else {
            0.0
        },
        highlight_radius: (ab.highlight_radius * f)
            .max(2.0 * ab.min_frac * gm)
            .max(2.0 * u),
        mean_lab: if busy > 0.0 {
            crate::report::mean_oklab(image)
        } else {
            [0.5, 0.0, 0.0]
        },
        edge_coarse_step: (style.watercolor.edge_width * f * ab.edge_scale).max(2.0 * u),
        edge_soften: ab.edge_soften,
        highlight_calm: ab.highlight_calm,
        chroma_retain: ab.chroma_retain,
    }
}

impl Abstraction {
    pub fn write(&self, p: &mut Params) {
        p.busy = self.busy;
        p.kuw_radius_coarse = self.radius_coarse;
        p.edge_coarse_step = self.edge_coarse_step;
        p.edge_soften = self.edge_soften;
        p.highlight_calm = self.highlight_calm;
        p.highlight_radius = self.highlight_radius;
        p.chroma_retain = self.chroma_retain;
        p.mean_l = self.mean_lab[0];
        p.mean_a = self.mean_lab[1];
        p.mean_b = self.mean_lab[2];
    }
}
