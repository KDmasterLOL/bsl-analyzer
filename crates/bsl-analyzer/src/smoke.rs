//! Smoke harness for measuring cold-start + critical-path performance.
//!
//! Layout (O.3 skeleton; O.4-O.6 fill in scenarios):
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
//! O.3 lands the type surface, CLI wiring, and a no-op `run` that just
//! echoes the requested scenarios so `bsl-analyzer-app smoke --help`
//! works end-to-end. Real scenario impls land incrementally:
//!
//! - O.4: Boot (cold-start vfs_done + RSS + degraded_files_count)
//! - O.5: FirstPaint (per-stage didOpen → tokens → symbols → folding → diag)
//! - O.6: Hover + Deps

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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

/// Entry point for `bsl-analyzer-app smoke`. O.3 skeleton: echoes the
/// scenario list and emits a stub report. O.4-O.6 plug per-scenario
/// runners in.
pub fn run(args: SmokeArgs) -> SmokeReport {
    let report = SmokeReport {
        scenarios_run: args.scenarios.iter().map(|s| s.as_str().to_string()).collect(),
        ..SmokeReport::default()
    };

    // O.3 skeleton stub: no scenario impl yet. The dispatch table below
    // lands as O.4-O.6 wire each runner in; until then every scenario
    // produces a friendly diagnostic.
    for scenario in &args.scenarios {
        match scenario {
            Scenario::Boot => {
                tracing::warn!(
                    "smoke[boot]: scenario not yet implemented (lands in O.4); \
                     workspace={} budgets vfs_done_ms={} rss_bytes={}",
                    args.source_dir.display(),
                    args.budgets.boot_vfs_done_ms,
                    args.budgets.boot_rss_bytes,
                );
            }
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
        eprintln!(
            "smoke harness skeleton: {} scenario(s) requested, none implemented yet \
             (full impl lands incrementally O.4-O.6).",
            args.scenarios.len(),
        );
    }

    report
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
}
