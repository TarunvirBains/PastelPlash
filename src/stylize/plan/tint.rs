//! Engine-tinted grayscale (tint-safe) textures of categories with `tint_safe_gray` (Link's tunic,
//! Goron and Zora tunics, gems, enemy color variants): the gray is raised so its median sits at
//! the target, then painted with brightness-only strokes along the texture's own flow (cloth
//! folds) and a few near-white highlights, capped below white (in `finish`).

use crate::config::Treatment;
use crate::image::Image;
use crate::stylize::facts::ImageFacts;
use crate::stylize::params::Params;

/// The brightest a tint-safe texel gets (the engine's tint provides the headroom).
pub(super) const MAX_L: f32 = 0.95;

pub(super) struct TintSafe {
    on: bool,
    shift: f32,
    amp: f32,
}

pub(super) fn plan(image: &Image, tr: &Treatment, facts: &ImageFacts) -> TintSafe {
    let off = TintSafe {
        on: false,
        shift: 0.0,
        amp: 0.0,
    };
    let Some(target) = tr.tint_safe_gray.filter(|_| facts.tint_safe) else {
        return off;
    };
    let step = (image.pixels.len() / 65_536).max(1);
    let mut l: Vec<f32> = image
        .pixels
        .iter()
        .step_by(step)
        .filter(|p| p[3] >= 0.5)
        .map(|p| crate::color::srgb_to_oklab([p[0], p[1], p[2]])[0])
        .collect();
    if l.is_empty() {
        return off;
    }
    let i = l.len() / 2;
    let median = *l.select_nth_unstable_by(i, f32::total_cmp).1;
    // A shift puts the median at the target (never darker); the folds keep their contrast.
    TintSafe {
        on: true,
        shift: (target - median).max(0.0),
        amp: tr.tint_safe_strokes,
    }
}

impl TintSafe {
    /// Whether the texture gets the tint-safe treatment (its strokes are three times as long).
    pub fn on(&self) -> bool {
        self.on
    }

    pub fn write(&self, p: &mut Params) {
        p.ts_on = if self.on { 1.0 } else { 0.0 };
        p.ts_shift = self.shift;
        p.ts_amp = self.amp;
        p.ts_max = MAX_L;
    }
}
