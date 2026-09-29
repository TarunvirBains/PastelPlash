//! `[terracotta]`: a warm rose-sienna on earth browns (Skyward Sword's Faron Woods), per texture.

use anyhow::Result;
use serde::Deserialize;

use crate::config::{check, non_negative, unit};

/// Terracotta: the earth hues of a qualifying texture turn toward a warm rose-sienna.
///
/// Per texture, not per color: a texture qualifies (the gate) when it is mostly earth (at least
/// `min_earth_share` of its opaque texels colored at least `min_chroma` with a hue in `band`),
/// mid-value (the earth texels' median lightness in `value`: not light sand), with a mean earth
/// hue at most `max_mean_hue` (brown, not olive or khaki) and without grain (`analysis::grain` at
/// most `max_grain`: bark, planks and wood floors keep their color). Only world textures qualify
/// (`Category::may_turn_terracotta`), and the pack map may opt areas in or out. In a qualifying
/// texture, each earth texel first gathers toward the texture's mean earth hue (`gather`), then
/// the whole family turns so its mean lands on `hue`, with a little more chroma (`chroma_boost`).
/// Off (`strength` 0) by default.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Terracotta {
    /// 0 = off, 1 = the family's mean hue lands on `hue`.
    pub strength: f32,
    /// Target OKLCH hue (degrees).
    pub hue: f32,
    /// Earth hues (degrees) that turn, feathered by `feather` inside the band's ends.
    pub band: [f32; 2],
    pub feather: f32,
    /// Texels below this chroma are neutral: they don't count as earth and don't turn.
    pub min_chroma: f32,
    /// How far each earth hue first moves toward the texture's mean earth hue (0..1).
    pub gather: f32,
    /// Relative chroma gain of turned texels.
    pub chroma_boost: f32,
    /// Gate: the least share of earth texels.
    pub min_earth_share: f32,
    /// Gate: the earth texels' median lightness range.
    pub value: [f32; 2],
    /// Gate: the highest mean earth hue (olive and khaki stay).
    pub max_mean_hue: f32,
    /// Gate: the most grain (directional structure) a texture may have.
    pub max_grain: f32,
}

impl Default for Terracotta {
    fn default() -> Self {
        Self {
            strength: 0.0,
            hue: 45.0,
            band: [30.0, 95.0],
            feather: 10.0,
            min_chroma: 0.03,
            gather: 0.5,
            chroma_boost: 0.2,
            min_earth_share: 0.6,
            value: [0.3, 0.62],
            max_mean_hue: 82.0,
            max_grain: 0.5,
        }
    }
}

impl Terracotta {
    pub fn validate(&self) -> Result<()> {
        unit("terracotta.strength", self.strength)?;
        unit("terracotta.gather", self.gather)?;
        unit("terracotta.min_earth_share", self.min_earth_share)?;
        unit("terracotta.max_grain", self.max_grain)?;
        non_negative("terracotta.chroma_boost", self.chroma_boost)?;
        non_negative("terracotta.feather", self.feather)?;
        non_negative("terracotta.min_chroma", self.min_chroma)?;
        check(
            self.band[0] < self.band[1] && self.band[1] - self.band[0] > 2.0 * self.feather,
            || {
                format!(
                    "terracotta.band = {:?} must be increasing and wider than twice the feather",
                    self.band
                )
            },
        )?;
        check(self.value[0] < self.value[1], || {
            format!("terracotta.value = {:?} must be increasing", self.value)
        })
    }
}
