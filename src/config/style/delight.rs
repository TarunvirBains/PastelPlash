//! `[delight]`: removal of baked shading.

use serde::Deserialize;

/// Removal of baked low-frequency shading/AO: linear color is divided by a large-radius blurred
/// luminance (relative to the image mean), raised to `strength × target delight`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Delight {
    /// Global multiplier on the target's per-category `delight`; 0 disables.
    pub strength: f32,
    /// Blur sigma as a fraction of the image's geometric-mean side.
    pub radius: f32,
    pub min_gain: f32,
    pub max_gain: f32,
}

impl Default for Delight {
    fn default() -> Self {
        Self {
            strength: 0.0,
            radius: 0.06,
            min_gain: 0.6,
            max_gain: 2.0,
        }
    }
}
