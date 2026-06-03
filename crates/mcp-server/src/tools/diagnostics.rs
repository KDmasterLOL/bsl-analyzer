//! Agent-facing diagnostics tool actions.
//!
//! Diagnostics are the second non-grep-able semantic primitive beside the call
//! `graph`: grep cannot tell unreachable code, a type mismatch, an unresolved call,
//! or an unused variable from ordinary text, but the analyzer can.
//!
//! This module ships the `catalog` action — the static, database-free list of every
//! registered diagnostic code with the metadata an agent needs for cold-start
//! discovery and the request→narrow→request entry point. Rule prose lives here,
//! keyed by `code`, so a later per-file action's findings never repeat it. The
//! `schema` action advertises the contract. Both are computed from compile-time
//! metadata, so no resident analysis database is required.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::str::FromStr;

use ide::{
    catalog_entry, diagnostic_catalog, DiagnosticCode, DocumentSymbol, Locale, SeverityBucket,
    SymbolKind, TextRange,
};
use rmcp::model::CallToolResult;
use serde_json::{json, Value};

use crate::diagnostics_state::{DiagnosticsResident, Freshness, SweepOptions};
use crate::tools::response::structured;

/// Server-side cap on returned findings, honouring Anthropic's tool-response budget.
/// `counts` still reports the full severity histogram, so a capped response is honest.
pub(crate) const DEFAULT_MAX_FINDINGS: usize = 200;

/// Default cap on files swept by the opt-in `workspace` action. Bounds the cost of a
/// whole-config pass; the agent can raise it up to [`MAX_SWEEP_FILES_CEILING`].
pub(crate) const DEFAULT_MAX_SWEEP_FILES: usize = 1000;

/// Hard ceiling on `max_files`: the sweep holds the resident lock for its whole
/// duration (blocking other diagnostics calls), so an agent cannot request an
/// unbounded whole-config pass that stalls the server for minutes. A larger config is
/// reported as `truncated` with the true `files_total`.
pub(crate) const MAX_SWEEP_FILES_CEILING: usize = 5000;

/// Filters applied to a file's diagnostics before they become findings.
pub(crate) struct FileFilters {
    /// Inclusive severity floor (findings below it are dropped from `findings`, but
    /// still counted in `counts`).
    pub min_severity: SeverityBucket,
    /// Keep only these codes (empty = all).
    pub codes: Vec<String>,
    /// Keep only findings intersecting this inclusive 0-based line range (None = all).
    pub range: Option<(usize, usize)>,
    /// Cap on `findings` length.
    pub max_findings: usize,
    /// Detailed mode adds `internal_severity` (7-grade) and the fix label per finding.
    pub detailed: bool,
}

/// The static catalog of diagnostic codes in `locale`, optionally narrowed to
/// `codes`. Unparseable / unknown requested codes are reported back in
/// `unknown_codes` rather than silently dropped, so the agent can correct itself.
pub fn catalog(locale: Locale, codes: &[String]) -> CallToolResult {
    let (entries, unknown): (Vec<_>, Vec<String>) = if codes.is_empty() {
        (diagnostic_catalog(locale), Vec::new())
    } else {
        let mut entries = Vec::with_capacity(codes.len());
        let mut unknown = Vec::new();
        for raw in codes {
            match DiagnosticCode::from_str(raw).ok().and_then(|c| catalog_entry(c, locale)) {
                Some(entry) => entries.push(entry),
                None => unknown.push(raw.clone()),
            }
        }
        (entries, unknown)
    };

    let mut body = json!({
        "action": "catalog",
        "locale": locale_str(locale),
        "count": entries.len(),
        "entries": entries,
    });
    if !unknown.is_empty() {
        body["unknown_codes"] = json!(unknown);
    }
    structured(body)
}

/// Static contract for cold-start discovery, mirroring `graph schema`. `schema_version`
/// is bumped in lockstep with any response-shape change.
pub fn schema() -> CallToolResult {
    structured(schema_json())
}

/// A transient "still building the resident database" result, emitted while the
/// background load runs. Not an error — the agent should retry shortly.
pub fn loading() -> CallToolResult {
    structured(
        json!({ "status": "loading", "detail": "diagnostics database is building; retry shortly" }),
    )
}

/// Wrap a `file` result in the freshness envelope, matching the `graph` tool.
pub fn envelope(freshness: Freshness, result: Value) -> CallToolResult {
    structured(json!({
        "revision": freshness.revision,
        "stale": freshness.stale,
        "reload": freshness.reload,
        "result": result,
    }))
}

/// Parse a `min_severity` floor, defaulting to `warning` (drops info/hint noise by
/// default). An unrecognised label is an error so the agent learns the vocabulary.
pub(crate) fn parse_min_severity(s: Option<&str>) -> Result<SeverityBucket, String> {
    match s {
        None => Ok(SeverityBucket::Warning),
        Some(label) => SeverityBucket::parse(label).ok_or_else(|| {
            format!("unknown min_severity '{label}'; expected error|warning|info|hint")
        }),
    }
}

/// Compute the `file` action's result body from the resident database: resolve the
/// path to its FileId, run diagnostics, then filter (codes / range / floor), bucket
/// severity, and shape findings + the full `counts` histogram + a content-hash
/// `result_id`. Runs inside the resident read lock, on the calling thread.
pub(crate) fn file_findings(
    resident: &DiagnosticsResident,
    path: &Path,
    filters: &FileFilters,
    generation: u64,
) -> Value {
    let Some(file_id) = resident.file_id_for(path) else {
        return json!({
            "error": "not_in_workspace",
            "detail": "path is not a resident workspace .bsl file",
            "path": path.to_string_lossy(),
        });
    };
    let analysis = resident.analysis();
    let file_text = analysis.file_text(file_id);
    // Analyse against the project's effective config (the single source of truth shared
    // with LSP and CLI), so disabled rules and tuned thresholds are honoured.
    let diagnostics = analysis.diagnostics(file_id, resident.config());
    // Method spans for the graph bridge: each finding inside a method carries the
    // method's durable graph id so the agent can pivot to `graph callers`.
    let methods = method_ranges(&analysis.document_symbols(file_id));

    let mut counts = Counts::default();
    let mut findings: Vec<Value> = Vec::new();
    let mut truncated = false;

    for diag in &diagnostics {
        let out = diag.to_output(&file_text);
        if !filters.codes.is_empty() && !filters.codes.iter().any(|c| c == &out.code) {
            continue;
        }
        if let Some((start, end)) = filters.range {
            // Keep a finding whose line span intersects the requested range.
            if out.end_line < start || out.start_line > end {
                continue;
            }
        }
        let bucket = SeverityBucket::from(diag.severity);
        // `counts` is the full histogram of what passed codes/range, before the floor.
        counts.add(bucket);
        if bucket < filters.min_severity {
            continue;
        }
        if findings.len() >= filters.max_findings {
            truncated = true;
            continue;
        }
        let graph_id = graph_id_for(diag.range, &methods, path, resident.workspace_root());
        findings.push(finding_value(diag, &out, bucket, graph_id, filters.detailed));
    }

    json!({
        "result_id": result_id(path, generation, &file_text),
        "kind": "full",
        "counts": counts.to_value(),
        "truncated": truncated,
        "findings": findings,
    })
}

/// The `workspace` action's result body: whole-config diagnostics aggregated per code
/// (`{code, severity, count, files_affected}`), never per-finding — an opt-in, bounded
/// overview. `result_id` is generation-keyed (no per-file content hash applies).
pub(crate) fn workspace_findings(
    resident: &DiagnosticsResident,
    opts: &SweepOptions,
    generation: u64,
) -> Value {
    let sweep = resident.workspace_aggregates(resident.config(), opts);
    let aggregates: Vec<Value> = sweep
        .aggregates
        .iter()
        .map(|a| {
            json!({
                "code": a.code,
                "severity": a.severity.as_str(),
                "count": a.count,
                "files_affected": a.files_affected,
            })
        })
        .collect();
    json!({
        "result_id": format!("workspace@{generation}"),
        "files_swept": sweep.files_swept,
        "files_total": sweep.files_total,
        "truncated": sweep.truncated,
        "aggregates": aggregates,
    })
}

fn finding_value(
    diag: &ide::Diagnostic,
    out: &ide::DiagnosticOutput,
    bucket: SeverityBucket,
    graph_id: Option<String>,
    detailed: bool,
) -> Value {
    let mut v = json!({
        "code": out.code,
        "severity": bucket.as_str(),
        "message": out.message,
        "range": {
            "start_line": out.start_line,
            "start_column": out.start_column,
            "end_line": out.end_line,
            "end_column": out.end_column,
        },
        "has_fix": !diag.fixes.is_empty(),
    });
    if !out.tags.is_empty() {
        v["tags"] = json!(out.tags);
    }
    if let Some(graph_id) = graph_id {
        v["graph_id"] = json!(graph_id);
    }
    if detailed {
        v["internal_severity"] = json!(diag.severity.as_str());
        if let Some(fix) = diag.fixes.first() {
            v["fix"] = json!({ "label": fix.label });
        }
    }
    v
}

/// Flatten document symbols into `(method_name, source_range)` for every procedure
/// and function, descending through `#Область` regions (whose method children are
/// nested). Module-level vars and the regions themselves are skipped.
fn method_ranges(symbols: &[DocumentSymbol]) -> Vec<(String, TextRange)> {
    let mut out = Vec::new();
    collect_methods(symbols, &mut out);
    out
}

fn collect_methods(symbols: &[DocumentSymbol], out: &mut Vec<(String, TextRange)>) {
    for s in symbols {
        if matches!(s.kind, SymbolKind::Procedure | SymbolKind::Function) {
            out.push((s.name.clone(), s.range));
        }
        if !s.children.is_empty() {
            collect_methods(&s.children, out);
        }
    }
}

/// The durable graph id of the method whose span contains `range`, if any. `None` when the
/// finding is not inside a method (module body). Module-keyed methods (common/object/manager)
/// resolve regardless of `workspace_root`; form/command/file-module methods fall back to the
/// `method/file/<rel>::<name>` id the graph also mints — `workspace_root` strips an absolute
/// request path to the encoder's rel. `graph_id` is best-effort decoration.
fn graph_id_for(
    range: TextRange,
    methods: &[(String, TextRange)],
    path: &Path,
    workspace_root: &Path,
) -> Option<String> {
    let name = methods.iter().find(|(_, r)| r.contains_range(range)).map(|(n, _)| n.as_str())?;
    ide::method_graph_id(&path.to_string_lossy(), name, Some(workspace_root))
}

/// The pull-model freshness handle: `<path>@<generation>@<content-hash>`. The content
/// hash MUST be present so a body edit inside the drift-scan throttle window cannot be
/// masked by an unchanged generation (a stale `unchanged` would otherwise be possible
/// once `previous_result_id` lands).
fn result_id(path: &Path, generation: u64, text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{}@{}@{:016x}", path.to_string_lossy(), generation, hasher.finish())
}

/// The four-bucket severity histogram, always emitted so the agent knows what a
/// floor or a cap hid.
#[derive(Default)]
struct Counts {
    error: usize,
    warning: usize,
    info: usize,
    hint: usize,
}

impl Counts {
    fn add(&mut self, bucket: SeverityBucket) {
        match bucket {
            SeverityBucket::Error => self.error += 1,
            SeverityBucket::Warning => self.warning += 1,
            SeverityBucket::Info => self.info += 1,
            SeverityBucket::Hint => self.hint += 1,
        }
    }

    fn to_value(&self) -> Value {
        json!({ "error": self.error, "warning": self.warning, "info": self.info, "hint": self.hint })
    }
}

fn schema_json() -> Value {
    json!({
        "schema_version": "4",
        "actions": ["catalog", "schema", "file", "workspace"],
        "severities": ["error", "warning", "info", "hint"],
        "catalog_entry": {
            "code": "string — stable diagnostic code (e.g. CyclomaticComplexity)",
            "title": "string — localized name",
            "default_severity": "error | warning | info | hint",
            "type": "error | code_smell | vulnerability | security_hotspot",
            "activated_by_default": "bool — whether enabled under the default config",
            "clean_code_attribute": "consistent | intentional | adaptable | responsible",
            "tags": "string[] — omitted when empty"
        },
        "catalog_params": {
            "codes": "string[] — narrow the catalog to these codes (optional)",
            "locale": "ru | en (default ru) — title language"
        },
        "file_params": {
            "path": "string — absolute or workspace-relative .bsl path (required)",
            "min_severity": "error | warning | info | hint (default warning) — inclusive floor",
            "codes": "string[] — keep only these codes (optional)",
            "range_start": "usize — 0-based first line to include (optional)",
            "range_end": "usize — 0-based last line to include (optional)",
            "detail": "concise | detailed (default concise) — detailed adds internal_severity + fix",
            "max_findings": "usize — cap on findings (default 200)"
        },
        "finding": {
            "code": "string",
            "severity": "error | warning | info | hint (4-bucket)",
            "message": "string",
            "range": "{ start_line, start_column, end_line, end_column } — 0-based",
            "tags": "string[] — omitted when empty",
            "has_fix": "bool — whether an automatic fix is attached",
            "graph_id": "durable id of the containing method — pass to `graph callers`/`node` (omitted when not in a method or not an indexable module)",
            "internal_severity": "7-grade name (detailed only)",
            "fix": "{ label } (detailed only, when present)"
        },
        "file_result": {
            "result_id": "<path>@<generation>@<content-hash> — pull-model freshness handle",
            "kind": "full — (unchanged reserved for a future previous_result_id round-trip)",
            "counts": "{ error, warning, info, hint } — full histogram before the floor/cap",
            "truncated": "bool — the findings cap was hit; counts still complete",
            "findings": "finding[]"
        },
        "workspace_params": {
            "min_severity": "error | warning | info | hint (default warning) — inclusive floor",
            "codes": "string[] — keep only these codes (optional)",
            "max_files": "usize — cap on files swept (default 1000, hard ceiling 5000); a larger config surfaces as truncated with the true files_total"
        },
        "workspace_result": {
            "result_id": "workspace@<generation>",
            "files_swept": "usize — files actually analyzed",
            "files_total": "usize — resident files (files_swept < files_total ⇒ capped)",
            "truncated": "bool — the file cap was hit",
            "aggregates": "{ code, severity, count, files_affected }[] — per-code, most-severe-then-most-frequent first; NO per-finding detail"
        },
        "envelope": {
            "revision": "u64 — resident-db generation the answer was computed at",
            "stale": "bool — workspace drifted on disk since this generation",
            "reload": "none | running | failed — background reload state",
            "result": "the action payload (or an {error} object)"
        }
    })
}

fn locale_str(locale: Locale) -> &'static str {
    match locale {
        Locale::Ru => "ru",
        Locale::En => "en",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_of(result: &CallToolResult) -> &Value {
        result.structured_content.as_ref().expect("structuredContent must be populated")
    }

    /// The text content block must parse back to exactly the `structuredContent`
    /// field, so structured-aware and plain clients see byte-identical JSON.
    fn assert_structured_mirrors_text(result: &CallToolResult) {
        let structured = body_of(result);
        let text = result.content[0].raw.as_text().expect("text mirror").text.as_str();
        let parsed: Value = serde_json::from_str(text).expect("text mirror must be valid JSON");
        assert_eq!(&parsed, structured, "text mirror must match structuredContent");
    }

    #[test]
    fn schema_advertises_the_catalog_contract() {
        let result = schema();
        assert_structured_mirrors_text(&result);
        let body = body_of(&result);
        assert_eq!(body["schema_version"], "4");
        let actions = body["actions"].as_array().unwrap();
        assert!(actions.iter().any(|a| a == "catalog"));
        assert!(actions.iter().any(|a| a == "file"));
        assert!(actions.iter().any(|a| a == "workspace"));
        assert!(body["finding"]["graph_id"].is_string(), "graph bridge advertised");
        assert!(body["workspace_result"]["aggregates"].is_string(), "workspace advertised");
        let sev = body["severities"].as_array().unwrap();
        assert_eq!(sev.len(), 4);
        assert!(sev.iter().any(|s| s == "error"));
        assert!(sev.iter().any(|s| s == "hint"));
        // The file contract is advertised for cold-start discovery.
        assert!(body["file_params"]["path"].is_string());
        assert!(body["file_result"]["result_id"].is_string());
        assert!(body["envelope"]["revision"].is_string());
    }

    #[test]
    fn min_severity_defaults_to_warning_and_rejects_unknown() {
        assert_eq!(parse_min_severity(None).unwrap(), SeverityBucket::Warning);
        assert_eq!(parse_min_severity(Some("error")).unwrap(), SeverityBucket::Error);
        assert_eq!(parse_min_severity(Some("hint")).unwrap(), SeverityBucket::Hint);
        assert!(parse_min_severity(Some("blocker")).is_err());
    }

    #[test]
    fn catalog_lists_every_code_with_required_fields() {
        let result = catalog(Locale::Ru, &[]);
        assert_structured_mirrors_text(&result);
        let body = body_of(&result);
        assert_eq!(body["action"], "catalog");
        assert_eq!(body["locale"], "ru");
        let count = body["count"].as_u64().unwrap();
        assert!(count >= 170, "expected the full catalog, got {count}");
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len() as u64, count);
        let first = &entries[0];
        for field in ["code", "title", "default_severity", "type", "activated_by_default"] {
            assert!(!first[field].is_null(), "entry missing `{field}`");
        }
        assert!(body.get("unknown_codes").is_none(), "no unknown codes for full catalog");
    }

    #[test]
    fn catalog_filters_to_requested_codes() {
        let result = catalog(Locale::En, &["CyclomaticComplexity".to_string()]);
        let body = body_of(&result);
        assert_eq!(body["count"], 1);
        assert_eq!(body["entries"][0]["code"], "CyclomaticComplexity");
        assert!(body.get("unknown_codes").is_none());
    }

    #[test]
    fn catalog_reports_unknown_codes() {
        let result =
            catalog(Locale::Ru, &["CyclomaticComplexity".to_string(), "NoSuchCode".to_string()]);
        let body = body_of(&result);
        assert_eq!(body["count"], 1, "only the valid code yields an entry");
        let unknown = body["unknown_codes"].as_array().unwrap();
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0], "NoSuchCode");
    }

    mod file_action {
        use super::*;
        use crate::diagnostics_state::{
            DiagnosticsState, DiagnosticsStatus, ResidentOutcome, SweepOptions,
        };
        use std::fs;
        use std::path::{Path, PathBuf};
        use std::time::Duration;

        fn write(root: &Path, rel: &str, text: &str) {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }

        fn write_common_module(root: &Path, name: &str, body: &str) {
            let xml = format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core">
	<CommonModule uuid="00000000-0000-0000-0000-0000000000{id:02}">
		<Properties>
			<Name>{name}</Name>
			<Global>false</Global>
			<ClientManagedApplication>false</ClientManagedApplication>
			<Server>true</Server>
			<ExternalConnection>false</ExternalConnection>
			<ClientOrdinaryApplication>false</ClientOrdinaryApplication>
			<ServerCall>false</ServerCall>
			<Privileged>false</Privileged>
			<ReturnValuesReuse>DontUse</ReturnValuesReuse>
		</Properties>
	</CommonModule>
</MetaDataObject>"#,
                id = name.len(),
            );
            write(root, &format!("CommonModules/{name}.xml"), &xml);
            write(root, &format!("CommonModules/{name}/Ext/Module.bsl"), body);
        }

        fn sample_workspace(root: &Path) {
            write_common_module(
                root,
                "Сервер",
                "&НаСервере\nФункция Считать() Экспорт Возврат 1; КонецФункции\n",
            );
        }

        fn ready_state(root: &Path) -> DiagnosticsState {
            let state = DiagnosticsState::for_workspace(root.to_path_buf());
            state.ensure_loading();
            for _ in 0..300 {
                match state.status() {
                    DiagnosticsStatus::Ready { .. } => return state,
                    DiagnosticsStatus::Failed(m) => panic!("load failed: {m}"),
                    _ => std::thread::sleep(Duration::from_millis(10)),
                }
            }
            panic!("db did not become ready");
        }

        fn run(state: &DiagnosticsState, path: &Path, filters: &FileFilters) -> Value {
            // `read` supplies the generation under the lock, so the closure must not
            // re-lock `&self`.
            match state
                .read(|resident, generation| file_findings(resident, path, filters, generation))
            {
                ResidentOutcome::Ready(v, _) => v,
                _ => panic!("expected Ready outcome"),
            }
        }

        fn default_filters() -> FileFilters {
            FileFilters {
                min_severity: SeverityBucket::Hint, // keep everything for shape assertions
                codes: Vec::new(),
                range: None,
                max_findings: DEFAULT_MAX_FINDINGS,
                detailed: false,
            }
        }

        #[test]
        fn file_findings_shapes_the_result() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            sample_workspace(root);
            let state = ready_state(root);
            let path = root.join("CommonModules/Сервер/Ext/Module.bsl");

            let body = run(&state, &path, &default_filters());

            assert_eq!(body["kind"], "full");
            assert!(body["truncated"].is_boolean());
            for sev in ["error", "warning", "info", "hint"] {
                assert!(body["counts"][sev].is_u64(), "counts.{sev} present");
            }
            assert!(body["findings"].is_array());
            // result_id = <path>@<generation>@<content-hash>.
            let id = body["result_id"].as_str().unwrap();
            let parts: Vec<&str> = id.rsplitn(3, '@').collect();
            assert_eq!(parts.len(), 3, "result_id has path@gen@hash shape: {id}");
            assert_eq!(parts[0].len(), 16, "content hash is 16 hex chars");
        }

        #[test]
        fn unknown_path_is_an_in_band_error() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            sample_workspace(root);
            let state = ready_state(root);

            let body = run(&state, &PathBuf::from("/nope/Missing.bsl"), &default_filters());
            assert_eq!(body["error"], "not_in_workspace");
        }

        #[test]
        fn codes_filter_narrows_findings() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            sample_workspace(root);
            let state = ready_state(root);
            let path = root.join("CommonModules/Сервер/Ext/Module.bsl");

            // A code that cannot match drops every finding while counts stay structural.
            let filters =
                FileFilters { codes: vec!["NoSuchCodeFilter".to_string()], ..default_filters() };
            let body = run(&state, &path, &filters);
            assert_eq!(body["findings"].as_array().unwrap().len(), 0);
        }

        /// The project's diagnostics settings reach the findings end-to-end, not just
        /// the resident's stored config: the same module yields a LineLength finding
        /// under a tiny threshold and none under a huge one.
        #[test]
        fn file_findings_honor_project_threshold() {
            let body = "Функция Ф() Экспорт\n\tВозврат 1;\nКонецФункции\n";
            let has_line_length = |v: &Value| {
                v["findings"].as_array().unwrap().iter().any(|f| f["code"] == "LineLength")
            };

            // A tiny maxLineLength fires LineLength on ordinary lines.
            let tight = tempfile::tempdir().unwrap();
            write_common_module(tight.path(), "Модуль", body);
            write(
                tight.path(),
                "bsl-analyzer.toml",
                "[diagnostics.parameters.LineLength]\nmaxLineLength = 5\n",
            );
            let tight_state = ready_state(tight.path());
            let tight_body = run(
                &tight_state,
                &tight.path().join("CommonModules/Модуль/Ext/Module.bsl"),
                &default_filters(),
            );
            assert!(has_line_length(&tight_body), "tiny threshold fires LineLength");

            // A huge maxLineLength suppresses it for the same module.
            let loose = tempfile::tempdir().unwrap();
            write_common_module(loose.path(), "Модуль", body);
            write(
                loose.path(),
                "bsl-analyzer.toml",
                "[diagnostics.parameters.LineLength]\nmaxLineLength = 5000\n",
            );
            let loose_state = ready_state(loose.path());
            let loose_body = run(
                &loose_state,
                &loose.path().join("CommonModules/Модуль/Ext/Module.bsl"),
                &default_filters(),
            );
            assert!(!has_line_length(&loose_body), "huge threshold suppresses LineLength");
        }

        fn run_workspace(state: &DiagnosticsState, opts: &SweepOptions) -> Value {
            match state.read(|resident, gen| workspace_findings(resident, opts, gen)) {
                ResidentOutcome::Ready(v, _) => v,
                _ => panic!("expected Ready outcome"),
            }
        }

        fn sweep_opts(max_files: usize) -> SweepOptions {
            SweepOptions { min_severity: SeverityBucket::Hint, codes: Vec::new(), max_files }
        }

        #[test]
        fn workspace_sweep_shapes_the_result() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            sample_workspace(root);
            let state = ready_state(root);

            let body = run_workspace(&state, &sweep_opts(DEFAULT_MAX_SWEEP_FILES));
            assert_eq!(body["files_total"], 1);
            assert_eq!(body["files_swept"], 1);
            assert_eq!(body["truncated"], false);
            assert!(body["aggregates"].is_array(), "per-code aggregates, no per-finding detail");
            assert!(body["result_id"].as_str().unwrap().starts_with("workspace@"));
        }

        #[test]
        fn workspace_sweep_honors_the_file_cap() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            write_common_module(root, "Сервер", "Функция Ф() Экспорт Возврат 1; КонецФункции\n");
            write_common_module(root, "Клиент", "Процедура П() Экспорт КонецПроцедуры\n");
            let state = ready_state(root);

            let body = run_workspace(&state, &sweep_opts(1));
            assert_eq!(body["files_total"], 2);
            assert_eq!(body["files_swept"], 1, "capped at one file");
            assert_eq!(body["truncated"], true);
        }
    }

    #[test]
    fn graph_id_maps_a_finding_to_its_containing_method() {
        let method = DocumentSymbol {
            name: "Считать".to_string(),
            kind: SymbolKind::Function,
            range: TextRange::new(10u32.into(), 50u32.into()),
            selection_range: TextRange::new(10u32.into(), 20u32.into()),
            children: Vec::new(),
        };
        let methods = method_ranges(std::slice::from_ref(&method));
        let root = std::path::Path::new("/ws");
        let module = std::path::Path::new("/ws/CommonModules/Сервер/Ext/Module.bsl");

        // A finding inside the method span resolves to the method's durable graph id.
        let inside = TextRange::new(30u32.into(), 35u32.into());
        assert_eq!(
            graph_id_for(inside, &methods, module, root).as_deref(),
            Some("method/common/Сервер/Считать")
        );
        // A module-body finding (outside any method) carries no graph id.
        let outside = TextRange::new(0u32.into(), 5u32.into());
        assert_eq!(graph_id_for(outside, &methods, module, root), None);
        // A form module: a finding inside a method now falls back to the
        // `method/file/<rel>::<name>` id the graph mints, with the rel stripped to `root`.
        let form = std::path::Path::new("/ws/CommonForms/Форма/Ext/Form/Module.bsl");
        assert_eq!(
            graph_id_for(inside, &methods, form, root).as_deref(),
            Some("method/file/CommonForms/Форма/Ext/Form/Module.bsl::Считать")
        );
    }

    #[test]
    fn method_ranges_descends_into_regions() {
        // A function nested inside an `#Область` region (a non-method symbol with
        // children) is still collected.
        let region = DocumentSymbol {
            name: "Служебные".to_string(),
            kind: SymbolKind::Region,
            range: TextRange::new(0u32.into(), 100u32.into()),
            selection_range: TextRange::new(0u32.into(), 10u32.into()),
            children: vec![DocumentSymbol {
                name: "Внутренняя".to_string(),
                kind: SymbolKind::Procedure,
                range: TextRange::new(20u32.into(), 80u32.into()),
                selection_range: TextRange::new(20u32.into(), 30u32.into()),
                children: Vec::new(),
            }],
        };
        let methods = method_ranges(std::slice::from_ref(&region));
        assert_eq!(methods.len(), 1, "the region itself is not a method, its child is");
        assert_eq!(methods[0].0, "Внутренняя");
    }

    #[test]
    fn result_id_folds_the_content_hash() {
        let path = std::path::Path::new("/ws/Mod.bsl");
        let a = result_id(path, 3, "Процедура А() КонецПроцедуры");
        let a_again = result_id(path, 3, "Процедура А() КонецПроцедуры");
        let b = result_id(path, 3, "Процедура Б() КонецПроцедуры");
        assert_eq!(a, a_again, "same path+gen+content → same id");
        assert_ne!(a, b, "different content → different id even at the same generation");
        assert!(a.starts_with("/ws/Mod.bsl@3@"), "id is <path>@<gen>@<hash>: {a}");
    }
}
