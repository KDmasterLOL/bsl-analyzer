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

struct Fixture {
    analysis: Analysis,
    main_file: FileId,
    ext_file: FileId,
}

fn setup() -> Fixture {
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
    db.set_file_text(main_file, BASE);
    db.set_file_text(ext_file, EXT);

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
