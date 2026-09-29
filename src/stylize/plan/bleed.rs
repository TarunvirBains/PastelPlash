//! Edge-aware color bleeding (`bleed` pass).

use super::cells;
use crate::config::Style;
use crate::stylize::facts::ImageFacts;
use crate::stylize::params::Params;

pub(super) struct Bleed {
    amount: f32,
    /// Reach in texels (also part of the filter reach).
    pub radius: f32,
    range: f32,
}

pub(super) fn plan(style: &Style, facts: &ImageFacts) -> Bleed {
    let wc = &style.watercolor;
    Bleed {
        amount: wc.bleed,
        radius: (wc.bleed_radius * facts.scale).clamp(facts.upscale, 48.0 * facts.upscale),
        range: wc.bleed_range,
    }
}

impl Bleed {
    pub fn write(&self, p: &mut Params, facts: &ImageFacts) {
        p.bleed = self.amount;
        p.bleed_radius = self.radius;
        p.bleed_range = self.range;
        p.bloom_cells_x = cells(facts.w, self.radius * 6.0);
        p.bloom_cells_y = cells(facts.h, self.radius * 6.0);
    }
}
