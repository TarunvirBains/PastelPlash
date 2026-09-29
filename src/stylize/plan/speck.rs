//! Speck cleaning (in the `delight` pass), the no-clip neighborhood and thin-structure
//! protection (in `finish`).

use crate::config::Category;
use crate::config::Style;
use crate::pipeline::FileContext;
use crate::stylize::facts::ImageFacts;
use crate::stylize::params::Params;

pub(super) struct Speck {
    /// Speck ring radius in texels (0 = off; the outer ring is 1.5× this: filter reach).
    pub radius: f32,
    depth: f32,
    /// Ring radius for telling compact blown highlights from large blown regions.
    pub clip_radius: f32,
    /// Thin-structure detection radius (texels) and protection amount.
    pub thin_radius: f32,
    thin_amount: f32,
}

/// World and background textures only: actors' small dots are eyes, rivets and studs.
pub(super) fn plan(style: &Style, ctx: &FileContext, facts: &ImageFacts) -> Speck {
    let (mk, u) = (&style.marks, facts.upscale);
    let on =
        mk.speck_radius > 0.0 && matches!(ctx.category, Category::World | Category::Background);
    Speck {
        radius: if on {
            (mk.speck_radius * facts.scale).clamp(2.5 * u, 6.0 * u)
        } else {
            0.0
        },
        depth: mk.speck_depth,
        clip_radius: (6.0 * facts.scale).clamp(6.0 * u, 24.0 * u),
        thin_radius: (2.5 * facts.scale).clamp(2.0 * u, 8.0 * u),
        // World and background only: actors are banded by the cel shader, fluids and skies have
        // no handles.
        thin_amount: if matches!(ctx.category, Category::World | Category::Background) {
            mk.thin_protect
        } else {
            0.0
        },
    }
}

impl Speck {
    pub fn write(&self, p: &mut Params) {
        p.speck_r = self.radius;
        p.speck_thr = self.depth;
        p.clip_r = self.clip_radius;
        p.thin_r = self.thin_radius;
        p.thin_amount = self.thin_amount;
    }
}
