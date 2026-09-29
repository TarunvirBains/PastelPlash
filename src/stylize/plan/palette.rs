//! Palette LUT, tint safety and the lightness ceiling (in `finish`).

use std::sync::Arc;

use super::LutSpec;
use crate::config::{Style, Treatment};
use crate::lut::Lut3d;
use crate::stylize::cache::fingerprint;
use crate::stylize::facts::ImageFacts;
use crate::stylize::params::Params;

/// The palette LUT for the (mood's) palette and a category treatment, if the style has one and
/// the category is mapped through a palette at all.
pub(super) fn lut(external: Option<&Arc<Lut3d>>, style: &Style, tr: &Treatment) -> Option<LutSpec> {
    if !tr.palette {
        return None;
    }
    if let Some(lut) = external {
        return Some(LutSpec::External(lut.clone()));
    }
    if !style.palette.enabled {
        return None;
    }
    Some(LutSpec::Palette {
        key: (
            fingerprint(&style.palette),
            [
                tr.floor_scale,
                tr.shadow_tint,
                tr.hue,
                tr.warmth,
                tr.chroma_floor,
                tr.dark_chroma,
                tr.reference,
            ]
            .map(f32::to_bits),
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

/// Writes the per-texel dark floor (`crate::palette::DarkFloor`: off unless the generated palette
/// LUT defers it) and returns its reach in texels (the neighborhood radius).
pub(super) fn write_dark_floor(
    p: &mut Params,
    lut: Option<&LutSpec>,
    tr: &Treatment,
    facts: &ImageFacts,
) -> f32 {
    let Some(LutSpec::Palette { palette, .. }) = lut else {
        return 0.0;
    };
    let u = facts.upscale;
    let r = (palette.dark_context.radius * facts.scale).clamp(3.0 * u, 32.0 * u);
    // A moonlight cast's stone handling reads the same neighborhood.
    if palette.dark_context.radius > 0.0
        && palette.cast.stone_chroma > 0.0
        && crate::palette::cast_strength(palette, 1.0) > 0.0
    {
        p.cast_stone = palette.cast.stone_chroma;
        p.ctx_r = r;
    }
    let Some(f) = crate::palette::DarkFloor::new(palette, tr) else {
        return if p.cast_stone > 0.0 { r } else { 0.0 };
    };
    p.df_on = 1.0;
    p.df_chroma = f.chroma;
    p.df_below = f.below;
    p.df_hue = f.hue;
    p.df_tint = f.tint;
    p.df_tint_chroma = f.tint_chroma;
    p.df_tint_below = f.tint_below;
    p.df_cool_bias = f.cool_bias;
    p.df_cool_hue = f.cool_hue;
    p.ctx_r = r;
    p.ctx_neutral = f.neutral;
    p.ctx_gain = f.gain;
    p.ctx_warm0 = f.warm_band[0];
    p.ctx_warm1 = f.warm_band[1];
    r
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
    (p.cast_black0, p.cast_black1) = (k.black[0], k.black[1]);
    p.cast_black_chroma = k.black_chroma;
    p.cast_on = if has_lut && crate::palette::cast_strength(pal, 1.0) > 0.0 {
        1.0
    } else {
        0.0
    };
}
