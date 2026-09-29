//! `[strokes]`: directional brushstrokes.

use serde::Deserialize;

/// Directional brushstrokes: noise integrated along the structure-tensor flow (LIC), modulating
/// lightness and chroma slightly. Never moves edges.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Strokes {
    /// Lightness modulation amplitude (OKLab L); 0 disables.
    pub strength: f32,
    /// Relative chroma modulation.
    pub chroma: f32,
    /// Bristle/stroke width in reference texels.
    pub width: f32,
    /// Stroke half-length in reference texels.
    pub length: f32,
    /// 0..1: how far the painted wash is pulled toward the source color averaged along the
    /// flow, bringing detail back as streaks that follow form (not across edges).
    pub smear: f32,
}

impl Default for Strokes {
    fn default() -> Self {
        Self {
            strength: 0.0,
            chroma: 0.15,
            width: 2.0,
            length: 10.0,
            smear: 0.0,
        }
    }
}

impl Strokes {
    pub fn validate(&self) -> anyhow::Result<()> {
        crate::config::non_negative("strokes.strength", self.strength)
    }
}
