use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use pastelplash::config::{Category, Config};
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
    /// Bake a style's palette into a `.cube` 3D LUT.
    BakeLut(BakeLutArgs),
    /// Write downsized before/after images and 1:1 crops for visual comparison.
    #[command(hide = true)]
    DevCompare(DevCompareArgs),
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
    /// Treat every PNG as this category (actor, world, skybox, ui, skip), overriding the pack map.
    #[arg(long, value_name = "CATEGORY")]
    category: Option<Category>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Process(args) => process(args),
        Command::GpuInfo => pastelplash::gpu::info().map(|()| ExitCode::SUCCESS),
        Command::BakeLut(args) => bake_lut(args).map(|()| ExitCode::SUCCESS),
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
    let palette = config.style.palette.clone();
    let lut = pastelplash::palette::Mapping {
        palette: &palette,
        lift_scale: tr.floor_scale,
        shadow_scale: tr.shadow_tint,
    }
    .bake();
    let title = format!("{} ({:?})", config.style.name, args.category);
    lut.save(&args.output, &title)?;
    println!("wrote {}^3 LUT to {}", lut.size, args.output.display());
    Ok(())
}
