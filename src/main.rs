use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use pastelplash::config::Config;
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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Process(args) => process(args),
        Command::GpuInfo => pastelplash::gpu::info().map(|()| ExitCode::SUCCESS),
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
