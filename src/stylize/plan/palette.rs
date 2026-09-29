//! Palette LUT, tint safety and the lightness ceiling (in `finish`).

use std::sync::Arc;

use super::LutSpec;
use crate::config::{Style, Treatment};
use crate::lut::Lut3d;
use crate::stylize::cache::fingerprint;
use crate::stylize::facts::ImageFacts;
use crate::stylize::params::Params;

/// The palette LUT for the (mood's) palette and a category treatment, if the style has one.
pub(super) fn lut(external: Option<&Arc<Lut3d>>, style: &Style, tr: &Treatment) -> Option<LutSpec> {
    if let Some(lut) = external {
        return Some(LutSpec::External(lut.clone()));
    }
    if !style.palette.enabled {
        return None;
    }
    Some(LutSpec::Palette {
        key: (
            fingerprint(&style.palette),
            [tr.floor_scale, tr.shadow_tint, tr.hue, tr.warmth].map(f32::to_bits),
        ),
        palette: Box::new(style.palette.clone()),
        treatment: tr.clone(),
    })
}

/// Writes the LUT size, tint safety and the relit categories' lightness ceiling.
pub(super) fn write(p: &mut Params, lut: Option<&LutSpec>, facts: &ImageFacts, tr: &Treatment) {
    p.lut_size = lut.map_or(0, LutSpec::size);
    p.tint_safe = facts.tint_safe as i32;
    p.ceiling = tr.lightness_ceiling.unwrap_or(1.0).min(1.0);
    p.ceiling_knee = tr.ceiling_knee;
}
