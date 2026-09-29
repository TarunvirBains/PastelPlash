//! Watercolor finish: wet edges, granulation and paper (in `finish`).

use super::cells;
use crate::config::{Style, Treatment};
use crate::stylize::facts::ImageFacts;
use crate::stylize::params::Params;

pub(super) struct Watercolor {
    /// Wet-edge step in texels (also part of the filter reach).
    pub edge_step: f32,
    gran_px: f32,
    paper_px: f32,
    paper: f32,
    paper_tint: f32,
    edge_dark: f32,
    gran: f32,
}

pub(super) fn plan(style: &Style, tr: &Treatment, facts: &ImageFacts) -> Watercolor {
    let f = facts.scale;
    let wc = &style.watercolor;
    Watercolor {
        edge_step: (wc.edge_width * f).max(1.0),
        gran_px: (wc.granulation_scale * f).max(0.75),
        paper_px: (wc.paper_scale * f).max(0.75),
        paper: wc.paper_grain * tr.paper,
        paper_tint: wc.paper_tint * tr.paper,
        edge_dark: wc.edge_darkening * tr.wet_edges,
        gran: wc.granulation * tr.granulation,
    }
}

impl Watercolor {
    /// Granulation valley ring radius in texels (also part of the filter reach).
    pub fn gran_radius(&self) -> f32 {
        (self.gran_px * 0.75).max(1.0)
    }

    pub fn write(&self, p: &mut Params, style: &Style, facts: &ImageFacts) {
        let wc = &style.watercolor;
        p.seed = wc.seed;
        p.edge_dark = self.edge_dark;
        p.edge_step = self.edge_step;
        p.edge_rel = wc.edge_relative;
        p.edge_threshold = wc.edge_threshold;
        p.edge_feather = wc.edge_feather;
        p.gran = self.gran;
        p.gran_cells_x = cells(facts.w, self.gran_px);
        p.gran_cells_y = cells(facts.h, self.gran_px);
        p.gran_valley = wc.granulation_valley.clamp(0.0, 1.0);
        p.gran_radius = self.gran_radius();
        p.paper = self.paper;
        p.paper_tint = self.paper_tint;
        p.paper_cells_x = cells(facts.w, self.paper_px);
        p.paper_cells_y = cells(facts.h, self.paper_px);
        p.paper_hl = wc.paper_highlight;
        p.paper_r = wc.paper_color[0];
        p.paper_g = wc.paper_color[1];
        p.paper_b = wc.paper_color[2];
        p.floor_margin = wc.floor_margin;
    }
}
