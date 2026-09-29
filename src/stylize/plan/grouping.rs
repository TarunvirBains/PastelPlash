//! Soft value grouping (notan): the value masses (CPU) and the `group` pass.

use super::delight::Delight;
use crate::analysis::Lowres;
use crate::config::{Style, Treatment};
use crate::grouping::{self, Masses};
use crate::image::Image;
use crate::pipeline::FileContext;
use crate::stylize::facts::ImageFacts;
use crate::stylize::params::Params;

/// Whether the file may be grouped: world and background textures only (never actors, which
/// the cel shader bands, nor UI), unless the pack map opts the file out.
pub(super) fn on(style: &Style, tr: &Treatment, ctx: &FileContext) -> bool {
    style.grouping.strength > 0.0
        && tr.grouping > 0.0
        && ctx.category.may_group()
        && ctx.config.pack.grouping_allowed(ctx.rel)
}

pub(super) struct Grouping {
    /// Pull toward the masses, 0..1 (0 when skipped).
    pub amount: f32,
    masses: Option<Masses>,
    sigma: f32,
    /// Edge-aware smoothing radius in texels.
    pub radius: f32,
    /// For the log line.
    pub note: String,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn plan(
    image: &Image,
    style: &Style,
    tr: &Treatment,
    facts: &ImageFacts,
    on: bool,
    gate: f32,
    lowres: Option<&Lowres>,
    delight: &Delight,
) -> Grouping {
    let gr = &style.grouping;
    let mut grp = if on {
        (gr.strength * gate * tr.grouping).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut note = String::new();
    let masses = if grp > 0.0 {
        let field = delight.field(lowres);
        match grouping::value_masses(image, field.as_ref(), facts.wrap, gr) {
            Ok(m) => {
                let lch: Vec<String> = (0..m.count)
                    .map(|k| {
                        let [a, b] = m.ab[k];
                        format!(
                            "{:.2}/{:.3}/{:.0}@{:.0}%",
                            m.l[k],
                            a.hypot(b),
                            b.atan2(a).to_degrees().rem_euclid(360.0),
                            m.share[k] * 100.0
                        )
                    })
                    .collect();
                note = format!(
                    " grp={grp:.2} masses={} [{}] expl={:.2}/{:.2}",
                    m.count,
                    lch.join(" "),
                    m.explained,
                    m.explained2
                );
                Some(m)
            }
            Err(skip) => {
                note = format!(" grp=skip({skip:?})");
                grp = 0.0;
                None
            }
        }
    } else {
        None
    };
    let sigma = masses.as_ref().map_or(0.0, |m| {
        let gap = (0..m.count - 1)
            .map(|i| m.l[i + 1] - m.l[i])
            .fold(f32::MAX, f32::min);
        (gr.softness * 0.5 * gap).max(1e-3)
    });
    Grouping {
        amount: grp,
        masses,
        sigma,
        radius: (gr.radius * facts.gm).max(facts.upscale),
        note,
    }
}

impl Grouping {
    pub fn write(&self, p: &mut Params, style: &Style) {
        let gr = &style.grouping;
        let m = self.masses.as_ref();
        p.grp = self.amount;
        p.grp_sigma = self.sigma;
        p.grp_radius = self.radius;
        p.grp_range = gr.range;
        p.grp_color = gr.color;
        p.grp_family = gr.color_family;
        p.grp_stroke = gr.stroke_value;
        p.grp_sal0 = gr.salient[0];
        p.grp_sal1 = gr.salient[1];
        p.grp_count = m.map_or(0.0, |m| m.count as f32);
        p.grp_l = m.map_or([0.0; 4], |m| m.l);
        p.grp_a = m.map_or([0.0; 4], |m| m.ab.map(|v| v[0]));
        p.grp_b = m.map_or([0.0; 4], |m| m.ab.map(|v| v[1]));
    }
}
