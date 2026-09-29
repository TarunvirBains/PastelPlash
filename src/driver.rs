//! The per-file flow every front end shares (a PNG folder, an archive adapter): classify a file
//! by the pack map (or an override), pick its mood, and run the pipeline. Front ends only read
//! and write files; they never decide what a file is.

use std::path::Path;

use anyhow::Result;

use crate::config::{Category, Config, FluidRuleKind, Mood};
use crate::fluid::FluidKind;
use crate::image::Image;
use crate::pipeline::{FileContext, Pipeline};

pub struct Driver<'a> {
    pub config: &'a Config,
    pub pipeline: &'a Pipeline,
    /// Category for every file, overriding the pack map.
    pub category: Option<Category>,
    /// Mood for every file, overriding the pack map's mood rules.
    pub mood: Option<Mood>,
}

impl Driver<'_> {
    /// The category a file is restyled as, or `None` if it is copied through untouched
    /// (non-color maps, and categories that are not stylized). Fluids are decided later, from
    /// the pixels ([`Driver::material`]).
    pub fn category(&self, rel: &Path) -> Option<Category> {
        if self.config.pack.is_non_color_map(rel) {
            return None;
        }
        let category = self
            .category
            .unwrap_or_else(|| self.config.pack.classify(rel));
        category.is_stylized().then_some(category)
    }

    /// The material a stylized file is restyled as: a pack-map fluid rule decides first; else,
    /// for categories that may be fluids (world geometry), the fluid detector; else the file's
    /// category. A CLI category override is taken as is.
    pub fn material(&self, image: &Image, rel: &Path, category: Category) -> Category {
        if self.category.is_some() {
            return category;
        }
        let pack = &self.config.pack;
        let kind = match pack.fluid_rule_for(rel).and_then(|r| r.kind) {
            Some(FluidRuleKind::Water) => Some(FluidKind::Water),
            Some(FluidRuleKind::Lava) => Some(FluidKind::Lava),
            Some(FluidRuleKind::Liquid) => Some(FluidKind::Liquid),
            Some(FluidRuleKind::None) => None,
            None if pack.detect_fluids && category.may_be_detected_fluid() => {
                crate::fluid::detect(image).kind
            }
            None => None,
        };
        match kind {
            Some(FluidKind::Water) => Category::Water,
            Some(FluidKind::Lava) => Category::Lava,
            Some(FluidKind::Liquid) => Category::Liquid,
            None => category,
        }
    }

    /// The mood a file gets (with its area's reference water tone, if a fluid rule sets one).
    pub fn mood(&self, rel: &Path) -> Mood {
        let mut mood = self
            .mood
            .clone()
            .unwrap_or_else(|| self.config.pack.mood_for(rel));
        if let Some(r) = self.config.pack.fluid_rule_for(rel) {
            mood.water_hue = r.water_hue;
            mood.water_chroma = r.water_chroma;
            mood.water_pull = r.water_pull;
            mood.water_lightness = r.water_lightness;
        }
        mood
    }

    /// Runs the pipeline on a file's image (as its material: see [`Driver::material`]).
    pub fn run(&self, image: &mut Image, rel: &Path, category: Category) -> Result<()> {
        let category = self.material(image, rel, category);
        let ctx = FileContext {
            rel,
            category,
            mood: self.mood(rel),
            config: self.config,
        };
        self.pipeline.run(image, &ctx)
    }
}
