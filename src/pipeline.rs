//! The per-texture processing pipeline: an ordered list of stages applied to an [`Image`].

use std::path::Path;

use anyhow::{Context, Result};

use crate::config::{Category, Config, Mood};
use crate::image::Image;

/// What a stage knows about the file it is processing.
pub struct FileContext<'a> {
    /// Path relative to the input root.
    pub rel: &'a Path,
    pub category: Category,
    /// The mood the pack map (or an override) assigns to this file.
    pub mood: Mood,
    pub config: &'a Config,
    /// Texels of the image per texel of the source file: above 1 when the image was enlarged for
    /// an output resolution floor (paint marks keep their size relative to the source content).
    pub upscale: f32,
}

/// One processing step. Stages run on worker threads, so they must be `Send + Sync`; GPU stages
/// share one device and queue, which `wgpu` allows.
pub trait Stage: Send + Sync {
    fn name(&self) -> &str;
    fn apply(&self, image: &mut Image, ctx: &FileContext) -> Result<()>;
}

#[derive(Default)]
pub struct Pipeline {
    stages: Vec<Box<dyn Stage>>,
}

impl Pipeline {
    /// Builds the pipeline a config asks for. A neutral config gives the identity pipeline and
    /// never opens the GPU.
    pub fn from_config(config: &Config) -> Result<Self> {
        let mut pipeline = Self::default();
        if crate::stylize::wanted(config) {
            pipeline.push(crate::stylize::Stylize::new(config)?);
        }
        Ok(pipeline)
    }

    pub fn push(&mut self, stage: impl Stage + 'static) {
        self.stages.push(Box::new(stage));
    }

    pub fn run(&self, image: &mut Image, ctx: &FileContext) -> Result<()> {
        for stage in &self.stages {
            stage
                .apply(image, ctx)
                .with_context(|| format!("stage {}", stage.name()))?;
        }
        Ok(())
    }
}
