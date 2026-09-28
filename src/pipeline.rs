//! The per-texture processing pipeline: an ordered list of stages applied to an [`Image`].

use std::path::Path;

use anyhow::{Context, Result};

use crate::config::{Category, Config};
use crate::image::Image;

/// What a stage knows about the file it is processing.
pub struct FileContext<'a> {
    /// Path relative to the input root.
    pub rel: &'a Path,
    pub category: Category,
    pub config: &'a Config,
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
    /// Builds the pipeline a config asks for. No stages exist yet, so this is the identity.
    pub fn from_config(_config: &Config) -> Result<Self> {
        Ok(Self::default())
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
