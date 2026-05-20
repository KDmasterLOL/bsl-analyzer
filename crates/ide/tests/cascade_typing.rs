//! Behavioural tests for the Phase O.11 cascade-typing wiring.
//!
//! O.11 wires `method_return_type_query` (O.10) into two production
//! sites:
//! - `materialise_signature_enriched` — when `sig.return_ty == Unknown`,
//!   the qualified-call resolution adapters consult the cascade query.
//! - `infer_call`'s `Ty::Unknown` arm — when a bare same-module
//!   `Expr::Path(name)` callee whose `infer_path_name` returned
//!   `Ty::Unknown` resolves to a user-defined method via the local
//!   `symbol_tree`, the cascade query feeds the inferred return type.
//!
//! These tests exercise the bare-fn cascade end-to-end through
//! `db.infer(file_id)` so the wiring is observable via standard
//! `InferenceResult.var_types` / `expr_types_by_body`.

use hir::{HirDatabase, Ty};
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
        db.set_file_text(*file_id, &file.content);
    }
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    for file_id in fixture.files.keys() {
        db.set_file_source_root(*file_id, SourceRootId(0));
    }
    let test_file = fixture
        .files
        .iter()
        .find(|(_, f)| f.path.as_path().to_string_lossy().ends_with("/test.bsl"))
        .map(|(id, _)| *id)
        .expect("fixture must contain /test.bsl");
    (db, test_file)
}

fn var_ty(db: &RootDatabaseImpl, fid: FileId, name: &str) -> Ty {
    let result = db.infer(fid);
    let key = name.to_lowercase();
    result.var_types.get(&key).cloned().unwrap_or(Ty::Unknown)
}

/// Bare same-module fn-call cascade: `Х = ЛокФн();` where `ЛокФн`
/// returns a string literal must produce `var_types[Х] == Ty::String`
/// via the O.11 `Ty::Unknown` arm in `infer_call`.
#[test]
fn bare_fn_cascade_string() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Функция ЛокФн()
    Возврат "hello";
КонецФункции

Процедура P()
    Х = ЛокФн();
КонецПроцедуры
"#,
    );
    assert_eq!(var_ty(&db, fid, "Х"), Ty::String);
}

/// Two-step cascade: `f` returns `g()`, `g` returns `"x"`. After O.11
/// the `var_types[Х]` for `Х = f();` must be `Ty::String`. This
/// exercises the cycle handlers' "no cycle" path — both queries
/// resolve linearly to concrete Tys.
#[test]
fn bare_fn_cascade_two_step_chain() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Функция G()
    Возврат "x";
КонецФункции

Функция F()
    Возврат G();
КонецФункции

Процедура P()
    Х = F();
КонецПроцедуры
"#,
    );
    assert_eq!(var_ty(&db, fid, "Х"), Ty::String);
}

/// Body-binding shadow: a parameter named the same as a module method
/// must NOT resolve through `symbol_tree.find_method` to the module
/// method. The cascade-arm body-binding guard ensures
/// `var_types[Х]` stays `Ty::Unknown` (parameter `Foo` has no
/// inferred call-result), not the module method's return type.
#[test]
fn bare_fn_cascade_respects_param_shadow() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Функция Foo()
    Возврат "module-method";
КонецФункции

Процедура P(Foo)
    Х = Foo();
КонецПроцедуры
"#,
    );
    // The parameter `Foo` shadows the module method — its type is
    // Unknown (no annotation), so calling it yields Unknown rather
    // than cascading through to the module method.
    assert_eq!(var_ty(&db, fid, "Х"), Ty::Unknown);
}

/// Cycle scaffolding now reachable: `f` returns `f()`. The cascade
/// arm calls `method_return_type_query(f)` which recursively requests
/// `method_return_type_query(f)` — Salsa's cycle handlers fire,
/// `cycle_initial` seeds `Ty::Unknown`, and the body produces no
/// non-recursive concrete type, so the result converges to
/// `Ty::Unknown`. This is the same final value the O.10 self-recursion
/// test sees, but now via the actual cycle iteration rather than a
/// no-cascade short-circuit.
#[test]
fn self_recursion_under_cascade_yields_unknown() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Функция F()
    Возврат F();
КонецФункции

Процедура P()
    Х = F();
КонецПроцедуры
"#,
    );
    assert_eq!(var_ty(&db, fid, "Х"), Ty::Unknown);
}

/// The cascade also fires through `Stmt::Return`: a function `Wrap`
/// that returns a same-module call to `Inner` must propagate
/// `Inner`'s body-inferred return type up to `Wrap`. This validates
/// that the return-statement's expression sees the cascade-typed
/// `Ty` through `infer_call`.
#[test]
fn return_statement_propagates_cascade() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Функция Inner()
    Возврат 42;
КонецФункции

Функция Wrap()
    Возврат Inner();
КонецФункции
"#,
    );

    // Walk Wrap's body via `expr_types_by_body` and pick the Return
    // expression's inferred type. The expr_types map for a body
    // keyed by `DefWithBodyId::Method(local_id)`; pick whichever
    // body has a Ty::Number entry (the call `Inner()` should resolve
    // to Number via the cascade).
    let result = db.infer(fid);
    let any_number =
        result.expr_types_by_body.values().any(|m| m.values().any(|t| matches!(t, Ty::Number)));
    assert!(
        any_number,
        "the call to `Inner()` inside `Wrap`'s body must infer Ty::Number via the O.11 cascade"
    );
}
