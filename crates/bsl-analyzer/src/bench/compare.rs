//! Regression gate: compare a candidate bench run against a baseline.
//!
//! Consumes the raw per-run JSON reports `run-matrix.sh` collects (a directory
//! of `PointReport` files, or a single file holding one report or an array),
//! groups them by point id, and applies a variance-aware rule per point:
//!
//! ```text
//! regression  ⇔  cand_median > max(base_median × median_ratio,
//!                                   base_median + mad_sigma × MAD(base))
//! ```
//!
//! Only `latency`-mode reports participate; instrumented modes are counted and
//! skipped. Outcomes map to process exit codes in the CLI: pass → 0,
//! regression → 1, incompatible (schema mismatch, empty input, or a missing
//! point under `missing_point = "fail"`) → 2.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::bench::report::PointReport;

pub const POLICY_SCHEMA_VERSION: u32 = 1;
pub const COMPARE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparePolicy {
    pub schema_version: u32,
    /// Candidate median may exceed the baseline median by this factor.
    #[serde(default = "default_median_ratio")]
    pub median_ratio: f64,
    /// … or by this many baseline MADs, whichever bound is larger.
    #[serde(default = "default_mad_sigma")]
    pub mad_sigma: f64,
    #[serde(default)]
    pub missing_point: MissingPointPolicy,
    /// Per-point overrides keyed by point id.
    #[serde(default)]
    pub overrides: BTreeMap<String, PointOverride>,
}

fn default_median_ratio() -> f64 {
    1.15
}
fn default_mad_sigma() -> f64 {
    4.0
}

impl Default for ComparePolicy {
    fn default() -> Self {
        Self {
            schema_version: POLICY_SCHEMA_VERSION,
            median_ratio: default_median_ratio(),
            mad_sigma: default_mad_sigma(),
            missing_point: MissingPointPolicy::default(),
            overrides: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissingPointPolicy {
    /// A point present on one side only makes the runs incomparable (exit 2).
    #[default]
    Fail,
    /// Report the mismatch but keep comparing the intersection.
    Warn,
    /// Silently compare the intersection.
    Ignore,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointOverride {
    pub median_ratio: Option<f64>,
    pub mad_sigma: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOutcome {
    Pass,
    Regression,
    Incompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareReport {
    pub schema_version: u32,
    pub verdict: String,
    pub points: Vec<PointComparison>,
    /// Points present on only one side, with which side they were missing
    /// from; fatal under `missing_point = "fail"`.
    pub missing: Vec<MissingPoint>,
    pub skipped_non_latency: usize,
    pub regressions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointComparison {
    pub point_id: String,
    pub baseline_samples: usize,
    pub candidate_samples: usize,
    pub baseline_median_ns: u64,
    pub baseline_mad_ns: u64,
    pub candidate_median_ns: u64,
    pub threshold_ns: u64,
    pub regression: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingPoint {
    pub point_id: String,
    pub missing_from: String, // "baseline" | "candidate"
}

pub fn load_policy(path: Option<&Path>) -> Result<ComparePolicy, String> {
    let Some(path) = path else { return Ok(ComparePolicy::default()) };
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read policy {}: {e}", path.display()))?;
    let policy: ComparePolicy =
        serde_json::from_str(&text).map_err(|e| format!("policy parse error: {e}"))?;
    if policy.schema_version != POLICY_SCHEMA_VERSION {
        return Err(format!(
            "unsupported policy schema_version {} (expected {POLICY_SCHEMA_VERSION})",
            policy.schema_version
        ));
    }
    if policy.median_ratio < 1.0 || policy.mad_sigma < 0.0 {
        return Err("policy: median_ratio must be >= 1.0 and mad_sigma >= 0".to_string());
    }
    for (point, over) in &policy.overrides {
        if over.median_ratio.is_some_and(|r| r < 1.0) || over.mad_sigma.is_some_and(|s| s < 0.0) {
            return Err(format!(
                "policy override for `{point}`: median_ratio must be >= 1.0 and mad_sigma >= 0"
            ));
        }
    }
    Ok(policy)
}

/// Load every `PointReport` under `path`: a directory of `*.json` run files,
/// or a single file holding one report or an array of reports.
pub fn load_reports(path: &Path) -> Result<Vec<PointReport>, String> {
    let mut reports = Vec::new();
    if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
            .collect();
        entries.sort();
        for file in entries {
            reports.extend(parse_report_file(&file)?);
        }
    } else {
        reports.extend(parse_report_file(path)?);
    }
    if reports.is_empty() {
        return Err(format!("no reports found under {}", path.display()));
    }
    for report in &reports {
        if report.schema_version != crate::bench::report::REPORT_SCHEMA_VERSION {
            return Err(format!(
                "{}: report schema_version {} (expected {})",
                report.point_id,
                report.schema_version,
                crate::bench::report::REPORT_SCHEMA_VERSION
            ));
        }
    }
    Ok(reports)
}

fn parse_report_file(path: &Path) -> Result<Vec<PointReport>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if text.trim_start().starts_with('[') {
        serde_json::from_str::<Vec<PointReport>>(&text)
            .map_err(|e| format!("{}: parse error: {e}", path.display()))
    } else {
        serde_json::from_str::<PointReport>(&text)
            .map(|r| vec![r])
            .map_err(|e| format!("{}: parse error: {e}", path.display()))
    }
}

/// Group latency reports by point id → cold_ns samples. Returns the samples
/// and how many non-latency reports were skipped.
fn latency_samples(reports: &[PointReport]) -> (BTreeMap<String, Vec<u64>>, usize) {
    let mut by_point: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    let mut skipped = 0usize;
    for report in reports {
        if report.mode != "latency" {
            skipped += 1;
            continue;
        }
        by_point.entry(report.point_id.clone()).or_default().push(report.cold_ns);
    }
    (by_point, skipped)
}

fn median(samples: &mut [u64]) -> u64 {
    samples.sort_unstable();
    let n = samples.len();
    if n == 0 {
        return 0;
    }
    if n % 2 == 1 {
        samples[n / 2]
    } else {
        (samples[n / 2 - 1] + samples[n / 2]) / 2
    }
}

/// Raw median absolute deviation (no consistency constant — the policy's
/// `mad_sigma` is calibrated against the raw value).
fn mad(samples: &[u64], med: u64) -> u64 {
    let mut deviations: Vec<u64> = samples.iter().map(|&s| s.abs_diff(med)).collect();
    median(&mut deviations)
}

pub fn compare(
    baseline: &[PointReport],
    candidate: &[PointReport],
    policy: &ComparePolicy,
) -> (CompareOutcome, CompareReport) {
    let (base_points, base_skipped) = latency_samples(baseline);
    let (cand_points, cand_skipped) = latency_samples(candidate);

    let mut missing: Vec<MissingPoint> = Vec::new();
    for id in base_points.keys() {
        if !cand_points.contains_key(id) {
            missing
                .push(MissingPoint { point_id: id.clone(), missing_from: "candidate".to_string() });
        }
    }
    for id in cand_points.keys() {
        if !base_points.contains_key(id) {
            missing
                .push(MissingPoint { point_id: id.clone(), missing_from: "baseline".to_string() });
        }
    }

    let mut points = Vec::new();
    let mut regressions = 0usize;
    for (id, base_samples) in &base_points {
        let Some(cand_samples) = cand_points.get(id) else { continue };
        let mut base = base_samples.clone();
        let mut cand = cand_samples.clone();
        let base_median = median(&mut base);
        let base_mad = mad(&base, base_median);
        let cand_median = median(&mut cand);

        let (ratio, sigma) = match policy.overrides.get(id) {
            Some(o) => (
                o.median_ratio.unwrap_or(policy.median_ratio),
                o.mad_sigma.unwrap_or(policy.mad_sigma),
            ),
            None => (policy.median_ratio, policy.mad_sigma),
        };
        let by_ratio = (base_median as f64 * ratio) as u64;
        let by_mad = base_median.saturating_add((sigma * base_mad as f64) as u64);
        let threshold = by_ratio.max(by_mad);
        let regression = cand_median > threshold;
        regressions += regression as usize;

        points.push(PointComparison {
            point_id: id.clone(),
            baseline_samples: base.len(),
            candidate_samples: cand.len(),
            baseline_median_ns: base_median,
            baseline_mad_ns: base_mad,
            candidate_median_ns: cand_median,
            threshold_ns: threshold,
            regression,
        });
    }

    let incompatible = points.is_empty()
        || (!missing.is_empty() && policy.missing_point == MissingPointPolicy::Fail);
    let outcome = if incompatible {
        CompareOutcome::Incompatible
    } else if regressions > 0 {
        CompareOutcome::Regression
    } else {
        CompareOutcome::Pass
    };

    let report = CompareReport {
        schema_version: COMPARE_SCHEMA_VERSION,
        verdict: match outcome {
            CompareOutcome::Pass => "pass",
            CompareOutcome::Regression => "regression",
            CompareOutcome::Incompatible => "incompatible",
        }
        .to_string(),
        points,
        missing: if policy.missing_point == MissingPointPolicy::Ignore {
            Vec::new()
        } else {
            missing
        },
        skipped_non_latency: base_skipped + cand_skipped,
        regressions,
    };
    (outcome, report)
}

pub fn render_report(report: &CompareReport) -> Result<String, String> {
    serde_json::to_string_pretty(report).map_err(|e| format!("compare serialization failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::report::REPORT_SCHEMA_VERSION;

    fn report(point_id: &str, mode: &str, cold_ns: u64) -> PointReport {
        serde_json::from_value(serde_json::json!({
            "schema_version": REPORT_SCHEMA_VERSION,
            "point_id": point_id,
            "feature": "hover",
            "mode": mode,
            "workspace_root": "/w",
            "relative_path": "Module.bsl",
            "boot_ms": 1,
            "cold_ns": cold_ns,
            "warm_ns": [],
            "warm_p50_ns": 0,
            "warm_p95_ns": 0,
            "observed_count": 1,
            "digest": "d",
            "invariant_ok": true
        }))
        .unwrap()
    }

    fn latency(point_id: &str, samples: &[u64]) -> Vec<PointReport> {
        samples.iter().map(|&ns| report(point_id, "latency", ns)).collect()
    }

    #[test]
    fn identical_runs_pass() {
        let base = latency("hover/01", &[100, 110, 105, 95, 108]);
        let (outcome, rep) = compare(&base, &base.clone(), &ComparePolicy::default());
        assert_eq!(outcome, CompareOutcome::Pass);
        assert_eq!(rep.verdict, "pass");
        assert_eq!(rep.regressions, 0);
        assert_eq!(rep.points.len(), 1);
    }

    #[test]
    fn slowdown_beyond_ratio_and_mad_is_a_regression() {
        let base = latency("hover/01", &[1000, 1010, 990, 1005, 995]);
        let cand = latency("hover/01", &[2000, 2010, 1990, 2005, 1995]);
        let (outcome, rep) = compare(&base, &cand, &ComparePolicy::default());
        assert_eq!(outcome, CompareOutcome::Regression, "{rep:?}");
        assert_eq!(rep.regressions, 1);
        assert!(rep.points[0].regression);
    }

    #[test]
    fn noisy_baseline_tolerates_via_mad_bound() {
        // Median 1000 but MAD ~200: a 1300 candidate stays under
        // median + 4×MAD even though it exceeds median × 1.15.
        let base = latency("hover/01", &[800, 900, 1000, 1200, 1250]);
        let cand = latency("hover/01", &[1300, 1300, 1300, 1300, 1300]);
        let (outcome, _) = compare(&base, &cand, &ComparePolicy::default());
        assert_eq!(outcome, CompareOutcome::Pass);
    }

    #[test]
    fn missing_point_policy_matrix() {
        let base = latency("hover/01", &[100]);
        let mut cand = latency("hover/01", &[100]);
        cand.extend(latency("goto/01", &[50]));

        let (outcome, rep) = compare(&base, &cand, &ComparePolicy::default());
        assert_eq!(outcome, CompareOutcome::Incompatible, "fail is the default policy");
        assert_eq!(rep.missing.len(), 1);
        assert_eq!(rep.missing[0].missing_from, "baseline");

        let warn =
            ComparePolicy { missing_point: MissingPointPolicy::Warn, ..ComparePolicy::default() };
        let (outcome, rep) = compare(&base, &cand, &warn);
        assert_eq!(outcome, CompareOutcome::Pass);
        assert_eq!(rep.missing.len(), 1, "warn keeps the mismatch visible");

        let ignore =
            ComparePolicy { missing_point: MissingPointPolicy::Ignore, ..ComparePolicy::default() };
        let (outcome, rep) = compare(&base, &cand, &ignore);
        assert_eq!(outcome, CompareOutcome::Pass);
        assert!(rep.missing.is_empty(), "ignore hides the mismatch");
    }

    #[test]
    fn empty_intersection_is_incompatible() {
        let base = latency("hover/01", &[100]);
        let cand = latency("goto/01", &[100]);
        let ignore =
            ComparePolicy { missing_point: MissingPointPolicy::Ignore, ..ComparePolicy::default() };
        let (outcome, _) = compare(&base, &cand, &ignore);
        assert_eq!(outcome, CompareOutcome::Incompatible, "nothing comparable");
    }

    #[test]
    fn non_latency_reports_are_skipped_not_compared() {
        let mut base = latency("hover/01", &[100]);
        base.push(report("hover/01", "recompute", 999_999));
        let cand = latency("hover/01", &[100]);
        let (outcome, rep) = compare(&base, &cand, &ComparePolicy::default());
        assert_eq!(outcome, CompareOutcome::Pass);
        assert_eq!(rep.skipped_non_latency, 1);
    }

    #[test]
    fn per_point_override_loosens_the_bound() {
        let base = latency("hover/01", &[1000, 1000, 1000]);
        let cand = latency("hover/01", &[1500, 1500, 1500]);
        let (outcome, _) = compare(&base, &cand, &ComparePolicy::default());
        assert_eq!(outcome, CompareOutcome::Regression);

        let mut policy = ComparePolicy::default();
        policy.overrides.insert(
            "hover/01".to_string(),
            PointOverride { median_ratio: Some(2.0), mad_sigma: None },
        );
        let (outcome, _) = compare(&base, &cand, &policy);
        assert_eq!(outcome, CompareOutcome::Pass);
    }

    #[test]
    fn reports_load_from_directory_and_policy_validates() {
        let tmp = tempfile::tempdir().unwrap();
        for (i, r) in latency("hover/01", &[100, 110]).iter().enumerate() {
            std::fs::write(
                tmp.path().join(format!("r{i}.json")),
                serde_json::to_string(r).unwrap(),
            )
            .unwrap();
        }
        let loaded = load_reports(tmp.path()).unwrap();
        assert_eq!(loaded.len(), 2);

        assert!(load_reports(&tmp.path().join("absent")).is_err());

        let policy_path = tmp.path().join("policy.json");
        std::fs::write(&policy_path, r#"{"schema_version": 99}"#).unwrap();
        assert!(load_policy(Some(&policy_path)).unwrap_err().contains("schema_version"));
        std::fs::write(&policy_path, r#"{"schema_version": 1, "median_ratio": 0.5}"#).unwrap();
        assert!(load_policy(Some(&policy_path)).unwrap_err().contains("median_ratio"));
        std::fs::write(&policy_path, r#"{"schema_version": 1, "missing_point": "warn"}"#).unwrap();
        assert_eq!(
            load_policy(Some(&policy_path)).unwrap().missing_point,
            MissingPointPolicy::Warn
        );
        std::fs::write(
            &policy_path,
            r#"{"schema_version": 1, "overrides": {"hover/01": {"median_ratio": 0.2}}}"#,
        )
        .unwrap();
        let err = load_policy(Some(&policy_path)).unwrap_err();
        assert!(err.contains("override") && err.contains("hover/01"), "{err}");
    }
}
