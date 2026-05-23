//! End-to-end regression for indexing a parameterised array.
//!
//! `Ty::TypedArray(elem)` carries the element schema, so `arr[i]` must
//! resolve to `*elem` rather than collapsing to `Ty::Unknown`. The
//! motivating chain from the form-items work is
//! `Элементы.Переприемка.ВыделенныеСтроки[0].ШтрихКод`: the
//! `.ВыделенныеСтроки` refinement returns `TypedArray(row)` (Phase 5),
//! and the indexing step is what unwraps the row's tabular-section
//! schema for downstream field access.
//!
//! These tests pin the indexing rule independently of the form-control
//! refinement by using JSDoc `Массив из <T>` to land a `TypedArray(T)`
//! binding directly — no Form.xml fixture required.

use hir::{HirDatabase, Name, Ty};
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

fn var_ty(db: &RootDatabaseImpl, file_id: FileId, var_lower: &str) -> Option<Ty> {
    let id = db.infer(file_id).var_types.get(var_lower).copied()?;
    Some(hir::ty_bridge::typeid_to_ty(db, id))
}

#[test]
fn indexing_typed_array_of_string_returns_string() {
    // JSDoc lowers `Массив из Строка` to `TypedArray(String)` (Phase 0).
    // `arr[0]` must unwrap to the element type — `Строка`.
    let (db, file_id) = setup(
        r#"
//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
// Возвращаемое значение:
//   Массив из Строка - элементы коллекции
Функция Получить() Экспорт
    Возврат Новый Массив;
КонецФункции

//- /test.bsl
Процедура Тест()
    Массив = ОбщегоНазначения.Получить();
    Элемент = Массив[0];
КонецПроцедуры
"#,
    );

    let ty = var_ty(&db, file_id, "элемент");
    assert_eq!(ty, Some(Ty::String), "TypedArray(String)[i] must yield String, got {:?}", ty);
}

#[test]
fn indexing_typed_array_of_metadata_ref_returns_ref() {
    // Indexing on a JSDoc-typed catalog-ref array surfaces the element
    // schema so downstream `.Code` / `.Description` access typechecks.
    // This is the invariant the form-items chain relies on for
    // `.ВыделенныеСтроки[i].ШтрихКод` (the row schema is reached via
    // `MetadataRef { TabularSectionRow, ... }` rather than this
    // primitive case, but the unwrap step is identical).
    let (db, file_id) = setup(
        r#"
//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
// Возвращаемое значение:
//   Массив из СправочникСсылка.Товары - подобранная номенклатура
Функция Получить() Экспорт
    Возврат Новый Массив;
КонецФункции

//- /test.bsl
Процедура Тест()
    Массив = ОбщегоНазначения.Получить();
    Элемент = Массив[0];
КонецПроцедуры
"#,
    );

    let ty = var_ty(&db, file_id, "элемент");
    assert_eq!(
        ty,
        Some(Ty::MetadataRef {
            kind: hir::MetadataKind::CatalogRef, name: Name::new("Товары")
        }),
        "TypedArray(CatalogRef.Товары)[i] must yield the ref Ty, got {:?}",
        ty
    );
}

#[test]
fn indexing_unparameterised_array_stays_unknown() {
    // The legacy `Ty::Array` (no element schema) carries no information
    // for the indexing step to unwrap, so `arr[i]` keeps returning
    // `Ty::Unknown`. This pins the boundary: only the parameterised
    // variant gets the new behaviour, so existing call sites that build
    // a bare `Новый Массив` without JSDoc continue to behave as before.
    let (db, file_id) = setup(
        r#"
//- /test.bsl
Процедура Тест()
    Массив = Новый Массив;
    Массив.Добавить(1);
    Элемент = Массив[0];
КонецПроцедуры
"#,
    );

    // `Новый Массив` infers to bare `Ty::Array`, so the index step has
    // no element schema and inference deliberately stays silent.
    // Asserting `None` (rather than `Some(Ty::Unknown)`) matches the
    // existing convention in `var_types`: unresolved bindings are
    // simply absent.
    assert!(
        matches!(var_ty(&db, file_id, "элемент"), None | Some(Ty::Unknown)),
        "bare Новый Массив indexing must not invent an element type"
    );
}
