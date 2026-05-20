//! Behavioural tests for `method_return_type_query` (Phase O.10).
//!
//! O.10 ships the per-method cascade-typing primitive with cycle
//! safety (`cycle_fn` / `cycle_initial`, lru=16384). NO production
//! callers exist yet — O.11 wires `dispatch_bare_ident_field_call`
//! gate-3 + `materialise_signature_enriched` to consume the query.
//!
//! Phase O drops the J.5b.1 residency gate (PLAN-v5 §3): total-VFS
//! invariant (commit `6c578f3a`) guarantees populated text for all
//! BSL fids registered in a `FileSet`, so cold-file Test 5 from the
//! J reference is not portable and is omitted here.
//!
//! Cycle handlers are present as scaffolding. They are NOT exercised
//! by the self-recursion / mutual-recursion tests below — the
//! cascade wiring inside `dispatch_bare_ident_field_call` that would
//! cause `Возврат M()` to recursively invoke
//! `method_return_type_query` lands in O.11. In the O.10 baseline
//! these tests still pass (Unknown) because infer does not yet
//! consult the per-method return-type query.

use hir::{method_return_type_query, DefDatabase, MethodId, MethodIdInput, ModuleId, Name, Ty};
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

fn find_method(db: &RootDatabaseImpl, file_id: FileId, name: &str) -> MethodId {
    let symbol_tree = db.symbol_tree(ModuleId::new(file_id));
    symbol_tree
        .find_method(&Name::new(name))
        .unwrap_or_else(|| panic!("expected method `{name}` in {file_id:?}"))
        .id
}

fn return_ty_for(db: &RootDatabaseImpl, file_id: FileId, name: &str) -> Ty {
    let mid = find_method(db, file_id, name);
    let input = MethodIdInput::new(db, mid);
    method_return_type_query(db, input)
}

// ---------------------------------------------------------------------
// Test 1 — single literal return → concrete `Ty::String`.
// ---------------------------------------------------------------------
#[test]
fn single_return_yields_inferred_ty() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Функция F()
    Возврат "hello";
КонецФункции
"#,
    );
    assert_eq!(return_ty_for(&db, fid, "F"), Ty::String);
}

// ---------------------------------------------------------------------
// Test 2 — procedure with no `Возврат` → `Ty::Unknown`.
// ---------------------------------------------------------------------
#[test]
fn no_return_yields_unknown() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Процедура P()
    Х = 1;
КонецПроцедуры
"#,
    );
    assert_eq!(return_ty_for(&db, fid, "P"), Ty::Unknown);
}

// ---------------------------------------------------------------------
// Test 3 — two identical return Tys collapse to a single Ty via the
// `Ty::union` smart constructor (singleton path).
// ---------------------------------------------------------------------
#[test]
fn multiple_same_return_tys_unify() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Функция F(X)
    Если X Тогда
        Возврат "a";
    Иначе
        Возврат "b";
    КонецЕсли;
КонецФункции
"#,
    );
    assert_eq!(return_ty_for(&db, fid, "F"), Ty::String);
}

// ---------------------------------------------------------------------
// Test 4 — mixed return Tys yield `Ty::Union` with exact cardinality.
// ---------------------------------------------------------------------
#[test]
fn mixed_return_tys_yield_union() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Функция F(X)
    Если X Тогда
        Возврат "a";
    Иначе
        Возврат 1;
    КонецЕсли;
КонецФункции
"#,
    );
    match return_ty_for(&db, fid, "F") {
        Ty::Union(variants) => {
            assert_eq!(variants.len(), 2, "Union must have exactly String and Number");
            assert!(variants.iter().any(|t| matches!(t, Ty::String)));
            assert!(variants.iter().any(|t| matches!(t, Ty::Number)));
        }
        other => panic!("expected Ty::Union, got {other:?}"),
    }
}

// ---------------------------------------------------------------------
// Test 4b — bare `Возврат;` (no value) contributes no expression →
// stays `Ty::Unknown`.
// ---------------------------------------------------------------------
#[test]
fn bare_return_yields_unknown() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Процедура P()
    Возврат;
КонецПроцедуры
"#,
    );
    assert_eq!(return_ty_for(&db, fid, "P"), Ty::Unknown);
}

// ---------------------------------------------------------------------
// Test 4c — nested `Возврат` inside `Для Каждого ... Цикл`.
// ---------------------------------------------------------------------
#[test]
fn nested_return_in_for_loop() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Функция F(Коллекция)
    Для Каждого Элемент Из Коллекция Цикл
        Возврат "hit";
    КонецЦикла;
    Возврат "miss";
КонецФункции
"#,
    );
    assert_eq!(return_ty_for(&db, fid, "F"), Ty::String);
}

// ---------------------------------------------------------------------
// Test 5 — self-recursion: `Функция M() Возврат M() КонецФункции`.
// Without the O.11 cascade wiring, infer does not invoke
// `method_return_type_query` for the recursive call; the body
// produces no concrete return-type and the query returns
// `Ty::Unknown`. Once O.11 lands the cycle handler takes over and
// converges to the same value.
// ---------------------------------------------------------------------
#[test]
fn self_recursion_yields_unknown() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Функция M()
    Возврат M();
КонецФункции
"#,
    );
    assert_eq!(return_ty_for(&db, fid, "M"), Ty::Unknown);
}

// ---------------------------------------------------------------------
// Test 6 — salsa cache: a second call returns the same `Ty` (Salsa
// caches the tracked-fn result; structural equality is observable
// since `Ty: Clone + Eq`).
// ---------------------------------------------------------------------
#[test]
fn return_type_caches_via_salsa() {
    let (db, fid) = setup(
        r#"
//- /test.bsl
Функция F()
    Возврат "cached";
КонецФункции
"#,
    );
    let mid = find_method(&db, fid, "F");
    let input = MethodIdInput::new(&db, mid);

    let first = method_return_type_query(&db, input);
    let second = method_return_type_query(&db, input);
    assert_eq!(first, second);
    assert_eq!(first, Ty::String);
}
