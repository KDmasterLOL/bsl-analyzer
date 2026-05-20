//! Smoke harness for measuring cold-start + critical-path performance.
//!
//! - [`Scenario`] — Boot / FirstPaint / Hover / Deps. Parsed from CLI.
//! - [`Budgets`] — pass/fail thresholds (vfs_done_ms, rss_mb, first-paint
//!   per-stage, hover cold/warm, deps cold). Configurable via JSON file.
//! - [`SmokeReport`] — top-level result; aggregates per-scenario results
//!   plus a flat [`BudgetViolation`] list. JSON-serializable for CI.
//! - [`read_rss_bytes`] — best-effort per-OS process RSS reader.
//! - [`run`] — entry point; takes scenario list + workspace + budgets and
//!   returns the populated report.
//!
//! Scenario landings (Phase O):
//!
//! - O.4: Boot — cold-start `vfs_done_ms` + RSS + `degraded_files_count`.
//! - O.5: FirstPaint — per-stage didOpen → tokens → symbols → folding → diag.
//! - O.6: Hover + Deps.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use crossbeam_channel::RecvTimeoutError;
use serde::{Deserialize, Serialize};
use vfs::loader::{self, LoadingProgress};

use crate::global_state::GlobalState;

/// Smoke scenarios.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scenario {
    /// Cold-start: load workspace, measure `vfs_done_ms`, RSS, degraded files.
    Boot,
    /// didOpen → semantic_tokens → document_symbols → folding_ranges → diagnostics.
    FirstPaint,
    /// Hover on auto-picked targets; cold + warm pairs.
    Hover,
    /// `file_dependencies` BFS percentiles per root.
    Deps,
}

impl Scenario {
    /// Parse a single scenario name from the CLI surface. Case-insensitive.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "boot" => Ok(Scenario::Boot),
            "first_paint" | "first-paint" | "firstpaint" => Ok(Scenario::FirstPaint),
            "hover" => Ok(Scenario::Hover),
            "deps" => Ok(Scenario::Deps),
            other => {
                Err(format!("unknown scenario `{other}` (valid: boot, first_paint, hover, deps)"))
            }
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Scenario::Boot => "boot",
            Scenario::FirstPaint => "first_paint",
            Scenario::Hover => "hover",
            Scenario::Deps => "deps",
        }
    }

    pub fn all() -> &'static [Scenario] {
        &[Scenario::Boot, Scenario::FirstPaint, Scenario::Hover, Scenario::Deps]
    }
}

/// Pass/fail thresholds. Defaults are calibrated for niagara_ut + ERP per
/// PLAN-v5 §7; override via `--budgets path/to/budgets.json` if you need
/// scenario-specific tuning for a different workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budgets {
    /// Boot: max wall-clock from server start to `vfs_done`.
    pub boot_vfs_done_ms: u64,
    /// Boot: max RSS bytes after `vfs_done` (default 1 GiB).
    pub boot_rss_bytes: u64,
    /// Boot: max acceptable count of unreadable/degraded BSL files.
    pub boot_degraded_files_max: usize,
    /// FirstPaint: total time from didOpen to last paint stage.
    pub first_paint_total_ms: u64,
    /// Hover: cold target turnaround.
    pub hover_cold_ms: u64,
    /// Hover: warm target turnaround.
    pub hover_warm_ms: u64,
    /// Deps: p95 cold `file_dependencies` BFS time.
    pub deps_cold_p95_ms: u64,
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            boot_vfs_done_ms: 30_000,
            boot_rss_bytes: 3 * 1024 * 1024 * 1024,
            boot_degraded_files_max: 0,
            first_paint_total_ms: 1_500,
            hover_cold_ms: 1_300,
            hover_warm_ms: 200,
            deps_cold_p95_ms: 1_000,
        }
    }
}

impl Budgets {
    /// Load budgets from a JSON file; falls back to [`Default`] on
    /// I/O or parse failure (logged via `eprintln!`).
    pub fn load_or_default(path: Option<&Path>) -> Self {
        let Some(path) = path else { return Self::default() };
        match std::fs::read_to_string(path).and_then(|s| {
            serde_json::from_str::<Budgets>(&s)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        }) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("smoke: budgets file {} unreadable: {e}; using defaults", path.display());
                Self::default()
            }
        }
    }
}

/// One failed threshold in the report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetViolation {
    pub scenario: String,
    pub metric: String,
    pub observed: u64,
    pub budget: u64,
}

/// Top-level smoke report. JSON-serialized to stdout when `--json` is set.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SmokeReport {
    pub scenarios_run: Vec<String>,
    pub boot: Option<BootResult>,
    pub first_paint: Option<FirstPaintResult>,
    pub hover: Option<HoverResult>,
    pub deps: Option<DepsResult>,
    pub violations: Vec<BudgetViolation>,
}

impl SmokeReport {
    /// Convenience: true iff no scenario raised a budget violation.
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct BootResult {
    pub vfs_done_ms: u64,
    pub rss_bytes_post_boot: Option<u64>,
    pub degraded_files_count: usize,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct FirstPaintResult {
    pub phases: Vec<FirstPaintPhase>,
    pub total_ms: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct FirstPaintPhase {
    pub name: String,
    pub cold_ms: u64,
    pub warm_ms: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct HoverResult {
    pub targets: Vec<HoverTargetResult>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct HoverTargetResult {
    pub file: String,
    pub line: u32,
    pub character: u32,
    pub cold_ms: u64,
    pub warm_ms: u64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DepsResult {
    pub roots_sampled: usize,
    pub cold_ms_p50: u64,
    pub cold_ms_p95: u64,
}

/// Best-effort per-OS process RSS reader. Returns bytes on Linux via
/// `/proc/self/status`; `None` elsewhere (Boot scenario tolerates this and
/// records `None` in the report).
pub fn read_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
                return Some(kb * 1024);
            }
        }
        None
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// CLI args bundled together so [`run`] doesn't grow a long signature.
#[derive(Debug, Clone)]
pub struct SmokeArgs {
    pub source_dir: PathBuf,
    pub scenarios: Vec<Scenario>,
    pub budgets: Budgets,
    pub json: bool,
}

/// Entry point for `bsl-analyzer-app smoke`. Dispatches each requested
/// scenario, populates the matching slot on [`SmokeReport`] and pushes any
/// budget violations into the flat list.
pub fn run(args: SmokeArgs) -> SmokeReport {
    let scenarios_run: Vec<String> =
        args.scenarios.iter().map(|s| s.as_str().to_string()).collect();
    let mut report = SmokeReport { scenarios_run, ..SmokeReport::default() };

    for scenario in &args.scenarios {
        match scenario {
            Scenario::Boot => match run_boot(&args) {
                Ok(boot) => {
                    check_boot_budgets(&boot, &args.budgets, &mut report.violations);
                    report.boot = Some(boot);
                }
                Err(e) => {
                    tracing::error!(error = %e, "smoke[boot]: failed");
                    report.violations.push(BudgetViolation {
                        scenario: "boot".to_string(),
                        metric: "run_error".to_string(),
                        observed: 0,
                        budget: 0,
                    });
                }
            },
            Scenario::FirstPaint => {
                tracing::warn!("smoke[first_paint]: scenario not yet implemented (lands in O.5)");
            }
            Scenario::Hover => {
                tracing::warn!("smoke[hover]: scenario not yet implemented (lands in O.6)");
            }
            Scenario::Deps => {
                tracing::warn!("smoke[deps]: scenario not yet implemented (lands in O.6)");
            }
        }
    }

    if args.json {
        match serde_json::to_string_pretty(&report) {
            Ok(s) => println!("{s}"),
            Err(e) => eprintln!("smoke: JSON serialisation failed: {e}"),
        }
    } else {
        emit_text_report(&report);
    }

    report
}

fn emit_text_report(report: &SmokeReport) {
    if let Some(boot) = &report.boot {
        eprintln!(
            "smoke[boot]: vfs_done_ms={} rss={} degraded_files={}",
            boot.vfs_done_ms,
            boot.rss_bytes_post_boot
                .map(|b| format!("{:.1}MB", b as f64 / (1024.0 * 1024.0)))
                .unwrap_or_else(|| "n/a".to_string()),
            boot.degraded_files_count,
        );
    }
    if !report.violations.is_empty() {
        eprintln!("smoke: {} budget violation(s):", report.violations.len());
        for v in &report.violations {
            eprintln!(
                "  - {} {}: observed={} budget={}",
                v.scenario, v.metric, v.observed, v.budget
            );
        }
    } else if !report.scenarios_run.is_empty() {
        eprintln!("smoke: all budgets within thresholds");
    }
}

/// Boot scenario — mirrors `server::main_loop` cold-start without the LSP
/// transport, background indexer, or name-index priming.
///
/// Bootstrap sequence (matches `server::handle_loader_msg::Finished`):
///
///   1. `GlobalState::new(dummy_sender)` + `init_empty_source_root`
///   2. `set_workspace_root(args.source_dir)` — kicks off vfs-notify loader
///   3. drain `loader_receiver`, streaming `Loaded`/`Changed` batches into
///      VFS via [`stream_loader_batch`] (the smoke twin of
///      `server::handle_vfs_msg` with `sync_to_salsa=false`)
///   4. on `LoadingProgress::Finished`:
///      - `process_changes(true)` (suppress metadata bump for the bulk sweep)
///      - `init_source_root`
///      - `warm_metadata_cache`
///      - sync `degraded_files_count = skipped_bsl.len()`
///      - defensive `assert_total_vfs_invariant` scan (log-only)
///
/// Measures `vfs_done_ms`, post-finalize RSS, and `degraded_files_count`.
fn run_boot(args: &SmokeArgs) -> Result<BootResult, String> {
    let (sender, receiver) = crossbeam_channel::unbounded::<lsp_server::Message>();
    // Drain the LSP sender in a background thread so progress notifications
    // emitted by `report_progress` during bootstrap do not pile up
    // unboundedly. The thread exits when the sender is dropped along with
    // `state` below.
    let drain_handle = std::thread::spawn(move || while receiver.recv().is_ok() {});

    let mut state = GlobalState::new(sender);
    state.init_empty_source_root();

    let start = Instant::now();
    state.set_workspace_root(args.source_dir.clone());

    // Hard deadline: 4× the budget + 60 s buffer. Prevents the smoke run
    // from hanging indefinitely if the loader stalls on a pathological
    // workspace; the budget gate (checked after success) still fails the
    // run when the actual elapsed time exceeds `boot_vfs_done_ms`.
    let safety = args.budgets.boot_vfs_done_ms.saturating_mul(4).saturating_add(60_000);
    let deadline = start + Duration::from_millis(safety);

    let mut finished = false;
    while !finished {
        let now = Instant::now();
        if now >= deadline {
            drop(state);
            let _ = drain_handle.join();
            return Err(format!(
                "boot exceeded hard deadline ({} ms = 4× budget + 60 s buffer)",
                safety,
            ));
        }
        let remaining = deadline - now;
        let msg = match state.loader_receiver.recv_timeout(remaining) {
            Ok(m) => m,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                drop(state);
                let _ = drain_handle.join();
                return Err("loader channel disconnected before Finished arrived".to_string());
            }
        };

        match msg {
            loader::Message::Progress { n_done: LoadingProgress::Finished, .. } => {
                state.process_changes(true);
                state.init_source_root();
                state.warm_metadata_cache();
                state.degraded_files_count = state.skipped_bsl.len();
                let extra = state.assert_total_vfs_invariant();
                if extra > 0 {
                    tracing::error!(
                        extra,
                        "smoke[boot]: total-VFS invariant violated — B1/B2-A missed a path",
                    );
                }
                state.vfs_done = true;
                finished = true;
            }
            loader::Message::Progress { .. } => {
                // Started / Scanning / Progress(_) — ignore in smoke; no UI.
            }
            loader::Message::Loaded { files } | loader::Message::Changed { files } => {
                stream_loader_batch(&mut state, files);
            }
            loader::Message::WatchOnly { files } => {
                let mut vfs = state.vfs.write();
                for path in files {
                    vfs.register_watch_only(vfs::VfsPath::new(path.as_path()));
                }
            }
        }
    }

    let vfs_done_ms = start.elapsed().as_millis() as u64;
    let rss_bytes_post_boot = read_rss_bytes();
    let degraded_files_count = state.degraded_files_count;

    drop(state);
    let _ = drain_handle.join();

    Ok(BootResult { vfs_done_ms, rss_bytes_post_boot, degraded_files_count })
}

/// Smoke twin of `server::handle_vfs_msg` for `sync_to_salsa = false`: convert
/// `Vec<u8>` → `Arc<str>` (stripping UTF-8 BOM), record skipped BSL paths in
/// `skipped_bsl` (B1 hook from O.2), drain into VFS in mini-batches so a
/// large `Loaded` chunk does not monopolise the write lock.
fn stream_loader_batch(state: &mut GlobalState, files: Vec<(paths::AbsPathBuf, Option<Vec<u8>>)>) {
    const VFS_WRITE_MINI_BATCH: usize = 16;

    let mut converted: Vec<(vfs::VfsPath, Option<Arc<str>>)> = Vec::with_capacity(files.len());
    for (path, contents) in files {
        let std_path: &std::path::Path = path.as_ref();
        let vfs_path = vfs::VfsPath::new(std_path);

        let contents_str = contents.and_then(|bytes| {
            String::from_utf8(bytes).ok().map(|s| {
                let s = s.strip_prefix('\u{FEFF}').unwrap_or(&s);
                Arc::from(s)
            })
        });

        if project_model::is_bsl_source_path(std_path) {
            let mutated = if contents_str.is_some() {
                state.skipped_bsl.remove(&path)
            } else if state.skipped_bsl.insert(path.clone()) {
                tracing::warn!(
                    path = %path,
                    "BSL file unreadable by VFS; recorded as skipped",
                );
                true
            } else {
                false
            };
            if mutated {
                state.degraded_files_count = state.skipped_bsl.len();
            }
        }

        converted.push((vfs_path, contents_str));
    }

    for chunk in converted.chunks(VFS_WRITE_MINI_BATCH) {
        let mut vfs = state.vfs.write();
        for (vfs_path, contents_str) in chunk {
            vfs.set_file_contents(vfs_path.clone(), contents_str.clone());
        }
    }
}

fn check_boot_budgets(boot: &BootResult, budgets: &Budgets, out: &mut Vec<BudgetViolation>) {
    if boot.vfs_done_ms > budgets.boot_vfs_done_ms {
        out.push(BudgetViolation {
            scenario: "boot".to_string(),
            metric: "vfs_done_ms".to_string(),
            observed: boot.vfs_done_ms,
            budget: budgets.boot_vfs_done_ms,
        });
    }
    if let Some(rss) = boot.rss_bytes_post_boot {
        if rss > budgets.boot_rss_bytes {
            out.push(BudgetViolation {
                scenario: "boot".to_string(),
                metric: "rss_bytes_post_boot".to_string(),
                observed: rss,
                budget: budgets.boot_rss_bytes,
            });
        }
    }
    if boot.degraded_files_count > budgets.boot_degraded_files_max {
        out.push(BudgetViolation {
            scenario: "boot".to_string(),
            metric: "degraded_files_count".to_string(),
            observed: boot.degraded_files_count as u64,
            budget: budgets.boot_degraded_files_max as u64,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scenario_parse_round_trip() {
        for &s in Scenario::all() {
            let name = s.as_str();
            assert_eq!(Scenario::parse(name).unwrap(), s);
        }
    }

    #[test]
    fn scenario_parse_case_insensitive() {
        assert_eq!(Scenario::parse("BOOT").unwrap(), Scenario::Boot);
        assert_eq!(Scenario::parse("First-Paint").unwrap(), Scenario::FirstPaint);
        assert_eq!(Scenario::parse(" hover ").unwrap(), Scenario::Hover);
    }

    #[test]
    fn scenario_parse_rejects_unknown() {
        assert!(Scenario::parse("refs").is_err());
    }

    #[test]
    fn budgets_default_is_sane() {
        let b = Budgets::default();
        assert!(b.boot_vfs_done_ms > 0);
        assert!(b.boot_rss_bytes > 0);
        assert!(b.hover_warm_ms < b.hover_cold_ms);
    }

    #[test]
    fn report_passes_when_no_violations() {
        let r = SmokeReport::default();
        assert!(r.passed());
    }

    #[test]
    fn rss_reader_returns_value_on_linux() {
        #[cfg(target_os = "linux")]
        {
            let rss = read_rss_bytes();
            assert!(rss.is_some(), "/proc/self/status should expose VmRSS on Linux");
            assert!(rss.unwrap() > 0);
        }
    }

    #[test]
    fn check_boot_budgets_flags_vfs_done() {
        let boot = BootResult {
            vfs_done_ms: 1000,
            rss_bytes_post_boot: Some(100),
            degraded_files_count: 0,
        };
        let budgets = Budgets { boot_vfs_done_ms: 500, ..Budgets::default() };
        let mut out = Vec::new();
        check_boot_budgets(&boot, &budgets, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].metric, "vfs_done_ms");
        assert_eq!(out[0].observed, 1000);
        assert_eq!(out[0].budget, 500);
    }

    #[test]
    fn check_boot_budgets_flags_rss_and_degraded() {
        let boot = BootResult {
            vfs_done_ms: 10,
            rss_bytes_post_boot: Some(2_000),
            degraded_files_count: 3,
        };
        let budgets = Budgets {
            boot_vfs_done_ms: 10_000,
            boot_rss_bytes: 1_000,
            boot_degraded_files_max: 0,
            ..Budgets::default()
        };
        let mut out = Vec::new();
        check_boot_budgets(&boot, &budgets, &mut out);
        let metrics: Vec<_> = out.iter().map(|v| v.metric.as_str()).collect();
        assert!(metrics.contains(&"rss_bytes_post_boot"));
        assert!(metrics.contains(&"degraded_files_count"));
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn check_boot_budgets_passes_within_thresholds() {
        let boot = BootResult {
            vfs_done_ms: 100,
            rss_bytes_post_boot: Some(100),
            degraded_files_count: 0,
        };
        let mut out = Vec::new();
        check_boot_budgets(&boot, &Budgets::default(), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn check_boot_budgets_tolerates_missing_rss() {
        // RSS reading is best-effort; absence must not raise a violation.
        let boot =
            BootResult { vfs_done_ms: 10, rss_bytes_post_boot: None, degraded_files_count: 0 };
        let mut out = Vec::new();
        check_boot_budgets(&boot, &Budgets::default(), &mut out);
        assert!(out.is_empty());
    }

    /// End-to-end boot smoke on a synthetic empty workspace. Validates that
    /// `run_boot` completes without panicking when the loader has nothing to
    /// load and that the report shape is consistent (`degraded_files_count =
    /// 0`, no violations against the generous default budgets).
    #[test]
    fn run_boot_on_empty_tmpdir_succeeds() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let args = SmokeArgs {
            source_dir: tmp.path().to_path_buf(),
            scenarios: vec![Scenario::Boot],
            budgets: Budgets::default(),
            json: false,
        };
        let boot = run_boot(&args).expect("boot on empty tmpdir should succeed");
        assert_eq!(boot.degraded_files_count, 0);
        // No assertion on `vfs_done_ms` itself — even an empty dir takes a few
        // ms because of the loader's scan + finalize pipeline. Just confirm
        // it is within the (very generous) default budget.
        assert!(boot.vfs_done_ms < Budgets::default().boot_vfs_done_ms);
    }
}
