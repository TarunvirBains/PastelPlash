use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use pastelplash::config::{Category, Config, Mood};
use pastelplash::pipeline::Pipeline;
use pastelplash::process;

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
    #[arg(long, value_name = "FILE")]
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
    #[arg(long, value_name = "MOOD", default_value = "pastel")]
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
    /// Style config (TOML).
    #[arg(long, value_name = "FILE")]
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
    /// Treat every PNG as this category (actor, world, skybox, background, ui, skip), overriding
    /// the pack map.
    #[arg(long, value_name = "CATEGORY")]
    category: Option<Category>,
    /// Give every PNG this mood (NAME or NAME:STRENGTH, e.g. nocturne:0.6), overriding the pack
    /// map.
    #[arg(long, value_name = "MOOD")]
    mood: Option<Mood>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Process(args) => process(args),
        Command::GpuInfo => pastelplash::gpu::info().map(|()| ExitCode::SUCCESS),
        Command::BakeLut(args) => bake_lut(args).map(|()| ExitCode::SUCCESS),
        Command::O2r(args) => o2r(args).map(|()| ExitCode::SUCCESS),
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
        Command::DevSheet(args) => {
            pastelplash::compare::sheet(&args.input, &args.output, args.thumb, args.cols)
                .map(|()| ExitCode::SUCCESS)
        }
        Command::O2rExport(args) => {
            pastelplash::o2r::export(&args.input, &args.output, &args.include).map(|n| {
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

fn process(args: ProcessArgs) -> anyhow::Result<ExitCode> {
    let config = Config::load(
        args.style.as_deref(),
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
    Ok(if s.failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn bake_lut(args: BakeLutArgs) -> anyhow::Result<()> {
    let config = Config::load(Some(&args.style), args.target.as_deref(), None)?;
    let tr = config.target.treatment(args.category);
    let style = config.style.for_mood(&args.mood)?;
    let lut = pastelplash::palette::Mapping::new(&style.palette, &tr).bake();
    let title = format!("{} ({:?}, {})", config.style.name, args.category, args.mood);
    lut.save(&args.output, &title)?;
    println!("wrote {}^3 LUT to {}", lut.size, args.output.display());
    Ok(())
}

fn o2r(args: O2rArgs) -> anyhow::Result<()> {
    let config = Config::load(
        args.style.as_deref(),
        args.target.as_deref(),
        args.pack.as_deref(),
    )?;
    let pipeline = Pipeline::from_config(&config)?;
    let opts = pastelplash::o2r::Options {
        input: args.input,
        output: args.output,
        include: args.include,
        category: args.category,
        mood: args.mood,
        complete: args.complete,
        jobs: args.jobs,
    };
    pastelplash::o2r::run(&opts, &config, &pipeline)
}
