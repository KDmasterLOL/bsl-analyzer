//! Per-point measurement report emitted by `bench run` as JSON.

use serde::{Deserialize, Serialize};

pub const REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointReport {
    pub schema_version: u32,
    pub point_id: String,
    pub feature: String,
    pub mode: String,
    pub workspace_root: String,
    pub relative_path: String,
    /// Wall time of the workspace boot preceding the measurement (not part of
    /// the feature latency; recorded so orchestration can budget cold runs).
    pub boot_ms: u64,
    /// Handler-boundary points only: cost of building the frozen request
    /// context. Excluded from `cold_ns`/`warm_ns` — production dispatch pays it
    /// per request and Tier B measures it end-to-end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctx_build_ns: Option<u64>,
    /// First invocation after boot (process-cold). For `edit` points this is
    /// the first invocation after the `didChange` (the after-edit metric).
    pub cold_ns: u64,
    /// Warm repeats, in execution order.
    pub warm_ns: Vec<u64>,
    pub warm_p50_ns: u64,
    pub warm_p95_ns: u64,
    pub observed_count: usize,
    /// blake3 hex of the feature-specific digest input (order-normalized).
    pub digest: String,
    pub invariant_ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invariant_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit: Option<EditPhases>,
}

/// Extra timings recorded for `edit` points.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditPhases {
    /// `body` or `signature` — which invalidation class the patch models.
    pub edit_kind: String,
    /// p50 of the followup feature *before* the edit (steady warm baseline).
    pub warm_before_p50_ns: u64,
    /// Applying the `didChange` itself (mem_docs + VFS + `process_changes`).
    pub edit_apply_ns: u64,
    /// First followup invocation after the edit (same value as `cold_ns`).
    pub after_edit_ns: u64,
}

/// Nearest-rank percentile; `p` in 0..=100. Empty input yields 0.
pub fn percentile_ns(samples: &[u64], p: u32) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = ((p as usize) * sorted.len()).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

pub fn render_json(report: &PointReport) -> Result<String, String> {
    serde_json::to_string_pretty(report).map_err(|e| format!("report serialization failed: {e}"))
}

pub fn write_json(report: &PointReport, path: &std::path::Path) -> Result<(), String> {
    let text = render_json(report)?;
    std::fs::write(path, text).map_err(|e| format!("cannot write report {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_nearest_rank() {
        let s: Vec<u64> = (1..=20).collect();
        assert_eq!(percentile_ns(&s, 50), 10);
        assert_eq!(percentile_ns(&s, 95), 19);
        assert_eq!(percentile_ns(&s, 100), 20);
        assert_eq!(percentile_ns(&[], 50), 0);
        assert_eq!(percentile_ns(&[7], 95), 7);
    }

    #[test]
    fn report_roundtrips() {
        let r = PointReport {
            schema_version: REPORT_SCHEMA_VERSION,
            point_id: "hover/01".to_string(),
            feature: "hover".to_string(),
            mode: "latency".to_string(),
            workspace_root: "/tmp/w".to_string(),
            relative_path: "Module.bsl".to_string(),
            boot_ms: 12,
            ctx_build_ns: None,
            cold_ns: 1000,
            warm_ns: vec![10, 20],
            warm_p50_ns: 10,
            warm_p95_ns: 20,
            observed_count: 1,
            digest: "ab".to_string(),
            invariant_ok: true,
            invariant_error: None,
            edit: None,
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: PointReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.point_id, "hover/01");
        assert!(!json.contains("ctx_build_ns"), "None fields must be omitted: {json}");
    }
}
