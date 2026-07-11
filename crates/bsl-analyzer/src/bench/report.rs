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
    /// Mode B only. Timings in a recompute report are instrumented (the event
    /// callback adds overhead) and must never be compared with mode-A samples.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recompute: Option<RecomputeReport>,
    /// Mode C only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_hierarchy_index: Option<CallHierarchyIndexReport>,
}

/// Salsa churn observed in one measurement window (mode B).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecomputeReport {
    /// Per query-family counters, non-zero rows only, hottest first.
    pub families: Vec<FamilyChurn>,
    /// Exact number of distinct query keys that executed in the window.
    pub distinct_keys: usize,
    /// Exact number of distinct modules the executed keys resolved to.
    pub distinct_modules: usize,
    /// Sorted module paths, capped at [`MODULES_CAP`]; `modules_truncated`
    /// makes the cap explicit (never a silent truncation).
    pub modules: Vec<String>,
    pub modules_truncated: bool,
    pub check_cancellation: u64,
    pub set_cancellation: u64,
    pub discard_accumulated: u64,
}

pub const MODULES_CAP: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilyChurn {
    pub name: String,
    pub execute: u64,
    pub validate: u64,
    pub did_discard: u64,
    pub discard_stale: u64,
    pub intern_new: u64,
    pub intern_reuse: u64,
    pub intern_validate: u64,
    pub block_on: u64,
}

/// RSS bracketing of one feature execution (mode C), all values in bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryReport {
    pub rss_before_bytes: u64,
    /// Phase-local max(VmRSS) observed by the in-process sampler while the
    /// feature ran. With fewer than 3 samples the peak is only a lower bound.
    pub phase_peak_bytes: u64,
    pub sample_count: u64,
    pub peak_is_lower_bound: bool,
    pub rss_after_bytes: u64,
    /// After the normative trim protocol (close overlays → enforce_lru →
    /// green-node cache clear on main + rayon → allocator purge → 2 s settle).
    pub rss_after_trim_bytes: u64,
    /// After `enforce_lru_deep` plus the same clear/purge/settle steps.
    pub rss_after_deep_trim_bytes: u64,
    /// Lifetime process high-water mark — sanity only (dominated by boot).
    pub vm_hwm_bytes: Option<u64>,
    /// Top salsa ingredients by live entry count at `rss_after_bytes` time.
    pub ingredient_counts: Vec<(String, usize)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallHierarchyIndexReport {
    pub method_count: usize,
    pub unique_pair_count: usize,
    pub reverse_target_count: usize,
    pub estimated_heap_bytes: usize,
    pub batch_size: usize,
    pub build_duration_ns: u64,
    pub boot_rss_bytes: u64,
    pub pre_build_rss_bytes: u64,
    pub post_build_rss_bytes: u64,
    pub post_trim_rss_bytes: u64,
    pub vm_hwm_bytes: Option<u64>,
    pub digest: String,
}

/// Extra timings recorded for `edit` points.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditPhases {
    /// `body` or `signature` — which invalidation class the patch models.
    pub edit_kind: String,
    /// p50 of the followup feature *before* the edit (steady warm baseline).
    /// Mode A only — a recompute window has no uninstrumented baseline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warm_before_p50_ns: Option<u64>,
    /// Applying the `didChange` itself (mem_docs + VFS + `process_changes`).
    /// Mode A only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_apply_ns: Option<u64>,
    /// First followup invocation after the edit (same value as `cold_ns`).
    /// In mode B this is the whole instrumented apply+request window.
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
            recompute: None,
            memory: None,
            call_hierarchy_index: Some(CallHierarchyIndexReport {
                method_count: 2,
                unique_pair_count: 1,
                reverse_target_count: 1,
                estimated_heap_bytes: 512,
                batch_size: 1,
                build_duration_ns: 42,
                boot_rss_bytes: 1_024,
                pre_build_rss_bytes: 2_048,
                post_build_rss_bytes: 4_096,
                post_trim_rss_bytes: 3_072,
                vm_hwm_bytes: Some(4_096),
                digest: "cd".to_string(),
            }),
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: PointReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.point_id, "hover/01");
        assert!(!json.contains("ctx_build_ns"), "None fields must be omitted: {json}");
        let index = back.call_hierarchy_index.expect("index-build metrics must round-trip");
        assert_eq!(index.method_count, 2);
        assert_eq!(index.unique_pair_count, 1);
        assert_eq!(index.reverse_target_count, 1);
        assert_eq!(index.estimated_heap_bytes, 512);
        assert_eq!(index.batch_size, 1);
        assert_eq!(index.build_duration_ns, 42);
        assert_eq!(index.boot_rss_bytes, 1_024);
        assert_eq!(index.pre_build_rss_bytes, 2_048);
        assert_eq!(index.post_build_rss_bytes, 4_096);
        assert_eq!(index.post_trim_rss_bytes, 3_072);
        assert_eq!(index.vm_hwm_bytes, Some(4_096));
        assert_eq!(index.digest, "cd");

        let mut without_index = r.clone();
        without_index.call_hierarchy_index = None;
        let without_index_json = serde_json::to_string(&without_index).unwrap();
        assert!(
            !without_index_json.contains("call_hierarchy_index"),
            "None fields must be omitted: {without_index_json}"
        );
    }
}
