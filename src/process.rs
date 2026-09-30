//! Runs the pipeline over an input folder, mirroring it into the output folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::config::{Category, Config, Mood};
use crate::driver::Driver;
use crate::pipeline::Pipeline;
use crate::png_io;
use crate::summary::{Failure, Phases};
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
    /// Why each of them failed, in path order.
    pub failures: Vec<Failure>,
    /// Symlinks not followed (without `--follow-links`) or cut because they loop.
    pub links_ignored: usize,
    pub elapsed: Duration,
    /// Time per phase, summed over workers (decoding counts as reading, encoding as writing).
    pub phases: Phases,
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
        links_ignored: walked
            .skipped
            .iter()
            .filter(|(_, r)| matches!(r, SkipReason::Symlink | SkipReason::Loop))
            .count(),
        ..Summary::default()
    };
    for (rel, e) in &walked.errors {
        eprintln!("error: {}: {e}", opts.input.join(rel).display());
        summary.failures.push(Failure {
            path: rel.display().to_string(),
            reason: e.to_string(),
        });
    }

    let driver = Driver {
        config,
        pipeline,
        category: opts.category,
        mood: opts.mood.clone(),
    };
    let mut jobs: Vec<(&Path, Action)> = walked
        .entries
        .iter()
        .filter_map(|entry| {
            let action = if !entry.is_png {
                opts.copy_other.then_some(Action::CopyOther)?
            } else {
                driver
                    .category(&entry.rel)
                    .map_or(Action::PassThrough, Action::Process)
            };
            Some((entry.rel.as_path(), action))
        })
        .collect();
    let (est, cost) = estimate(opts, &driver, &jobs);
    println!("{}", est.summary());
    crate::preflight::check_disk(&opts.output, est.bytes)?;
    // Costliest first (enlarged pre-rendered backgrounds), so small files fill the workers
    // around them instead of a tail of big ones at the end.
    let mut order: Vec<usize> = (0..jobs.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(cost[i].0));
    let weights: Vec<u64> = order.iter().map(|&i| cost[i].1).collect();
    jobs = order.iter().map(|&i| jobs[i]).collect();

    // Files are driven from plain threads, never rayon workers (see `util::map_on_threads`).
    let results: Vec<(Action, Result<Phases>)> = crate::util::map_on_threads_budgeted(
        &jobs,
        &weights,
        &crate::driver::MEMORY,
        opts.jobs.unwrap_or(0),
        || (),
        |(), &(rel, action)| {
            let result = handle(opts, &driver, rel, action);
            if let Err(e) = &result {
                eprintln!("error: {e:#}");
            }
            (action, result)
        },
    );

    for ((rel, _), (action, result)) in jobs.iter().zip(results) {
        let count = match (action, result) {
            (_, Err(e)) => {
                summary.failures.push(Failure {
                    path: rel.display().to_string(),
                    reason: format!("{e:#}"),
                });
                continue;
            }
            (action, Ok(phases)) => {
                summary.phases += phases;
                match action {
                    Action::Process(_) => &mut summary.processed,
                    Action::PassThrough => &mut summary.skipped,
                    Action::CopyOther => &mut summary.copied,
                }
            }
        };
        *count += 1;
    }
    summary.failures.sort_by(|a, b| a.path.cmp(&b.path));
    summary.failed = summary.failures.len();
    summary.elapsed = start.elapsed();
    Ok(summary)
}

fn handle(opts: &Options, driver: &Driver, rel: &Path, action: Action) -> Result<Phases> {
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
            driver
                .run(&mut image, rel, category)
                .with_context(|| format!("processing {}", src.display()))?;
            let t_run = t0.elapsed() - t_read;
            png_io::write(&image, &dst)?;
            let total = t0.elapsed();
            crate::log::detail!(
                "{}: {}x{} total {} (read {}, pipeline {}, write {})",
                rel.display(),
                image.width,
                image.height,
                ms(total),
                ms(t_read),
                ms(t_run),
                ms(total - t_read - t_run)
            );
            Ok(Phases {
                read: t_read,
                pipeline: t_run,
                write: total - t_read - t_run,
                ..Phases::default()
            })
        }
        Action::PassThrough | Action::CopyOther => {
            let t0 = Instant::now();
            fs::copy(&src, &dst)
                .with_context(|| format!("copying {} to {}", src.display(), dst.display()))?;
            Ok(Phases {
                write: t0.elapsed(),
                ..Phases::default()
            })
        }
    }
}

/// What a run will write: sources at their size, enlarged PNGs by the square of their factor
/// (compressed size scales about with the texel count). Also, per job, the texels the pipeline
/// paints (its cost) and those it holds of the memory budget ([`Driver::texels_held`]).
fn estimate(
    opts: &Options,
    driver: &Driver,
    jobs: &[(&Path, Action)],
) -> (crate::preflight::Estimate, Vec<(u64, u64)>) {
    let mut est = crate::preflight::Estimate::default();
    let mut cost = Vec::with_capacity(jobs.len());
    for &(rel, action) in jobs {
        let src = opts.input.join(rel);
        let len = fs::metadata(&src).map_or(0, |m| m.len());
        match (action, png_io::dimensions(&src)) {
            (Action::Process(c), Some((w, h))) => {
                let (k, internal) = driver.resolution_plan(c, w, h);
                let texels = u64::from(w) * u64::from(h) * u64::from(internal * internal);
                est.add(len * u64::from(k * k), k, texels);
                cost.push((texels, driver.texels_held(c, w, h)));
            }
            _ => {
                est.add(len, 1, 0);
                cost.push((0, 0));
            }
        }
    }
    (est, cost)
}
