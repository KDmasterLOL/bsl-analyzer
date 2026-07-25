//! The resident analysis host's lifecycle, rendered for every tool that reads it.
//!
//! `metadata`, `symbol_info` and `diagnostics` all answer off ONE resident database, so
//! "not ready yet" and "how far along is the build" must read identically whichever of
//! them the consumer asked. A per-tool copy of these shapes drifts — and a consumer that
//! learned to recognize the envelope from one tool then fails to recognize it from the
//! next, which is worse than no envelope at all: an unrecognized "retry" reads as a
//! content answer, so an object that exists looks absent.

use rmcp::model::CallToolResult;
use serde_json::{json, Value};

use crate::diagnostics_state::StatusReport;
use crate::tools::response::structured;

/// The `status` action's body: the resident lifecycle snapshot. Always available (it
/// needs no built resident), so an agent can poll readiness and progress instead of
/// firing a data action just to read its `loading` envelope.
pub(crate) fn status(report: &StatusReport) -> CallToolResult {
    let mut body = json!({
        "state": report.state,
        "generation": report.generation,
        "reload": report.reload,
    });
    if let Some(files) = report.files {
        body["files"] = json!(files);
    }
    if let Some(ms) = report.elapsed_ms {
        body["elapsed_ms"] = json!(ms);
    }
    if let Some(err) = &report.error {
        body["error"] = json!(err);
    }
    if let Some(watch) = &report.watch {
        body["watch"] = json!({
            "mode": watch.mode,
            "health": watch.health,
            "events_seen": watch.events_seen,
        });
    }
    structured(body)
}

/// The body of a "still building the resident database" envelope: the machine-readable
/// `status`/`detail` pair plus the lifecycle snapshot, so a poller can tell a build that
/// is progressing from one that is stuck or failed. Split out from [`loading`] because
/// `metadata` carries the same body under a human sentence rather than the JSON mirror.
pub(crate) fn loading_body(report: &StatusReport, detail: &str) -> Value {
    let mut body = json!({
        "status": "loading",
        "detail": detail,
        "state": report.state,
        "generation": report.generation,
    });
    if let Some(ms) = report.elapsed_ms {
        body["elapsed_ms"] = json!(ms);
    }
    if let Some(err) = &report.error {
        body["error"] = json!(err);
    }
    body
}

/// A transient "still building the resident database" result, emitted while the
/// background build runs. Not an error — the agent should retry shortly.
pub(crate) fn loading(report: &StatusReport, detail: &str) -> CallToolResult {
    structured(loading_body(report, detail))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics_state::WatchReport;

    fn ready_report() -> StatusReport {
        StatusReport {
            state: "ready",
            generation: 3,
            files: Some(17),
            reload: "none",
            error: None,
            elapsed_ms: None,
            watch: Some(WatchReport { mode: "event-driven", health: "healthy", events_seen: 5 }),
        }
    }

    /// The snapshot every tool reading the resident answers `status` with — `metadata` and
    /// `diagnostics` both render through this one function, so a consumer polling either
    /// reads the same fields.
    #[test]
    fn status_reports_the_lifecycle_snapshot() {
        let body = status(&ready_report()).structured_content.expect("structuredContent");

        assert_eq!(body["state"], "ready");
        assert_eq!(body["generation"], 3);
        assert_eq!(body["files"], 17);
        assert_eq!(body["reload"], "none");
        assert_eq!(body["watch"]["mode"], "event-driven");
    }

    /// A failed build reports the reason instead of an eternal "loading": an agent polling
    /// readiness must be able to stop rather than retry a build that will never finish.
    #[test]
    fn a_failed_build_surfaces_its_error_in_both_shapes() {
        let report = StatusReport {
            state: "failed",
            generation: 0,
            files: None,
            reload: "none",
            error: Some("builder panicked".to_owned()),
            elapsed_ms: None,
            watch: None,
        };

        let status_body = status(&report).structured_content.expect("structuredContent");
        assert_eq!(status_body["state"], "failed");
        assert_eq!(status_body["error"], "builder panicked");

        let loading_body = loading_body(&report, "building; retry shortly");
        assert_eq!(loading_body["status"], "loading");
        assert_eq!(loading_body["state"], "failed");
        assert_eq!(loading_body["error"], "builder panicked");
    }
}
