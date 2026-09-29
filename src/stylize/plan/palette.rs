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

/// Writes the moonlight cast (applied per texel after the palette LUT; see
/// `crate::palette::apply_cast`). Off without a palette.
pub(super) fn write_cast(p: &mut Params, style: &Style, tr: &Treatment, has_lut: bool) {
    let pal = &style.palette;
    let k = &pal.cast;
    p.cast_s = if has_lut {
        crate::palette::cast_strength(pal, tr.cast)
    } else {
        0.0
    };
    p.cast_hue = k.hue.to_radians();
    let f = crate::palette::cast_filter(k.hue, p.cast_s * k.tint);
    (p.cast_fr, p.cast_fg, p.cast_fb) = (f[0], f[1], f[2]);
    p.cast_exposure = k.exposure;
    p.cast_chroma = k.chroma;
    p.cast_dark_cap = k.dark_cap;
    p.cast_dark_min = k.dark_min;
    p.cast_dark_chroma = k.dark_chroma;
    p.cast_pivot = pal.l_floor;
    p.cast_dark_below = pal.dark_below;
    p.cast_warm0 = k.warm_band[0];
    p.cast_warm1 = k.warm_band[1];
    p.cast_on = if has_lut && crate::palette::cast_strength(pal, 1.0) > 0.0 {
        1.0
    } else {
        0.0
    };
}
