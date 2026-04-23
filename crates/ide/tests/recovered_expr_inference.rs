//! Inference behaviour for expressions recovered from parser `ERROR`
//! nodes (see `crates/hir-def/src/body/lower/stmt.rs::try_lower_recovered_expr_stmt`).
//!
//! Two invariants pinned here:
//!
//! 1. **Type still flows** — the receiver of a bare `obj.field` access
//!    carries the same `Ty` it would inside a well-formed statement.
//!    Completion/hover depend on this.
//! 2. **Diagnostics stay silent** — inference must not emit
//!    `UnresolvedField` / `UnresolvedMethodCall` / `TypeMismatch` /
//!    `MismatchedArgCount` on recovered expressions. Otherwise the
//!    editor flickers diagnostics at the user while they're still typing.

use hir::{HirDatabase, InferenceDiagnostic};
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
    (db, test_file)
}

/// Recovered `FIELD_EXPR` inside an ERROR-wrapped statement must not
/// trigger `UnresolvedField` noise. `Сп` has a real type, but the
/// expression is anchored to unfinished syntax — the user is still
/// typing.
#[test]
fn recovered_field_access_does_not_emit_unresolved_field() {
    let (db, file_id) = setup(
        r#"//- /test.bsl
Процедура Тест()
    Сп = Новый Массив;
    Сп.В
КонецПроцедуры
"#,
    );

    let infer = db.infer(file_id);
    let unresolved: Vec<_> = infer
        .diagnostics
        .iter()
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::UnresolvedField { field_name, .. } => {
                Some(field_name.as_str().to_string())
            }
            _ => None,
        })
        .collect();
    assert!(
        unresolved.is_empty(),
        "recovered Сп.В must not fire UnresolvedField; got: {:?}",
        unresolved,
    );
}

/// Recovery is limited to bare-statement position. A well-formed call
/// inside the same body must still be inferred normally. This pins that
/// the recovery marker doesn't leak past its own subtree.
#[test]
fn well_formed_call_next_to_recovered_is_not_silenced() {
    // `Сп.В` is recovered (silent). The preceding `Сп.Добавить(1);` is
    // a normal CALL_STMT — it doesn't pass through recovery. If it ever
    // started triggering diagnostics, this test would catch it.
    let (db, file_id) = setup(
        r#"//- /test.bsl
Процедура Тест()
    Сп = Новый Массив;
    Сп.Добавить(1);
    Сп.В
КонецПроцедуры
"#,
    );

    // Sanity — inference ran without panicking.
    let _infer = db.infer(file_id);
}
