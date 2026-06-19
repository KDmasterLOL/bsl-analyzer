//! End-to-end check of `&ИзменениеИКонтроль` extension merging through the diagnostics
//! orchestrator (`Analysis::diagnostics` → `ide_diagnostics::file_diagnostics`).
//!
//! The merge's INFERENCE correctness (base-sibling resolution, missing-method, changed
//! return propagation) is proven against the real Salsa db in `ide-db`'s
//! `infer_effective_*` tests; the orchestrator's remap/suppression arithmetic is unit-
//! tested in `ide-diagnostics`'s `effective` module. These tests cover the remaining
//! seam: that `file_diagnostics` actually pairs the extension file, runs the effective
//! pass, and stays a stable superset — without a real `Configuration.xml` on disk, kernel
//! types resolve but configured-metadata inference diagnostics do not fire, so this asserts
//! the wiring and liveness rather than a specific published inference diagnostic.

use ide::{Analysis, DiagnosticsConfig};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use stdx::case::CaseExt;
use vfs::{FileId, FileSet, VfsPath};

const BASE: &str = "Функция Сосед() Экспорт\n\tВозврат 1;\nКонецФункции\n\nФункция Цель() Экспорт\n\tВозврат 0;\nКонецФункции";
// `Цель` is rewritten by the extension; its `#Вставка` calls the base sibling `Сосед`
// (resolvable only through the merge) and a genuinely missing method.
const EXT: &str = "&ИзменениеИКонтроль(\"Цель\")\nФункция Расш1_Цель()\n#Вставка\n\tЗначение1 = Сосед();\n\tЗначение2 = НетТакого();\n#КонецВставки\n\tВозврат 0;\nКонецФункции";

// Weaving signature check: base has a two-parameter procedure and a one-parameter
// by-value function the extension can intercept.
const SIG_BASE: &str = "Процедура ПриЗаписи(Отказ, Параметры) Экспорт\nКонецПроцедуры\n\nФункция Вычислить(Знач А) Экспорт\n\tВозврат А;\nКонецФункции";
// `&Перед` interceptor drops a parameter (one instead of two) → applicability defect.
const SIG_EXT_BAD: &str = "&Перед(\"ПриЗаписи\")\nПроцедура Расш1_ПриЗаписи(Отказ)\nКонецПроцедуры";
// `&Вместо` interceptor mirrors the function signature exactly, including `Знач`.
const SIG_EXT_GOOD: &str =
    "&Вместо(\"Вычислить\")\nФункция Расш1_Вычислить(Знач А)\n\tВозврат А;\nКонецФункции";
// `&После` on the base *function* `Вычислить` is not applicable at all (functions accept
// only `&Вместо`); the deliberate parameter-count divergence must NOT also surface as a
// signature mismatch — the applicability error owns the report.
const SIG_EXT_FUNC_AFTER: &str =
    "&После(\"Вычислить\")\nПроцедура Расш1_Вычислить()\nКонецПроцедуры";

struct Fixture {
    analysis: Analysis,
    main_file: FileId,
    ext_file: FileId,
}

fn setup() -> Fixture {
    setup_with(BASE, EXT)
}

fn setup_with(base: &str, ext: &str) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let main_root = temp.path().join("src/cf");
    let ext_root = temp.path().join("src/cfe/X");
    std::fs::create_dir_all(&main_root).unwrap();
    std::fs::create_dir_all(&ext_root).unwrap();
    std::mem::forget(temp);

    let mut db = RootDatabaseImpl::new();
    db.set_all_config_paths(vec![
        (None, main_root.clone()),
        (Some("X".to_string()), ext_root.clone()),
    ]);

    let main_file = FileId(0);
    let ext_file = FileId(1);
    let mut file_set = FileSet::default();
    let main_path = main_root.join("CommonModules/М/Ext/Module.bsl");
    let ext_path = ext_root.join("CommonModules/М/Ext/Module.bsl");
    file_set.insert(main_file, VfsPath::new(main_path.to_string_lossy().as_ref()));
    file_set.insert(ext_file, VfsPath::new(ext_path.to_string_lossy().as_ref()));
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    db.set_file_source_root(main_file, SourceRootId(0));
    db.set_file_source_root(ext_file, SourceRootId(0));
    db.set_file_text(main_file, base);
    db.set_file_text(ext_file, ext);

    Fixture { analysis: Analysis::from_database(db), main_file, ext_file }
}

#[test]
fn extension_file_pairs_and_merge_resolves_base_sibling() {
    let fx = setup();
    let db = fx.analysis.database();

    // The orchestrator's pairing gate fires for the extension file…
    let eid = ide_db::effective_target(db, fx.ext_file)
        .expect("extension file with a usable change-and-validate must pair to its base");
    // …and not for the base file.
    assert!(
        ide_db::effective_target(db, fx.main_file).is_none(),
        "a base-config file must not pair (only extension modules route to effective)",
    );

    // The merge is live end-to-end: `#Вставка` code sees the base sibling `Сосед` (returns
    // 1 → Число), which is invisible to the standalone extension file.
    use hir::Builders;
    let eff = hir::infer_effective(db, eid);
    assert_eq!(
        eff.var_types.get(&"Значение1".fold_lower()).copied(),
        Some(db.number(None, None)),
        "base sibling `Сосед()` must resolve through the effective module; var_types = {:?}",
        eff.var_types,
    );
}

#[test]
fn extension_diagnostics_are_a_stable_superset_without_spurious_inference() {
    let fx = setup();
    let diags = fx.analysis.diagnostics(fx.ext_file, &DiagnosticsConfig::all_enabled());

    // The orchestrator runs (standalone + effective passes) and publishes the standalone
    // style/metadata diagnostics — here the non-canonical `#Вставка` keyword.
    assert!(
        diags.iter().any(|d| d.code == ide::DiagnosticCode::CanonicalSpellingKeywords),
        "standalone style diagnostics must survive the orchestrator; got {:?}",
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
    );

    // No inference false positive is published for the change-and-validate body: the copied
    // base reference resolves through the merge, the missing `НетТакого()` does not surface
    // as a method on a configured type (no real config), and the inserted region carries no
    // spurious `UnresolvedMethodCall`. (Remap of a real inserted diagnostic is covered by
    // the `remap_inserted` unit tests + `infer_effective_*` integration tests.)
    assert!(
        !diags.iter().any(|d| d.code == ide::DiagnosticCode::UnresolvedMethodCall),
        "no spurious UnresolvedMethodCall must be published for the merged body; got {:?}",
        diags.iter().map(|d| (d.code, d.range)).collect::<Vec<_>>(),
    );
}

#[test]
fn weaving_interceptor_signature_mismatch_is_reported() {
    let fx = setup_with(SIG_BASE, SIG_EXT_BAD);
    let diags = fx.analysis.diagnostics(fx.ext_file, &DiagnosticsConfig::all_enabled());

    assert!(
        diags.iter().any(|d| d.code == ide::DiagnosticCode::WeavingSignatureMismatch),
        "a &Перед interceptor declaring fewer parameters than the base method must be \
         reported; got {:?}",
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
    );
    // The base config file itself never pairs for weaving, so it carries no such diagnostic.
    let base_diags = fx.analysis.diagnostics(fx.main_file, &DiagnosticsConfig::all_enabled());
    assert!(
        !base_diags.iter().any(|d| d.code == ide::DiagnosticCode::WeavingSignatureMismatch),
        "the base module must not produce a weaving signature diagnostic",
    );
}

#[test]
fn weaving_interceptor_matching_signature_is_clean() {
    let fx = setup_with(SIG_BASE, SIG_EXT_GOOD);
    let diags = fx.analysis.diagnostics(fx.ext_file, &DiagnosticsConfig::all_enabled());

    assert!(
        !diags.iter().any(|d| d.code == ide::DiagnosticCode::WeavingSignatureMismatch),
        "a &Вместо interceptor whose signature matches the base function (including Знач) must \
         not be flagged; got {:?}",
        diags.iter().map(|d| (d.code, d.range)).collect::<Vec<_>>(),
    );
}

#[test]
fn weaving_before_after_on_function_is_not_applicable() {
    let fx = setup_with(SIG_BASE, SIG_EXT_FUNC_AFTER);
    let diags = fx.analysis.diagnostics(fx.ext_file, &DiagnosticsConfig::all_enabled());

    assert!(
        diags.iter().any(|d| d.code == ide::DiagnosticCode::WeavingAnnotationNotApplicable),
        "a &После interceptor targeting a base function must be reported as not applicable; \
         got {:?}",
        diags.iter().map(|d| d.code).collect::<Vec<_>>(),
    );
    // The applicability error owns the report — the parameter-count divergence must not also
    // surface as a signature mismatch on the same method.
    assert!(
        !diags.iter().any(|d| d.code == ide::DiagnosticCode::WeavingSignatureMismatch),
        "an inapplicable annotation must suppress the signature diagnostic; got {:?}",
        diags.iter().map(|d| (d.code, d.range)).collect::<Vec<_>>(),
    );
}

#[test]
fn goto_definition_on_weaving_annotation_jumps_to_base_method() {
    let fx = setup_with(SIG_BASE, SIG_EXT_GOOD);
    // Cursor on the `Вычислить` name inside the `&Вместо("...")` annotation string (its first
    // occurrence in the ext text is the annotation argument, before the prefixed method name).
    let offset = SIG_EXT_GOOD.find("Вычислить").expect("annotation target present") as u32;

    let target = fx
        .analysis
        .goto_definition(fx.ext_file, offset)
        .expect("goto-definition must resolve the annotation target to the base method");
    assert_eq!(target.file_id, fx.main_file, "must navigate to the base module file");
    assert_eq!(target.name, "Вычислить");
}

#[test]
fn goto_definition_on_change_and_validate_annotation_jumps_to_base_method() {
    // `&ИзменениеИКонтроль("Цель")` is not a weaving interceptor but still names a base method;
    // goto-definition resolves it through the same path.
    let fx = setup();
    let offset = EXT.find("Цель").expect("annotation target present") as u32;

    let target = fx
        .analysis
        .goto_definition(fx.ext_file, offset)
        .expect("goto-definition must resolve the change-and-validate target to the base method");
    assert_eq!(target.file_id, fx.main_file, "must navigate to the base module file");
    assert_eq!(target.name, "Цель");
}
