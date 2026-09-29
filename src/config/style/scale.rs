//! `[scale]`: how style sizes become texels of a given image.

use serde::Deserialize;

/// How style sizes (in "reference texels") become texels of a given image.
///
/// When the image's **source scale** (HD texels per original texel, from an adapter, the pack
/// map or [`Scale::source_scale`]) is known, the factor is `source_scale /
/// reference_source_scale`, so brush sizes stay consistent in world space. Otherwise it falls
/// back to image size: `(sqrt(w·h) / reference_size) ^ exponent`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Scale {
    /// Geometric-mean side length at which the style's texel sizes apply as written.
    pub reference_size: f32,
    /// Size-relative fallback exponent; 1 = fully proportional.
    pub exponent: f32,
    /// Source scale at which the style's texel sizes apply as written.
    pub reference_source_scale: f32,
    /// Default source scale for images whose adapter and pack map give none.
    pub source_scale: Option<f32>,
}

impl Default for Scale {
    fn default() -> Self {
        Self {
            reference_size: 1024.0,
            exponent: 1.0,
            reference_source_scale: 16.0,
            source_scale: None,
        }
    }
}

impl Scale {
    pub fn validate(&self) -> anyhow::Result<()> {
        crate::config::check(self.reference_size > 0.0, || {
            "scale.reference_size must be > 0".into()
        })
    }
}
