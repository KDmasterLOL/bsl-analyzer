//! LSP feature benchmark harness (`bsl-analyzer bench`).
//!
//! Deterministic per-point measurements over a real workspace:
//! `discover` probes the workspace and pins verified targets into a manifest;
//! `run` boots a fresh analysis state, executes exactly one manifest point and
//! reports nanosecond latencies plus a result invariant, so a measurement can
//! never silently degrade into timing a no-op. Process-cold isolation (one
//! process per point) is orchestrated by `scripts/bench/run-matrix.sh`.

pub mod discover;
pub mod manifest;
pub mod report;
pub mod runner;

#[cfg(test)]
mod tests {
    use super::discover::{discover, DiscoverArgs};
    use super::manifest::{
        hash_text, validate, BenchManifest, EditKind, EditPatch, Expect, FeatureSpec, OffsetRange,
        Target, SCHEMA_VERSION,
    };
    use super::report::PointReport;
    use super::runner::{run_point, RunArgs, RunError};

    const FIXTURE: &str = "\
Перем МодульнаяПеременная Экспорт;

Процедура Внутренняя(Знач Пар1, Пар2 = 0)
	Локальная = Пар1 + Пар2;
	Сообщить(Локальная);
КонецПроцедуры

Процедура БенчЭкспортная() Экспорт
	Внутренняя(1, 2);
	Внутренняя(3, 4);
КонецПроцедуры
";

    fn off(pat: &str) -> u32 {
        FIXTURE.find(pat).unwrap_or_else(|| panic!("fixture must contain `{pat}`")) as u32
    }

    fn write_fixture() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("Module.bsl"), FIXTURE).expect("write fixture");
        tmp
    }

    fn run_one_with_hash(
        spec: FeatureSpec,
        expect: Expect,
        file_hash: String,
    ) -> Result<PointReport, RunError> {
        let tmp = write_fixture();
        let manifest = BenchManifest {
            schema_version: SCHEMA_VERSION,
            workspace_commit: None,
            config_hash: None,
            targets: vec![Target {
                id: "t/01".to_string(),
                relative_path: "Module.bsl".to_string(),
                file_hash,
                spec,
                expect,
                note: None,
            }],
        };
        let manifest_path = tmp.path().join("manifest.json");
        std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
        run_point(&RunArgs {
            source_dir: tmp.path().to_path_buf(),
            manifest_path,
            point_id: "t/01".to_string(),
            warm_iterations: 2,
            boot_budget_ms: 60_000,
        })
    }

    fn run_one(spec: FeatureSpec, expect: Expect) -> Result<PointReport, RunError> {
        run_one_with_hash(spec, expect, hash_text(FIXTURE))
    }

    fn assert_measured(report: &PointReport) {
        assert!(report.invariant_ok, "invariant must hold: {:?}", report.invariant_error);
        assert!(report.cold_ns > 0, "cold time must be non-zero ns");
        assert_eq!(report.warm_ns.len(), 2);
        assert!(report.warm_ns.iter().all(|&ns| ns > 0), "warm ns must be non-zero");
        assert_eq!(report.digest.len(), 64);
    }

    #[test]
    fn hover_point_measures_non_empty_result() {
        let r = run_one(FeatureSpec::Hover { offset: off("Сообщить") }, Expect::NonEmpty).unwrap();
        assert_measured(&r);
        assert_eq!(r.feature, "hover");
        assert_eq!(r.observed_count, 1);
    }

    #[test]
    fn completion_point_returns_items() {
        let offset = off("Сообщить") + 4;
        let r = run_one(FeatureSpec::Completion { offset }, Expect::NonEmpty).unwrap();
        assert_measured(&r);
        assert!(r.observed_count > 0);
    }

    #[test]
    fn goto_definition_point_resolves_local_procedure() {
        let r =
            run_one(FeatureSpec::GotoDefinition { offset: off("Внутренняя(1") }, Expect::NonEmpty)
                .unwrap();
        assert_measured(&r);
    }

    #[test]
    fn references_point_finds_call_sites() {
        let offset = off("Процедура Внутренняя") + "Процедура ".len() as u32;
        let r =
            run_one(FeatureSpec::References { offset }, Expect::Cardinality { min: 2, max: 10 })
                .unwrap();
        assert_measured(&r);
    }

    #[test]
    fn rename_point_reports_edit_locations() {
        let r = run_one(
            FeatureSpec::Rename {
                offset: off("Локальная"),
                new_name: "Переименованная".to_string(),
            },
            Expect::Cardinality { min: 2, max: 10 },
        )
        .unwrap();
        assert_measured(&r);
    }

    #[test]
    fn call_hierarchy_points_resolve_within_module() {
        let decl = off("Процедура Внутренняя") + "Процедура ".len() as u32;
        let r =
            run_one(FeatureSpec::CallHierarchyPrepare { offset: decl }, Expect::NonEmpty).unwrap();
        assert_measured(&r);

        let r = run_one(
            FeatureSpec::CallHierarchyIncoming { offset: decl },
            Expect::Cardinality { min: 1, max: 10 },
        )
        .unwrap();
        assert_measured(&r);

        let caller = off("Процедура БенчЭкспортная") + "Процедура ".len() as u32;
        let r = run_one(
            FeatureSpec::CallHierarchyOutgoing { offset: caller },
            Expect::Cardinality { min: 1, max: 10 },
        )
        .unwrap();
        assert_measured(&r);
    }

    #[test]
    fn inlay_hints_point_covers_whole_file() {
        let r = run_one(FeatureSpec::InlayHints { range: None }, Expect::NonEmpty).unwrap();
        assert_measured(&r);
        assert!(r.observed_count >= 2, "literal args must produce parameter hints");
    }

    #[test]
    fn selection_range_point_builds_chains() {
        let r = run_one(
            FeatureSpec::SelectionRange { offsets: vec![off("Локальная")] },
            Expect::NonEmpty,
        )
        .unwrap();
        assert_measured(&r);
    }

    #[test]
    fn document_symbol_and_folding_points_see_structure() {
        let r =
            run_one(FeatureSpec::DocumentSymbol, Expect::Cardinality { min: 2, max: 20 }).unwrap();
        assert_measured(&r);
        let r = run_one(FeatureSpec::FoldingRange, Expect::NonEmpty).unwrap();
        assert_measured(&r);
    }

    #[test]
    fn signature_help_point_resolves_inside_call() {
        let offset = off("Внутренняя(1") + "Внутренняя(".len() as u32;
        let r = run_one(FeatureSpec::SignatureHelp { offset }, Expect::NonEmpty).unwrap();
        assert_measured(&r);
    }

    #[test]
    fn semantic_tokens_point_runs_at_handler_boundary() {
        let r = run_one(FeatureSpec::SemanticTokensFull, Expect::NonEmpty).unwrap();
        assert_measured(&r);
        assert!(r.ctx_build_ns.is_some(), "handler boundary must report ctx build cost");
    }

    #[test]
    fn code_action_point_runs_at_handler_boundary() {
        let r = run_one(
            FeatureSpec::CodeAction { range: OffsetRange { start: 0, end: FIXTURE.len() as u32 } },
            Expect::Cardinality { min: 0, max: 1000 },
        )
        .unwrap();
        assert_measured(&r);
        assert!(r.ctx_build_ns.is_some());
    }

    #[test]
    fn diagnostics_points_run_on_both_boundaries() {
        let r = run_one(FeatureSpec::DiagnosticsPush, Expect::Cardinality { min: 0, max: 1000 })
            .unwrap();
        assert_measured(&r);
        let r = run_one(FeatureSpec::DiagnosticsPull, Expect::Cardinality { min: 0, max: 1000 })
            .unwrap();
        assert_measured(&r);
        assert!(r.ctx_build_ns.is_some());
    }

    #[test]
    fn workspace_symbol_point_scans_and_empty_query_is_control() {
        let r = run_one(
            FeatureSpec::WorkspaceSymbol { query: "БенчЭ".to_string() },
            Expect::Cardinality { min: 1, max: 10 },
        )
        .unwrap();
        assert_measured(&r);
        let r = run_one(
            FeatureSpec::WorkspaceSymbol { query: String::new() },
            Expect::Cardinality { min: 0, max: 0 },
        )
        .unwrap();
        assert!(r.invariant_ok);
        assert_eq!(r.observed_count, 0);
    }

    #[test]
    fn edit_point_reports_all_phases() {
        let end = FIXTURE.len() as u32;
        let r = run_one(
            FeatureSpec::Edit {
                patch: EditPatch {
                    range: OffsetRange { start: end, end },
                    new_text: "\n// бенч-правка\n".to_string(),
                },
                edit_kind: EditKind::Body,
                followup: Box::new(FeatureSpec::Hover { offset: off("Сообщить") }),
            },
            Expect::NonEmpty,
        )
        .unwrap();
        assert_measured(&r);
        let phases = r.edit.expect("edit point must report phases");
        assert_eq!(phases.edit_kind, "body");
        assert!(phases.edit_apply_ns > 0);
        assert_eq!(phases.after_edit_ns, r.cold_ns);
    }

    #[test]
    fn burst_point_sums_sequential_core_cost() {
        let r = run_one(
            FeatureSpec::Burst {
                sequence: vec![
                    FeatureSpec::DocumentSymbol,
                    FeatureSpec::FoldingRange,
                    FeatureSpec::SemanticTokensFull,
                    FeatureSpec::DiagnosticsPush,
                ],
            },
            Expect::NonEmpty,
        )
        .unwrap();
        assert_measured(&r);
    }

    #[test]
    fn no_op_measurement_fails_the_invariant() {
        // Type definition has no metadata to point at in this workspace: the
        // result is None and the run must fail rather than time the no-op.
        let err =
            run_one(FeatureSpec::TypeDefinition { offset: off("Локальная") }, Expect::NonEmpty)
                .unwrap_err();
        assert!(matches!(err, RunError::Invariant(_)), "{err}");
    }

    #[test]
    fn drifted_file_hash_fails_before_measuring() {
        let err = run_one_with_hash(
            FeatureSpec::DocumentSymbol,
            Expect::NonEmpty,
            hash_text("stale content"),
        )
        .unwrap_err();
        assert!(matches!(err, RunError::Invariant(_)), "{err}");
        assert!(err.to_string().contains("file_hash mismatch"), "{err}");
    }

    #[test]
    fn unknown_point_id_is_a_manifest_error() {
        let tmp = write_fixture();
        let manifest = BenchManifest {
            schema_version: SCHEMA_VERSION,
            workspace_commit: None,
            config_hash: None,
            targets: vec![Target {
                id: "present/01".to_string(),
                relative_path: "Module.bsl".to_string(),
                file_hash: hash_text(FIXTURE),
                spec: FeatureSpec::DocumentSymbol,
                expect: Expect::NonEmpty,
                note: None,
            }],
        };
        let manifest_path = tmp.path().join("manifest.json");
        std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap()).unwrap();
        let err = run_point(&RunArgs {
            source_dir: tmp.path().to_path_buf(),
            manifest_path,
            point_id: "absent/01".to_string(),
            warm_iterations: 1,
            boot_budget_ms: 60_000,
        })
        .unwrap_err();
        assert!(matches!(err, RunError::Manifest(_)), "{err}");
    }

    #[test]
    fn discover_produces_a_valid_runnable_manifest() {
        let tmp = write_fixture();
        let manifest = discover(&DiscoverArgs {
            source_dir: tmp.path().to_path_buf(),
            boot_budget_ms: 60_000,
        })
        .unwrap();
        validate(&manifest).unwrap();
        assert!(
            manifest.targets.iter().any(|t| t.spec.feature_name() == "hover"),
            "hover must be discoverable on the fixture"
        );

        let manifest_path = tmp.path().join("discovered.json");
        std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
        let first = manifest.targets.first().unwrap().id.clone();
        let report = run_point(&RunArgs {
            source_dir: tmp.path().to_path_buf(),
            manifest_path,
            point_id: first,
            warm_iterations: 1,
            boot_budget_ms: 60_000,
        })
        .unwrap();
        assert!(report.invariant_ok, "{:?}", report.invariant_error);
    }
}
