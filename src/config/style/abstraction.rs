//! `[abstraction]`: design-like simplification of busy textures.

use serde::Deserialize;

/// Design-like abstraction for busy, photographic textures (the same trigger as [`Contrast`]):
/// a large-radius Kuwahara pass first simplifies the texture into big shapes; brushstrokes then
/// follow the coarse structure and paint the simplified wash instead of the photo; wet edges only
/// outline large regions; bright grayish glare calms into the surface color; texels keep their
/// source chroma. Shape-based textures below the trigger (vines, ground) are unaffected.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Abstraction {
    /// 0..1 overall amount on fully busy textures; 0 disables.
    pub strength: f32,
    /// Radius of the large-scale Kuwahara pass, in reference texels.
    pub radius: f32,
    /// Structure-tensor smoothing multiplier (strokes follow the coarse structure).
    pub flow_scale: f32,
    /// Stroke length multiplier.
    pub stroke_scale: f32,
    /// Coarse step for large-region edges, as a multiple of the wet-edge width.
    pub edge_scale: f32,
    /// 0..1 reduction of the remaining wet edges.
    pub edge_soften: f32,
    /// 0..1 calming of bright grayish highlight patches, and their neighborhood radius
    /// (reference texels).
    pub highlight_calm: f32,
    pub highlight_radius: f32,
    /// 0..1: output chroma kept at least this share of the source texel's chroma.
    pub chroma_retain: f32,
    /// Lower bound for the abstraction radius as a fraction of the texture's size (highlight
    /// radius: twice this), so low-resolution textures stretched over big surfaces still get
    /// big shapes.
    pub min_frac: f32,
}

impl Default for Abstraction {
    fn default() -> Self {
        Self {
            min_frac: 0.03,
            strength: 0.0,
            radius: 20.0,
            flow_scale: 3.0,
            stroke_scale: 1.8,
            edge_scale: 4.0,
            edge_soften: 0.5,
            highlight_calm: 0.8,
            highlight_radius: 12.0,
            chroma_retain: 0.9,
        }
    }
}
