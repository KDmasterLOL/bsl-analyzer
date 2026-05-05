//! End-to-end hover regression for form-element names
//! (`Элементы.<имя>`) per Codex review of commit 47f4e1a4.
//!
//! Form elements live in `Form.xml`, NOT `platform_data` and NOT BSL
//! definitions, so the existing `hover_field` chain (platform-property
//! → platform-method → resolve_name_to_definition) misses them. The
//! Phase-13.1 fallback asks `Semantics::type_of_expr` on the
//! surrounding FieldExpr; `infer.rs` routes through
//! `form_items::lookup_form_item_field` and produces a
//! `Ty::FormControl{kind, binding}`, which the hover layer renders as
//! `**<имя>**\n**Тип:** <kind display name>`.
//!
//! The test mounts the designer fixture's
//! `Catalogs/Справочник1/Forms/ФормаЭлемента/Ext/Form/Module.bsl` at
//! its actual on-disk path so `module_metadata_query` resolves the
//! sibling `Form.xml` (with `<InputField name="Код">`).

use bsl_platform::PlatformDataInner;
use ide::Analysis;
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::path::PathBuf;
use vfs::{FileId, FileSet, VfsPath};

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

fn module_disk_path() -> PathBuf {
    designer_fixture_path().join("Catalogs/Справочник1/Forms/ФормаЭлемента/Ext/Form/Module.bsl")
}

/// Mount a synthetic BSL file at the on-disk path that has `Form.xml`
/// as a sibling — `module_metadata_query` reads `Form.xml` from disk
/// (`load_form_from_path` → `parse_form_from_bsl_path`), so the VFS
/// path MUST be the real on-disk path or the form payload is None.
/// `set_file_text` overrides the file content so the test controls
/// what BSL the hover sees, while the on-disk `Form.xml` provides
/// the form-element table.
fn setup_form_module(bsl_text: &str, cursor_offset: u32) -> (Analysis, FileId, u32) {
    let disk_path = module_disk_path();
    assert!(disk_path.exists(), "designer fixture Module.bsl missing at {}", disk_path.display());

    let mut db = RootDatabaseImpl::new();
    let file_id = FileId(0);
    let mut file_set = FileSet::default();
    let vfs_path = VfsPath::new(disk_path.to_string_lossy().as_ref());
    file_set.insert(file_id, vfs_path);
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    db.set_file_source_root(file_id, SourceRootId(0));
    db.set_file_text(file_id, bsl_text);
    db.set_all_config_paths(vec![(None, designer_fixture_path())]);

    (Analysis::from_database(db), file_id, cursor_offset)
}

fn has_platform_data() -> bool {
    !PlatformDataInner::instance().all_methods().is_empty()
}

#[test]
fn hover_on_form_input_field_name_renders_kind_label() {
    // The bug Codex flagged in review of 47f4e1a4: hover on the
    // field-name token of `Элементы.<form-element>` was returning
    // "No information available" because the platform-prop /
    // platform-method / resolve_name_to_definition chain doesn't see
    // names declared in `Form.xml`. The fallback at the end of
    // `hover_field` should now surface `**Тип:** ПолеФормы` (the
    // base-kind display label for `<InputField>`).
    if !has_platform_data() {
        eprintln!("Skipping: no platform data available (HBK not present)");
        return;
    }

    // Cursor sits on `Код` so the hover token is `Код` (FieldName slot).
    // `Form.xml` declares `<InputField name="Код" id="1">` — Phase 9
    // `tag_to_kind` maps `<InputField>` to `FormElementKind::Field`,
    // and `form_control_platform_type_chain(Field)` yields `["ПолеФормы"]`.
    let bsl = "Процедура Тест()\n    Х = Элементы.Код;\nКонецПроцедуры\n";
    let cursor_offset = bsl.find("Код;").expect("cursor anchor present") as u32;

    let (analysis, file_id, offset) = setup_form_module(bsl, cursor_offset);
    let result = analysis
        .hover(file_id, offset, ide::Locale::Ru)
        .expect("hover must resolve via the Phase-13.1 type_of_expr fallback");

    assert!(
        result.markup.contains("**Код**"),
        "hover must surface the form-element name as a heading; got:\n{}",
        result.markup
    );
    assert!(
        result.markup.contains("ПолеФормы"),
        "hover must render the kind display label `ПолеФормы`; got:\n{}",
        result.markup
    );
}
