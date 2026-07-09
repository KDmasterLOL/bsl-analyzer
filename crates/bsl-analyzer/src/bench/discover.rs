//! Probe-based target discovery.
//!
//! Discovery boots the workspace once (its own process, outside any
//! measurement), picks representative files per class, and *verifies* every
//! candidate by actually executing the feature through the same executors the
//! runner uses: a target enters the manifest only if the probe observed a
//! non-empty result, and its cardinality bound is derived from that
//! observation. This is what guarantees a later `bench run` measures a
//! semantic path rather than a fast `None`.
//!
//! Determinism: files and probe offsets are enumerated in sorted order; no
//! randomness is involved.

use std::path::{Path, PathBuf};

use base_db::{RootQueryDb, SourceDatabase};

use crate::bench::manifest::{
    BenchManifest, EditKind, EditPatch, Expect, FeatureSpec, OffsetRange, Target, SCHEMA_VERSION,
};
use crate::bench::runner::{self, BenchEnv, ResolvedTarget, RunError};

#[derive(Debug, Clone)]
pub struct DiscoverArgs {
    pub source_dir: PathBuf,
    pub boot_budget_ms: u64,
}

const PROBE_OFFSET_CAP: usize = 400;

pub fn discover(args: &DiscoverArgs) -> Result<BenchManifest, RunError> {
    let mut env = runner::boot(&args.source_dir, args.boot_budget_ms)?;
    let files = enumerate_bsl_files(&env);
    if files.is_empty() {
        return Err(RunError::Other("no BSL files in workspace".to_string()));
    }

    let mut targets: Vec<Target> = Vec::new();
    for (class, relative_path) in representative_files(&files) {
        let resolved = runner::resolve_target(&env, &relative_path, None)?;
        discover_in_file(&mut env, &class, &relative_path, &resolved, &mut targets)?;
    }

    let config_hash = std::fs::read_to_string(args.source_dir.join("bsl-analyzer.toml"))
        .ok()
        .map(|text| crate::bench::manifest::hash_text(&text));

    Ok(BenchManifest {
        schema_version: SCHEMA_VERSION,
        workspace_commit: workspace_commit(&args.source_dir),
        config_hash,
        targets,
    })
}

fn workspace_commit(source_dir: &Path) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(source_dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let rev = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!rev.is_empty()).then_some(rev)
}

/// `(class, relative_path, text_len)` for every BSL file under source root 0.
fn enumerate_bsl_files(env: &BenchEnv) -> Vec<(String, String, usize)> {
    let db = env.state.analysis_host.raw_database();
    let source_root = db.source_root_input(base_db::SourceRootId(0)).root(db);
    let file_set = source_root.file_set();

    let mut files: Vec<(String, String, usize)> = file_set
        .iter()
        .filter_map(|fid| {
            let vfs_path = file_set.path_for_file(&fid)?;
            let std_path = vfs_path.as_path().to_path_buf();
            if !project_model::is_bsl_source_path(&std_path) {
                return None;
            }
            db.try_file_revision_input(fid)?;
            let relative = relative_to_root(&env.workspace_root, &std_path)?;
            // Disk size, not DB text: enumeration must not force lazy text
            // loads for the whole workspace.
            let len = std::fs::metadata(&std_path).map(|m| m.len() as usize).unwrap_or(0);
            Some((classify(&relative), relative, len))
        })
        .collect();
    files.sort();
    files
}

fn relative_to_root(root: &Path, path: &Path) -> Option<String> {
    let stripped = path.strip_prefix(root).ok().or_else(|| {
        let canonical = std::fs::canonicalize(root).ok()?;
        path.strip_prefix(&canonical).ok()
    })?;
    Some(stripped.to_string_lossy().replace('\\', "/"))
}

fn classify(relative_path: &str) -> String {
    let p = relative_path;
    if p.contains("/CommonModules/") || p.starts_with("CommonModules/") {
        "common".to_string()
    } else if p.contains("/Forms/") {
        "form".to_string()
    } else if p.ends_with("ObjectModule.bsl") {
        "object".to_string()
    } else if p.ends_with("ManagerModule.bsl") {
        "manager".to_string()
    } else {
        "other".to_string()
    }
}

/// One representative file per class: the largest of the class (the stress
/// case), plus the smallest common module as the fast-path control. Classes
/// absent from the workspace are silently skipped — the manifest simply has no
/// targets there.
fn representative_files(files: &[(String, String, usize)]) -> Vec<(String, String)> {
    let mut picks: Vec<(String, String)> = Vec::new();
    for class in ["common", "form", "object", "manager", "other"] {
        let of_class: Vec<&(String, String, usize)> =
            files.iter().filter(|(c, ..)| c == class).collect();
        if let Some((_, path, _)) = of_class.iter().max_by_key(|(_, path, len)| (*len, path)) {
            picks.push((format!("{class}_large"), path.clone()));
        }
        if class == "common" && of_class.len() > 1 {
            if let Some((_, path, _)) = of_class.iter().min_by_key(|(_, path, len)| (*len, path)) {
                picks.push(("common_small".to_string(), path.clone()));
            }
        }
    }
    picks
}

fn discover_in_file(
    env: &mut BenchEnv,
    class: &str,
    relative_path: &str,
    resolved: &ResolvedTarget,
    targets: &mut Vec<Target>,
) -> Result<(), RunError> {
    let offsets = ident_offsets(env, resolved);
    let file_hash = crate::bench::manifest::hash_text(&resolved.text);

    let push = |spec: FeatureSpec, count: usize, targets: &mut Vec<Target>| {
        let feature = spec.feature_name();
        let seq = targets.iter().filter(|t| t.spec.feature_name() == feature).count() + 1;
        targets.push(Target {
            id: format!("{feature}/{class}/{seq:02}"),
            relative_path: relative_path.to_string(),
            file_hash: file_hash.clone(),
            spec,
            expect: expect_from_count(count),
            note: None,
        });
    };

    type Ctor = fn(u32) -> FeatureSpec;
    let positional: &[Ctor] = &[
        |o| FeatureSpec::Hover { offset: o },
        |o| FeatureSpec::GotoDefinition { offset: o },
        |o| FeatureSpec::TypeDefinition { offset: o },
        |o| FeatureSpec::References { offset: o },
        |o| FeatureSpec::CallHierarchyPrepare { offset: o },
        |o| FeatureSpec::CallHierarchyIncoming { offset: o },
        |o| FeatureSpec::CallHierarchyOutgoing { offset: o },
        |o| FeatureSpec::Completion { offset: o },
        |o| FeatureSpec::SignatureHelp { offset: o },
        |o| FeatureSpec::Rename { offset: o, new_name: "БенчНовоеИмя".to_string() },
    ];
    for ctor in positional {
        if let Some((spec, count)) = probe_offsets(env, resolved, &offsets, *ctor) {
            push(spec, count, targets);
        }
    }

    let fileless: &[FeatureSpec] = &[
        FeatureSpec::DocumentSymbol,
        FeatureSpec::FoldingRange,
        FeatureSpec::InlayHints { range: None },
        FeatureSpec::SemanticTokensFull,
        FeatureSpec::DiagnosticsPush,
        FeatureSpec::DiagnosticsPull,
        FeatureSpec::CodeAction {
            range: OffsetRange { start: 0, end: resolved.text.len() as u32 },
        },
    ];
    for spec in fileless {
        overlay_and_probe(env, resolved, spec.clone()).into_iter().for_each(|count| {
            // Zero-count scans (no actions / no diagnostics) are still a real
            // path, but a NonEmpty/positive bound would be a lie — keep only
            // observed-positive targets, mirroring the positional probes.
            if count > 0 {
                push(spec.clone(), count, targets);
            }
        });
    }

    if let Some(offsets3) = offsets.get(..offsets.len().min(3)) {
        if !offsets3.is_empty() {
            let spec = FeatureSpec::SelectionRange { offsets: offsets3.to_vec() };
            if let Some(count) = overlay_and_probe(env, resolved, spec.clone()) {
                if count > 0 {
                    push(spec, count, targets);
                }
            }
        }
    }

    if class.ends_with("_large") {
        discover_workspace_symbol(env, resolved, relative_path, &file_hash, targets);
        discover_edit_and_burst(env, resolved, relative_path, &file_hash, &offsets, targets);
    }

    Ok(())
}

fn discover_workspace_symbol(
    env: &mut BenchEnv,
    resolved: &ResolvedTarget,
    relative_path: &str,
    file_hash: &str,
    targets: &mut Vec<Target>,
) {
    // Prefix queries derived from the file's own symbol names; the empty query
    // stays as the documented fast-path control (must return nothing).
    let (_, symbols) = match runner::execute_once(env, resolved, &FeatureSpec::DocumentSymbol) {
        Ok(r) => r,
        Err(_) => return,
    };
    if symbols.count == 0 {
        return;
    }
    let prefix: String = resolved
        .text
        .lines()
        .find_map(|line| {
            let line = line.trim_start();
            let rest = line
                .strip_prefix("Процедура ")
                .or_else(|| line.strip_prefix("Функция "))
                .or_else(|| line.strip_prefix("Procedure "))
                .or_else(|| line.strip_prefix("Function "))?;
            Some(rest.chars().take(3).collect())
        })
        .unwrap_or_default();
    if !prefix.is_empty() {
        let spec = FeatureSpec::WorkspaceSymbol { query: prefix };
        if let Some(count) = overlay_and_probe(env, resolved, spec.clone()) {
            if count > 0 {
                targets.push(Target {
                    id: "workspace_symbol/scan/01".to_string(),
                    relative_path: relative_path.to_string(),
                    file_hash: file_hash.to_string(),
                    spec,
                    expect: expect_from_count(count),
                    note: None,
                });
            }
        }
    }
    targets.push(Target {
        id: "workspace_symbol/empty_control/01".to_string(),
        relative_path: relative_path.to_string(),
        file_hash: file_hash.to_string(),
        spec: FeatureSpec::WorkspaceSymbol { query: String::new() },
        expect: Expect::Cardinality { min: 0, max: 0 },
        note: Some("fast-path control: empty query returns immediately".to_string()),
    });
}

fn discover_edit_and_burst(
    env: &mut BenchEnv,
    resolved: &ResolvedTarget,
    relative_path: &str,
    file_hash: &str,
    offsets: &[u32],
    targets: &mut Vec<Target>,
) {
    let Some(&first_ident) = offsets.first() else { return };
    let hover = FeatureSpec::Hover { offset: first_ident };
    let Some(hover_count) = overlay_and_probe(env, resolved, hover.clone()) else { return };
    if hover_count == 0 {
        return;
    }

    // Pilot-grade body edit: append a trailing comment. It exercises the full
    // didChange → invalidation → re-request path; semantically-typed
    // body-vs-signature ERP patches are curated separately.
    let end = resolved.text.len() as u32;
    if end > first_ident {
        targets.push(Target {
            id: "edit/body_append/01".to_string(),
            relative_path: relative_path.to_string(),
            file_hash: file_hash.to_string(),
            spec: FeatureSpec::Edit {
                patch: EditPatch {
                    range: OffsetRange { start: end, end },
                    new_text: "\n// бенч-правка\n".to_string(),
                },
                edit_kind: EditKind::Body,
                followup: Box::new(hover),
            },
            expect: Expect::NonEmpty,
            note: None,
        });
    }

    targets.push(Target {
        id: "burst/did_open/01".to_string(),
        relative_path: relative_path.to_string(),
        file_hash: file_hash.to_string(),
        spec: FeatureSpec::Burst {
            sequence: vec![
                FeatureSpec::SemanticTokensFull,
                FeatureSpec::DocumentSymbol,
                FeatureSpec::FoldingRange,
                FeatureSpec::InlayHints { range: None },
                FeatureSpec::DiagnosticsPush,
            ],
        },
        expect: Expect::NonEmpty,
        note: Some("post-didOpen sequential core cost".to_string()),
    });
}

fn probe_offsets(
    env: &mut BenchEnv,
    resolved: &ResolvedTarget,
    offsets: &[u32],
    ctor: fn(u32) -> FeatureSpec,
) -> Option<(FeatureSpec, usize)> {
    for &offset in offsets {
        let spec = ctor(offset);
        if let Some(count) = overlay_and_probe(env, resolved, spec.clone()) {
            if count > 0 {
                return Some((spec, count));
            }
        }
    }
    None
}

fn overlay_and_probe(
    env: &mut BenchEnv,
    resolved: &ResolvedTarget,
    spec: FeatureSpec,
) -> Option<usize> {
    runner::ensure_overlay(env, resolved, &spec).ok()?;
    runner::execute_once(env, resolved, &spec).ok().map(|(_, obs)| obs.count)
}

/// Observed count → generous cardinality bound. The lower bound stays at 1 so
/// a later run still fails on a no-op; the upper bound tolerates workspace
/// evolution without silently accepting an order-of-magnitude blowup.
fn expect_from_count(count: usize) -> Expect {
    Expect::Cardinality { min: 1, max: count.saturating_mul(4).saturating_add(8) }
}

fn ident_offsets(env: &BenchEnv, resolved: &ResolvedTarget) -> Vec<u32> {
    let db = env.state.analysis_host.raw_database();
    let parse = db.parse(resolved.file_id);
    let root = parse.syntax_node();
    let mut offsets = Vec::new();
    for elem in root.descendants_with_tokens() {
        if let Some(token) = elem.as_token() {
            if token.kind() == syntax::SyntaxKind::IDENT {
                offsets.push(u32::from(token.text_range().start()));
                if offsets.len() >= PROBE_OFFSET_CAP {
                    break;
                }
            }
        }
    }
    offsets
}
