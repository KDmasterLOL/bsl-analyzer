//! Behavioural tests for the JSDoc → FunctionSignature wiring landed in
//! Task 11.
//!
//! The call sites read `var_types` because `InferenceResult` does not
//! expose method signatures directly — assigning the call result to a
//! variable surfaces the lowered return type through the merged
//! file-level var_types map. Each test sets up a CommonModule with a
//! JSDoc-annotated method, calls it from `/test.bsl`, and asserts the
//! resulting `Ty`.

use hir::{DefDatabase, HirDatabase, MetadataKind, ModuleId, Name, Ty};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use test_fixture::Fixture;
use vfs::FileId;

fn setup(fixture_text: &str) -> (RootDatabaseImpl, FileId) {
    let fixture = Fixture::parse(fixture_text);
    let mut db = RootDatabaseImpl::new();
    let mut file_set = vfs::FileSet::default();
    for (file_id, file) in &fixture.files {
        file_set.insert(*file_id, file.path.clone());
    }
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    for (file_id, file) in &fixture.files {
        db.set_file_source_root(*file_id, SourceRootId(0));
        db.set_file_text(*file_id, &file.content);
    }
    let test_file = fixture
        .files
        .iter()
        .find(|(_, f)| f.path.as_path().to_string_lossy().ends_with("/test.bsl"))
        .map(|(id, _)| *id)
        .expect("fixture must contain /test.bsl");
    let _ = db.module_bodies(ModuleId::new(test_file));
    (db, test_file)
}

fn var_ty(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) -> Option<Ty> {
    db.infer(file_id).var_types.get(var_lower).cloned()
}

#[test]
fn jsdoc_return_type_primitive_flows_into_var_types() {
    // Simplest case: JSDoc names a primitive return type. The cascade in
    // `materialise_signature` must lower `TypeRef::Builtin(String)` to
    // `Ty::String`, so `Х = ОбщегоНазначения.Имя()` types `х` as String.
    let fixture = r#"
//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
// Возвращаемое значение:
//   Строка - имя реквизита
Функция Имя() Экспорт
    Возврат "";
КонецФункции

//- /test.bsl
Функция Тест()
    Х = ОбщегоНазначения.Имя();
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(Ty::String),
        "JSDoc `Возвращаемое значение: Строка` must lower into Ty::String"
    );
}

#[test]
fn jsdoc_catalog_ref_return_lowers_to_metadata_ref() {
    // Qualified JSDoc return (`СправочникСсылка.Номенклатура`) must
    // survive the TypeRef round-trip and produce
    // `Ty::MetadataRef { CatalogRef, Номенклатура }`. This is the first
    // end-to-end proof that TypeRef → TyLoweringContext → FunctionSignature
    // honours user-authored JSDoc — the user-visible effect Task 11 was
    // built for.
    let fixture = r#"
//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
// Возвращаемое значение:
//   СправочникСсылка.Номенклатура - ссылка на номенклатуру
Функция ПолучитьСсылку() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Функция Тест()
    Ссылка = ОбщегоНазначения.ПолучитьСсылку();
    Возврат Ссылка;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "ссылка"),
        Some(Ty::MetadataRef {
            kind: MetadataKind::CatalogRef, name: Name::new("Номенклатура")
        }),
        "JSDoc qualified return must lower to Ty::MetadataRef{{ CatalogRef, Номенклатура }}"
    );
}

#[test]
fn missing_jsdoc_keeps_unknown_no_regression() {
    // Regression guard: a CommonModule function without JSDoc must keep
    // the legacy `Ty::Unknown` return type — the cascade in
    // `materialise_signature` uses `return_type_ref` only when present,
    // then falls back to `MethodSymbol::return_type` (`Ty::Unknown` for
    // functions).
    //
    // The inference pipeline treats `Ty::Unknown` as "no assignment
    // tracked": `infer_stmt` skips the `var_types` write when the RHS is
    // Unknown (see `crates/hir-ty/src/infer.rs` Stmt::Assign branch).
    // So `var_ty("х") == None` is the observable proof that the legacy
    // shape is unchanged — a stray `Some(Ty::<anything else>)` would
    // mean the new wiring accidentally propagated a type we shouldn't
    // have known. We also check no `UnresolvedMethodCall` fires — the
    // method did resolve, it just carries no typed return.
    let fixture = r#"
//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
Функция БезКомментария() Экспорт
    Возврат Истина;
КонецФункции

//- /test.bsl
Функция Тест()
    Х = ОбщегоНазначения.БезКомментария();
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        None,
        "Ty::Unknown return is not tracked in var_types — matches legacy behaviour"
    );
    let infer = db.infer(file_id);
    let unresolved: Vec<_> = infer
        .diagnostics
        .iter()
        .filter(|d| matches!(d, hir::InferenceDiagnostic::UnresolvedMethodCall { .. }))
        .collect();
    assert!(unresolved.is_empty(), "method must resolve even without JSDoc");
}

#[test]
fn jsdoc_union_return_lowers_to_ty_union() {
    // End-to-end: JSDoc `// Возвращаемое значение: Число, Строка` lowers
    // through `parse_method_doc_types` → `MethodSymbol.return_type_ref` →
    // `materialise_signature` → `TyLoweringContext::lower_type_ref(Union)`
    // → `Ty::union(...)`, producing a canonicalised union at the call site.
    let fixture = r#"
//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
// Возвращаемое значение:
//   Число, Строка - результат
Функция Результат() Экспорт
    Возврат "";
КонецФункции

//- /test.bsl
Функция Тест()
    Р = ОбщегоНазначения.Результат();
    Возврат Р;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    let ty = var_ty(&db, file_id, "р").expect("var_types must track union return");
    match ty {
        Ty::Union(ref parts) => {
            assert_eq!(parts.len(), 2, "Union should have exactly 2 members");
            assert!(parts.contains(&Ty::Number));
            assert!(parts.contains(&Ty::String));
        }
        other => panic!("expected Ty::Union, got {other:?}"),
    }
}

#[test]
fn jsdoc_three_level_return_lowers_through_manager_chain() {
    // Same wiring must hold for three-segment calls:
    // `Документы.ПКО.ПолучитьСсылку()` should see the manager-module's
    // JSDoc-annotated return type. Proves `resolve_three_level_call`
    // funnels through the same `materialise_signature`.
    let fixture = r#"
//- /Documents/ПКО/Ext/ManagerModule.bsl
// Возвращаемое значение:
//   ДокументСсылка.ПКО - ссылка на документ
Функция ПолучитьСсылку() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Функция Тест()
    Ссылка = Документы.ПКО.ПолучитьСсылку();
    Возврат Ссылка;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "ссылка"),
        Some(Ty::MetadataRef { kind: MetadataKind::DocumentRef, name: Name::new("ПКО") }),
        "3-level call must lower JSDoc `ДокументСсылка.ПКО` to Ty::MetadataRef"
    );
}
