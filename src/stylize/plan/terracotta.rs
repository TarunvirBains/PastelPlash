//! Terracotta (in `finish`): a qualifying texture's earth hues turn toward a warm rose-sienna.
//! The gate is decided here, per texture, on the source (see `config::Terracotta`).

use crate::config::Style;
use crate::image::Image;
use crate::pipeline::FileContext;
use crate::stylize::params::Params;

pub(super) struct Terracotta {
    /// Turn amount, 0..1 (0 when the texture does not qualify).
    pub amount: f32,
    /// The texture's mean earth hue (degrees).
    pub mean_hue: f32,
    /// For the log line.
    pub note: String,
}

/// Engine-tinted textures carry no hue of their own: `tint_safe` never turns.
pub(super) fn plan(image: &Image, style: &Style, ctx: &FileContext, tint_safe: bool) -> Terracotta {
    let tc = &style.terracotta;
    let off = |note: &str| Terracotta {
        amount: 0.0,
        mean_hue: 0.0,
        note: note.to_string(),
    };
    if tc.strength <= 0.0
        || tint_safe
        || !ctx.category.may_turn_terracotta()
        || !ctx.config.pack.terracotta_allowed(ctx.rel)
    {
        return off("");
    }
    let src = ctx.source.unwrap_or(image);
    let earth = crate::analysis::earth_stats(src, tc.band, tc.min_chroma);
    let qualifies = earth.share >= tc.min_earth_share
        && (tc.value[0]..=tc.value[1]).contains(&earth.median_l)
        && earth.mean_hue <= tc.max_mean_hue;
    if !qualifies {
        return off("");
    }
    let grain = crate::analysis::grain(src);
    if grain > tc.max_grain {
        return off(&format!(" terracotta=grain({grain:.2})"));
    }
    Terracotta {
        amount: tc.strength,
        mean_hue: earth.mean_hue,
        note: format!(
            " terracotta={:.2} (earth {:.0}% L {:.2} h {:.0} grain {grain:.2})",
            tc.strength,
            earth.share * 100.0,
            earth.median_l,
            earth.mean_hue
        ),
    }
}

impl Terracotta {
    pub fn write(&self, p: &mut Params, style: &Style) {
        let tc = &style.terracotta;
        p.tc_amount = self.amount;
        p.tc_hue = tc.hue;
        p.tc_mean = self.mean_hue;
        p.tc_gather = tc.gather;
        p.tc_band0 = tc.band[0];
        p.tc_band1 = tc.band[1];
        p.tc_feather = tc.feather;
        p.tc_min_c = tc.min_chroma;
        p.tc_boost = tc.chroma_boost;
    }
}
