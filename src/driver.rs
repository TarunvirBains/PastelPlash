//! The per-file flow every front end shares (a PNG folder, an archive adapter): classify a file
//! by the pack map (or an override), pick its mood, and run the pipeline. Front ends only read
//! and write files; they never decide what a file is.

use std::path::Path;

use anyhow::Result;

use crate::config::{Category, Config, Mood};
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
    /// (non-color maps, and categories that are not stylized).
    pub fn category(&self, rel: &Path) -> Option<Category> {
        if self.config.pack.is_non_color_map(rel) {
            return None;
        }
        let category = self
            .category
            .unwrap_or_else(|| self.config.pack.classify(rel));
        category.is_stylized().then_some(category)
    }

    /// The mood a file gets.
    pub fn mood(&self, rel: &Path) -> Mood {
        self.mood
            .clone()
            .unwrap_or_else(|| self.config.pack.mood_for(rel))
    }

    /// Runs the pipeline on a file's image.
    pub fn run(&self, image: &mut Image, rel: &Path, category: Category) -> Result<()> {
        let ctx = FileContext {
            rel,
            category,
            mood: self.mood(rel),
            config: self.config,
        };
        self.pipeline.run(image, &ctx)
    }
}
