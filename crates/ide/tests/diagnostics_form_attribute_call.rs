//! Phase 5 regression coverage for the qualified-call clean-architecture
//! refactor. The user-reported bug — `ТаблицаДокументов.Очистить()` in a
//! managed form's BSL module producing two false-positive diagnostics
//! (`MissingCommonModuleMethod` + `UnresolvedMethodCall`) — was the
//! driver of the entire refactor. This test pins the negative
//! invariant from the surface inference exposes (no
//! `UnresolvedMethodCall` for valid form-data method calls), so a
//! regression in any layer (parser → loader → schema → form_attr →
//! field_lookup → cascade gate → PlatformObject bridge) surfaces
//! immediately.
//!
//! The fixture re-uses
//! `crates/bsl-metadata/fixtures/designer/DataProcessors/ТестоваяОбработка/`
//! which has a `НастройкиЭксель` `TabularSection` on the
//! `Объект` MainAttribute (added during the previous Объект.<dot>
//! iteration), so chained calls
//! (`Объект.НастройкиЭксель.<TabularSectionMethod>(…)`) reach the
//! same `ДанныеФормыКоллекция` HBK type the user's
//! `ТаблицаДокументов.Очистить()` resolves to.
//!
//! Method-surface invariant (mirrors `completion_form_object.rs`):
//! methods on the FormData wrapper resolve through
//! `ДанныеФормыКоллекция` / `ДанныеФормыСтруктура` — they MUST stay
//! silent in the inference diagnostic surface.

use bsl_platform::PlatformDataInner;
use hir::{
    Builders, InferenceDiagnostic, MetadataKind, TypeId, TypeKernelDb, TypeKind,
    UnresolvedMethodKind,
};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::path::PathBuf;
use vfs::{FileId, FileSet, VfsPath};

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

fn data_processor_module_path() -> PathBuf {
    designer_fixture_path().join("DataProcessors/ТестоваяОбработка/Forms/Форма/Ext/Form/Module.bsl")
}

fn setup_form_module(disk_path: PathBuf, bsl: &str) -> (RootDatabaseImpl, FileId) {
    assert!(disk_path.exists(), "fixture missing: {}", disk_path.display());
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);
    let mut file_set = FileSet::default();
    file_set.insert(file_id, VfsPath::new(disk_path.to_string_lossy().as_ref()));
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    db.set_file_source_root(file_id, SourceRootId(0));
    db.set_file_text(file_id, bsl);
    db.set_all_config_paths(vec![(None, designer_fixture_path())]);
    (db, file_id)
}

fn has_platform_data() -> bool {
    !PlatformDataInner::instance().all_methods().is_empty()
}

fn unresolved_kinds(db: &RootDatabaseImpl, file_id: FileId) -> Vec<UnresolvedMethodKind> {
    use hir::HirDatabase;
    db.infer(file_id)
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedMethodCall { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect()
}

fn var_ty(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) -> Option<TypeId> {
    use hir::HirDatabase;
    db.infer(file_id).var_types.get(var_lower).copied()
}

fn is_metadata_ref(db: &RootDatabaseImpl, ty: TypeId, kind: MetadataKind, name: &str) -> bool {
    matches!(
        db.lookup_type(ty),
        TypeKind::MetadataRef(facet) if facet.kind == kind && facet.name.as_str() == name
    )
}

#[test]
fn chained_form_attribute_method_call_silent() {
    // The user-reported scenario expressed through the chained shape
    // available in our DataProcessor fixture: `Объект.НастройкиЭксель`
    // is a `TabularSection`, so `.Очистить()` resolves through the
    // FormData wrapper (`ДанныеФормыКоллекция.Очистить`) — exactly
    // the platform method the user's `ТаблицаДокументов.Очистить()`
    // resolved to in their real project.
    //
    // Pre-Phase-2 the eager `M.Method()` ⇒ CommonModule classification
    // in body lowering forced `MissingCommonModuleMethod` on this
    // pattern (lowering had no `db` to see the form attribute) and
    // inference followed up with `UnresolvedMethodCall`. The cascade
    // gate now resolves through `infer_path_name`'s steps 4 / 4b, the
    // resulting `Ty::FormData{Collection,…}` flows through
    // `lookup_method`, and the platform method resolves cleanly.
    if !has_platform_data() {
        eprintln!("Skipping: no platform data available");
        return;
    }

    let bsl = "Процедура Тест()\n    Объект.НастройкиЭксель.Очистить();\nКонецПроцедуры\n";
    let (db, file_id) = setup_form_module(data_processor_module_path(), bsl);

    let kinds = unresolved_kinds(&db, file_id);
    assert!(
        kinds.is_empty(),
        "form-attribute method call must not produce UnresolvedMethodCall, got: {:?}",
        kinds
    );
}

#[test]
fn form_data_collection_find_by_id_silent_and_preserves_row_schema() {
    // `НайтиПоИдентификатору` exists on `ДанныеФормыКоллекция`, not on the
    // ordinary object-module `ТабличнаяЧасть` surface. In a managed form,
    // `Объект.<ТЧ>` must therefore keep the form-data collection wrapper
    // while still remembering the tabular-section row schema for chained
    // column access.
    if !has_platform_data() {
        eprintln!("Skipping: no platform data available");
        return;
    }

    let bsl = "Процедура Тест()\n    \
        Строка = Объект.НастройкиЭксель.НайтиПоИдентификатору(1);\n    \
        ИтогАктивна = Строка.Активна;\n\
        КонецПроцедуры\n";
    let (db, file_id) = setup_form_module(data_processor_module_path(), bsl);

    let kinds = unresolved_kinds(&db, file_id);
    assert!(
        kinds.is_empty(),
        "FormDataCollection.НайтиПоИдентификатору must not produce UnresolvedMethodCall, got: {:?}",
        kinds
    );

    let row = var_ty(&db, file_id, "строка").expect("строка must be typed");
    assert!(
        matches!(db.lookup_type(row), TypeKind::Union(members)
            if members.contains(&db.undefined())
                && members.iter().any(|member| is_metadata_ref(
                    &db,
                    *member,
                    MetadataKind::TabularSectionRow {
                        parent: bsl_metadata::MdoType::DataProcessor
                    },
                    "ТестоваяОбработка.НастройкиЭксель",
                ))
        ),
        "FindByID must rebind FormDataCollectionItem to the concrete tabular-section row, got {:?}",
        db.lookup_type(row),
    );
    assert_eq!(
        var_ty(&db, file_id, "итогактивна"),
        Some(db.boolean()),
        "row column access after FindByID must keep the tabular-section schema",
    );
}

#[test]
fn chained_form_attribute_misspelled_method_emits_method_not_found() {
    // Cross-validation for `chained_form_attribute_method_call_silent`:
    // if a regression silenced ALL diagnostics on this receiver shape
    // (e.g. by widening the cascade-gate's silent arm or losing
    // `receiver_display_name` for `ДанныеФормыКоллекция`), the
    // sibling test would still pass on `Ty::Unknown`. This test pins
    // the positive direction — the receiver IS typed correctly, so
    // a misspelled method emits exactly one
    // `UnresolvedMethodCall { MethodNotFound }`.
    if !has_platform_data() {
        eprintln!("Skipping: no platform data available");
        return;
    }

    let bsl = "Процедура Тест()\n    \
        Объект.НастройкиЭксель.СовершенноНетТакогоМетода();\n\
        КонецПроцедуры\n";
    let (db, file_id) = setup_form_module(data_processor_module_path(), bsl);

    let kinds = unresolved_kinds(&db, file_id);
    assert_eq!(
        kinds,
        vec![UnresolvedMethodKind::MethodNotFound],
        "misspelled tabular-section method must surface MethodNotFound exactly once \
         (proves the receiver positively resolves to its FormData type rather than \
          falling through silently as Ty::Unknown), got: {:?}",
        kinds
    );
}

#[test]
fn value_list_form_attribute_method_call_silent() {
    // User-reported: `СценарииВыгрузки.Добавить()` on a `v8:ValueListType`
    // form attribute falsely fired `UnresolvedMethodCall { ReceiverNotResolved }`
    // ("Не удалось разрешить модуль …"). The attribute type used to lower to
    // `Unknown`, which routed the call into the bare-ident module cascade.
    // Now `v8:ValueListType` lowers to the kernel `ValueList`, so `.Добавить()`
    // resolves through the platform method tables.
    if !has_platform_data() {
        eprintln!("Skipping: no platform data available");
        return;
    }

    let bsl = "Процедура Тест()\n    \
        НоваяСтр = СписокЗначенийРеквизит.Добавить();\n\
        КонецПроцедуры\n";
    let (db, file_id) = setup_form_module(data_processor_module_path(), bsl);

    let kinds = unresolved_kinds(&db, file_id);
    assert!(
        kinds.is_empty(),
        "ValueList form-attribute method call must not produce UnresolvedMethodCall, got: {:?}",
        kinds
    );
}

#[test]
fn value_list_form_attribute_types_to_kernel_value_list() {
    // Positive cross-check for the silent sibling: the form attribute must
    // type to the kernel `ValueList` (not `Unknown`). A miss on platform
    // value-collection receivers is deliberately silent — so MethodNotFound
    // can't distinguish "resolved" from "fell through on Unknown"; the
    // inferred receiver type can.
    if !has_platform_data() {
        eprintln!("Skipping: no platform data available");
        return;
    }

    let bsl = "Процедура Тест()\n    \
        Список = СписокЗначенийРеквизит;\n\
        КонецПроцедуры\n";
    let (db, file_id) = setup_form_module(data_processor_module_path(), bsl);

    assert_eq!(
        var_ty(&db, file_id, "список"),
        Some(db.value_list(None)),
        "v8:ValueListType form attribute must infer to the kernel ValueList",
    );
}

#[test]
fn findrows_chain_no_unresolved_method_call() {
    // Cross-check with the typed-array rebind iteration (commit
    // c9cab461). `НайтиСтроки` returns `TypedArray<TabularSectionRow>`
    // so `[0].Значение` chains through to the row's column type.
    // Both link types must travel through the inference pipeline
    // without firing UnresolvedMethodCall — a regression at the
    // PlatformObject bridge or the cascade gate would surface here.
    if !has_platform_data() {
        eprintln!("Skipping: no platform data available");
        return;
    }

    let bsl = "Процедура Тест(Отбор)\n    \
        Х = Объект.НастройкиЭксель.НайтиСтроки(Отбор)[0].Значение;\n\
        КонецПроцедуры\n";
    let (db, file_id) = setup_form_module(data_processor_module_path(), bsl);

    let kinds = unresolved_kinds(&db, file_id);
    assert!(
        kinds.is_empty(),
        "chained НайтиСтроки result must not produce UnresolvedMethodCall, got: {:?}",
        kinds
    );
}
