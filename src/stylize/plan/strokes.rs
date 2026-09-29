//! Brushstrokes and smear along the flow (in `finish`).

use super::cells;
use crate::config::{Style, Treatment};
use crate::stylize::facts::ImageFacts;
use crate::stylize::params::Params;

pub(super) struct Strokes {
    strength: f32,
    chroma: f32,
    /// Stroke half-length in texels (also part of the filter reach).
    pub len: f32,
    width: f32,
    smear: f32,
}

/// Busy textures (`busy` from the abstraction) get longer strokes.
pub(super) fn plan(style: &Style, tr: &Treatment, facts: &ImageFacts, busy: f32) -> Strokes {
    let f = facts.scale;
    let st = &style.strokes;
    let ab = &style.abstraction;
    Strokes {
        strength: st.strength * tr.strokes,
        chroma: st.chroma * tr.strokes,
        len: (st.length * f * tr.stroke_scale * (1.0 + busy * (ab.stroke_scale - 1.0))).max(1.0),
        width: (st.width * f * tr.stroke_scale).max(0.75),
        smear: st.smear * tr.strokes,
    }
}

impl Strokes {
    pub fn write(&self, p: &mut Params, facts: &ImageFacts) {
        p.stroke_strength = self.strength;
        p.stroke_chroma = self.chroma;
        p.stroke_len = self.len;
        p.stroke_step = (self.len / 16.0).max(1.0);
        p.stroke_cells_x = cells(facts.w, self.width);
        p.stroke_cells_y = cells(facts.h, self.width);
        p.smear = self.smear;
    }
}
