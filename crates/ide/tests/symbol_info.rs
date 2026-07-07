use ide::{symbol_info, SymbolInfoRequest, SymbolInfoSections, SymbolPosition};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::metadata::{MdoEntry, MetadataListingData};
use ide_db::RootDatabaseImpl;
use std::path::PathBuf;
use test_fixture::Fixture;
use vfs::{FileId, FileSet, VfsPath};

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

const CATALOG_XML: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../bsl-metadata/fixtures/designer/Catalogs/Справочник1.xml"
));

const REGISTER_XML: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../bsl-metadata/fixtures/designer/InformationRegisters/РегистрСведений1.xml"
));

fn empty_listing_data() -> MetadataListingData {
    MetadataListingData {
        entries: Vec::new(),
        defined_types: Vec::new(),
        common_modules: Vec::new(),
        event_subscriptions: Vec::new(),
        scheduled_jobs: Vec::new(),
        roles: Vec::new(),
        http_services: Vec::new(),
        web_services: Vec::new(),
        integration_services: Vec::new(),
        subsystems: Vec::new(),
    }
}

/// A db carrying only BSL modules, with the module index built from file paths (no metadata
/// substrate). Enough for common-module and positional resolution.
fn setup_bsl(fixture_text: &str) -> (RootDatabaseImpl, FileId) {
    let fixture = Fixture::parse(fixture_text);
    let mut db = RootDatabaseImpl::new();
    let mut file_set = FileSet::default();
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
        .unwrap_or_else(|| *fixture.files.keys().next().expect("fixture has a file"));
    (db, test_file)
}

/// A db with the `Справочник.Справочник1` catalog wired into the metadata substrate the way
/// the resident host does (config path + a listing entry mapping the object to its XML).
fn setup_catalog() -> RootDatabaseImpl {
    let mut db = RootDatabaseImpl::new();
    let bsl = FileId(0);
    let xml = FileId(1);
    let designer = designer_fixture_path();
    let mut file_set = FileSet::new();
    file_set.insert(bsl, VfsPath::new("/test.bsl"));
    file_set.insert(xml, VfsPath::from(designer.join("Catalogs/Справочник1.xml")));
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    db.set_file_source_root(bsl, SourceRootId(0));
    db.set_file_source_root(xml, SourceRootId(0));
    db.set_file_text(bsl, "Процедура Тест()\nКонецПроцедуры\n");
    db.set_file_text(xml, CATALOG_XML);
    db.set_all_config_paths(vec![(None, designer.clone())]);
    db.set_metadata_listing(
        &designer.to_string_lossy(),
        MetadataListingData {
            entries: vec![MdoEntry {
                kind: bsl_metadata::MdoType::Catalog,
                name: "Справочник1".to_string(),
                main: xml,
                predefined: None,
            }],
            ..empty_listing_data()
        },
    );
    db
}

/// A db with `РегистрСведений.РегистрСведений1` wired into the metadata substrate, mirroring
/// how the resident host lists a register object.
fn setup_register() -> RootDatabaseImpl {
    let mut db = RootDatabaseImpl::new();
    let bsl = FileId(0);
    let xml = FileId(1);
    let designer = designer_fixture_path();
    let mut file_set = FileSet::new();
    file_set.insert(bsl, VfsPath::new("/test.bsl"));
    file_set.insert(xml, VfsPath::from(designer.join("InformationRegisters/РегистрСведений1.xml")));
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    db.set_file_source_root(bsl, SourceRootId(0));
    db.set_file_source_root(xml, SourceRootId(0));
    db.set_file_text(bsl, "Процедура Тест()\nКонецПроцедуры\n");
    db.set_file_text(xml, REGISTER_XML);
    db.set_all_config_paths(vec![(None, designer.clone())]);
    db.set_metadata_listing(
        &designer.to_string_lossy(),
        MetadataListingData {
            entries: vec![MdoEntry {
                kind: bsl_metadata::MdoType::InformationRegister,
                name: "РегистрСведений1".to_string(),
                main: xml,
                predefined: None,
            }],
            ..empty_listing_data()
        },
    );
    db
}

fn by_name(db: &RootDatabaseImpl, symbol: &str) -> Option<ide::SymbolInfoCard> {
    symbol_info(
        db,
        &SymbolInfoRequest {
            symbol: Some(symbol.to_string()),
            position: None,
            locale: ide::Locale::default(),
            sections: SymbolInfoSections::all(),
        },
    )
}

const COMMON_MODULE: &str = r#"
//- /CommonModules/МойМодуль/Ext/Module.bsl
// Складывает два числа.
Функция Сложить(А, Б) Экспорт
    Возврат А + Б;
КонецФункции

Процедура ВнутренняяПроцедура()
КонецПроцедуры

Функция СУмолчаниями(Знач Ссылка, Флаг = Ложь, Значение = Неопределено) Экспорт
    Возврат Ссылка;
КонецФункции

Функция СоСтрокой(Текст = "Иван  Петров") Экспорт
    Возврат Текст;
КонецФункции

//- /test.bsl
Процедура Тест(Парам)
    Б = Парам;
КонецПроцедуры
"#;

const OBJECT_MODULE: &str = r#"
//- /Documents/Документ1/Ext/ObjectModule.bsl
Процедура ПубличныйМетод() Экспорт
КонецПроцедуры

Процедура ПриватныйМетод()
КонецПроцедуры

//- /test.bsl
Процедура Тест()
КонецПроцедуры
"#;

#[test]
fn common_module_method_by_qualified_name() {
    let (db, _) = setup_bsl(COMMON_MODULE);
    let card = by_name(&db, "МойМодуль.Сложить").expect("resident resolves the method");
    assert_eq!(card.kind, "function");
    assert_eq!(card.symbol, "МойМодуль.Сложить");
    let container = card.container.expect("method carries a container");
    assert_eq!(container.kind, "ОбщийМодуль");
    assert_eq!(container.name, "МойМодуль");
    let signature = card.signature.expect("signature rendered");
    // A definition-site declaration: keyword + bare name, no call qualifier, export marker kept.
    assert!(
        signature.starts_with("Функция Сложить("),
        "declaration starts with keyword+bare name: {signature:?}"
    );
    assert!(
        !signature.contains("МойМодуль"),
        "qualifier must not leak into the declaration: {signature:?}"
    );
    assert!(signature.contains("Экспорт"), "export marker present: {signature:?}");
    let def = card.definition.expect("definition site");
    assert!(def.line >= 1);
    assert!(def.snippet.is_some());
}

#[test]
fn declaration_renders_by_value_marker_and_default_values() {
    let (db, _) = setup_bsl(COMMON_MODULE);
    let card = by_name(&db, "МойМодуль.СУмолчаниями").expect("resident resolves the method");
    let signature = card.signature.expect("signature rendered");
    // The declaration keeps `Знач` and the default-value expressions, and drops the qualifier.
    assert_eq!(
        signature,
        "Функция СУмолчаниями(Знач Ссылка, Флаг = Ложь, Значение = Неопределено) Экспорт",
        "faithful declaration with Знач + defaults: {signature:?}"
    );
}

#[test]
fn string_literal_default_keeps_internal_spaces() {
    let (db, _) = setup_bsl(COMMON_MODULE);
    let card = by_name(&db, "МойМодуль.СоСтрокой").expect("resident resolves the method");
    let signature = card.signature.expect("signature rendered");
    // A single-line string-literal default must survive verbatim — its internal double space is
    // not collapsed (only multi-line defaults are whitespace-normalised).
    assert_eq!(
        signature, "Функция СоСтрокой(Текст = \"Иван  Петров\") Экспорт",
        "string default preserved: {signature:?}"
    );
}

#[test]
fn non_exported_common_module_method_does_not_resolve() {
    // Only exported common-module methods are addressable by qualified name.
    let (db, _) = setup_bsl(COMMON_MODULE);
    assert!(by_name(&db, "МойМодуль.ВнутренняяПроцедура").is_none());
}

#[test]
fn unknown_symbol_is_a_miss_not_a_panic() {
    let (db, _) = setup_bsl(COMMON_MODULE);
    // A resident miss returns None; the adapter offers graph candidates.
    assert!(by_name(&db, "НетТакогоМодуля.НетТакогоМетода").is_none());
}

#[test]
fn metadata_object_card_lists_members() {
    let db = setup_catalog();
    let card = by_name(&db, "Справочник.Справочник1").expect("object resolves");
    assert_eq!(card.kind, "metadata object");
    let container = card.container.expect("object container");
    assert_eq!(container.kind, "Справочник");
    assert!(
        card.members.iter().any(|m| m.name == "Реквизит1" && m.kind == "Реквизит"),
        "members were {:?}",
        card.members
    );
}

#[test]
fn metadata_attribute_card_has_type_and_ownership() {
    let db = setup_catalog();
    let card = by_name(&db, "Справочник.Справочник1.Реквизит1").expect("attribute resolves");
    assert_eq!(card.kind, "attribute");
    assert!(card.return_type.is_some(), "attribute carries its type");
    let container = card.container.expect("attribute ownership");
    assert_eq!(container.kind, "Справочник");
    assert_eq!(container.name, "Справочник1");
}

#[test]
fn plural_mdo_keyword_resolves_the_same_object() {
    let db = setup_catalog();
    // The MdoType keyword accepts the plural form too.
    let card = by_name(&db, "Справочники.Справочник1").expect("plural keyword resolves");
    assert_eq!(card.kind, "metadata object");
}

#[test]
fn positional_local_or_parameter_resolves() {
    let (db, file_id) = setup_bsl(COMMON_MODULE);
    // `Б = Парам;` on line 1 (0-based) of /test.bsl; `Парам` starts at column 8.
    let card = symbol_info(
        &db,
        &SymbolInfoRequest {
            symbol: None,
            position: Some(SymbolPosition { file_id, line: 1, column: 8 }),
            locale: ide::Locale::default(),
            sections: SymbolInfoSections::all(),
        },
    )
    .expect("positional resolution");
    assert_eq!(card.symbol, "Парам");
    assert!(matches!(card.kind, "parameter" | "local variable"), "unexpected kind {:?}", card.kind);
}

#[test]
fn positional_out_of_range_column_is_a_miss() {
    let (db, file_id) = setup_bsl(COMMON_MODULE);
    // Column 999 is past the end of `    Б = Парам;`; the resolver must not bleed into the next
    // line's token and answer for a symbol the caller never pointed at.
    let card = symbol_info(
        &db,
        &SymbolInfoRequest {
            symbol: None,
            position: Some(SymbolPosition { file_id, line: 1, column: 999 }),
            locale: ide::Locale::default(),
            sections: SymbolInfoSections::all(),
        },
    );
    assert!(card.is_none(), "out-of-range column must be a miss, got {card:?}");
}

#[test]
fn exported_object_module_method_resolves_and_private_does_not() {
    let (db, _) = setup_bsl(OBJECT_MODULE);
    // A qualified `<Object>.<Method>` names an externally addressable method: only the exported
    // one resolves; the private object-module method is not a valid qualified symbol.
    assert!(
        by_name(&db, "Документ.Документ1.ПубличныйМетод").is_some(),
        "exported object-module method resolves"
    );
    assert!(
        by_name(&db, "Документ.Документ1.ПриватныйМетод").is_none(),
        "non-exported object-module method is not addressable by qualified name"
    );
}

#[test]
fn register_card_lists_dimensions() {
    let db = setup_register();
    let card = by_name(&db, "РегистрСведений.РегистрСведений1").expect("register resolves");
    assert_eq!(card.kind, "metadata object");
    let container = card.container.expect("register container");
    assert_eq!(container.kind, "РегистрСведений");
    assert!(
        card.members.iter().any(|m| m.name == "Справочник1" && m.kind == "Измерение"),
        "dimensions were {:?}",
        card.members
    );
}

#[test]
fn register_dimension_card_has_type_and_ownership() {
    let db = setup_register();
    let card = by_name(&db, "РегистрСведений.РегистрСведений1.Справочник1")
        .expect("register dimension resolves");
    assert_eq!(card.kind, "attribute");
    let container = card.container.expect("dimension ownership");
    assert!(
        container.kind.starts_with("РегистрСведений"),
        "container kind was {:?}",
        container.kind
    );
    assert_eq!(container.name, "РегистрСведений1");
}
