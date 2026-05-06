//! End-to-end hover regression for `Объект.<attr>` inside a managed form
//! (DataProcessor / Report). Mirrors `completion_form_object.rs` —
//! exercises the same parser → loader → schema → form_attr →
//! lookup_field path that completion uses, but renders the resolved Ty
//! through the hover markup.

use bsl_platform::PlatformDataInner;
use ide::Analysis;
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::path::PathBuf;
use vfs::{FileId, FileSet, VfsPath};

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

fn setup_form_module(disk_path: PathBuf, bsl: &str, cursor: u32) -> (Analysis, FileId, u32) {
    assert!(disk_path.exists(), "fixture missing: {}", disk_path.display());
    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);
    let mut file_set = FileSet::default();
    file_set.insert(file_id, VfsPath::new(disk_path.to_string_lossy().as_ref()));
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    db.set_file_source_root(file_id, SourceRootId(0));
    db.set_file_text(file_id, bsl);
    db.set_all_config_paths(vec![(None, designer_fixture_path())]);
    (Analysis::from_database(db), file_id, cursor)
}

fn has_platform_data() -> bool {
    !PlatformDataInner::instance().all_methods().is_empty()
}

#[test]
fn hover_on_data_processor_object_attribute_renders_type() {
    if !has_platform_data() {
        eprintln!("Skipping: no platform data available");
        return;
    }

    let bsl = "Процедура Тест()\n    Х = Объект.СоздаватьГруппы;\nКонецПроцедуры\n";
    let cursor = bsl.find("СоздаватьГруппы").expect("anchor") as u32;

    let (analysis, file_id, offset) = setup_form_module(
        designer_fixture_path()
            .join("DataProcessors/ТестоваяОбработка/Forms/Форма/Ext/Form/Module.bsl"),
        bsl,
        cursor,
    );
    let result = analysis.hover(file_id, offset, ide::Locale::Ru).expect("hover must resolve");

    assert!(
        result.markup.contains("Булево"),
        "hover must render `Булево` for `Объект.СоздаватьГруппы`; got:\n{}",
        result.markup
    );
}

#[test]
fn hover_on_report_object_attribute_renders_type() {
    if !has_platform_data() {
        eprintln!("Skipping: no platform data available");
        return;
    }

    let bsl = "Процедура Тест()\n    Х = Объект.ПериодОтчёта;\nКонецПроцедуры\n";
    let cursor = bsl.find("ПериодОтчёта").expect("anchor") as u32;

    let (analysis, file_id, offset) = setup_form_module(
        designer_fixture_path().join("Reports/ТестовыйОтчёт/Forms/Форма/Ext/Form/Module.bsl"),
        bsl,
        cursor,
    );
    let result = analysis.hover(file_id, offset, ide::Locale::Ru).expect("hover must resolve");

    // `ПериодОтчёта` is typed as `xs:dateTime` with `<DateFractions>Date</...>`,
    // which `parse_type_xml` lowers to `AttributeType::Date`. Hover should
    // surface either `Дата` or its English name.
    assert!(
        result.markup.contains("Дата") || result.markup.contains("Date"),
        "hover must render `Дата` for `Объект.ПериодОтчёта`; got:\n{}",
        result.markup
    );
}
