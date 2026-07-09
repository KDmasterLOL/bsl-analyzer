use std::path::PathBuf;

use clap::{Subcommand, ValueEnum};

/// Exit codes: 0 ok, 1 execution error, 2 manifest error, 3 invariant violation.
const EXIT_OTHER: i32 = 1;
const EXIT_MANIFEST: i32 = 2;
const EXIT_INVARIANT: i32 = 3;

#[derive(Subcommand)]
pub enum BenchCommands {
    /// Probe the workspace and write a verified target manifest.
    Discover {
        #[arg(short = 's', long = "source-dir", default_value = ".")]
        source_dir: PathBuf,

        /// Manifest output path; stdout if omitted.
        #[arg(short = 'o', long = "output")]
        output: Option<PathBuf>,

        #[arg(long = "boot-budget-ms", default_value = "120000")]
        boot_budget_ms: u64,
    },
    /// Execute exactly one manifest point in this process and report timings.
    Run {
        #[arg(short = 's', long = "source-dir", default_value = ".")]
        source_dir: PathBuf,

        #[arg(short = 'm', long = "manifest")]
        manifest: PathBuf,

        #[arg(short = 'p', long = "point")]
        point: String,

        #[arg(long = "mode", value_enum, default_value_t = BenchMode::Latency)]
        mode: BenchMode,

        /// At least 1: zero warm samples would degrade p50/p95 to silent zeros.
        #[arg(long = "warm-iterations", default_value = "20",
              value_parser = clap::value_parser!(u32).range(1..))]
        warm_iterations: u32,

        /// Report output path; stdout if omitted.
        #[arg(long = "json")]
        json: Option<PathBuf>,

        #[arg(long = "boot-budget-ms", default_value = "120000")]
        boot_budget_ms: u64,
    },
}

pub fn run_bench(command: BenchCommands) -> ! {
    match command {
        BenchCommands::Discover { source_dir, output, boot_budget_ms } => {
            let args = bsl_analyzer::bench::discover::DiscoverArgs { source_dir, boot_budget_ms };
            match bsl_analyzer::bench::discover::discover(&args) {
                Ok(manifest) => {
                    let text = match serde_json::to_string_pretty(&manifest) {
                        Ok(text) => text,
                        Err(e) => {
                            eprintln!("bench discover: serialization failed: {e}");
                            std::process::exit(EXIT_OTHER);
                        }
                    };
                    match &output {
                        Some(path) => {
                            if let Err(e) = std::fs::write(path, text) {
                                eprintln!("bench discover: cannot write {}: {e}", path.display());
                                std::process::exit(EXIT_OTHER);
                            }
                            eprintln!(
                                "bench discover: {} targets -> {}",
                                manifest.targets.len(),
                                path.display()
                            );
                        }
                        None => println!("{text}"),
                    }
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("bench discover: {e}");
                    std::process::exit(exit_code(&e));
                }
            }
        }
        BenchCommands::Run {
            source_dir,
            manifest,
            point,
            mode,
            warm_iterations,
            json,
            boot_budget_ms,
        } => {
            if !matches!(mode, BenchMode::Latency) {
                eprintln!(
                    "bench run: mode `{mode:?}` is not implemented yet; only `latency` is available"
                );
                std::process::exit(EXIT_OTHER);
            }
            let args = bsl_analyzer::bench::runner::RunArgs {
                source_dir,
                manifest_path: manifest,
                point_id: point,
                warm_iterations: warm_iterations as usize,
                boot_budget_ms,
            };
            match bsl_analyzer::bench::runner::run_point(&args) {
                Ok(report) => {
                    let emitted = match &json {
                        Some(path) => bsl_analyzer::bench::report::write_json(&report, path),
                        None => bsl_analyzer::bench::report::render_json(&report)
                            .map(|text| println!("{text}")),
                    };
                    if let Err(e) = emitted {
                        eprintln!("bench run: {e}");
                        std::process::exit(EXIT_OTHER);
                    }
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("bench run: {e}");
                    std::process::exit(exit_code(&e));
                }
            }
        }
    }
}

fn exit_code(e: &bsl_analyzer::bench::runner::RunError) -> i32 {
    use bsl_analyzer::bench::runner::RunError;
    match e {
        RunError::Manifest(_) => EXIT_MANIFEST,
        RunError::Invariant(_) => EXIT_INVARIANT,
        RunError::Other(_) => EXIT_OTHER,
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum BenchMode {
    Latency,
    Recompute,
    Memory,
}
