//! De-light: removal of baked low-frequency shading (`delight` pass).

use crate::analysis::Lowres;
use crate::config::{Style, Treatment};
use crate::stylize::params::Params;

pub(super) struct Delight {
    /// Style strength × the category's de-light.
    pub strength: f32,
    min_gain: f32,
    max_gain: f32,
}

pub(super) fn plan(style: &Style, tr: &Treatment) -> Delight {
    Delight {
        strength: style.delight.strength * tr.delight,
        min_gain: style.delight.min_gain,
        max_gain: style.delight.max_gain,
    }
}

impl Delight {
    /// The field for the CPU-side value masses, which must de-light the way the GPU does.
    pub fn field<'a>(
        &self,
        lowres: Option<&'a Lowres>,
    ) -> Option<crate::grouping::DelightField<'a>> {
        lowres
            .filter(|_| self.strength > 0.0)
            .map(|l| crate::grouping::DelightField {
                field: l,
                strength: self.strength,
                min_gain: self.min_gain,
                max_gain: self.max_gain,
            })
    }

    pub fn write(&self, p: &mut Params, lowres: Option<&Lowres>) {
        p.delight_strength = self.strength;
        p.delight_min = self.min_gain;
        p.delight_max = self.max_gain;
        p.delight_mean = lowres.map_or(0.0, |l| l.mean);
    }
}
