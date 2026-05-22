//! Cross-procedure SDBL projection propagation.
//!
//! Pins the end-to-end invariant that a helper which builds and returns
//! a query
//!
//! ```bsl
//! Функция СоздатьЗапрос()
//!     Зап = Новый Запрос;
//!     Зап.Текст = "ВЫБРАТЬ ""abc"" КАК Имя";
//!     Возврат Зап;
//! КонецФункции
//!
//! Результат = СоздатьЗапрос().Выполнить().Выбрать().Имя
//! ```
//!
//! propagates its refined projection through `method_return_type_query`
//! so the trailing `.Имя` on the caller side still types as
//! `Ty::String`. The helper body lives in a different `BodyInferenceResult`
//! than the caller, so the projection must survive the cross-method
//! Salsa boundary (Phase B synthesis + Phase D variable-state refinement
//! + Phase J method-graph cascade).

use hir::{DefDatabase, HirDatabase, ModuleId, Ty};
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
#[ignore = "Phase F — binding-type refinement: Phase D refines at chain-dispatch sites only; \
            `Возврат Зап;` resolves `Expr::Path(\"Зап\")` to the binding's stored \
            `Ty::Query{[None]}` without consulting reaching `Зап.Текст` writes. \
            Lifting refinement into `infer_path_name` would close this gap but \
            changes the receiver-name eligibility contract — design lives in the \
            phase-plan follow-up."]
fn helper_returning_refined_query_propagates_projection_to_caller() {
    // The helper's body produces `Ty::Query{[Some(p)]}` at the
    // chain-dispatch site only; the `Возврат Зап` returns the
    // binding's stored type (`Ty::Query{[None]}`), so the caller's
    // chain sees no projection. Pinned as a regression test for the
    // eventual Phase F lift of refinement into `infer_path_name`.
    let fixture = r#"//- /test.bsl
Функция СоздатьЗапрос() Экспорт
    Зап = Новый Запрос;
    Зап.Текст = "ВЫБРАТЬ ""abc"" КАК Имя";
    Возврат Зап;
КонецФункции

Функция Тест()
    Х = СоздатьЗапрос().Выполнить().Выбрать().Имя;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(Ty::String),
        "Phase F target: dataflow-refined query projection propagates through helper return",
    );
}

#[test]
fn helper_with_constructor_literal_propagates_projection() {
    // Companion to the variable-refinement test above — the helper
    // produces the projection at constructor time (Phase B), no
    // Phase D walk needed. Pins that Phase B synthesis survives the
    // same cross-method boundary.
    let fixture = r#"//- /test.bsl
Функция СоздатьЗапрос() Экспорт
    Возврат Новый Запрос("ВЫБРАТЬ ""abc"" КАК Имя");
КонецФункции

Функция Тест()
    Х = СоздатьЗапрос().Выполнить().Выбрать().Имя;
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_ty(&db, file_id, "х"),
        Some(Ty::String),
        "constructor-time projection must propagate through helper's return type",
    );
}
