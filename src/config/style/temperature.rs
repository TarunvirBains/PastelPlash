//! `[temperature]`: warm/cool from residual shading.

use serde::Deserialize;

/// Warm/cool contrast: residual low-frequency shading becomes hue temperature (lit → warm,
/// shade → cool). Scaled per category by the target's `warm_cool` (0 for relit actors).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Temperature {
    /// OKLab chroma added at full temperature; 0 disables.
    pub chroma: f32,
    pub warm_hue: f32,
    pub cool_hue: f32,
    /// Temperature per stop of shading (log2 of blurred luminance vs. mean).
    pub sensitivity: f32,
}

impl Default for Temperature {
    fn default() -> Self {
        Self {
            chroma: 0.0,
            warm_hue: 70.0,
            cool_hue: 285.0,
            sensitivity: 1.5,
        }
    }
}

impl Temperature {
    pub fn validate(&self) -> anyhow::Result<()> {
        crate::config::non_negative("temperature.chroma", self.chroma)
    }
}
