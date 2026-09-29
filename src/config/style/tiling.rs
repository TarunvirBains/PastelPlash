//! `[tiling]`: seamless-tiling detection.

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tiling {
    /// An axis wraps when its seam discontinuity is at most this multiple of the average
    /// neighbor difference inside the image.
    pub threshold: f32,
}

impl Default for Tiling {
    fn default() -> Self {
        Self { threshold: 2.5 }
    }
}
