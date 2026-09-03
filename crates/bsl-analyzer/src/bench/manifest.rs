//! Benchmark target manifest: schema, parsing, validation.
//!
//! A manifest pins a deterministic set of measurement targets produced by a
//! separate discovery process. Every target carries the file it points into
//! (as a workspace-relative path plus a content hash), a typed feature spec,
//! and an invariant the measured result must satisfy — so a run measures the
//! intended semantic path, never a fast no-op.
//!
//! Positions are **byte offsets** into the file text as stored in the analysis
//! database (after BOM handling). BSL sources are Cyrillic-heavy, so UTF-8 vs
//! UTF-16 column numbers diverge on almost every line; a byte offset is the
//! only unambiguous coordinate. Discovery may add human-readable context via
//! `note`, which the runner ignores.

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BenchManifest {
    pub schema_version: u32,
    /// VCS revision of the measured workspace, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_commit: Option<String>,
    /// blake3 hex of the workspace `bsl-analyzer.toml`, if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_hash: Option<String>,
    pub targets: Vec<Target>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// Unique point id, e.g. `hover/common_large/01`.
    pub id: String,
    /// Path relative to the workspace root, `/`-separated.
    pub relative_path: String,
    /// blake3 hex (64 chars) of the file text as loaded into the analysis DB.
    pub file_hash: String,
    pub spec: FeatureSpec,
    pub expect: Expect,
    /// Free-form human context (line/column preview etc.); ignored by the runner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// One measurement recipe. The variant set mirrors the measurement-boundary
/// table in the perf plan: which entrypoint runs, at which boundary, with
/// which overlay, is decided by the variant alone.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "feature", rename_all = "snake_case")]
pub enum FeatureSpec {
    Hover {
        offset: u32,
    },
    Completion {
        offset: u32,
    },
    GotoDefinition {
        offset: u32,
    },
    TypeDefinition {
        offset: u32,
    },
    References {
        offset: u32,
    },
    Rename {
        offset: u32,
        new_name: String,
    },
    CallHierarchyPrepare {
        offset: u32,
    },
    CallHierarchyIncoming {
        offset: u32,
    },
    CallHierarchyOutgoing {
        offset: u32,
    },
    CallHierarchyIndexBuild {
        batch_size: usize,
    },
    /// `range: None` = the whole file.
    InlayHints {
        range: Option<OffsetRange>,
    },
    SelectionRange {
        offsets: Vec<u32>,
    },
    DocumentSymbol,
    FoldingRange,
    SemanticTokensFull,
    CodeAction {
        range: OffsetRange,
    },
    DiagnosticsPush,
    DiagnosticsPull,
    WorkspaceSymbol {
        query: String,
    },
    SignatureHelp {
        offset: u32,
    },
    /// Apply `patch` through the real `didChange` path, then re-run `followup`.
    /// All `followup` offsets must lie strictly before `patch.start`, so they
    /// are valid both before and after the edit (this is the
    /// `position_after_edit` guarantee).
    Edit {
        patch: EditPatch,
        edit_kind: EditKind,
        followup: Box<FeatureSpec>,
    },
    /// A typing burst: every patch goes through its own `didChange`, then
    /// `followup` runs once. This is the shape the server sees when edits
    /// arrive faster than diagnostics can start — the main loop drains the
    /// channel and schedules one pass for the last revision — so the point
    /// measures "catch up with the latest text", not N separate edits.
    /// Each patch is addressed in the coordinates of the text *after* the
    /// patches before it. `followup` offsets must lie strictly before every
    /// patch start, as for [`FeatureSpec::Edit`].
    EditBurst {
        patches: Vec<EditPatch>,
        edit_kind: EditKind,
        followup: Box<FeatureSpec>,
    },
    /// Sequential post-`didOpen` request batch (core cost, no task pool).
    Burst {
        sequence: Vec<FeatureSpec>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OffsetRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditPatch {
    /// Byte range in the pre-edit text replaced by `new_text` (empty range = insertion).
    pub range: OffsetRange,
    pub new_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditKind {
    Body,
    Signature,
    /// A whole method appears or disappears: the module interface changes and
    /// every method after the edit point moves to another top-level position.
    Method,
}

impl EditKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EditKind::Body => "body",
            EditKind::Signature => "signature",
            EditKind::Method => "method",
        }
    }
}

/// Result invariant. `Digest` is reserved for hand-curated targets whose
/// digest input is verified stable across processes: observed digests are
/// built from `Debug` renderings, which may embed `FileId`s whose numbering
/// depends on VFS load order. Discovery therefore emits only
/// `NonEmpty`/`Cardinality`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Expect {
    NonEmpty,
    Cardinality { min: usize, max: usize },
    Digest { hash: String },
}

impl Expect {
    pub fn check(&self, count: usize, digest_hex: &str) -> Result<(), String> {
        match self {
            Expect::NonEmpty => {
                if count == 0 {
                    return Err(
                        "expected non-empty result, observed count=0 (no-op measured)".to_string()
                    );
                }
            }
            Expect::Cardinality { min, max } => {
                if count < *min || count > *max {
                    return Err(format!(
                        "expected cardinality in [{min}, {max}], observed {count}"
                    ));
                }
            }
            Expect::Digest { hash } => {
                if !hash.eq_ignore_ascii_case(digest_hex) {
                    return Err(format!("expected digest {hash}, observed {digest_hex}"));
                }
            }
        }
        Ok(())
    }
}

impl FeatureSpec {
    /// Stable tag matching the serde `feature` value; used in reports and ids.
    pub fn feature_name(&self) -> &'static str {
        match self {
            FeatureSpec::Hover { .. } => "hover",
            FeatureSpec::Completion { .. } => "completion",
            FeatureSpec::GotoDefinition { .. } => "goto_definition",
            FeatureSpec::TypeDefinition { .. } => "type_definition",
            FeatureSpec::References { .. } => "references",
            FeatureSpec::Rename { .. } => "rename",
            FeatureSpec::CallHierarchyPrepare { .. } => "call_hierarchy_prepare",
            FeatureSpec::CallHierarchyIncoming { .. } => "call_hierarchy_incoming",
            FeatureSpec::CallHierarchyOutgoing { .. } => "call_hierarchy_outgoing",
            FeatureSpec::CallHierarchyIndexBuild { .. } => "call_hierarchy_index_build",
            FeatureSpec::InlayHints { .. } => "inlay_hints",
            FeatureSpec::SelectionRange { .. } => "selection_range",
            FeatureSpec::DocumentSymbol => "document_symbol",
            FeatureSpec::FoldingRange => "folding_range",
            FeatureSpec::SemanticTokensFull => "semantic_tokens_full",
            FeatureSpec::CodeAction { .. } => "code_action",
            FeatureSpec::DiagnosticsPush => "diagnostics_push",
            FeatureSpec::DiagnosticsPull => "diagnostics_pull",
            FeatureSpec::WorkspaceSymbol { .. } => "workspace_symbol",
            FeatureSpec::SignatureHelp { .. } => "signature_help",
            FeatureSpec::Edit { .. } => "edit",
            FeatureSpec::EditBurst { .. } => "edit_burst",
            FeatureSpec::Burst { .. } => "burst",
        }
    }

    /// Whether the spec applies edits itself (`Edit` / `EditBurst`); such specs
    /// run through the edit runner and may not nest inside another spec.
    pub fn is_edit(&self) -> bool {
        matches!(self, FeatureSpec::Edit { .. } | FeatureSpec::EditBurst { .. })
    }

    /// The smallest offset the spec dereferences, if any — used by the edit
    /// validator to enforce "followup strictly before patch".
    fn max_offset(&self) -> Option<u32> {
        match self {
            FeatureSpec::Hover { offset }
            | FeatureSpec::Completion { offset }
            | FeatureSpec::GotoDefinition { offset }
            | FeatureSpec::TypeDefinition { offset }
            | FeatureSpec::References { offset }
            | FeatureSpec::Rename { offset, .. }
            | FeatureSpec::CallHierarchyPrepare { offset }
            | FeatureSpec::CallHierarchyIncoming { offset }
            | FeatureSpec::CallHierarchyOutgoing { offset }
            | FeatureSpec::SignatureHelp { offset } => Some(*offset),
            FeatureSpec::InlayHints { range } => range.map(|r| r.end),
            FeatureSpec::SelectionRange { offsets } => offsets.iter().copied().max(),
            FeatureSpec::CodeAction { range } => Some(range.end),
            FeatureSpec::DocumentSymbol
            | FeatureSpec::FoldingRange
            | FeatureSpec::SemanticTokensFull
            | FeatureSpec::DiagnosticsPush
            | FeatureSpec::DiagnosticsPull
            | FeatureSpec::WorkspaceSymbol { .. }
            | FeatureSpec::CallHierarchyIndexBuild { .. } => None,
            FeatureSpec::Edit { .. }
            | FeatureSpec::EditBurst { .. }
            | FeatureSpec::Burst { .. } => None,
        }
    }
}

pub fn parse(text: &str) -> Result<BenchManifest, String> {
    let manifest: BenchManifest =
        serde_json::from_str(text).map_err(|e| format!("manifest parse error: {e}"))?;
    validate(&manifest)?;
    Ok(manifest)
}

pub fn load(path: &std::path::Path) -> Result<BenchManifest, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read manifest {}: {e}", path.display()))?;
    parse(&text)
}

pub fn validate(manifest: &BenchManifest) -> Result<(), String> {
    let mut errors: Vec<String> = Vec::new();

    if manifest.schema_version != SCHEMA_VERSION {
        errors.push(format!(
            "unsupported schema_version {} (expected {SCHEMA_VERSION})",
            manifest.schema_version
        ));
    }
    if manifest.targets.is_empty() {
        errors.push("manifest has no targets".to_string());
    }

    let mut seen_ids = std::collections::HashSet::new();
    for target in &manifest.targets {
        let id = &target.id;
        if id.is_empty() {
            errors.push("target with empty id".to_string());
        }
        if !seen_ids.insert(id.clone()) {
            errors.push(format!("duplicate target id `{id}`"));
        }
        if target.relative_path.is_empty() {
            errors.push(format!("{id}: empty relative_path"));
        }
        if target.relative_path.starts_with('/')
            || target.relative_path.split('/').any(|seg| seg == "..")
        {
            errors.push(format!(
                "{id}: relative_path must be workspace-relative without `..`: {}",
                target.relative_path
            ));
        }
        if !is_blake3_hex(&target.file_hash) {
            errors.push(format!("{id}: file_hash must be 64 hex chars (blake3)"));
        }
        if let Expect::Cardinality { min, max } = &target.expect {
            if min > max {
                errors.push(format!("{id}: cardinality min {min} > max {max}"));
            }
        }
        if let Expect::Digest { hash } = &target.expect {
            if !is_blake3_hex(hash) {
                errors.push(format!("{id}: digest hash must be 64 hex chars (blake3)"));
            }
        }
        validate_spec(id, &target.spec, &mut errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!("manifest validation failed:\n  - {}", errors.join("\n  - ")))
    }
}

fn validate_spec(id: &str, spec: &FeatureSpec, errors: &mut Vec<String>) {
    match spec {
        FeatureSpec::Rename { new_name, .. } if new_name.is_empty() => {
            errors.push(format!("{id}: rename new_name is empty"));
        }
        FeatureSpec::SelectionRange { offsets } if offsets.is_empty() => {
            errors.push(format!("{id}: selection_range needs at least one offset"));
        }
        FeatureSpec::CallHierarchyIndexBuild { batch_size } if *batch_size == 0 => {
            errors.push(format!(
                "{id}: call_hierarchy_index_build batch_size must be greater than zero"
            ));
        }
        FeatureSpec::InlayHints { range: Some(r) } | FeatureSpec::CodeAction { range: r }
            if r.start > r.end =>
        {
            errors.push(format!("{id}: range start {} > end {}", r.start, r.end));
        }
        FeatureSpec::Edit { patch, followup, .. } => {
            validate_edit(id, std::slice::from_ref(patch), followup, errors);
        }
        FeatureSpec::EditBurst { patches, followup, .. } => {
            if patches.is_empty() {
                errors.push(format!("{id}: edit_burst has no patches"));
            }
            validate_edit(id, patches, followup, errors);
        }
        FeatureSpec::Burst { sequence } => {
            if sequence.is_empty() {
                errors.push(format!("{id}: burst sequence is empty"));
            }
            for inner in sequence {
                if inner.is_edit() || matches!(inner, FeatureSpec::Burst { .. }) {
                    errors.push(format!("{id}: burst must not nest edit/burst"));
                }
            }
        }
        _ => {}
    }
}

/// Shared rules of `Edit` and `EditBurst`: well-formed patch ranges, a plain
/// followup, and followup offsets strictly before every patch start so they
/// stay valid before and after the edits.
fn validate_edit(
    id: &str,
    patches: &[EditPatch],
    followup: &FeatureSpec,
    errors: &mut Vec<String>,
) {
    for patch in patches {
        if patch.range.start > patch.range.end {
            errors.push(format!(
                "{id}: patch range start {} > end {}",
                patch.range.start, patch.range.end
            ));
        }
    }
    if followup.is_edit() || matches!(followup, FeatureSpec::Burst { .. }) {
        errors.push(format!("{id}: edit followup must be a plain feature"));
        return;
    }
    let Some(first_start) = patches.iter().map(|p| p.range.start).min() else { return };
    if let Some(max) = followup.max_offset() {
        if max >= first_start {
            errors.push(format!(
                "{id}: followup offset {max} not strictly before patch start {first_start} — \
                 position would shift with the edit"
            ));
        }
    }
}

fn is_blake3_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn hash_text(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(id: &str, spec: FeatureSpec, expect: Expect) -> Target {
        Target {
            id: id.to_string(),
            relative_path: "Module.bsl".to_string(),
            file_hash: hash_text("x"),
            spec,
            expect,
            note: None,
        }
    }

    fn manifest(targets: Vec<Target>) -> BenchManifest {
        BenchManifest {
            schema_version: SCHEMA_VERSION,
            workspace_commit: None,
            config_hash: None,
            targets,
        }
    }

    #[test]
    fn every_variant_roundtrips_through_json() {
        let specs = vec![
            FeatureSpec::Hover { offset: 1 },
            FeatureSpec::Completion { offset: 1 },
            FeatureSpec::GotoDefinition { offset: 1 },
            FeatureSpec::TypeDefinition { offset: 1 },
            FeatureSpec::References { offset: 1 },
            FeatureSpec::Rename { offset: 1, new_name: "Имя".to_string() },
            FeatureSpec::CallHierarchyPrepare { offset: 1 },
            FeatureSpec::CallHierarchyIncoming { offset: 1 },
            FeatureSpec::CallHierarchyOutgoing { offset: 1 },
            FeatureSpec::CallHierarchyIndexBuild { batch_size: 1 },
            FeatureSpec::InlayHints { range: Some(OffsetRange { start: 0, end: 5 }) },
            FeatureSpec::InlayHints { range: None },
            FeatureSpec::SelectionRange { offsets: vec![1, 2] },
            FeatureSpec::DocumentSymbol,
            FeatureSpec::FoldingRange,
            FeatureSpec::SemanticTokensFull,
            FeatureSpec::CodeAction { range: OffsetRange { start: 0, end: 5 } },
            FeatureSpec::DiagnosticsPush,
            FeatureSpec::DiagnosticsPull,
            FeatureSpec::WorkspaceSymbol { query: "Общ".to_string() },
            FeatureSpec::SignatureHelp { offset: 1 },
            FeatureSpec::Edit {
                patch: EditPatch {
                    range: OffsetRange { start: 10, end: 10 },
                    new_text: "Х = 1;".to_string(),
                },
                edit_kind: EditKind::Body,
                followup: Box::new(FeatureSpec::Hover { offset: 1 }),
            },
            FeatureSpec::EditBurst {
                patches: vec![
                    EditPatch {
                        range: OffsetRange { start: 10, end: 10 },
                        new_text: "Х = 1;".to_string(),
                    },
                    EditPatch {
                        range: OffsetRange { start: 16, end: 16 },
                        new_text: "Х = 2;".to_string(),
                    },
                ],
                edit_kind: EditKind::Body,
                followup: Box::new(FeatureSpec::DiagnosticsPush),
            },
            FeatureSpec::Burst { sequence: vec![FeatureSpec::DocumentSymbol] },
        ];
        for spec in specs {
            let name = spec.feature_name();
            let json = serde_json::to_string(&spec).unwrap();
            assert!(json.contains(&format!("\"feature\":\"{name}\"")), "{json}");
            let back: FeatureSpec = serde_json::from_str(&json).unwrap();
            assert_eq!(back.feature_name(), name);
        }
    }

    #[test]
    fn valid_manifest_passes() {
        let m = manifest(vec![
            target("hover/01", FeatureSpec::Hover { offset: 3 }, Expect::NonEmpty),
            target(
                "refs/01",
                FeatureSpec::References { offset: 3 },
                Expect::Cardinality { min: 1, max: 10 },
            ),
        ]);
        validate(&m).unwrap();
        let text = serde_json::to_string_pretty(&m).unwrap();
        parse(&text).unwrap();
    }

    #[test]
    fn unknown_feature_tag_is_rejected() {
        let text = r#"{"schema_version":1,"targets":[{"id":"x","relative_path":"m.bsl",
            "file_hash":"0000000000000000000000000000000000000000000000000000000000000000",
            "spec":{"feature":"telepathy"},"expect":{"kind":"non_empty"}}]}"#;
        let err = parse(text).unwrap_err();
        assert!(err.contains("parse error"), "{err}");
    }

    #[test]
    fn malformed_json_is_rejected() {
        assert!(parse("{not json").is_err());
    }

    #[test]
    fn duplicate_ids_and_bad_hash_are_rejected() {
        let mut bad = target("dup", FeatureSpec::DocumentSymbol, Expect::NonEmpty);
        bad.file_hash = "abc".to_string();
        let m = manifest(vec![bad, target("dup", FeatureSpec::FoldingRange, Expect::NonEmpty)]);
        let err = validate(&m).unwrap_err();
        assert!(err.contains("duplicate target id"), "{err}");
        assert!(err.contains("file_hash"), "{err}");
    }

    #[test]
    fn cardinality_min_above_max_is_rejected() {
        let m = manifest(vec![target(
            "c",
            FeatureSpec::DocumentSymbol,
            Expect::Cardinality { min: 5, max: 1 },
        )]);
        assert!(validate(&m).unwrap_err().contains("min 5 > max 1"));
    }

    #[test]
    fn call_hierarchy_index_batch_size_must_be_positive() {
        // Given: an index-build target with no modules per batch.
        let m = manifest(vec![target(
            "call-hierarchy-index/zero",
            FeatureSpec::CallHierarchyIndexBuild { batch_size: 0 },
            Expect::NonEmpty,
        )]);

        // When: manifest validation runs.
        let err = validate(&m).unwrap_err();

        // Then: the invalid batching contract is named explicitly.
        assert!(err.contains("batch_size must be greater than zero"), "{err}");
    }

    #[test]
    fn edit_followup_must_precede_patch() {
        let m = manifest(vec![target(
            "e",
            FeatureSpec::Edit {
                patch: EditPatch {
                    range: OffsetRange { start: 10, end: 10 },
                    new_text: "x".to_string(),
                },
                edit_kind: EditKind::Body,
                followup: Box::new(FeatureSpec::Hover { offset: 10 }),
            },
            Expect::NonEmpty,
        )]);
        assert!(validate(&m).unwrap_err().contains("strictly before patch"));
    }

    #[test]
    fn nested_edit_and_burst_are_rejected() {
        let inner_edit = FeatureSpec::Edit {
            patch: EditPatch { range: OffsetRange { start: 0, end: 0 }, new_text: String::new() },
            edit_kind: EditKind::Body,
            followup: Box::new(FeatureSpec::DocumentSymbol),
        };
        let m = manifest(vec![
            target("b", FeatureSpec::Burst { sequence: vec![inner_edit] }, Expect::NonEmpty),
            target("b2", FeatureSpec::Burst { sequence: vec![] }, Expect::NonEmpty),
        ]);
        let err = validate(&m).unwrap_err();
        assert!(err.contains("must not nest"), "{err}");
        assert!(err.contains("sequence is empty"), "{err}");
    }

    #[test]
    fn edit_burst_shares_the_edit_rules() {
        let patch = |start: u32| EditPatch {
            range: OffsetRange { start, end: start },
            new_text: "Х = 1;".to_string(),
        };
        // Followup offsets must precede the *earliest* patch: later patches
        // are addressed in shifted coordinates, and only offsets before all
        // of them survive every shift.
        let too_late = manifest(vec![target(
            "eb",
            FeatureSpec::EditBurst {
                patches: vec![patch(20), patch(10)],
                edit_kind: EditKind::Body,
                followup: Box::new(FeatureSpec::Hover { offset: 10 }),
            },
            Expect::NonEmpty,
        )]);
        let err = validate(&too_late).unwrap_err();
        assert!(err.contains("not strictly before patch start 10"), "{err}");

        let nested = manifest(vec![
            target(
                "eb-empty",
                FeatureSpec::EditBurst {
                    patches: vec![],
                    edit_kind: EditKind::Body,
                    followup: Box::new(FeatureSpec::DiagnosticsPush),
                },
                Expect::NonEmpty,
            ),
            target(
                "eb-nested",
                FeatureSpec::Burst {
                    sequence: vec![FeatureSpec::EditBurst {
                        patches: vec![patch(5)],
                        edit_kind: EditKind::Body,
                        followup: Box::new(FeatureSpec::DiagnosticsPush),
                    }],
                },
                Expect::NonEmpty,
            ),
        ]);
        let err = validate(&nested).unwrap_err();
        assert!(err.contains("edit_burst has no patches"), "{err}");
        assert!(err.contains("must not nest"), "{err}");

        let ok = manifest(vec![target(
            "eb-ok",
            FeatureSpec::EditBurst {
                patches: vec![patch(20), patch(10)],
                edit_kind: EditKind::Body,
                followup: Box::new(FeatureSpec::Hover { offset: 9 }),
            },
            Expect::NonEmpty,
        )]);
        validate(&ok).unwrap();
    }

    #[test]
    fn expect_check_matrix() {
        assert!(Expect::NonEmpty.check(1, "").is_ok());
        assert!(Expect::NonEmpty.check(0, "").is_err());
        assert!(Expect::Cardinality { min: 2, max: 4 }.check(3, "").is_ok());
        assert!(Expect::Cardinality { min: 2, max: 4 }.check(5, "").is_err());
        let h = hash_text("payload");
        assert!(Expect::Digest { hash: h.clone() }.check(0, &h).is_ok());
        assert!(Expect::Digest { hash: h }.check(0, &hash_text("other")).is_err());
    }
}
