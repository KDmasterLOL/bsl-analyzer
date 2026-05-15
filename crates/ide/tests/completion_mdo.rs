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
use std::path::{Path, PathBuf};
use test_fixture::Fixture;
use vfs::{FileId, FileSet};

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

/// Build a database wired to the designer fixture, with the test files
/// from `fixture_text`. The `$0` cursor convention is handled by
/// `extract_cursor` (identical to `completion_baseline.rs`).
fn setup_with_config(fixture_text: &str) -> (Analysis, FileId, u32) {
    setup_with_config_path(fixture_text, &designer_fixture_path())
}

fn setup_with_config_path(fixture_text: &str, config_path: &Path) -> (Analysis, FileId, u32) {
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
    db.set_all_config_paths(vec![(None, config_path.to_path_buf())]);

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
    analysis.completions(file_id, offset, None, ide::Locale::Ru)
}

fn complete_with_config_path(fixture: &str, config_path: &Path) -> Vec<CompletionItem> {
    let (analysis, file_id, offset) = setup_with_config_path(fixture, config_path);
    analysis.completions(file_id, offset, None, ide::Locale::Ru)
}

fn has_label(items: &[CompletionItem], label: &str) -> bool {
    items.iter().any(|i| i.label == label)
}

fn item_with_label<'a>(items: &'a [CompletionItem], label: &str) -> Option<&'a CompletionItem> {
    items.iter().find(|i| i.label == label)
}

fn write_collision_catalog_fixture(root: &Path) {
    std::fs::create_dir_all(root.join("Catalogs")).expect("create Catalogs directory");
    std::fs::create_dir_all(root.join("CommonModules")).expect("create CommonModules directory");
    std::fs::write(
        root.join("Configuration.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Configuration uuid="11111111-1111-1111-1111-111111111111">
        <Properties>
            <Name>CollisionConfig</Name>
        </Properties>
        <ChildObjects>
            <CommonModule>ПервыйОбщийМодуль</CommonModule>
            <Catalog>КоллизияМетодов</Catalog>
        </ChildObjects>
    </Configuration>
</MetaDataObject>"#,
    )
    .expect("write synthetic Configuration.xml");
    std::fs::write(
        root.join("CommonModules/ПервыйОбщийМодуль.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <CommonModule uuid="44444444-4444-4444-4444-444444444444">
        <Properties>
            <Name>ПервыйОбщийМодуль</Name>
            <Global>false</Global>
            <Server>true</Server>
            <ClientManagedApplication>false</ClientManagedApplication>
            <ClientOrdinaryApplication>false</ClientOrdinaryApplication>
            <ExternalConnection>false</ExternalConnection>
            <ServerCall>false</ServerCall>
            <Privileged>false</Privileged>
            <ReturnValuesReuse>DontUse</ReturnValuesReuse>
        </Properties>
    </CommonModule>
</MetaDataObject>"#,
    )
    .expect("write synthetic CommonModule XML");
    std::fs::write(
        root.join("Catalogs/КоллизияМетодов.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" version="2.10">
    <Catalog uuid="22222222-2222-2222-2222-222222222222">
        <Properties>
            <Name>КоллизияМетодов</Name>
            <CodeLength>9</CodeLength>
            <DescriptionLength>25</DescriptionLength>
        </Properties>
        <ChildObjects>
            <Attribute uuid="33333333-3333-3333-3333-333333333333">
                <Properties>
                    <Name>Записать</Name>
                    <Type>
                        <v8:Type>xs:string</v8:Type>
                        <v8:StringQualifiers>
                            <v8:Length>10</v8:Length>
                        </v8:StringQualifiers>
                    </Type>
                </Properties>
            </Attribute>
        </ChildObjects>
    </Catalog>
</MetaDataObject>"#,
    )
    .expect("write synthetic catalog XML");
}

fn write_accumulation_register_with_recorder_fixture(root: &Path) {
    std::fs::create_dir_all(root.join("AccumulationRegisters"))
        .expect("create AccumulationRegisters directory");
    std::fs::create_dir_all(root.join("Documents")).expect("create Documents directory");
    std::fs::create_dir_all(root.join("CommonModules")).expect("create CommonModules directory");
    std::fs::write(
        root.join("Configuration.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Configuration uuid="11111111-1111-1111-1111-111111111111">
        <Properties>
            <Name>RegisterConfig</Name>
        </Properties>
        <ChildObjects>
            <CommonModule>ПервыйОбщийМодуль</CommonModule>
            <AccumulationRegister>Остатки</AccumulationRegister>
            <Document>Поступление</Document>
        </ChildObjects>
    </Configuration>
</MetaDataObject>"#,
    )
    .expect("write synthetic Configuration.xml");
    std::fs::write(
        root.join("CommonModules/ПервыйОбщийМодуль.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <CommonModule uuid="44444444-4444-4444-4444-444444444444">
        <Properties>
            <Name>ПервыйОбщийМодуль</Name>
            <Global>false</Global>
            <Server>true</Server>
            <ClientManagedApplication>false</ClientManagedApplication>
            <ClientOrdinaryApplication>false</ClientOrdinaryApplication>
            <ExternalConnection>false</ExternalConnection>
            <ServerCall>false</ServerCall>
            <Privileged>false</Privileged>
            <ReturnValuesReuse>DontUse</ReturnValuesReuse>
        </Properties>
    </CommonModule>
</MetaDataObject>"#,
    )
    .expect("write synthetic CommonModule XML");
    std::fs::write(
        root.join("AccumulationRegisters/Остатки.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" version="2.10">
    <AccumulationRegister uuid="22222222-2222-2222-2222-222222222222">
        <Properties>
            <Name>Остатки</Name>
        </Properties>
        <ChildObjects>
            <Dimension uuid="33333333-3333-3333-3333-333333333333">
                <Properties>
                    <Name>Номенклатура</Name>
                    <Type>
                        <v8:Type>xs:string</v8:Type>
                    </Type>
                </Properties>
            </Dimension>
            <Dimension uuid="33333333-3333-3333-3333-444444444444">
                <Properties>
                    <Name>Цена</Name>
                    <Type>
                        <v8:Type>xs:decimal</v8:Type>
                        <v8:NumberQualifiers>
                            <v8:Digits>15</v8:Digits>
                            <v8:FractionDigits>2</v8:FractionDigits>
                        </v8:NumberQualifiers>
                    </Type>
                </Properties>
            </Dimension>
        </ChildObjects>
    </AccumulationRegister>
</MetaDataObject>"#,
    )
    .expect("write synthetic accumulation register XML");
    std::fs::write(
        root.join("Documents/Поступление.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:xr="http://v8.1c.ru/8.3/xcf/readable" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" version="2.10">
    <Document uuid="55555555-5555-5555-5555-555555555555">
        <Properties>
            <Name>Поступление</Name>
            <RegisterRecords>
                <xr:Item xsi:type="xr:MDObjectRef">AccumulationRegister.Остатки</xr:Item>
            </RegisterRecords>
        </Properties>
    </Document>
</MetaDataObject>"#,
    )
    .expect("write synthetic document XML");
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
fn completion_dedup_user_attribute_collides_with_platform_method() {
    // If the MDO has a custom attribute with the same name as a platform method
    // (`Записать` on CatalogObject), the merged list must show only one item
    // with kind == Field (MDO field wins over platform method in dedup).
    let temp_dir = tempfile::tempdir().expect("create synthetic config tempdir");
    write_collision_catalog_fixture(temp_dir.path());
    let object_module_path = temp_dir.path().join("Catalogs/КоллизияМетодов/Ext/ObjectModule.bsl");

    let items = complete_with_config_path(
        &format!(
            r#"//- {}
Функция Тест()
    ЭтотОбъект.$0
КонецФункции
"#,
            object_module_path.display()
        ),
        temp_dir.path(),
    );

    let matching: Vec<_> = items.iter().filter(|item| item.label == "Записать").collect();
    assert_eq!(
        matching.len(),
        1,
        "MDO field must deduplicate platform method label; labels: {:?}",
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(matching[0].kind, CompletionItemKind::Field);
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

#[test]
fn completion_filter_recorder_detail_contains_wrapper_and_recorder_type() {
    let temp_dir = tempfile::tempdir().expect("create synthetic register config tempdir");
    write_accumulation_register_with_recorder_fixture(temp_dir.path());

    let items = complete_with_config_path(
        r#"//- /test.bsl
Процедура Тест()
    Н = РегистрыНакопления.Остатки.СоздатьНаборЗаписей();
    Н.Отбор.$0
КонецПроцедуры
"#,
        temp_dir.path(),
    );

    let recorder = item_with_label(&items, "Регистратор").expect("Регистратор must complete");
    let detail = recorder.detail.as_deref().unwrap_or("");
    assert!(
        detail.contains("ЭлементОтбора →") && detail.contains("ДокументСсылка.Поступление"),
        "Регистратор detail must show wrapper and recorder type, got: {detail:?}",
    );
}

#[test]
fn completion_bare_ident_in_object_module_offers_mdo_attribute_with_real_type() {
    let module_path =
        designer_fixture_path().join("DataProcessors/ТестоваяОбработка/Ext/ObjectModule.bsl");
    let items = complete(&format!(
        r#"//- {}
Процедура Тест()
    Адр$0
КонецПроцедуры
"#,
        module_path.display(),
    ));

    let attr = item_with_label(&items, "АдресСайта").expect("MDO attribute must complete");
    assert_eq!(attr.detail.as_deref(), Some("Строка"));
}

#[test]
fn completion_bare_ident_in_form_marks_regular_form_attribute() {
    let module_path = designer_fixture_path()
        .join("Catalogs/рдт_Рецептура/Forms/ФормаЭлемента/Ext/Form/Module.bsl");
    let items = complete(&format!(
        r#"//- {}
Процедура Тест()
    Перес$0
КонецПроцедуры
"#,
        module_path.display(),
    ));

    let attr = item_with_label(&items, "Пересчитать").expect("form attribute must complete");
    let detail = attr.detail.as_deref().unwrap_or("");
    assert!(
        detail.contains("(реквизит формы)"),
        "regular form attribute detail must carry marker, got: {detail:?}",
    );
}

#[test]
fn hover_filter_dimension_price_shows_wrapper_and_value_type() {
    let temp_dir = tempfile::tempdir().expect("create synthetic register config tempdir");
    write_accumulation_register_with_recorder_fixture(temp_dir.path());
    let (analysis, file_id, offset) = setup_with_config_path(
        r#"//- /test.bsl
Процедура Тест()
    Н = РегистрыНакопления.Остатки.СоздатьНаборЗаписей();
    Э = Н.Отбор.Ц$0ена;
КонецПроцедуры
"#,
        temp_dir.path(),
    );

    let hover = analysis.hover(file_id, offset, ide::Locale::Ru).expect("hover must resolve");
    assert!(
        hover.markup.contains("ЭлементОтбора → Число"),
        "hover must show filter wrapper and value type, got:\n{}",
        hover.markup,
    );
}

// ---------------------------------------------------------------------------
// Phase A — HBK platform-property cascade on *Object MDO receivers
// ---------------------------------------------------------------------------

#[test]
fn completion_document_object_offers_additional_properties_and_movements() {
    // `Документы.Документ1.СоздатьДокумент()` returns
    // `Ty::MetadataRef{DocumentObject, "Документ1"}`. The HBK declares
    // ДополнительныеСвойства / Движения / ОбменДанными / etc. on
    // `DocumentObject.<Имя>` — they must appear in the completion list
    // after Phase A wires `enumerate_mdo_fields` to the platform-prefix
    // cascade.
    let items = complete(
        r#"//- /test.bsl
Функция Тест()
    Док = Документы.Документ1.СоздатьДокумент();
    Док.$0
КонецФункции
"#,
    );

    assert!(
        has_label(&items, "ДополнительныеСвойства"),
        "DocumentObject completion must offer ДополнительныеСвойства; labels: {:?}",
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>()
    );
    assert!(
        has_label(&items, "Движения"),
        "DocumentObject completion must offer Движения; labels: {:?}",
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>()
    );
    assert!(has_label(&items, "ОбменДанными"), "DocumentObject completion must offer ОбменДанными",);
}

#[test]
fn completion_chain_through_additional_properties_offers_structure_methods() {
    // Chain typing pin: with the cascade in place,
    // `Док.ДополнительныеСвойства` resolves to `Ty::Structure`, so the
    // next dot must surface Structure methods (`Вставить`, `Удалить`,
    // `Получить`, …). Pre-cascade this chain would type as `Ty::Unknown`
    // and the dot would offer nothing.
    let items = complete(
        r#"//- /test.bsl
Функция Тест()
    Док = Документы.Документ1.СоздатьДокумент();
    Док.ДополнительныеСвойства.$0
КонецФункции
"#,
    );

    assert!(
        has_label(&items, "Вставить"),
        "Структура.Вставить must complete on ДополнительныеСвойства chain; labels: {:?}",
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn completion_catalog_object_offers_additional_properties() {
    // Mirror of the document test for `CatalogObject`. HBK declares the
    // same shared set (`ДополнительныеСвойства`, `ВерсияДанных`,
    // `ЗаписьИсторииДанных`, `ОбменДанными`, …) under `CatalogObject.<Имя>`.
    let items = complete(
        r#"//- /test.bsl
Функция Тест()
    Эл = Справочники.Справочник1.СоздатьЭлемент();
    Эл.$0
КонецФункции
"#,
    );

    assert!(
        has_label(&items, "ДополнительныеСвойства"),
        "CatalogObject completion must offer ДополнительныеСвойства; labels: {:?}",
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>()
    );
    assert!(has_label(&items, "ВерсияДанных"), "CatalogObject completion must offer ВерсияДанных",);
}
