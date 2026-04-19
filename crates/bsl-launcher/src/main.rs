//! BSL Analyzer Launcher
//!
//! Минимальный бинарник (~1-2 MB) для запуска LSP сервера bsl-analyzer.
//! Автоматически скачивает, обновляет и верифицирует основное приложение.
//!
//! Архитектура:
//! ```text
//! bsl-analyzer (launcher) -> скачивает -> bsl-analyzer-app (LSP сервер)
//!                                              |
//!                                              v
//!                                     ~/.bsl-analyzer/bin/
//! ```

mod cache;
mod entities;
mod messages;
mod parent_death;
mod provider;
mod use_cases;

use std::env;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

use crate::messages::messages;
use crate::provider::ReleaseProvider;

const RELEASE_CONFIG: &str = include_str!("../release-source.json");

fn main() -> Result<()> {
    let config: entities::ReleaseConfig =
        serde_json::from_str(RELEASE_CONFIG).context("Failed to parse release-source.json")?;
    let provider = provider::create_provider(&config);

    let args: Vec<String> = env::args().skip(1).collect();

    match args.first().map(|s| s.as_str()) {
        Some("--launcher-update") => return use_cases::update_analyzer(&*provider),
        Some("--launcher-version") => {
            println!("bsl-analyzer-launcher {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--launcher-verify") => return use_cases::verify_installation(&*provider),
        Some("--launcher-self-update") => return use_cases::self_update_launcher(&*provider),
        Some("--launcher-cleanup") => return use_cases::cleanup_versions(&args),
        Some("--help" | "-h") if args.len() == 1 => {
            return show_help_with_launcher_commands(&*provider);
        }
        _ => {}
    }

    let (requested_version, remaining_args) = extract_launcher_use(&args);

    let requested_version = requested_version
        .or_else(|| env::var("BSL_ANALYZER_VERSION").ok().filter(|s| !s.is_empty()));

    let is_analyze_mode = remaining_args.first().map(|s| s.as_str()) == Some("analyze");

    let analyzer_path = match requested_version {
        Some(ver) => use_cases::ensure_specific_version(&*provider, &ver)?,
        None => use_cases::ensure_analyzer(&*provider, is_analyze_mode)?,
    };

    let mut cmd = Command::new(&analyzer_path);
    cmd.args(&remaining_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    // Before spawn: install Linux PR_SET_PDEATHSIG via pre_exec.
    parent_death::configure_parent_death(&mut cmd);

    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to execute bsl-analyzer at {:?}", analyzer_path))?;

    // After spawn: bind to a Windows Job Object. If binding fails we own a
    // live detached child — the very failure mode this module exists to
    // prevent — so kill it explicitly before returning the error.
    let _lifecycle = match parent_death::adopt_child(&child) {
        Ok(guard) => guard,
        Err(err) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(err).context("Failed to bind child analyzer to launcher lifetime");
        }
    };

    let status = child
        .wait()
        .with_context(|| format!("Failed to wait for bsl-analyzer at {:?}", analyzer_path))?;

    std::process::exit(status.code().unwrap_or(1));
}

fn extract_launcher_use(args: &[String]) -> (Option<String>, Vec<String>) {
    let mut version = None;
    let mut remaining = Vec::new();
    let mut skip_next = false;

    for (i, arg) in args.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }

        if arg == "--launcher-use" {
            if let Some(ver) = args.get(i + 1) {
                version = Some(ver.clone());
                skip_next = true;
            }
        } else if let Some(ver) = arg.strip_prefix("--launcher-use=") {
            version = Some(ver.to_string());
        } else {
            remaining.push(arg.clone());
        }
    }

    (version, remaining)
}

fn show_help_with_launcher_commands(provider: &dyn ReleaseProvider) -> Result<()> {
    let analyzer_path = use_cases::ensure_analyzer(provider, false)?;
    let m = messages();

    let output = Command::new(&analyzer_path)
        .arg("--help")
        .output()
        .with_context(|| format!("Failed to execute bsl-analyzer at {:?}", analyzer_path))?;

    print!("{}", String::from_utf8_lossy(&output.stdout));

    println!("\n{}", m.launcher_commands);
    println!("  {:30} {}", "--launcher-use <VERSION>", m.help_use);
    println!("  {:30} {}", "--launcher-self-update", m.help_self_update);
    println!("  {:30} {}", "--launcher-version", m.help_version);
    println!("  {:30} {}", "--launcher-update", m.help_update);
    println!("  {:30} {}", "--launcher-verify", m.help_verify);
    println!("  {:30} {}", "--launcher-cleanup", m.help_cleanup);
    println!();
    println!("  BSL_ANALYZER_VERSION=<VERSION>  {}", m.help_use);

    Ok(())
}
