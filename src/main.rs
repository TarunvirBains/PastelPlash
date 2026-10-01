use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use pastelplash::config::{Category, Config, Mood};
use pastelplash::pipeline::Pipeline;
use pastelplash::process;
use pastelplash::summary::{Report, Timings};

#[derive(Parser)]
#[command(
    name = "pastelplash",
    bin_name = "pastelplash",
    version,
    about = "Restyle PNG texture packs into a pastel, watercolor look"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Process a folder of PNGs into an output folder mirroring its layout.
    Process(ProcessArgs),
    /// Show the GPU adapter and run a compute self-test.
    GpuInfo,
    /// Restyle the textures of a `.o2r` pack (Ship of Harkinian) into a new `.o2r` mod.
    O2r(O2rArgs),
    /// Export textures from a `.o2r` pack as PNGs, keeping their archive paths.
    O2rExport(O2rExportArgs),
    /// Per hue group OKLCH statistics of a folder of PNGs next to a reference palette.
    PaletteReport(PaletteReportArgs),
    /// Bake a style's palette into a `.cube` 3D LUT.
    BakeLut(BakeLutArgs),
    /// Write downsized before/after images and 1:1 crops for visual comparison.
    #[command(hide = true)]
    DevCompare(DevCompareArgs),
    /// Write a contact sheet of every PNG under a folder.
    #[command(hide = true)]
    DevSheet(DevSheetArgs),
    /// Per-texture metrics of processed PNGs against their sources (and a baseline render).
    #[command(hide = true)]
    DevMetrics(DevMetricsArgs),
    /// A comparison grid: the same files from several folders side by side.
    #[command(hide = true)]
    DevGrid(DevGridArgs),
    /// OKLCH statistics of a PNG (or a rectangle of it).
    #[command(hide = true)]
    DevStats(DevStatsArgs),
    /// The palette mapping of OKLCH colors (L,C,h triples) for a style, mood and category.
    #[command(hide = true)]
    DevMap(DevMapArgs),
    /// Which textures changed between two render folders, by area, flagging changes outside a
    /// declared scope.
    #[command(hide = true)]
    DevScope(DevScopeArgs),
    /// Fluid detection scores of every texture of a `.o2r` pack (or PNG folder), as a TSV.
    #[command(hide = true)]
    DevFluidScan(DevFluidScanArgs),
    /// A numbered contact sheet of listed textures (first TSV column: path under a folder).
    #[command(hide = true)]
    DevFluidSheet(DevFluidSheetArgs),
    /// A rectangle of a PNG, as a PNG.
    #[command(hide = true)]
    DevCrop(DevCropArgs),
}

#[derive(Args)]
struct DevCropArgs {
    input: PathBuf,
    output: PathBuf,
    x: u32,
    y: u32,
    /// Square side.
    size: u32,
    /// Composite over this sRGB gray (0..1) at `--alpha`, like a translucent surface over a bed.
    #[arg(long)]
    over: Option<f32>,
    /// Opacity of the texture when composited with `--over`.
    #[arg(long, default_value_t = 1.0)]
    alpha: f32,
}

#[derive(Args)]
struct DevFluidScanArgs {
    /// `.o2r` pack (only read) or a folder of PNGs.
    input: PathBuf,
    /// Output TSV.
    output: PathBuf,
    /// Save the analysis thumbnails here (pack input only).
    #[arg(long, value_name = "DIR")]
    thumbs: Option<PathBuf>,
    #[arg(long, value_name = "GLOB")]
    include: Vec<String>,
    /// Pack map, for the category column.
    #[arg(long, value_name = "FILE")]
    pack: Option<PathBuf>,
    #[arg(short, long, default_value_t = 12)]
    jobs: usize,
}

#[derive(Args)]
struct DevFluidSheetArgs {
    /// Folder of PNGs (paths in the list are relative to it, without `.png`).
    dir: PathBuf,
    /// List (TSV; first column is the path; `#` lines and a `path` header are skipped).
    list: PathBuf,
    /// Output PNG.
    output: PathBuf,
    #[arg(long, default_value_t = 160)]
    thumb: u32,
    #[arg(long, default_value_t = 10)]
    cols: u32,
    /// Number of the first tile.
    #[arg(long, default_value_t = 0)]
    first: usize,
    /// Frame color r,g,b (0..1).
    #[arg(long, value_delimiter = ',', default_values_t = [0.5, 0.5, 0.5])]
    frame: Vec<f32>,
}

#[derive(Args)]
struct DevScopeArgs {
    /// Folder of the earlier render.
    before: PathBuf,
    /// Folder of the new render (same relative paths).
    after: PathBuf,
    /// The change's intended scope: path globs (repeatable; none = nothing should change).
    #[arg(long = "scope", value_name = "GLOB")]
    scope: Vec<String>,
}

#[derive(Args)]
struct DevMapArgs {
    #[arg(long, value_name = "FILE|NAME")]
    style: Option<PathBuf>,
    #[arg(long, value_name = "FILE")]
    target: Option<PathBuf>,
    #[arg(long, default_value = "world")]
    category: Category,
    #[arg(long, default_value = "base")]
    mood: Mood,
    /// Colors as L,C,h (OKLCH).
    colors: Vec<String>,
}

#[derive(Args)]
struct DevGridArgs {
    /// Output PNG.
    output: PathBuf,
    /// Folders, one column each (the first decides crop positions).
    #[arg(long = "col", value_name = "DIR", required = true)]
    cols: Vec<PathBuf>,
    /// File listing one relative path per row (blank lines and `#` comments ignored).
    #[arg(long, value_name = "FILE")]
    rows: PathBuf,
    #[arg(long, default_value_t = 256)]
    thumb: u32,
    /// Show a 1:1 crop of this size instead of the whole image.
    #[arg(long)]
    crop: Option<u32>,
    /// Paths per sheet row (each a block of one cell per folder).
    #[arg(long, default_value_t = 1)]
    wrap: u32,
}

#[derive(Args)]
struct DevStatsArgs {
    /// PNGs.
    inputs: Vec<PathBuf>,
    /// Rectangle x,y,w,h.
    #[arg(long, value_delimiter = ',', num_args = 4)]
    rect: Option<Vec<u32>>,
}

#[derive(Args)]
struct DevMetricsArgs {
    /// Folder of source PNGs.
    source: PathBuf,
    /// Folder of processed PNGs (same relative paths).
    output: PathBuf,
    /// Folder of an earlier render to compare against (per-texel ΔE).
    #[arg(long, value_name = "DIR")]
    baseline: Option<PathBuf>,
}

#[derive(Args)]
struct DevSheetArgs {
    /// Folder of PNGs (searched recursively).
    input: PathBuf,
    /// Output PNG.
    output: PathBuf,
    #[arg(long, default_value_t = 160)]
    thumb: u32,
    #[arg(long, default_value_t = 8)]
    cols: u32,
}

#[derive(Args)]
struct O2rArgs {
    /// Source `.o2r` pack.
    input: PathBuf,
    /// Output `.o2r` (by default only the restyled textures, to load after the source pack).
    output: PathBuf,
    /// Only entries matching this glob (repeatable), e.g. 'alt/scenes/*/spot04_scene/**'.
    #[arg(long, value_name = "GLOB")]
    include: Vec<String>,
    /// Also copy every unprocessed entry, producing a complete standalone pack.
    #[arg(long)]
    complete: bool,
    /// Style config (TOML file), built-in style name, or a stack `a+b` (a style plus layers, e.g.
    /// ss-baseline+impressionist); default: the built-in default style (impressionist).
    #[arg(long, value_name = "FILE|NAME")]
    style: Option<PathBuf>,
    #[arg(long, value_name = "FILE")]
    target: Option<PathBuf>,
    /// Pack map (TOML) that classifies entries by path.
    #[arg(long, value_name = "FILE")]
    pack: Option<PathBuf>,
    /// Treat every entry as this category, overriding the pack map.
    #[arg(long, value_name = "CATEGORY")]
    category: Option<Category>,
    /// Give every entry this mood (NAME or NAME:STRENGTH), overriding the pack map.
    #[arg(long, value_name = "MOOD")]
    mood: Option<Mood>,
    /// Worker threads (default or 0: all cores).
    #[arg(short, long, value_name = "N")]
    jobs: Option<usize>,
    #[command(flatten)]
    out: RunOutput,
}

/// How a run reports on itself.
#[derive(Args)]
struct RunOutput {
    /// No per-texture lines on stdout (the estimate and the final summary stay; errors and
    /// warnings still go to stderr).
    #[arg(short, long)]
    quiet: bool,
    /// Also write a machine-readable summary of the run (JSON: counts, failures with reasons,
    /// timings, output path) to this file, also when the run fails.
    #[arg(long, value_name = "FILE")]
    summary_json: Option<PathBuf>,
}

#[derive(Args)]
struct PaletteReportArgs {
    /// Folder of PNGs (searched recursively).
    input: PathBuf,
    /// Reference palette (TOML), e.g. reference/ss-lit.toml.
    #[arg(long, value_name = "FILE")]
    reference: PathBuf,
    /// Folder of the source PNGs (same relative paths): also report each file's mean-color ΔE.
    #[arg(long, value_name = "DIR")]
    source: Option<PathBuf>,
}

#[derive(Args)]
struct O2rExportArgs {
    /// Source `.o2r` pack (only read).
    input: PathBuf,
    /// Output folder.
    output: PathBuf,
    /// Only entries matching this glob (repeatable).
    #[arg(long, value_name = "GLOB")]
    include: Vec<String>,
}

#[derive(Args)]
struct BakeLutArgs {
    /// Style config (TOML) with a `[palette]` section.
    #[arg(long, value_name = "FILE")]
    style: PathBuf,
    /// Target profile, for a category's lift and shadow-tint scaling.
    #[arg(long, value_name = "FILE")]
    target: Option<PathBuf>,
    /// Category whose target treatment to apply.
    #[arg(long, value_name = "CATEGORY", default_value = "world")]
    category: Category,
    /// Mood to bake (NAME or NAME:STRENGTH).
    #[arg(long, value_name = "MOOD", default_value = "base")]
    mood: Mood,
    /// Output `.cube` file.
    output: PathBuf,
}

#[derive(Args)]
struct DevCompareArgs {
    /// Folder of original PNGs.
    before: PathBuf,
    /// Folder of processed PNGs (same file names).
    after: PathBuf,
    /// Output folder.
    output: PathBuf,
    /// Longest side of the downsized views.
    #[arg(long, default_value_t = 1024)]
    max_side: u32,
    /// Crop size (square, 1:1).
    #[arg(long, default_value_t = 512)]
    crop: u32,
}

#[derive(Args)]
struct ProcessArgs {
    /// Input folder.
    input: PathBuf,
    /// Output folder (created if missing).
    output: PathBuf,
    /// Walk subfolders; the output mirrors the tree.
    #[arg(short, long)]
    recursive: bool,
    /// Copy non-PNG files through, so the output is a complete drop-in pack.
    #[arg(long)]
    copy_other: bool,
    /// Follow symlinks when walking.
    #[arg(long)]
    follow_links: bool,
    /// Style config (TOML file), built-in style name, or a stack `a+b` (a style plus layers, e.g.
    /// ss-baseline+impressionist); default: the built-in default style (impressionist).
    #[arg(long, value_name = "FILE|NAME")]
    style: Option<PathBuf>,
    /// Target renderer profile (TOML).
    #[arg(long, value_name = "FILE")]
    target: Option<PathBuf>,
    /// Pack map (TOML).
    #[arg(long, value_name = "FILE")]
    pack: Option<PathBuf>,
    /// Worker threads (default or 0: all cores).
    #[arg(short, long, value_name = "N")]
    jobs: Option<usize>,
    /// Treat every PNG as this category (actor, world, skybox, background, ui, effect, skip), overriding
    /// the pack map.
    #[arg(long, value_name = "CATEGORY")]
    category: Option<Category>,
    /// Give every PNG this mood (NAME or NAME:STRENGTH, e.g. nocturne:0.6), overriding the pack
    /// map.
    #[arg(long, value_name = "MOOD")]
    mood: Option<Mood>,
    #[command(flatten)]
    out: RunOutput,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Process(args) => process(args),
        Command::GpuInfo => pastelplash::gpu::info().map(|()| ExitCode::SUCCESS),
        Command::BakeLut(args) => bake_lut(args).map(|()| ExitCode::SUCCESS),
        Command::O2r(args) => o2r(args),
        Command::PaletteReport(args) => pastelplash::report::Reference::load(&args.reference)
            .and_then(|r| pastelplash::report::report(&args.input, &r))
            .and_then(|text| {
                print!("{text}");
                if let Some(src) = &args.source {
                    let (lines, worst) = pastelplash::report::identity(src, &args.input)?;
                    print!("identity (mean color vs source):\n{lines}  worst ΔE {worst:.3}\n");
                }
                Ok(ExitCode::SUCCESS)
            }),
        Command::DevMetrics(args) => {
            pastelplash::report::metrics(&args.source, &args.output, args.baseline.as_deref()).map(
                |text| {
                    print!("{text}");
                    ExitCode::SUCCESS
                },
            )
        }
        Command::DevGrid(args) => std::fs::read_to_string(&args.rows)
            .map_err(anyhow::Error::from)
            .and_then(|text| {
                let rows: Vec<String> = text
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .map(String::from)
                    .collect();
                pastelplash::compare::grid(
                    &args.cols,
                    &rows,
                    &args.output,
                    args.thumb,
                    args.crop,
                    args.wrap,
                )
            })
            .map(|()| ExitCode::SUCCESS),
        Command::DevStats(args) => args
            .inputs
            .iter()
            .try_for_each(|p| {
                let rect = args.rect.as_ref().map(|r| [r[0], r[1], r[2], r[3]]);
                pastelplash::report::stats(p, rect).map(|t| print!("{t}"))
            })
            .map(|()| ExitCode::SUCCESS),
        Command::DevMap(args) => dev_map(args).map(|()| ExitCode::SUCCESS),
        Command::DevScope(args) => {
            pastelplash::compare::scope(&args.before, &args.after, &args.scope).map(
                |(text, flagged)| {
                    print!("{text}");
                    println!("{flagged} area(s) changed outside the declared scope");
                    ExitCode::SUCCESS
                },
            )
        }
        Command::DevFluidScan(args) => dev_fluid_scan(args).map(|()| ExitCode::SUCCESS),
        Command::DevCrop(a) => pastelplash::png_io::read(&a.input)
            .and_then(|img| {
                anyhow::ensure!(
                    a.x < img.width && a.y < img.height,
                    "rectangle outside image"
                );
                let mut c = pastelplash::compare::crop(&img, a.x, a.y, a.size);
                if let Some(bed) = a.over {
                    // Blended on gamma values, as the N64-style framebuffer blend does.
                    for p in &mut c.pixels {
                        let t = a.alpha * p[3];
                        for v in p.iter_mut().take(3) {
                            *v = *v * t + bed * (1.0 - t);
                        }
                        p[3] = 1.0;
                    }
                }
                pastelplash::png_io::write(&c, &a.output)
            })
            .map(|()| ExitCode::SUCCESS),
        Command::DevFluidSheet(args) => std::fs::read_to_string(&args.list)
            .map_err(anyhow::Error::from)
            .and_then(|text| {
                let rels: Vec<String> = text
                    .lines()
                    .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
                    .filter_map(|l| l.split('\t').next().map(str::to_string))
                    .filter(|p| p != "path")
                    .collect();
                anyhow::ensure!(args.frame.len() == 3, "--frame takes r,g,b");
                pastelplash::audit::sheet(
                    &args.dir,
                    &rels,
                    &args.output,
                    args.thumb,
                    args.cols,
                    args.first,
                    [args.frame[0], args.frame[1], args.frame[2]],
                )
            })
            .map(|()| ExitCode::SUCCESS),
        Command::DevSheet(args) => {
            pastelplash::compare::sheet(&args.input, &args.output, args.thumb, args.cols)
                .map(|()| ExitCode::SUCCESS)
        }
        Command::O2rExport(args) => {
            pastelplash::adapters::o2r::export(&args.input, &args.output, &args.include).map(|n| {
                println!("exported {n} textures to {}", args.output.display());
                ExitCode::SUCCESS
            })
        }
        Command::DevCompare(args) => pastelplash::compare::run(
            &args.before,
            &args.after,
            &args.output,
            args.max_side,
            args.crop,
        )
        .map(|()| ExitCode::SUCCESS),
    };
    result.unwrap_or_else(|e| {
        eprintln!("error: {e:#}");
        ExitCode::FAILURE
    })
}

/// The given style, or the built-in default style.
fn style_or_default(style: Option<PathBuf>) -> PathBuf {
    style.unwrap_or_else(|| PathBuf::from(pastelplash::config::DEFAULT_STYLE))
}

/// Finishes a run that reports on itself: writes the `--summary-json` report (with the error, if
/// the run failed as a whole) and gives the exit code (failure when any file failed).
fn finish(
    mut report: Report,
    result: anyhow::Result<()>,
    json: Option<&std::path::Path>,
) -> anyhow::Result<ExitCode> {
    let result = result.map(|()| report.failed == 0);
    match &result {
        Ok(ok) => report.ok = *ok,
        Err(e) => report.error = Some(format!("{e:#}")),
    }
    if let Some(path) = json
        && let Err(e) = report.save(path)
    {
        // The run's own error, if any, comes first.
        if let Err(run) = &result {
            eprintln!("error: {run:#}");
        }
        return Err(e);
    }
    let ok = result?;
    if report.failed > 0 {
        eprintln!("error: {} file(s) failed", report.failed);
    }
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn process(args: ProcessArgs) -> anyhow::Result<ExitCode> {
    pastelplash::log::set_quiet(args.out.quiet);
    let mut report = Report::new("process", &args.input, &args.output);
    let json = args.out.summary_json.clone();
    let result = run_process(args, &mut report);
    finish(report, result, json.as_deref())
}

fn run_process(args: ProcessArgs, report: &mut Report) -> anyhow::Result<()> {
    let config = Config::load(
        Some(&style_or_default(args.style)),
        args.target.as_deref(),
        args.pack.as_deref(),
    )?;
    let pipeline = Pipeline::from_config(&config)?;
    let opts = process::Options {
        input: args.input,
        output: args.output,
        recursive: args.recursive,
        follow_links: args.follow_links,
        copy_other: args.copy_other,
        jobs: args.jobs,
        category: args.category,
        mood: args.mood,
    };
    let s = process::run(&opts, &config, &pipeline)?;

    println!(
        "{} processed, {} copied, {} skipped, {} failed in {:.2?}",
        s.processed, s.copied, s.skipped, s.failed, s.elapsed
    );
    if s.links_ignored > 0 {
        println!(
            "{} symlinks not followed{}",
            s.links_ignored,
            if opts.follow_links {
                " (loops)"
            } else {
                " (use --follow-links)"
            }
        );
    }
    report.processed = s.processed;
    report.copied = s.copied;
    report.skipped = s.skipped;
    report.written = s.processed + s.copied + s.skipped;
    report.failed = s.failed;
    report.failures = s.failures;
    report.timings = Timings::new(s.elapsed, &s.phases);
    Ok(())
}

fn dev_map(args: DevMapArgs) -> anyhow::Result<()> {
    use pastelplash::color;
    let config = Config::load(
        Some(&style_or_default(args.style)),
        args.target.as_deref(),
        None,
    )?;
    let style = config.style.for_mood(&args.mood)?;
    let tr = config.target.treatment(args.category);
    let m = pastelplash::palette::Mapping::new(&style.palette, &tr);
    let lut = m.bake();
    for c in &args.colors {
        let (is_rgb, c) = match c.strip_prefix("rgb:") {
            Some(rest) => (true, rest),
            None => (false, c.as_str()),
        };
        let v: Vec<f32> = c
            .split(',')
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|e| anyhow::anyhow!("{c:?}: {e}"))?;
        anyhow::ensure!(v.len() == 3, "{c:?}: expected L,C,h or rgb:r,g,b");
        let rgb = if is_rgb {
            [v[0], v[1], v[2]]
        } else {
            color::oklch_to_srgb_gamut([v[0], v[1], v[2]])
        };
        let src = color::oklab_to_oklch(color::srgb_to_oklab(rgb));
        // As the GPU does it: the LUT, then the per-texel dark floor (in a neighborhood of the
        // same color) and cast.
        let finish = |o: [f32; 4]| {
            pastelplash::palette::rendered(&style.palette, &tr, rgb, [o[0], o[1], o[2]])
        };
        let via = finish(lut.sample(rgb));
        println!("  LUT: {:.3} {:.3} {:5.1}", via[0], via[1], via[2]);
        let o = m.map(rgb);
        let out = finish(o);
        println!(
            "{:.3} {:.3} {:5.1} -> {:.3} {:.3} {:5.1} (floor {:.3})",
            src[0], src[1], src[2], out[0], out[1], out[2], o[3]
        );
    }
    Ok(())
}

fn dev_fluid_scan(args: DevFluidScanArgs) -> anyhow::Result<()> {
    let pack = match &args.pack {
        Some(p) => Some(Config::load(None, None, Some(p))?.pack),
        None => None,
    };
    let t = std::time::Instant::now();
    let n = pastelplash::audit::scan(
        &args.input,
        &args.output,
        args.thumbs.as_deref(),
        &args.include,
        pack.as_ref(),
        args.jobs,
    )?;
    println!(
        "{n} textures scored in {:.1?} -> {}",
        t.elapsed(),
        args.output.display()
    );
    Ok(())
}

fn bake_lut(args: BakeLutArgs) -> anyhow::Result<()> {
    let config = Config::load(Some(&args.style), args.target.as_deref(), None)?;
    let tr = config.target.treatment(args.category);
    let style = config.style.for_mood(&args.mood)?;
    // A standalone `.cube` carries the whole mapping: the dark floor baked in (umber for neutral
    // darks), as the renderer's neighborhood switch cannot be expressed in a LUT.
    let lut = pastelplash::palette::Mapping::new(&style.palette, &tr)
        .inline_darks()
        .bake();
    let title = format!("{} ({:?}, {})", config.style.name, args.category, args.mood);
    lut.save(&args.output, &title)?;
    println!("wrote {}^3 LUT to {}", lut.size, args.output.display());
    Ok(())
}

fn o2r(args: O2rArgs) -> anyhow::Result<ExitCode> {
    pastelplash::log::set_quiet(args.out.quiet);
    let mut report = Report::new("o2r", &args.input, &args.output);
    let json = args.out.summary_json.clone();
    let result = run_o2r(args, &mut report);
    finish(report, result, json.as_deref())
}

fn run_o2r(args: O2rArgs, report: &mut Report) -> anyhow::Result<()> {
    let config = Config::load(
        Some(&style_or_default(args.style)),
        args.target.as_deref(),
        args.pack.as_deref(),
    )?;
    let pipeline = Pipeline::from_config(&config)?;
    let opts = pastelplash::adapters::o2r::Options {
        input: args.input,
        output: args.output,
        include: args.include,
        category: args.category,
        mood: args.mood,
        complete: args.complete,
        jobs: args.jobs,
    };
    let s = pastelplash::adapters::o2r::run(&opts, &config, &pipeline)?;
    report.processed = s.processed;
    report.reused = s.reused;
    report.copied = s.copied;
    report.skipped = s.skipped;
    report.written = s.written;
    report.failed = s.failed();
    report.failures = s.failures;
    report.timings = Timings::new(s.wall, &s.phases);
    report.bytes_in = Some(s.bytes_in);
    report.bytes_out = Some(s.bytes_out);
    Ok(())
}
