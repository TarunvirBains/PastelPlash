//! `[watercolor]`: the watercolor finish.

use serde::Deserialize;

/// Watercolor finish; every strength defaults to 0 (off).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Watercolor {
    /// Wet-edge darkening: maximum OKLab L drop where pigment pools on the darker side of a
    /// painted boundary; 0 disables.
    pub edge_darkening: f32,
    /// Rim darkening relative to the wash depth (`1 - L`), capped by `edge_darkening`.
    pub edge_relative: f32,
    /// Lightness step across a boundary at which rims start; higher rims fewer boundaries.
    pub edge_threshold: f32,
    /// Lightening on the lighter side of rimmed boundaries (OKLab L).
    pub edge_feather: f32,
    /// Rim half-width in reference texels.
    pub edge_width: f32,
    /// Color bleeding across soft boundaries (edge-aware; strong edges block it).
    pub bleed: f32,
    /// Bleed reach in texels at the reference size.
    pub bleed_radius: f32,
    /// OKLab distance at which a boundary stops the bleed.
    pub bleed_range: f32,
    /// Pigment granulation amplitude (OKLab L at full wash depth); 0 disables.
    pub granulation: f32,
    /// Granulation noise cell size in texels at the reference size.
    pub granulation_scale: f32,
    /// Share of granulation that settles in the source texture's own valleys (vs. noise).
    pub granulation_valley: f32,
    /// Paper tooth amplitude (OKLab L); 0 disables.
    pub paper_grain: f32,
    /// How far highlight colors mix toward the paper color (0..1 at full highlight); keep it
    /// small, or light colors turn chalky.
    pub paper_tint: f32,
    /// Paper texture cell size in texels at the reference size.
    pub paper_scale: f32,
    /// OKLab lightness above which the paper starts to show.
    pub paper_highlight: f32,
    /// Paper color, gamma sRGB.
    pub paper_color: [f32; 3],
    /// How far watercolor darkening may undercut the palette's lightness floor (OKLab L).
    pub floor_margin: f32,
    pub seed: u32,
}

impl Default for Watercolor {
    fn default() -> Self {
        Self {
            edge_darkening: 0.0,
            edge_relative: 0.5,
            edge_threshold: 0.02,
            edge_feather: 0.0,
            edge_width: 1.5,
            bleed: 0.0,
            bleed_radius: 4.0,
            bleed_range: 0.06,
            granulation: 0.0,
            granulation_scale: 3.0,
            granulation_valley: 0.6,
            paper_grain: 0.0,
            paper_tint: 0.0,
            paper_scale: 1.5,
            paper_highlight: 0.75,
            paper_color: [0.98, 0.965, 0.93],
            floor_margin: 0.06,
            seed: 0,
        }
    }
}
