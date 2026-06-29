use hir::{HirDatabase, InferenceDiagnostic};
use ide::{Analysis, CompletionItem, CompletionItemKind};
use ide_db::base_db::{SourceDatabase, SourceRoot, BSL_SOURCE_ROOT, METADATA_SOURCE_ROOT};
use ide_db::RootDatabaseImpl;
use test_fixture::Fixture;
use vfs::{FileId, FileSet, VfsPath};

const ROLE_XML_FILE_ID: FileId = FileId(1000);

fn designer_fixture_path() -> std::path::PathBuf {
    std::path::PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bsl-metadata/fixtures/designer"
    ))
}

fn role_xml_text() -> &'static str {
    include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../bsl-metadata/fixtures/designer/Roles/ПолныеПрава.xml"
    ))
}

fn setup(fixture_text: &str) -> (Analysis, FileId, u32) {
    setup_with_role_xml(fixture_text, role_xml_text())
}

fn setup_with_role_xml(fixture_text: &str, role_xml: &str) -> (Analysis, FileId, u32) {
    let (fixture_text, test_path, cursor_offset) = extract_cursor(fixture_text);
    let fixture = Fixture::parse(&fixture_text);
    let mut db = RootDatabaseImpl::new();
    let mut file_set = FileSet::default();
    for (file_id, file) in &fixture.files {
        file_set.insert(*file_id, file.path.clone());
    }

    let mut metadata_file_set = FileSet::default();
    metadata_file_set.insert(
        ROLE_XML_FILE_ID,
        VfsPath::from(designer_fixture_path().join("Roles/ПолныеПрава.xml")),
    );

    db.set_source_root(BSL_SOURCE_ROOT, SourceRoot::new_local(file_set));
    db.set_source_root(METADATA_SOURCE_ROOT, SourceRoot::new_local(metadata_file_set));
    for (file_id, file) in &fixture.files {
        db.set_file_source_root(*file_id, BSL_SOURCE_ROOT);
        db.set_file_text(*file_id, &file.content);
    }
    db.set_file_source_root(ROLE_XML_FILE_ID, METADATA_SOURCE_ROOT);
    db.set_file_text(ROLE_XML_FILE_ID, role_xml);
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
    let cursor_in_file =
        u32::try_from(abs_idx - file_offset_in_prefix).expect("fixture cursor offset must fit u32");
    let cleaned = fixture_text.replacen("$0", "", 1);
    (cleaned, path_line.to_string(), cursor_in_file)
}

fn complete(fixture: &str) -> Vec<CompletionItem> {
    let (analysis, file_id, offset) = setup(fixture);
    analysis.completions(file_id, offset, None, ide::Locale::Ru)
}

fn labels(items: &[CompletionItem]) -> Vec<&str> {
    items.iter().map(|item| item.label.as_str()).collect()
}

fn item<'a>(items: &'a [CompletionItem], label: &str) -> Option<&'a CompletionItem> {
    items.iter().find(|item| item.label == label)
}

#[test]
fn cold_plurals_complete_under_metadata() {
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    Метаданные.$0
КонецПроцедуры
"#,
    );

    for label in [
        "Роли",
        "ПодпискиНаСобытия",
        "РегламентныеЗадания",
        "HTTPСервисы",
        "WebСервисы",
        "Подсистемы",
    ] {
        let found = item(&items, label).unwrap_or_else(|| {
            panic!("{label} must complete under Метаданные.; got {:?}", labels(&items))
        });
        assert_eq!(found.kind, CompletionItemKind::MdoType, "{label} must be a metadata kind");
    }
}

#[test]
fn cold_collection_completes_existing_objects() {
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    Метаданные.Роли.$0
КонецПроцедуры
"#,
    );

    let role = item(&items, "ПолныеПрава").unwrap_or_else(|| {
        panic!("ПолныеПрава must complete under Роли; got {:?}", labels(&items))
    });
    assert_eq!(role.kind, CompletionItemKind::MdoObject);
}

#[test]
fn manager_metadata_collection_still_completes_existing_objects() {
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    Метаданные.Справочники.$0
КонецПроцедуры
"#,
    );

    let catalog = item(&items, "Справочник1").unwrap_or_else(|| {
        panic!("manager metadata path must still complete catalogs; got {:?}", labels(&items))
    });
    assert_eq!(catalog.kind, CompletionItemKind::MdoObject);
}

#[test]
fn metadata_object_member_is_not_typed_as_a_manager() {
    // `Метаданные.Справочники.Справочник1` is a metadata-description object (ОбъектМетаданных),
    // not a `СправочникМенеджер`, so it must NOT surface manager methods like СоздатьЭлемент.
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    Метаданные.Справочники.Справочник1.$0
КонецПроцедуры
"#,
    );

    assert!(
        item(&items, "СоздатьЭлемент").is_none(),
        "metadata-description object must not expose manager methods; got {:?}",
        labels(&items)
    );
}

#[test]
fn cold_metadata_ref_hover_and_goto_target_xml_in_metadata_source_root() {
    let (analysis, file_id, offset) = setup(
        r#"//- /test.bsl
Процедура Тест()
    Значение = Метаданные.Роли.Полные$0Права;
КонецПроцедуры
"#,
    );

    let hover = analysis.hover(file_id, offset, ide::Locale::Ru).expect("role hover must resolve");
    assert!(hover.markup.contains("Роль"), "hover must name the cold kind: {}", hover.markup);
    assert!(
        hover.markup.contains("ПолныеПрава"),
        "hover must name the concrete role: {}",
        hover.markup
    );
    assert!(
        !hover.markup.contains("Менеджер"),
        "cold metadata ref must not hover as a manager: {}",
        hover.markup
    );

    let nav = analysis.goto_definition(file_id, offset).expect("role goto must resolve");
    assert_eq!(nav.file_id, ROLE_XML_FILE_ID, "role goto must target XML metadata file");
    assert!(!nav.range.is_empty(), "role goto range must point at real XML name text");
    let xml_text = analysis.database().file_text(nav.file_id);
    let start = u32::from(nav.range.start()) as usize;
    let end = u32::from(nav.range.end()) as usize;
    assert_eq!(&xml_text[start..end], "ПолныеПрава");
}

#[test]
fn cold_metadata_goto_without_xml_name_range_returns_none() {
    let (analysis, file_id, offset) = setup_with_role_xml(
        r#"//- /test.bsl
Процедура Тест()
    Значение = Метаданные.Роли.Полные$0Права;
КонецПроцедуры
"#,
        r#"<MetaDataObject><Role><Properties><Name>ДругоеИмя</Name></Properties></Role></MetaDataObject>"#,
    );

    assert!(
        analysis.goto_definition(file_id, offset).is_none(),
        "role goto must not synthesize an empty range when XML lacks the role name"
    );
}

#[test]
fn missing_cold_member_stays_unknown_without_unresolved_field_diagnostic() {
    let (analysis, file_id, _offset) = setup(
        r#"//- /test.bsl
Процедура Тест()
    Значение = Метаданные.Роли.НетТакойРоли$0;
КонецПроцедуры
"#,
    );

    let diagnostics = analysis.database().arg_diagnostics(file_id);
    let unresolved_fields: Vec<_> = diagnostics
        .iter()
        .filter_map(|(_, diagnostic)| match diagnostic {
            InferenceDiagnostic::UnresolvedField { field_name, .. } => Some(field_name.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        unresolved_fields.is_empty(),
        "missing cold metadata members must degrade to Unknown without UnresolvedField; got {unresolved_fields:?}"
    );
}

#[test]
fn local_metadata_name_shadows_cold_plural_completion_without_blocking_receiver_fields() {
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    Метаданные = Новый Структура("Поле");
    Метаданные.$0
КонецПроцедуры
"#,
    );

    assert!(
        item(&items, "Поле").is_some(),
        "shadowed Метаданные must fall through to regular receiver completions; got {:?}",
        labels(&items)
    );
    assert!(
        item(&items, "Роли").is_none(),
        "local Метаданные must shadow global metadata completions; got {:?}",
        labels(&items)
    );
}
