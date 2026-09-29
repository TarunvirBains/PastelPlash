//! `[grouping]`: soft value grouping (notan).

use serde::Deserialize;

use anyhow::Result;

use crate::config::{check, unit};

/// Soft value grouping (notan) for busy, photographic world textures (the same trigger as
/// [`Contrast`]): the texture's 2–4 value masses are found (1D k-means on an edge-aware smoothed
/// lightness, never a plain blur), then each texel's lightness is pulled toward its soft-assigned
/// mass value and its color modestly toward the mass color within its own hue family. Masses keep
/// their separation; texels far from every mass (small salient objects) keep their value.
/// Brushwork then varies hue and chroma within each mass more than value. Never applied to actors
/// (the cel shader bands them) or UI; textures that are already graphic (two masses explain
/// nearly all their variance: lettering, flat art) are skipped.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Grouping {
    /// 0..1 pull toward the mass value on fully busy textures; 0 disables.
    pub strength: f32,
    /// Most masses per texture (2..=4).
    pub max_masses: u32,
    /// The smallest mass count whose masses explain this share of the lightness variance wins.
    pub explained: f32,
    /// Masses closer than this (OKLab L) merge.
    pub min_gap: f32,
    /// Masses holding less than this share of the texture merge into their neighbor.
    pub min_share: f32,
    /// Already-graphic textures (two masses explain at least this share: lettering, flat art)
    /// are left alone.
    pub skip_explained: f32,
    /// Edge-aware smoothing radius (fraction of the texture's size) and its lightness range.
    pub radius: f32,
    pub range: f32,
    /// Soft-assignment width as a fraction of half the smallest gap between masses (smaller =
    /// crisper mass boundaries; never a hard posterization).
    pub softness: f32,
    /// 0..1 pull of a texel's color (OKLab a/b) toward its mass color.
    pub color: f32,
    /// OKLab a/b distance beyond which a texel is a differently colored object and keeps its color.
    pub color_family: f32,
    /// Distance (OKLab L) from the nearest mass over which a texel counts as a salient object and
    /// keeps its value (fade range).
    pub salient: [f32; 2],
    /// Brushstroke lightness amplitude inside grouped textures (× the style's), so variation
    /// within a mass is mostly hue and chroma.
    pub stroke_value: f32,
}

impl Default for Grouping {
    fn default() -> Self {
        Self {
            strength: 0.0,
            max_masses: 4,
            explained: 0.85,
            min_gap: 0.08,
            min_share: 0.03,
            skip_explained: 0.96,
            radius: 0.008,
            range: 0.06,
            softness: 0.5,
            color: 0.4,
            color_family: 0.08,
            salient: [0.16, 0.26],
            stroke_value: 0.5,
        }
    }
}

impl Grouping {
    pub fn validate(&self) -> Result<()> {
        let g = self;
        unit("grouping.strength", g.strength)?;
        unit("grouping.color", g.color)?;
        unit("grouping.explained", g.explained)?;
        unit("grouping.skip_explained", g.skip_explained)?;
        check((2..=4).contains(&g.max_masses), || {
            format!(
                "grouping.max_masses = {} must be within 2..=4",
                g.max_masses
            )
        })?;
        check(g.softness > 0.0 && g.range > 0.0 && g.radius >= 0.0, || {
            "grouping.softness and grouping.range must be > 0, grouping.radius >= 0".into()
        })?;
        check(g.salient[0] < g.salient[1], || {
            "grouping.salient must be an increasing range".into()
        })?;
        Ok(())
    }
}
