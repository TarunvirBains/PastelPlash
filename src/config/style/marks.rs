//! `[marks]`: paint-mark size.

use serde::Deserialize;

/// Paint marks: the size of the soft dabs/blotches that fine color variation is simplified into.
///
/// Seamlessly tiling textures repeat many times across a surface, which shrinks their
/// texture-space marks in world space, so they get `tiling_multiplier` × the size — but only as
/// far as they are *speckled* (fine-scale lightness variation large relative to mid-scale, the
/// signature of photographic grit), so shape-based art (vines, leaves) keeps its shapes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Marks {
    /// Mark radius in reference texels; replaces `kuwahara.radius` when set.
    pub size: Option<f32>,
    /// Mark-size multiplier for tiling, speckled textures (1 = off).
    pub tiling_multiplier: f32,
    /// Speckle ratio (fine / mid-scale L std) range over which the multiplier fades in.
    pub speckle: [f32; 2],
}

impl Default for Marks {
    fn default() -> Self {
        Self {
            size: None,
            tiling_multiplier: 1.0,
            speckle: [0.45, 0.6],
        }
    }
}
