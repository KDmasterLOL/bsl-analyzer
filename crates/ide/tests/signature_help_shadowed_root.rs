use ide::Analysis;
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::path::{Path, PathBuf};
use test_fixture::Fixture;
use vfs::{FileId, FileSet};

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

fn setup(fixture_text: &str) -> (Analysis, FileId, u32) {
    setup_with_config_path(fixture_text, &designer_fixture_path())
}

fn setup_with_config_path(fixture_text: &str, config_path: &Path) -> (Analysis, FileId, u32) {
    let (fixture_text, test_path, cursor_offset) = extract_cursor(fixture_text);
    let fixture = Fixture::parse(&fixture_text);
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

/// Positive control: the manager chain resolves when nobody holds the root.
#[test]
fn unheld_manager_chain_resolves_signature() {
    let (analysis, file_id, offset) = setup(
        r#"//- /test.bsl
Функция Тест()
    Справочники.Справочник1.НайтиПоКоду($0);
КонецФункции
"#,
    );
    let help = analysis.signature_help(file_id, offset);
    assert!(help.is_some(), "unheld manager chain must produce signature help");
}

/// A configuration whose own symbols are named after the `Справочники`
/// manager collection. The designer fixture cannot express this — no attribute,
/// form attribute or common module there carries a collection name — so the
/// collision is built from scratch. `Справочник1` is declared as well: without a
/// resolvable catalog the chain would fall silent for lack of a target rather
/// than for shadowing, and the positive control could not tell the two apart.
fn write_member_collision_config(root: &Path) {
    std::fs::create_dir_all(root.join("Catalogs")).expect("create Catalogs directory");
    std::fs::create_dir_all(root.join("CommonModules")).expect("create CommonModules directory");
    std::fs::create_dir_all(root.join("Catalogs/Справочник1/Forms/ФормаЭлемента/Ext"))
        .expect("create form directory");
    std::fs::write(
        root.join("Configuration.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Configuration uuid="11111111-1111-1111-1111-111111111111">
        <Properties>
            <Name>ManagerRootCollisionConfig</Name>
        </Properties>
        <ChildObjects>
            <CommonModule>Тестовый</CommonModule>
            <Catalog>Справочник1</Catalog>
        </ChildObjects>
    </Configuration>
</MetaDataObject>"#,
    )
    .expect("write synthetic Configuration.xml");
    write_common_module(root, "Тестовый");
    std::fs::write(
        root.join("Catalogs/Справочник1.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" version="2.10">
    <Catalog uuid="22222222-2222-2222-2222-222222222222">
        <Properties>
            <Name>Справочник1</Name>
            <CodeLength>9</CodeLength>
            <DescriptionLength>25</DescriptionLength>
        </Properties>
        <ChildObjects>
            <Attribute uuid="33333333-3333-3333-3333-333333333333">
                <Properties>
                    <Name>Справочники</Name>
                    <Type>
                        <v8:Type>xs:string</v8:Type>
                        <v8:StringQualifiers>
                            <v8:Length>20</v8:Length>
                        </v8:StringQualifiers>
                    </Type>
                </Properties>
            </Attribute>
            <Form>ФормаЭлемента</Form>
        </ChildObjects>
    </Catalog>
</MetaDataObject>"#,
    )
    .expect("write synthetic catalog XML");
    std::fs::write(
        root.join("Catalogs/Справочник1/Forms/ФормаЭлемента/Ext/Form.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Form xmlns="http://v8.1c.ru/8.3/xcf/logform" version="2.10">
    <Properties>
        <Name>ФормаЭлемента</Name>
    </Properties>
    <Attributes>
        <Attribute name="Справочники" id="1">
            <Type/>
        </Attribute>
    </Attributes>
</Form>"#,
    )
    .expect("write synthetic Form.xml");
}

/// Same collision carried by a workspace common module instead of a member.
/// It shadows workspace-wide, so it cannot share a configuration with the
/// positive control.
fn write_common_module_collision_config(root: &Path) {
    std::fs::create_dir_all(root.join("Catalogs")).expect("create Catalogs directory");
    std::fs::create_dir_all(root.join("CommonModules")).expect("create CommonModules directory");
    std::fs::write(
        root.join("Configuration.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Configuration uuid="11111111-1111-1111-1111-111111111111">
        <Properties>
            <Name>ManagerRootModuleCollisionConfig</Name>
        </Properties>
        <ChildObjects>
            <CommonModule>Справочники</CommonModule>
            <CommonModule>Тестовый</CommonModule>
            <Catalog>Справочник1</Catalog>
        </ChildObjects>
    </Configuration>
</MetaDataObject>"#,
    )
    .expect("write synthetic Configuration.xml");
    write_common_module(root, "Справочники");
    write_common_module(root, "Тестовый");
    std::fs::write(
        root.join("Catalogs/Справочник1.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" xmlns:v8="http://v8.1c.ru/8.1/data/core" version="2.10">
    <Catalog uuid="22222222-2222-2222-2222-222222222222">
        <Properties>
            <Name>Справочник1</Name>
            <CodeLength>9</CodeLength>
            <DescriptionLength>25</DescriptionLength>
        </Properties>
    </Catalog>
</MetaDataObject>"#,
    )
    .expect("write synthetic catalog XML");
}

/// The configuration loader discovers a common module from its on-disk
/// presence, so the module directory has to exist even when the body itself is
/// supplied virtually through the fixture.
fn write_common_module(root: &Path, name: &str) {
    std::fs::create_dir_all(root.join(format!("CommonModules/{name}/Ext")))
        .expect("create common module directory");
    std::fs::write(root.join(format!("CommonModules/{name}/Ext/Module.bsl")), "")
        .expect("write common module body placeholder");
    std::fs::write(root.join(format!("CommonModules/{name}.xml")), common_module_xml(name))
        .expect("write common module XML");
}

fn common_module_xml(name: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <CommonModule uuid="44444444-4444-4444-4444-4444444444{:02x}">
        <Properties>
            <Name>{name}</Name>
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
        name.len()
    )
}

/// Positive control for the synthetic configuration: without it a `None` from
/// the tests below would equally mean "shadowing works" and "this hand-written
/// configuration resolves nothing at all".
#[test]
fn unheld_manager_chain_resolves_in_synthetic_config() {
    let temp_dir = tempfile::tempdir().expect("create synthetic collision config tempdir");
    write_member_collision_config(temp_dir.path());
    let module_path = temp_dir.path().join("CommonModules/Тестовый/Ext/Module.bsl");
    let (analysis, file_id, offset) = setup_with_config_path(
        &format!(
            r#"//- {}
Функция Тест()
    Справочники.Справочник1.НайтиПоКоду($0);
КонецФункции
"#,
            module_path.display()
        ),
        temp_dir.path(),
    );
    let help = analysis.signature_help(file_id, offset);
    assert!(
        help.is_some(),
        "synthetic config must resolve the unheld manager chain, otherwise the shadowing \
         assertions below prove nothing"
    );
}

#[test]
fn object_attribute_holding_manager_root_does_not_resolve_manager_signature() {
    let temp_dir = tempfile::tempdir().expect("create synthetic collision config tempdir");
    write_member_collision_config(temp_dir.path());
    let module_path = temp_dir.path().join("Catalogs/Справочник1/Ext/ObjectModule.bsl");
    let (analysis, file_id, offset) = setup_with_config_path(
        &format!(
            r#"//- {}
Функция Тест()
    Справочники.Справочник1.НайтиПоКоду($0);
КонецФункции
"#,
            module_path.display()
        ),
        temp_dir.path(),
    );
    let help = analysis.signature_help(file_id, offset);
    assert!(
        help.is_none(),
        "an implicit ЭтотОбъект attribute holds the root — the manager method's signature must \
         not be offered; got: {:?}",
        help.map(|h| h.signatures)
    );
}

#[test]
fn form_attribute_holding_manager_root_does_not_resolve_manager_signature() {
    let temp_dir = tempfile::tempdir().expect("create synthetic collision config tempdir");
    write_member_collision_config(temp_dir.path());
    let module_path =
        temp_dir.path().join("Catalogs/Справочник1/Forms/ФормаЭлемента/Ext/Form/Module.bsl");
    let (analysis, file_id, offset) = setup_with_config_path(
        &format!(
            r#"//- {}
&НаСервере
Процедура Тест()
    Справочники.Справочник1.НайтиПоКоду($0);
КонецПроцедуры
"#,
            module_path.display()
        ),
        temp_dir.path(),
    );
    let help = analysis.signature_help(file_id, offset);
    assert!(
        help.is_none(),
        "a form attribute holds the root — the manager method's signature must not be offered; \
         got: {:?}",
        help.map(|h| h.signatures)
    );
}

#[test]
fn common_module_holding_manager_root_does_not_resolve_manager_signature() {
    let temp_dir = tempfile::tempdir().expect("create synthetic collision config tempdir");
    write_common_module_collision_config(temp_dir.path());
    let holder_path = temp_dir.path().join("CommonModules/Справочники/Ext/Module.bsl");
    let module_path = temp_dir.path().join("CommonModules/Тестовый/Ext/Module.bsl");
    let (analysis, file_id, offset) = setup_with_config_path(
        &format!(
            r#"//- {}
Функция МойМетод() Экспорт
    Возврат 1;
КонецФункции

//- {}
Функция Тест()
    Справочники.Справочник1.НайтиПоКоду($0);
КонецФункции
"#,
            holder_path.display(),
            module_path.display()
        ),
        temp_dir.path(),
    );
    let help = analysis.signature_help(file_id, offset);
    assert!(
        help.is_none(),
        "a workspace common module holds the root — the manager method's signature must not be \
         offered; got: {:?}",
        help.map(|h| h.signatures)
    );
}

#[test]
fn module_variable_holding_manager_root_does_not_resolve_manager_signature() {
    let (analysis, file_id, offset) = setup(
        r#"//- /test.bsl
Перем Справочники;

Функция Тест()
    Справочники.Справочник1.НайтиПоКоду($0);
КонецФункции
"#,
    );
    let help = analysis.signature_help(file_id, offset);
    assert!(
        help.is_none(),
        "a module variable holds the root — the manager method's signature must not be offered; \
         got: {:?}",
        help.map(|h| h.signatures)
    );
}

#[test]
fn module_method_holding_manager_root_does_not_resolve_manager_signature() {
    let (analysis, file_id, offset) = setup(
        r#"//- /test.bsl
Функция Справочники()
    Возврат Неопределено;
КонецФункции

Функция Тест()
    Справочники.Справочник1.НайтиПоКоду($0);
КонецФункции
"#,
    );
    let help = analysis.signature_help(file_id, offset);
    assert!(
        help.is_none(),
        "a module method holds the root — the manager method's signature must not be offered; \
         got: {:?}",
        help.map(|h| h.signatures)
    );
}

#[test]
fn local_holding_manager_root_does_not_resolve_manager_signature() {
    let (analysis, file_id, offset) = setup(
        r#"//- /test.bsl
Функция Тест()
    Справочники = НеизвестнаяФункция();
    Справочники.Справочник1.НайтиПоКоду($0);
КонецФункции
"#,
    );
    let help = analysis.signature_help(file_id, offset);
    assert!(
        help.is_none(),
        "a local holds the root — the manager method's signature must not be offered; got: {:?}",
        help.map(|h| h.signatures.len())
    );
}
