//! `[value_contrast]` and `[contrast]`: value compression, fixed and adaptive.

use serde::Deserialize;

/// Local value-contrast compression: each texel's lightness is pulled toward edge-aware local
/// means at three scales, so a surface sits in a narrower lightness range (as in Skyward Sword)
/// and detail moves from light/dark into hue and brushwork. Scaled per category by the target's
/// `value_contrast`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ValueContrast {
    /// 0..1 compression of deviations from the fine-scale mean (grime, pores, grain).
    pub fine: f32,
    /// 0..1 compression of fine-vs-mid means (small shapes).
    pub mid: f32,
    /// 0..1 compression of mid-vs-coarse means (large shapes; keep low so blocks vs. mortar stay
    /// readable).
    pub coarse: f32,
    /// Chroma gain per unit of lightness removed, so detail migrates into color.
    pub chroma: f32,
    /// Radii of the three scales in reference texels.
    pub radius_fine: f32,
    pub radius_mid: f32,
    pub radius_coarse: f32,
    /// Lightness difference (OKLab L) beyond which a neighbor counts as across an edge.
    pub range: f32,
}

impl Default for ValueContrast {
    fn default() -> Self {
        Self {
            fine: 0.0,
            mid: 0.0,
            coarse: 0.0,
            chroma: 0.0,
            radius_fine: 3.0,
            radius_mid: 12.0,
            radius_coarse: 40.0,
            range: 0.12,
        }
    }
}

/// Adaptive contrast for busy, high-contrast textures (bark, cliffs): the texture's own
/// value spread (median OKLab L std over mid-scale windows) is measured, and if it is above
/// `trigger_spread` the value compression is raised at every scale — including the groove scale —
/// toward `target_spread` (the spread measured on the reference game's comparable surfaces).
/// Groove shapes stay; their light/dark amplitude shrinks. Textures below the trigger (the
/// ground) are untouched by this.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Contrast {
    /// 0..1: how far toward the target; 0 disables.
    pub strength: f32,
    /// Goal spread (median L std over mid-scale windows).
    pub target_spread: f32,
    /// Only textures whose spread exceeds this (feathered ±15%) adapt.
    pub trigger_spread: f32,
    /// Scale (fraction of the texture's size) above which the light/dark pattern is kept:
    /// variation finer than this (grooves, grit) is compressed, coarser (lichen patches, large
    /// lighting shapes) stays.
    pub pattern_radius: f32,
}

impl Default for Contrast {
    fn default() -> Self {
        Self {
            strength: 0.0,
            target_spread: 0.03,
            pattern_radius: 0.04,
            trigger_spread: 0.08,
        }
    }
}
