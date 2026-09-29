//! Warm/cool temperature from residual shading (in `finish`).

use crate::config::{Style, Treatment};
use crate::stylize::params::Params;

pub(super) struct Temperature {
    /// Style chroma × the category's warm/cool.
    pub strength: f32,
    warm_hue: f32,
    cool_hue: f32,
    sensitivity: f32,
}

pub(super) fn plan(style: &Style, tr: &Treatment) -> Temperature {
    let t = &style.temperature;
    Temperature {
        strength: t.chroma * tr.warm_cool,
        warm_hue: t.warm_hue.to_radians(),
        cool_hue: t.cool_hue.to_radians(),
        sensitivity: t.sensitivity,
    }
}

impl Temperature {
    pub fn write(&self, p: &mut Params) {
        p.temp_strength = self.strength;
        p.temp_warm_hue = self.warm_hue;
        p.temp_cool_hue = self.cool_hue;
        p.temp_sens = self.sensitivity;
    }
}
