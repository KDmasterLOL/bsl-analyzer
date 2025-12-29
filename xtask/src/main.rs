//! Build utilities for bsl-analyzer.

use clap::{Parser, Subcommand};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run all tests
    Test,

    /// Build release binary
    Build,

    /// Generate AST from grammar
    Codegen,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Test => {
            std::process::Command::new("cargo")
                .args(["test", "--all"])
                .status()?;
        }
        Commands::Build => {
            std::process::Command::new("cargo")
                .args(["build", "--release"])
                .status()?;
        }
        Commands::Codegen => {
            println!("Codegen not yet implemented");
        }
    }

    Ok(())
}
