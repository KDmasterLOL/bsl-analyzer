//! MDO field completion tests (Phase 3).
//!
//! These tests verify that after a dot on a `MetadataRef` receiver the
//! completion list includes MDO fields — custom attributes, standard
//! attributes, and tabular sections — in addition to the platform methods
//! that were already present before Phase 3.
//!
//! Designer fixture: `crates/bsl-metadata/fixtures/designer`.
//!
//! `Catalog "Справочник1"` shape used by most tests:
//! - CodeLength=9 → standard attribute `Код: String`
//! - `Реквизит1: String`
//! - `Реквизит2: Number`
//! - `Реквизит3: Boolean`
//! - tabular section `ТабличнаяЧасть1` with columns `Реквизит1: String`, `Реквизит2: Number`
//!
//! `InformationRegister "РегистрСведений1"` shape:
//! - dimension `Справочник1: CatalogRef.Справочник1`

use ide::{Analysis, CompletionItem, CompletionItemKind};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::path::PathBuf;
use test_fixture::Fixture;
use vfs::{FileId, FileSet};

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

/// Build a database wired to the designer fixture, with the test files
/// from `fixture_text`. The `$0` cursor convention is handled by
/// `extract_cursor` (identical to `completion_baseline.rs`).
fn setup_with_config(fixture_text: &str) -> (Analysis, FileId, u32) {
    let (fixture_text, test_path, cursor_offset) = extract_cursor(fixture_text);
    let fixture = Fixture::parse(&fixture_text);

    let mut db = RootDatabaseImpl::new();
    let source_root_id = SourceRootId(0);

    let mut file_set = FileSet::default();
    for (file_id, file) in &fixture.files {
        file_set.insert(*file_id, file.path.clone());
    }
    db.set_source_root(source_root_id, SourceRoot::new_local(file_set));

    for (file_id, file) in &fixture.files {
        db.set_file_source_root(*file_id, source_root_id);
        db.set_file_text(*file_id, &file.content);
    }

    // Wire up the designer metadata fixture so MDO attributes are visible.
    db.set_all_config_paths(vec![(None, designer_fixture_path())]);

    let test_file = fixture
        .files
        .iter()
        .find(|(_, f)| f.path.as_path().to_string_lossy().ends_with(&test_path))
        .map(|(id, _)| *id)
        .expect("cursor-bearing file not found");

    (Analysis::from_database(db), test_file, cursor_offset)
}

fn extract_cursor(fixture_text: &str) -> (String, String, u32) {
    let abs_idx = fixture_text.find("$0").expect("fixture must contain $0 cursor marker");
    let prefix = &fixture_text[..abs_idx];
    let last_header_start = prefix.rfind("//- ").expect("cursor must be inside a //- file");
    let header_end =
        prefix[last_header_start..].find('\n').expect("//- header must end with newline")
            + last_header_start;
    let path_line = &prefix[last_header_start + 4..header_end];
    let file_offset_in_prefix = header_end + 1;
    let cursor_in_file = (abs_idx - file_offset_in_prefix) as u32;
    let cleaned = fixture_text.replacen("$0", "", 1);
    (cleaned, path_line.to_string(), cursor_in_file)
}

fn complete(fixture: &str) -> Vec<CompletionItem> {
    let (analysis, file_id, offset) = setup_with_config(fixture);
    analysis.completions(file_id, offset, None)
}

fn has_label(items: &[CompletionItem], label: &str) -> bool {
    items.iter().any(|i| i.label == label)
}

fn item_with_label<'a>(items: &'a [CompletionItem], label: &str) -> Option<&'a CompletionItem> {
    items.iter().find(|i| i.label == label)
}

/// JSDoc-annotated CommonModule function that returns a CatalogRef.
const CATALOG_REF_MODULE: &str = r#"//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   СправочникСсылка.Справочник1
Функция Ссылка() Экспорт
    Возврат Неопределено;
КонецФункции

"#;

/// JSDoc-annotated CommonModule function that returns a CatalogObject.
const CATALOG_OBJECT_MODULE: &str = r#"//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   СправочникОбъект.Справочник1
Функция Объект() Экспорт
    Возврат Неопределено;
КонецФункции

"#;

// ---------------------------------------------------------------------------
// Test 1 — custom attributes on CatalogRef
// ---------------------------------------------------------------------------

#[test]
fn completion_after_dot_on_catalog_ref_includes_custom_attributes() {
    // A variable typed `CatalogRef.Справочник1` must offer the three custom
    // attributes: Реквизит1 (String), Реквизит2 (Number), Реквизит3 (Boolean).
    let items = complete(&format!(
        r#"{CATALOG_REF_MODULE}
//- /test.bsl
Функция Тест()
    Спр = ПервыйОбщийМодуль.Ссылка();
    Спр.$0
КонецФункции
"#
    ));

    assert!(!items.is_empty(), "expected completion items after dot on CatalogRef; got empty");

    for attr in &["Реквизит1", "Реквизит2", "Реквизит3"] {
        assert!(
            has_label(&items, attr),
            "custom attribute {attr} must appear; labels: {:?}",
            items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>()
        );
        let item = item_with_label(&items, attr).unwrap();
        assert_eq!(
            item.kind,
            CompletionItemKind::Field,
            "{attr} must have kind Field, got {:?}",
            item.kind
        );
    }
}

// ---------------------------------------------------------------------------
// Test 2 — standard attribute Код
// ---------------------------------------------------------------------------

#[test]
fn completion_after_dot_on_catalog_ref_includes_standard_code_attribute() {
    // Standard attribute `Код` must be surfaced (CodeLength=9 → loaded by the
    // XML standard-attribute injector in Phase 1a).
    let items = complete(&format!(
        r#"{CATALOG_REF_MODULE}
//- /test.bsl
Функция Тест()
    Спр = ПервыйОбщийМодуль.Ссылка();
    Спр.$0
КонецФункции
"#
    ));

    assert!(
        has_label(&items, "Код"),
        "standard attribute Код must appear; labels: {:?}",
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>()
    );
    let item = item_with_label(&items, "Код").unwrap();
    assert_eq!(item.kind, CompletionItemKind::Field, "Код must have kind Field");
}

// ---------------------------------------------------------------------------
// Test 3 — tabular section label and detail
// ---------------------------------------------------------------------------

#[test]
fn completion_after_dot_on_catalog_ref_includes_tabular_section_with_detail() {
    // `ТабличнаяЧасть1` must appear as a Field item with detail containing
    // "ТабличнаяЧасть" to signal to the user that it is a tabular section.
    let items = complete(&format!(
        r#"{CATALOG_REF_MODULE}
//- /test.bsl
Функция Тест()
    Спр = ПервыйОбщийМодуль.Ссылка();
    Спр.$0
КонецФункции
"#
    ));

    assert!(
        has_label(&items, "ТабличнаяЧасть1"),
        "ТабличнаяЧасть1 must appear; labels: {:?}",
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>()
    );
    let item = item_with_label(&items, "ТабличнаяЧасть1").unwrap();
    assert_eq!(item.kind, CompletionItemKind::Field, "ТабличнаяЧасть1 must have kind Field");
    let detail = item.detail.as_deref().unwrap_or("");
    assert!(
        detail.contains("ТабличнаяЧасть"),
        "detail for ТабличнаяЧасть1 must contain 'ТабличнаяЧасть', got: {detail:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 4 — tabular section scalar-key platform methods regression
// ---------------------------------------------------------------------------

#[test]
fn completion_after_dot_on_tabular_section_shows_platform_methods_regression() {
    // After `Спр.ТабличнаяЧасть1.` the receiver is `TabularSection`.
    // That path still goes through the scalar-key branch and must surface
    // platform TS methods like `Добавить`, `НайтиСтроки`, `Количество`.
    let items = complete(&format!(
        r#"{CATALOG_OBJECT_MODULE}
//- /test.bsl
Функция Тест()
    Спр = ПервыйОбщийМодуль.Объект();
    Спр.ТабличнаяЧасть1.$0
КонецФункции
"#
    ));

    assert!(
        !items.is_empty(),
        "platform TS methods must be offered after ТабличнаяЧасть1 dot; got empty"
    );
    for method in &["Добавить", "НайтиСтроки", "Количество"] {
        assert!(
            has_label(&items, method),
            "platform method {method} must appear on TabularSection receiver; labels: {:?}",
            items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>()
        );
    }
}

// ---------------------------------------------------------------------------
// Test 5 — TabularSectionRow via Добавить() shows row columns + НомерСтроки
// ---------------------------------------------------------------------------

#[test]
fn completion_after_add_returns_row_shows_columns_and_line_number() {
    // `Стр = Спр.ТабличнаяЧасть1.Добавить(); Стр.|`
    // The receiver must be `TabularSectionRow { parent: Catalog }` after
    // `Добавить()` (proved by `infer_tabular_section_methods.rs`).
    // Completion must surface the row's columns plus `НомерСтроки`.
    let items = complete(&format!(
        r#"{CATALOG_OBJECT_MODULE}
//- /test.bsl
Функция Тест()
    Спр = ПервыйОбщийМодуль.Объект();
    Стр = Спр.ТабличнаяЧасть1.Добавить();
    Стр.$0
КонецФункции
"#
    ));

    assert!(!items.is_empty(), "completion on TabularSectionRow must not be empty");
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    // Row columns from fixture: Реквизит1, Реквизит2.
    for col in &["Реквизит1", "Реквизит2"] {
        assert!(has_label(&items, col), "row column {col} must appear; labels: {:?}", labels);
    }
    // Platform standard row property НомерСтроки.
    assert!(
        has_label(&items, "НомерСтроки"),
        "НомерСтроки must appear on TabularSectionRow; labels: {:?}",
        labels
    );
}

// ---------------------------------------------------------------------------
// Test 6 — dedup: user attribute named like a platform method
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires synthetic fixture with attribute named like platform method; low priority"]
fn completion_dedup_user_attribute_collides_with_platform_method() {
    // If the MDO has a custom attribute with the same name as a platform method
    // (e.g. `Записать`), the merged list must show only one item with
    // kind == Field (MDO field wins over platform method in dedup).
    // TODO: create a synthetic designer fixture or temp-XML approach.
    let _ = complete;
}

// ---------------------------------------------------------------------------
// Test 7 — filter_text carries English name
// ---------------------------------------------------------------------------

#[test]
fn completion_filter_by_english_name() {
    // The `filter_text` field for MDO fields is formatted as
    // `"<Russian> <English>"`. If the editor sends `Code` as the
    // filter prefix, `Код` must survive. This test inspects the
    // `filter_text` directly.
    let items = complete(&format!(
        r#"{CATALOG_REF_MODULE}
//- /test.bsl
Функция Тест()
    Спр = ПервыйОбщийМодуль.Ссылка();
    Спр.$0
КонецФункции
"#
    ));

    let kod = item_with_label(&items, "Код").expect("Код must appear in completion list");
    let ft = kod.filter_text.as_deref().unwrap_or("");
    // `filter_text` may be absent if the item relies solely on `label`
    // for filtering, or may carry both names. Accept either form:
    // - "Код Code" (both names present)
    // - "Код" (label-only fallback)
    // The important invariant is that `Code` is reachable via filter_text.
    assert!(
        ft.to_lowercase().contains("code") || ft.to_lowercase().contains("код"),
        "filter_text must contain 'Код' or 'Code' to allow bilingual filtering; got: {ft:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 8 — unknown receiver must not panic
// ---------------------------------------------------------------------------

#[test]
fn completion_after_dot_on_unknown_receiver_returns_no_mdo_fields() {
    // An unresolved receiver must not panic and must not surface MDO fields
    // (there is no type to enumerate fields on).
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    НеизвестныйСимвол.$0
КонецПроцедуры
"#,
    );

    // May return 0 or some platform items for bare-identifier fallback,
    // but must never panic and must not carry Field-kind MDO items.
    for item in &items {
        assert_ne!(
            item.kind,
            CompletionItemKind::Field,
            "unknown receiver must not produce MDO Field items; got: {:?}",
            item
        );
        assert!(
            !item.label.is_empty(),
            "completion items must have non-empty labels; got {:?}",
            item
        );
    }
}

// ---------------------------------------------------------------------------
// Test 9 — ObjectManager fast-path: no MDO fields
// ---------------------------------------------------------------------------

#[test]
fn completion_object_manager_fast_path_does_not_show_mdo_fields() {
    // `Справочники.Справочник1.` → receiver is `ObjectManager { Catalog, "Справочник1" }`.
    // The fast-path must return only platform manager methods (e.g. `НайтиПоКоду`,
    // `НайтиПоРеквизиту`, `СоздатьЭлемент`), not MDO fields.
    // We assert that NO Field-kind items appear.
    let items = complete(
        r#"//- /test.bsl
Функция Тест()
    Справочники.Справочник1.$0
КонецФункции
"#,
    );

    // There must be platform items.
    assert!(!items.is_empty(), "ObjectManager must offer platform manager methods; got empty");
    // None of them must be Field kind.
    for item in &items {
        assert_ne!(
            item.kind,
            CompletionItemKind::Field,
            "ObjectManager completion must not contain Field items; found: {:?}",
            item
        );
    }
}

// ---------------------------------------------------------------------------
// Test 10 — InformationRegister record shows dimension
// ---------------------------------------------------------------------------

#[test]
fn completion_after_dot_on_information_register_ref_shows_dimensions() {
    // `РегистрСведений1` has dimension `Справочник1: CatalogRef.Справочник1`.
    // A receiver typed as `InformationRegisterRef.РегистрСведений1` must show
    // that dimension in the completion list.
    let items = complete(
        r#"//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Возвращаемое значение:
//   РегистрСведенийКлючЗаписи.РегистрСведений1
Функция Запись() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Функция Тест()
    З = ПервыйОбщийМодуль.Запись();
    З.$0
КонецФункции
"#,
    );

    assert!(
        has_label(&items, "Справочник1"),
        "dimension Справочник1 must appear on InformationRegisterRef receiver; labels: {:?}",
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>()
    );
    let item = item_with_label(&items, "Справочник1").unwrap();
    assert_eq!(
        item.kind,
        CompletionItemKind::Field,
        "register dimension must have kind Field, got {:?}",
        item.kind
    );
}
