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
    use super::runner::{run_point, RunArgs, RunError, RunMode};

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

    fn run_one_full(
        spec: FeatureSpec,
        expect: Expect,
        file_hash: String,
        mode: RunMode,
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
            mode,
            warm_iterations: 2,
            boot_budget_ms: 60_000,
            trim_settle_ms: 10,
        })
    }

    fn run_one_with_hash(
        spec: FeatureSpec,
        expect: Expect,
        file_hash: String,
    ) -> Result<PointReport, RunError> {
        run_one_full(spec, expect, file_hash, RunMode::Latency)
    }

    fn run_one(spec: FeatureSpec, expect: Expect) -> Result<PointReport, RunError> {
        run_one_with_hash(spec, expect, hash_text(FIXTURE))
    }

    fn run_one_mode(
        spec: FeatureSpec,
        expect: Expect,
        mode: RunMode,
    ) -> Result<PointReport, RunError> {
        run_one_full(spec, expect, hash_text(FIXTURE), mode)
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
        assert!(phases.edit_apply_ns.expect("mode A reports apply time") > 0);
        assert!(phases.warm_before_p50_ns.is_some());
        assert_eq!(phases.after_edit_ns, r.cold_ns);
    }

    fn body_edit_spec() -> FeatureSpec {
        let end = FIXTURE.len() as u32;
        FeatureSpec::Edit {
            patch: EditPatch {
                range: OffsetRange { start: end, end },
                new_text: "\n// бенч-правка\n".to_string(),
            },
            edit_kind: EditKind::Body,
            followup: Box::new(FeatureSpec::Hover { offset: off("Сообщить") }),
        }
    }

    fn signature_edit_spec() -> FeatureSpec {
        // Insert a defaulted parameter right before the closing paren of the
        // non-export procedure's signature.
        let paren = off("Пар2 = 0)") + "Пар2 = 0".len() as u32;
        FeatureSpec::Edit {
            patch: EditPatch {
                range: OffsetRange { start: paren, end: paren },
                new_text: ", ПарНовый = Неопределено".to_string(),
            },
            edit_kind: EditKind::Signature,
            followup: Box::new(FeatureSpec::Hover { offset: off("Внутренняя") }),
        }
    }

    #[test]
    fn recompute_mode_reports_churn_for_both_edit_kinds() {
        // The event callback is installed at database construction; the flag
        // must be visible before boot. Process-global and never unset — other
        // tests at most gain inert counters.
        std::env::set_var("BSL_SALSA_EVENTS", "1");

        for (spec, kind) in [(body_edit_spec(), "body"), (signature_edit_spec(), "signature")] {
            let r = run_one_mode(spec, Expect::NonEmpty, RunMode::Recompute).unwrap();
            assert_eq!(r.mode, "recompute");
            let churn = r.recompute.as_ref().expect("mode B must attach a recompute report");
            assert!(churn.distinct_keys >= 1, "{kind}: an edit must recompute something");
            assert!(!churn.families.is_empty(), "{kind}: query families must be attributed");
            assert!(
                churn.distinct_modules >= 1 && !churn.modules.is_empty(),
                "{kind}: keys must resolve to module paths"
            );
            assert!(!churn.modules_truncated);
            let phases = r.edit.expect("edit point keeps its kind in mode B");
            assert_eq!(phases.edit_kind, kind);
            assert!(phases.edit_apply_ns.is_none(), "no uninstrumented split in mode B");
        }
    }

    #[test]
    fn recompute_mode_profiles_cold_execution_of_plain_points() {
        std::env::set_var("BSL_SALSA_EVENTS", "1");
        let r = run_one_mode(FeatureSpec::DocumentSymbol, Expect::NonEmpty, RunMode::Recompute)
            .unwrap();
        let churn = r.recompute.expect("mode B must attach a recompute report");
        assert!(churn.distinct_keys >= 1, "a cold call must execute queries");
        assert!(churn.families.iter().any(|f| f.execute > 0));
        assert!(r.warm_ns.is_empty(), "mode B takes no warm latency samples");
    }

    #[test]
    fn memory_mode_brackets_the_execution_with_rss_points() {
        let r =
            run_one_mode(FeatureSpec::DocumentSymbol, Expect::NonEmpty, RunMode::Memory).unwrap();
        assert_eq!(r.mode, "memory");
        let mem = r.memory.expect("mode C must attach a memory report");
        assert!(mem.rss_before_bytes > 0);
        assert!(mem.rss_after_bytes > 0);
        assert!(mem.rss_after_trim_bytes > 0);
        assert!(mem.rss_after_deep_trim_bytes > 0);
        assert!(
            mem.phase_peak_bytes > 0 || mem.peak_is_lower_bound,
            "a zero peak is only acceptable when flagged as a lower bound"
        );
        assert_eq!(mem.peak_is_lower_bound, mem.sample_count < 3);
        assert!(!mem.ingredient_counts.is_empty());
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
            mode: RunMode::Latency,
            warm_iterations: 1,
            boot_budget_ms: 60_000,
            trim_settle_ms: 10,
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
            mode: RunMode::Latency,
            warm_iterations: 1,
            boot_budget_ms: 60_000,
            trim_settle_ms: 10,
        })
        .unwrap();
        assert!(report.invariant_ok, "{:?}", report.invariant_error);
    }
}
