//! Integration tests for `Expr::Path` inference after Task 1.7 routes name
//! resolution through the unified [`Resolver`] cascade.
//!
//! These lock down the edge cases Codex flagged during review of Task 1.7:
//! platform-global builtins, names present only in the hand-curated
//! `hir-ty::builtin` table (but absent from `bsl_platform`'s global-function
//! index), and BSL implicit locals that must shadow module-level methods.
//!
//! `InferenceResult::expr_types` is per-method-body and not merged into the
//! file-level result, so these tests assert on the file-level `var_types`
//! map that *is* merged. Feeding the names through `X = Foo(...)` turns the
//! return type of `Foo` into `var_types["x"]`, which is observable.

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
    let root = SourceRoot::new_local(file_set);
    db.set_source_root(SourceRootId(0), root);
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
    // Ensure HIR bodies are built; infer_query reads them.
    let _ = db.module_bodies(ModuleId::new(test_file));
    (db, test_file)
}

fn var_type(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) -> Option<Ty> {
    db.infer(file_id).var_types.get(var_lower).cloned()
}

// ---------- platform-global builtin via Resolver + hir-ty signature ----------

#[test]
fn path_resolves_platform_builtin_via_resolver() {
    // `СтрДлина` is in both `bsl_platform`'s global-function index (so
    // Resolver classifies it as `Resolution::Builtin`) and in
    // `hir-ty::builtin` (typed signature with ret = Number). The assignment
    // binds `Х` to the return type, which is observable through var_types.
    let fixture = r#"//- /test.bsl
Функция Тест()
    Х = СтрДлина("abc");
    Возврат Х;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_type(&db, file_id, "х"),
        Some(Ty::Number),
        "platform builtin call must type `Х` as Number (СтрДлина ret type)"
    );
}

// ---------- implicit locals shadow module-level methods ----------

#[test]
fn implicit_local_assignment_shadows_module_procedure() {
    // BSL has no explicit `Var` declarations. The first assignment inside a
    // method body creates an implicit local. `Данные = 42` inside `Тест`
    // must shadow the module-level `Процедура Данные()` so that
    // `Рез = Данные;` copies the Number from the local, not an Unknown
    // from the module-procedure branch.
    let fixture = r#"//- /test.bsl
Процедура Данные() Экспорт
КонецПроцедуры

Функция Тест()
    Данные = 42;
    Рез = Данные;
    Возврат Рез;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_type(&db, file_id, "рез"),
        Some(Ty::Number),
        "implicit local must shadow module procedure; `Рез` should inherit Number"
    );
}

#[test]
fn implicit_local_assignment_shadows_builtin_in_value_position() {
    // Builtins are resolved from call syntax (`Строка(...)`), not from a
    // bare value/receiver token. Once `Строка = 42` creates an implicit
    // local, `Рез = Строка` must read the local Number rather than collapse
    // to the platform builtin/function-name fallback.
    let fixture = r#"//- /test.bsl
Функция Тест()
    Строка = 42;
    Рез = Строка;
    Возврат Рез;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        var_type(&db, file_id, "рез"),
        Some(Ty::Number),
        "implicit local named like a builtin must win in value position"
    );
}
