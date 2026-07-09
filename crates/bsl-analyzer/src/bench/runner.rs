//! Single-point benchmark execution.
//!
//! One invocation = one booted workspace = one measurement point; process-cold
//! isolation is the orchestrator's job (`scripts/bench/run-matrix.sh` spawns a
//! fresh process per point). Inside the process: boot → resolve target → one
//! timed cold call → K timed warm repeats. The result invariant is enforced on
//! every observation, so a run can never silently measure a no-op.
//!
//! Measurement boundaries follow the plan's boundary table: features whose
//! logic lives in `ide::Analysis` are timed at that API; handler-boundary
//! features (`semantic_tokens_full`, `code_action`, `diagnostics_pull`) are
//! timed at the LSP handler with a pre-built frozen context (context
//! construction is reported separately as `ctx_build_ns` — production pays it
//! per request in dispatch, and Tier B measures that end-to-end).

use std::path::{Path, PathBuf};
use std::time::Instant;

use line_index::LineIndex;
use lsp_types::Url;
use syntax::{TextRange, TextSize};

use crate::bench::manifest::{self, EditPatch, FeatureSpec, Target};
use crate::bench::report::{percentile_ns, EditPhases, PointReport, REPORT_SCHEMA_VERSION};
use crate::frozen_context::{FrozenFilePaths, LatencyRequestContext};
use crate::global_state::GlobalState;
use crate::handlers::{notification, request};
use crate::smoke::{bootstrap_smoke, Budgets, Scenario, SmokeArgs};

pub const DEFAULT_WARM_ITERATIONS: usize = 20;
pub const DEFAULT_BOOT_BUDGET_MS: u64 = 120_000;

#[derive(Debug)]
pub enum RunError {
    /// Manifest unreadable / invalid / point id unknown → CLI exit 2.
    Manifest(String),
    /// Invariant or file-hash violation: a no-op or drifted target → CLI exit 3.
    Invariant(String),
    /// Everything else (boot failure, missing file, …) → CLI exit 1.
    Other(String),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Manifest(msg) => write!(f, "manifest: {msg}"),
            RunError::Invariant(msg) => write!(f, "invariant: {msg}"),
            RunError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunArgs {
    pub source_dir: PathBuf,
    pub manifest_path: PathBuf,
    pub point_id: String,
    pub warm_iterations: usize,
    pub boot_budget_ms: u64,
}

/// A feature invocation's observable outcome: result cardinality plus a
/// canonical, order-normalized digest input. Digest lines are built from
/// `Debug` renderings *after* the timed call returns, so summarization cost
/// never pollutes the latency sample.
pub(crate) struct Observation {
    pub(crate) count: usize,
    pub(crate) digest_input: String,
}

impl Observation {
    fn from_lines(count: usize, mut lines: Vec<String>) -> Self {
        lines.sort();
        Observation { count, digest_input: lines.join("\n") }
    }

    pub(crate) fn digest_hex(&self) -> String {
        manifest::hash_text(&self.digest_input)
    }
}

pub(crate) struct BenchEnv {
    pub(crate) state: GlobalState,
    // Keeps the client-message drain thread alive for the whole measurement.
    _drain_handle: std::thread::JoinHandle<()>,
    pub(crate) boot_ms: u64,
    pub(crate) workspace_root: PathBuf,
    pub(crate) ctx_build_ns: Option<u64>,
    /// Cached frozen VFS path table for handler contexts: freezing is O(files)
    /// (25k+ on ERP), and paths only change when the runner itself applies an
    /// edit — which resets this cache.
    frozen_paths: Option<FrozenFilePaths>,
}

pub(crate) struct ResolvedTarget {
    pub(crate) file_id: vfs::FileId,
    pub(crate) url: Url,
    pub(crate) text: String,
}

pub(crate) fn boot(source_dir: &Path, boot_budget_ms: u64) -> Result<BenchEnv, RunError> {
    // Canonicalize first: VFS paths and file URLs require absolute paths, and
    // the CLI default source dir is `.`.
    let source_dir = std::fs::canonicalize(source_dir).map_err(|e| {
        RunError::Other(format!("cannot canonicalize workspace root {}: {e}", source_dir.display()))
    })?;
    let args = SmokeArgs {
        source_dir: source_dir.clone(),
        scenarios: Vec::<Scenario>::new(),
        budgets: Budgets { boot_vfs_done_ms: boot_budget_ms, ..Budgets::default() },
        json: false,
    };
    let bootstrap = bootstrap_smoke(&args).map_err(RunError::Other)?;
    Ok(BenchEnv {
        state: bootstrap.state,
        _drain_handle: bootstrap._drain_handle,
        boot_ms: bootstrap.boot.vfs_done_ms,
        workspace_root: source_dir,
        ctx_build_ns: None,
        frozen_paths: None,
    })
}

pub(crate) fn resolve_target(
    env: &BenchEnv,
    relative_path: &str,
    expected_hash: Option<&str>,
) -> Result<ResolvedTarget, RunError> {
    let abs = env.workspace_root.join(relative_path);
    let file_id = lookup_file_id(&env.state, &abs).ok_or_else(|| {
        RunError::Other(format!("target file not in workspace VFS: {}", abs.display()))
    })?;
    let url = Url::from_file_path(&abs)
        .map_err(|_| RunError::Other(format!("cannot build file URL for {}", abs.display())))?;
    let text = env.state.analysis_host.analysis().file_text(file_id);
    if let Some(expected) = expected_hash {
        let actual = manifest::hash_text(&text);
        if !expected.eq_ignore_ascii_case(&actual) {
            return Err(RunError::Invariant(format!(
                "file_hash mismatch for {relative_path}: manifest {expected}, workspace {actual} \
                 — the workspace drifted since discovery"
            )));
        }
    }
    Ok(ResolvedTarget { file_id, url, text })
}

fn lookup_file_id(state: &GlobalState, abs: &Path) -> Option<vfs::FileId> {
    let vfs = state.vfs.read();
    if let Some(id) = vfs.file_id(&vfs::VfsPath::new(abs.to_path_buf())) {
        return Some(id);
    }
    let canonical = std::fs::canonicalize(abs).ok()?;
    vfs.file_id(&vfs::VfsPath::new(canonical))
}

pub fn run_point(args: &RunArgs) -> Result<PointReport, RunError> {
    let manifest = manifest::load(&args.manifest_path).map_err(RunError::Manifest)?;
    let target = manifest
        .targets
        .iter()
        .find(|t| t.id == args.point_id)
        .ok_or_else(|| {
            RunError::Manifest(format!("point `{}` not found in manifest", args.point_id))
        })?
        .clone();

    let mut env = boot(&args.source_dir, args.boot_budget_ms)?;
    let resolved = resolve_target(&env, &target.relative_path, Some(&target.file_hash))?;

    match &target.spec {
        FeatureSpec::Edit { patch, edit_kind, followup } => {
            run_edit_point(args, &mut env, &target, &resolved, patch, *edit_kind, followup)
        }
        spec => run_plain_point(args, &mut env, &target, &resolved, spec),
    }
}

fn run_plain_point(
    args: &RunArgs,
    env: &mut BenchEnv,
    target: &Target,
    resolved: &ResolvedTarget,
    spec: &FeatureSpec,
) -> Result<PointReport, RunError> {
    ensure_overlay(env, resolved, spec)?;

    let (cold_ns, cold_obs) = execute_once(env, resolved, spec)?;
    let mut warm_ns = Vec::with_capacity(args.warm_iterations);
    for _ in 0..args.warm_iterations {
        let (ns, obs) = execute_once(env, resolved, spec)?;
        if obs.count != cold_obs.count {
            return Err(RunError::Invariant(format!(
                "unstable result: cold count {} vs warm count {}",
                cold_obs.count, obs.count
            )));
        }
        warm_ns.push(ns);
    }

    finish_report(env, target, &cold_obs, cold_ns, warm_ns, None)
}

#[allow(clippy::too_many_arguments, reason = "one call site; grouping would only rename the args")]
fn run_edit_point(
    args: &RunArgs,
    env: &mut BenchEnv,
    target: &Target,
    resolved: &ResolvedTarget,
    patch: &EditPatch,
    edit_kind: manifest::EditKind,
    followup: &FeatureSpec,
) -> Result<PointReport, RunError> {
    let text = &resolved.text;
    let (start, end) = (patch.range.start as usize, patch.range.end as usize);
    if end > text.len() || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        return Err(RunError::Manifest(format!(
            "patch range {start}..{end} out of bounds or splits a UTF-8 char (file len {})",
            text.len()
        )));
    }

    // Resident overlay for the document; the edit itself flows through the
    // real didChange path.
    open_document(env, resolved)?;
    ensure_overlay(env, resolved, followup)?;

    // Steady warm baseline of the followup before the edit.
    let _ = execute_once(env, resolved, followup)?;
    let mut warm_before = Vec::with_capacity(args.warm_iterations);
    for _ in 0..args.warm_iterations {
        let (ns, _) = execute_once(env, resolved, followup)?;
        warm_before.push(ns);
    }

    let mut patched = String::with_capacity(text.len() + patch.new_text.len());
    patched.push_str(&text[..start]);
    patched.push_str(&patch.new_text);
    patched.push_str(&text[end..]);

    // Fully built before the timer: `edit_apply_ns` must cover the didChange
    // handler alone, not the benchmark's own document clone.
    let change_params = lsp_types::DidChangeTextDocumentParams {
        text_document: lsp_types::VersionedTextDocumentIdentifier {
            uri: resolved.url.clone(),
            version: 2,
        },
        content_changes: vec![lsp_types::TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: patched.clone(),
        }],
    };
    let apply_start = Instant::now();
    notification::handle_did_change(&mut env.state, change_params)
        .map_err(|e| RunError::Other(format!("didChange failed: {e}")))?;
    let edit_apply_ns = apply_start.elapsed().as_nanos() as u64;
    env.frozen_paths = None;

    // handle_did_change swallows a rejected edit (logs and returns Ok), so a
    // dropped patch would silently re-measure unchanged text — verify.
    let applied = env.state.mem_docs.get(&resolved.url);
    if applied.as_deref() != Some(patched.as_str()) {
        return Err(RunError::Other(
            "didChange did not apply: document text differs from the patched text".to_string(),
        ));
    }

    // Followup offsets are validated to precede the patch, so the pre-edit
    // resolved target stays valid; only the text snapshot must be refreshed.
    let post =
        ResolvedTarget { file_id: resolved.file_id, url: resolved.url.clone(), text: patched };

    let (after_edit_ns, after_obs) = execute_once(env, &post, followup)?;
    let mut warm_after = Vec::with_capacity(args.warm_iterations);
    for _ in 0..args.warm_iterations {
        let (ns, obs) = execute_once(env, &post, followup)?;
        if obs.count != after_obs.count {
            return Err(RunError::Invariant(format!(
                "unstable post-edit result: after-edit count {} vs warm count {}",
                after_obs.count, obs.count
            )));
        }
        warm_after.push(ns);
    }

    let phases = EditPhases {
        edit_kind: edit_kind.as_str().to_string(),
        warm_before_p50_ns: percentile_ns(&warm_before, 50),
        edit_apply_ns,
        after_edit_ns,
    };
    finish_report(env, target, &after_obs, after_edit_ns, warm_after, Some(phases))
}

fn finish_report(
    env: &BenchEnv,
    target: &Target,
    observation: &Observation,
    cold_ns: u64,
    warm_ns: Vec<u64>,
    edit: Option<EditPhases>,
) -> Result<PointReport, RunError> {
    let digest = observation.digest_hex();
    let invariant = target.expect.check(observation.count, &digest);
    let report = PointReport {
        schema_version: REPORT_SCHEMA_VERSION,
        point_id: target.id.clone(),
        feature: target.spec.feature_name().to_string(),
        mode: "latency".to_string(),
        workspace_root: env.workspace_root.display().to_string(),
        relative_path: target.relative_path.clone(),
        boot_ms: env.boot_ms,
        ctx_build_ns: env.ctx_build_ns,
        cold_ns,
        warm_p50_ns: percentile_ns(&warm_ns, 50),
        warm_p95_ns: percentile_ns(&warm_ns, 95),
        warm_ns,
        observed_count: observation.count,
        digest,
        invariant_ok: invariant.is_ok(),
        invariant_error: invariant.as_ref().err().cloned(),
        edit,
    };
    match invariant {
        Ok(()) => Ok(report),
        Err(e) => Err(RunError::Invariant(format!(
            "{} [{}]: {e}\nreport: {}",
            report.point_id,
            report.feature,
            serde_json::to_string(&report).unwrap_or_default()
        ))),
    }
}

/// didOpen overlay for handler-boundary features, per the plan's boundary
/// table. Runs the real `didOpen` handler so the document becomes a resident
/// VFS overlay with a matching salsa text input — a raw `mem_docs` insert
/// would leave `file_text` disk-backed and trip the lazy-text revision guard
/// on the first post-edit read. Core `ide::Analysis` features run without an
/// overlay.
pub(crate) fn ensure_overlay(
    env: &mut BenchEnv,
    resolved: &ResolvedTarget,
    spec: &FeatureSpec,
) -> Result<(), RunError> {
    let needs = match spec {
        FeatureSpec::SemanticTokensFull
        | FeatureSpec::CodeAction { .. }
        | FeatureSpec::DiagnosticsPull => true,
        FeatureSpec::Burst { sequence } => sequence.iter().any(|s| {
            matches!(
                s,
                FeatureSpec::SemanticTokensFull
                    | FeatureSpec::CodeAction { .. }
                    | FeatureSpec::DiagnosticsPull
            )
        }),
        _ => false,
    };
    if needs {
        open_document(env, resolved)?;
    }
    Ok(())
}

/// Inert didOpen (idempotent per URL): replicates the resident-overlay steps
/// of `handle_did_open` — mem_docs insert, open-file marking *before*
/// `process_changes`, VFS contents, and the explicit overlay re-pin — while
/// deliberately skipping `preload_dependencies` and `schedule_diagnostics`.
/// The real handler warms exactly the caches the timed call is about to read
/// (dependency preload runs synchronously) and spawns concurrent pool tasks,
/// which would turn the "process-cold" number into a warmed, jittery one. The
/// production didOpen pipeline itself is Tier B's measurement, not Tier A's.
pub(crate) fn open_document(env: &mut BenchEnv, resolved: &ResolvedTarget) -> Result<(), RunError> {
    use base_db::SourceDatabase as _;

    if env.state.mem_docs.get(&resolved.url).is_some() {
        return Ok(());
    }
    let state = &mut env.state;
    state.mem_docs.insert(resolved.url.clone(), resolved.text.clone(), 1);
    state.open_files.insert(resolved.file_id);
    {
        let path = resolved
            .url
            .to_file_path()
            .map_err(|()| RunError::Other(format!("not a file URI: {}", resolved.url)))?;
        let mut vfs = state.vfs.write();
        vfs.set_file_contents(
            vfs::VfsPath::new(path),
            Some(std::sync::Arc::from(resolved.text.as_str())),
        );
    }
    state.process_changes(false);

    // Same re-pin as handle_did_open: a content-identical open produces no
    // change event, leaving the file disk-backed; pin the overlay explicitly.
    if state.analysis_host.raw_database().try_file_text_input(resolved.file_id).is_none() {
        state.analysis_host.request_cancellation();
        let db = state.analysis_host.raw_database_mut();
        ide_host_core::set_file_text_source(
            db,
            resolved.file_id,
            ide_host_core::FileTextSource::Overlay(&resolved.text),
        );
    }
    env.frozen_paths = None;
    Ok(())
}

/// One timed invocation. Only the entrypoint call sits between the `Instant`
/// reads; observation summarization happens afterwards.
pub(crate) fn execute_once(
    env: &mut BenchEnv,
    resolved: &ResolvedTarget,
    spec: &FeatureSpec,
) -> Result<(u64, Observation), RunError> {
    let file_id = resolved.file_id;
    match spec {
        FeatureSpec::Hover { offset } => {
            let a = env.state.analysis_host.analysis();
            let t = Instant::now();
            let r = a.hover(file_id, *offset, ide::Locale::default());
            let ns = t.elapsed().as_nanos() as u64;
            let lines = r.iter().map(|h| format!("{h:?}")).collect();
            Ok((ns, Observation::from_lines(r.is_some() as usize, lines)))
        }
        FeatureSpec::Completion { offset } => {
            let root = env.state.workspace_root.clone();
            let a = env.state.analysis_host.analysis();
            let t = Instant::now();
            let r = a.completions(file_id, *offset, root, ide::Locale::default());
            let ns = t.elapsed().as_nanos() as u64;
            let lines = r.iter().map(|c| format!("{c:?}")).collect();
            Ok((ns, Observation::from_lines(r.len(), lines)))
        }
        FeatureSpec::GotoDefinition { offset } => {
            let a = env.state.analysis_host.analysis();
            let t = Instant::now();
            let r = a.goto_definition(file_id, *offset);
            let ns = t.elapsed().as_nanos() as u64;
            let lines = r.iter().map(|n| format!("{n:?}")).collect();
            Ok((ns, Observation::from_lines(r.is_some() as usize, lines)))
        }
        FeatureSpec::TypeDefinition { offset } => {
            let a = env.state.analysis_host.analysis();
            let t = Instant::now();
            let r = a.type_definition(file_id, *offset);
            let ns = t.elapsed().as_nanos() as u64;
            let lines = r.iter().map(|n| format!("{n:?}")).collect();
            Ok((ns, Observation::from_lines(r.is_some() as usize, lines)))
        }
        FeatureSpec::References { offset } => {
            let a = env.state.analysis_host.analysis();
            let t = Instant::now();
            let r = a.find_references(file_id, *offset);
            let ns = t.elapsed().as_nanos() as u64;
            let lines = r.iter().map(|l| format!("{l:?}")).collect();
            Ok((ns, Observation::from_lines(r.len(), lines)))
        }
        FeatureSpec::Rename { offset, new_name } => {
            let a = env.state.analysis_host.analysis();
            let t = Instant::now();
            let r = a.rename(file_id, *offset, new_name);
            let ns = t.elapsed().as_nanos() as u64;
            let obs = match r {
                Ok(locations) => {
                    let lines = locations.iter().map(|l| format!("{l:?}")).collect();
                    Observation::from_lines(locations.len(), lines)
                }
                Err(e) => Observation::from_lines(0, vec![format!("rename error: {e:?}")]),
            };
            Ok((ns, obs))
        }
        FeatureSpec::CallHierarchyPrepare { offset } => {
            let a = env.state.analysis_host.analysis();
            let t = Instant::now();
            let r = a.prepare_call_hierarchy(file_id, *offset);
            let ns = t.elapsed().as_nanos() as u64;
            let lines = r.iter().map(|i| format!("{i:?}")).collect();
            Ok((ns, Observation::from_lines(r.is_some() as usize, lines)))
        }
        FeatureSpec::CallHierarchyIncoming { offset } => {
            let a = env.state.analysis_host.analysis();
            let t = Instant::now();
            let r = a.call_hierarchy_incoming(file_id, *offset);
            let ns = t.elapsed().as_nanos() as u64;
            let lines = r.iter().map(|c| format!("{c:?}")).collect();
            Ok((ns, Observation::from_lines(r.len(), lines)))
        }
        FeatureSpec::CallHierarchyOutgoing { offset } => {
            let a = env.state.analysis_host.analysis();
            let t = Instant::now();
            let r = a.call_hierarchy_outgoing(file_id, *offset);
            let ns = t.elapsed().as_nanos() as u64;
            let lines = r.iter().map(|c| format!("{c:?}")).collect();
            Ok((ns, Observation::from_lines(r.len(), lines)))
        }
        FeatureSpec::InlayHints { range } => {
            let text_range = match range {
                Some(r) => TextRange::new(TextSize::from(r.start), TextSize::from(r.end)),
                None => {
                    TextRange::new(TextSize::from(0), TextSize::from(resolved.text.len() as u32))
                }
            };
            let a = env.state.analysis_host.analysis();
            let t = Instant::now();
            let r = a.inlay_hints(file_id, text_range);
            let ns = t.elapsed().as_nanos() as u64;
            let lines = r.iter().map(|h| format!("{h:?}")).collect();
            Ok((ns, Observation::from_lines(r.len(), lines)))
        }
        FeatureSpec::SelectionRange { offsets } => {
            let sizes: Vec<TextSize> = offsets.iter().map(|&o| TextSize::from(o)).collect();
            let a = env.state.analysis_host.analysis();
            let t = Instant::now();
            let r = a.selection_ranges(file_id, &sizes);
            let ns = t.elapsed().as_nanos() as u64;
            let count = r.iter().map(|chain| chain.len()).sum();
            let lines = r.iter().map(|chain| format!("{chain:?}")).collect();
            Ok((ns, Observation::from_lines(count, lines)))
        }
        FeatureSpec::DocumentSymbol => {
            let a = env.state.analysis_host.analysis();
            let t = Instant::now();
            let r = a.document_symbols(file_id);
            let ns = t.elapsed().as_nanos() as u64;
            let lines = r.iter().map(|s| format!("{s:?}")).collect();
            Ok((ns, Observation::from_lines(r.len(), lines)))
        }
        FeatureSpec::FoldingRange => {
            let a = env.state.analysis_host.analysis();
            let t = Instant::now();
            let r = a.folding_ranges(file_id);
            let ns = t.elapsed().as_nanos() as u64;
            let lines = r.iter().map(|f| format!("{f:?}")).collect();
            Ok((ns, Observation::from_lines(r.len(), lines)))
        }
        FeatureSpec::SignatureHelp { offset } => {
            let a = env.state.analysis_host.analysis();
            let t = Instant::now();
            let r = a.signature_help(file_id, *offset);
            let ns = t.elapsed().as_nanos() as u64;
            let lines = r.iter().map(|s| format!("{s:?}")).collect();
            Ok((ns, Observation::from_lines(r.is_some() as usize, lines)))
        }
        FeatureSpec::WorkspaceSymbol { query } => {
            let a = env.state.analysis_host.analysis();
            let t = Instant::now();
            let r = a.workspace_symbols(query);
            let ns = t.elapsed().as_nanos() as u64;
            let lines = r.iter().map(|s| format!("{s:?}")).collect();
            Ok((ns, Observation::from_lines(r.len(), lines)))
        }
        FeatureSpec::DiagnosticsPush => {
            let config = env.state.diagnostics_config.clone();
            let a = env.state.analysis_host.analysis();
            let t = Instant::now();
            let r = a.file_diagnostics_cached(file_id, config);
            let ns = t.elapsed().as_nanos() as u64;
            let lines = r.iter().map(|d| format!("{d:?}")).collect();
            Ok((ns, Observation::from_lines(r.len(), lines)))
        }
        FeatureSpec::SemanticTokensFull => {
            let ctx = build_latency_ctx(env);
            let params = lsp_types::SemanticTokensParams {
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                text_document: lsp_types::TextDocumentIdentifier { uri: resolved.url.clone() },
            };
            let t = Instant::now();
            let r = request::handle_semantic_tokens_full(ctx, params)
                .map_err(|e| RunError::Other(format!("semantic tokens handler failed: {e}")))?;
            let ns = t.elapsed().as_nanos() as u64;
            let count = match &r {
                Some(lsp_types::SemanticTokensResult::Tokens(tokens)) => tokens.data.len(),
                Some(lsp_types::SemanticTokensResult::Partial(partial)) => partial.data.len(),
                None => 0,
            };
            let lines = vec![format!("{r:?}")];
            Ok((ns, Observation::from_lines(count, lines)))
        }
        FeatureSpec::CodeAction { range } => {
            let lsp_range = offset_range_to_lsp(&resolved.text, range.start, range.end)
                .ok_or_else(|| {
                    RunError::Manifest(format!(
                        "code_action range {}..{} out of bounds",
                        range.start, range.end
                    ))
                })?;
            let ctx = build_latency_ctx(env);
            let params = lsp_types::CodeActionParams {
                text_document: lsp_types::TextDocumentIdentifier { uri: resolved.url.clone() },
                range: lsp_range,
                context: lsp_types::CodeActionContext::default(),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            let t = Instant::now();
            let r = request::handle_code_action(ctx, params)
                .map_err(|e| RunError::Other(format!("code action handler failed: {e}")))?;
            let ns = t.elapsed().as_nanos() as u64;
            let actions = r.unwrap_or_default();
            let lines = actions.iter().map(|a| format!("{a:?}")).collect();
            Ok((ns, Observation::from_lines(actions.len(), lines)))
        }
        FeatureSpec::DiagnosticsPull => {
            let ctx = build_latency_ctx(env);
            let params = lsp_types::DocumentDiagnosticParams {
                text_document: lsp_types::TextDocumentIdentifier { uri: resolved.url.clone() },
                identifier: None,
                previous_result_id: None,
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            let t = Instant::now();
            let r = request::handle_document_diagnostic(ctx, params)
                .map_err(|e| RunError::Other(format!("pull diagnostics handler failed: {e}")))?;
            let ns = t.elapsed().as_nanos() as u64;
            let count = pull_report_count(&r);
            let lines = vec![format!("{r:?}")];
            Ok((ns, Observation::from_lines(count, lines)))
        }
        FeatureSpec::Burst { sequence } => {
            let mut total_ns = 0u64;
            let mut count = 0usize;
            let mut lines = Vec::new();
            for inner in sequence {
                let (ns, obs) = execute_once(env, resolved, inner)?;
                total_ns += ns;
                count += obs.count;
                lines.push(format!("{}:{}", inner.feature_name(), obs.digest_input));
            }
            Ok((total_ns, Observation::from_lines(count, lines)))
        }
        FeatureSpec::Edit { .. } => Err(RunError::Other(
            "edit points are executed by run_edit_point, not execute_once".to_string(),
        )),
    }
}

fn pull_report_count(result: &lsp_types::DocumentDiagnosticReportResult) -> usize {
    use lsp_types::{DocumentDiagnosticReport, DocumentDiagnosticReportResult};
    match result {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            full.full_document_diagnostic_report.items.len()
        }
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Unchanged(_)) => 0,
        DocumentDiagnosticReportResult::Partial(_) => 0,
    }
}

/// Frozen request context, mirroring what dispatch builds per request. Built
/// outside the timed section; the first build's cost is recorded once.
fn build_latency_ctx(env: &mut BenchEnv) -> LatencyRequestContext {
    let t = Instant::now();
    // Freezing the path table is O(files) — cache it across warm iterations;
    // `open_document` / the edit path reset the cache when paths could move.
    let file_paths = env
        .frozen_paths
        .get_or_insert_with(|| FrozenFilePaths::freeze(&env.state.vfs.read()))
        .clone();
    let state = &env.state;
    let ctx = LatencyRequestContext {
        analysis: state.analysis_host.analysis(),
        workspace_root: state.workspace_root.clone(),
        project: state.project.clone(),
        diagnostics_config: state.diagnostics_config.clone(),
        position_encoding: state.position_encoding,
        supports_insert_text_mode_adjust_indentation: state
            .supports_insert_text_mode_adjust_indentation,
        supports_workspace_edit_document_changes: state.supports_workspace_edit_document_changes,
        task_sender: state.task_pool.pool.sender.clone(),
        client_sender: state.sender.clone(),
        mem_docs: state.mem_docs.freeze(),
        file_paths,
    };
    let ns = t.elapsed().as_nanos() as u64;
    env.ctx_build_ns.get_or_insert(ns);
    ctx
}

/// Byte offsets → LSP UTF-16 range (the runner's manifests store byte offsets;
/// handler params speak LSP positions).
fn offset_range_to_lsp(text: &str, start: u32, end: u32) -> Option<lsp_types::Range> {
    let index = LineIndex::new(text);
    let start = crate::lsp::to_proto::position_utf16(&index, text, TextSize::from(start))?;
    let end = crate::lsp::to_proto::position_utf16(&index, text, TextSize::from(end))?;
    Some(lsp_types::Range { start, end })
}
