//! Accent darks from structural crevices (`accent_hist`, `accent_threshold`, `finish`).

use crate::config::{Style, Treatment};
use crate::stylize::facts::ImageFacts;
use crate::stylize::params::Params;

pub(super) struct Accent {
    fraction: f32,
    /// Band-pass radius in texels (also part of the filter reach).
    pub radius: f32,
    softness: f32,
    min_depth: f32,
    min_l: f32,
    hue: f32,
    chroma: f32,
    depth: f32,
}

/// Accents need a palette (`has_lut`).
pub(super) fn plan(style: &Style, tr: &Treatment, facts: &ImageFacts, has_lut: bool) -> Accent {
    let pal = &style.palette;
    Accent {
        fraction: if has_lut {
            pal.accent_fraction * tr.accent
        } else {
            0.0
        },
        radius: (pal.accent_radius * facts.scale).max(1.0),
        softness: pal.accent_softness,
        min_depth: pal.accent_min_depth,
        min_l: pal.accent_min_l,
        hue: pal.accent_hue.to_radians(),
        chroma: pal.accent_chroma,
        depth: tr.accent.clamp(0.0, 1.0),
    }
}

impl Accent {
    pub fn write(&self, p: &mut Params) {
        p.accent_fraction = self.fraction;
        p.accent_radius = self.radius;
        p.accent_softness = self.softness;
        p.accent_min_depth = self.min_depth;
        p.accent_min_l = self.min_l;
        p.accent_hue = self.hue;
        p.accent_chroma = self.chroma;
        p.accent_depth = self.depth;
    }
}
