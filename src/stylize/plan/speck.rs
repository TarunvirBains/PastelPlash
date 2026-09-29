//! Speck cleaning (in the `delight` pass) and the no-clip neighborhood (in `finish`).

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
}

/// World and background textures only: actors' small dots are eyes, rivets and studs.
pub(super) fn plan(style: &Style, ctx: &FileContext, facts: &ImageFacts) -> Speck {
    let mk = &style.marks;
    let on =
        mk.speck_radius > 0.0 && matches!(ctx.category, Category::World | Category::Background);
    Speck {
        radius: if on {
            (mk.speck_radius * facts.scale).clamp(2.5, 6.0)
        } else {
            0.0
        },
        depth: mk.speck_depth,
        clip_radius: (6.0 * facts.scale).clamp(3.0, 24.0),
    }
}

impl Speck {
    pub fn write(&self, p: &mut Params) {
        p.speck_r = self.radius;
        p.speck_thr = self.depth;
        p.clip_r = self.clip_radius;
    }
}
