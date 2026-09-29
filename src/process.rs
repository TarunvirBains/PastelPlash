//! Runs the pipeline over an input folder, mirroring it into the output folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use rayon::prelude::*;

use crate::config::{Category, Config, Mood};
use crate::pipeline::{FileContext, Pipeline};
use crate::png_io;
use crate::util::ms;
use crate::walk::{self, SkipReason, WalkOptions};

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub input: PathBuf,
    pub output: PathBuf,
    pub recursive: bool,
    pub follow_links: bool,
    /// Copy non-PNG files through verbatim.
    pub copy_other: bool,
    /// Worker threads; `None` uses all cores.
    pub jobs: Option<usize>,
    /// Category for every PNG, overriding the pack map (prototyping before classification).
    pub category: Option<Category>,
    /// Mood for every PNG, overriding the pack map's mood rules.
    pub mood: Option<Mood>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    /// PNGs run through the pipeline.
    pub processed: usize,
    /// Non-PNG files copied (`--copy-other`).
    pub copied: usize,
    /// PNGs copied through unchanged (non-color maps, `skip` category).
    pub skipped: usize,
    /// Files or folders that could not be read or written.
    pub failed: usize,
    /// Symlinks not followed (without `--follow-links`) or cut because they loop.
    pub links_ignored: usize,
    pub elapsed: Duration,
}

#[derive(Clone, Copy)]
enum Action {
    Process(Category),
    PassThrough,
    CopyOther,
}

pub fn run(opts: &Options, config: &Config, pipeline: &Pipeline) -> Result<Summary> {
    let start = Instant::now();
    if !opts.input.is_dir() {
        bail!("input {} is not a folder", opts.input.display());
    }
    fs::create_dir_all(&opts.output)
        .with_context(|| format!("creating {}", opts.output.display()))?;
    if fs::canonicalize(&opts.input)? == fs::canonicalize(&opts.output)? {
        bail!("output must differ from input");
    }

    let walk_opts = WalkOptions {
        recursive: opts.recursive,
        follow_links: opts.follow_links,
        exclude: Some(opts.output.clone()),
    };
    let walked = walk::walk(&opts.input, &walk_opts)
        .with_context(|| format!("reading {}", opts.input.display()))?;

    let mut summary = Summary {
        failed: walked.errors.len(),
        links_ignored: walked
            .skipped
            .iter()
            .filter(|(_, r)| matches!(r, SkipReason::Symlink | SkipReason::Loop))
            .count(),
        ..Summary::default()
    };
    for (rel, e) in &walked.errors {
        eprintln!("error: {}: {e}", opts.input.join(rel).display());
    }

    let jobs: Vec<(&Path, Action)> = walked
        .entries
        .iter()
        .filter_map(|entry| {
            let action = if !entry.is_png {
                opts.copy_other.then_some(Action::CopyOther)?
            } else if config.pack.is_non_color_map(&entry.rel) {
                Action::PassThrough
            } else {
                match opts
                    .category
                    .unwrap_or_else(|| config.pack.classify(&entry.rel))
                {
                    // UI is copied through until it gets its own treatment.
                    category if !category.is_stylized() => Action::PassThrough,
                    category => Action::Process(category),
                }
            };
            Some((entry.rel.as_path(), action))
        })
        .collect();

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(opts.jobs.unwrap_or(0))
        .build()?;
    let results: Vec<(Action, bool)> = pool.install(|| {
        jobs.par_iter()
            .map(|&(rel, action)| {
                let result = handle(opts, config, pipeline, rel, action);
                if let Err(e) = &result {
                    eprintln!("error: {e:#}");
                }
                (action, result.is_ok())
            })
            .collect()
    });

    for (action, ok) in results {
        let count = match action {
            _ if !ok => &mut summary.failed,
            Action::Process(_) => &mut summary.processed,
            Action::PassThrough => &mut summary.skipped,
            Action::CopyOther => &mut summary.copied,
        };
        *count += 1;
    }
    summary.elapsed = start.elapsed();
    Ok(summary)
}

fn handle(
    opts: &Options,
    config: &Config,
    pipeline: &Pipeline,
    rel: &Path,
    action: Action,
) -> Result<()> {
    let src = opts.input.join(rel);
    let dst = opts.output.join(rel);
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    match action {
        Action::Process(category) => {
            let t0 = Instant::now();
            let mut image = png_io::read(&src)?;
            let t_read = t0.elapsed();
            let ctx = FileContext {
                rel,
                category,
                mood: opts
                    .mood
                    .clone()
                    .unwrap_or_else(|| config.pack.mood_for(rel)),
                config,
            };
            pipeline
                .run(&mut image, &ctx)
                .with_context(|| format!("processing {}", src.display()))?;
            let t_run = t0.elapsed() - t_read;
            png_io::write(&image, &dst)?;
            let total = t0.elapsed();
            println!(
                "{}: {}x{} total {} (read {}, pipeline {}, write {})",
                rel.display(),
                image.width,
                image.height,
                ms(total),
                ms(t_read),
                ms(t_run),
                ms(total - t_read - t_run)
            );
            Ok(())
        }
        Action::PassThrough | Action::CopyOther => fs::copy(&src, &dst)
            .map(drop)
            .with_context(|| format!("copying {} to {}", src.display(), dst.display())),
    }
}
