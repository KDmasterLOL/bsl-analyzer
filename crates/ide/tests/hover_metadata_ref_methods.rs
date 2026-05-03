//! End-to-end hover regression for platform methods on `Ty::MetadataRef`
//! receivers with composite `platform_prefix`.
//!
//! Pins the original bug:
//!
//! ```bsl
//! Набор = РегистрыСведений.X.СоздатьНаборЗаписей();
//! Набор.Прочитать()  // ← hover used to silently return None
//! ```
//!
//! Reason: `Semantics::resolve_method_call_to_definition` routed through
//! `lookup_method_with_key`, which returned `None` for `Ty::MetadataRef`
//! kinds without a `scalar_platform_key`. After the unified
//! `resolve_method` use case (`hir_ty::platform_resolution`), the
//! composite-prefix path under `"InformationRegisterRecordSet.<MDO>"`
//! resolves through `prefixed_method_query` and surfaces real platform
//! docs for the method.
//!
//! These tests use the shared designer fixture
//! (`crates/bsl-metadata/fixtures/designer`) which declares a single
//! information register `РегистрСведений1`. Snapshot output is asserted
//! by minimal-prefix matching (presence of the method name in the markup)
//! to avoid breaking on HBK doc-text re-scrapes.

use bsl_platform::PlatformDataInner;
use ide::Analysis;
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::path::PathBuf;
use test_fixture::Fixture;
use vfs::FileId;

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

fn setup(fixture_text: &str) -> (Analysis, FileId, u32) {
    let (fixture_text, test_path, cursor_offset) = extract_cursor(fixture_text);
    let fixture = Fixture::parse(&fixture_text);

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

/// Returns `true` when platform data was loaded (HBK present at build
/// time). Tests that depend on a real method body must short-circuit
/// when the table is empty — but they MUST NOT short-circuit on
/// `hover.is_none()`, which would mask a regression where the method
/// silently fails to resolve. The skip decision is taken before the
/// hover call.
fn has_platform_data() -> bool {
    !PlatformDataInner::instance().all_methods().is_empty()
}

fn assert_hover_contains(fixture: &str, expected_substring: &str) {
    if !has_platform_data() {
        eprintln!("Skipping: no platform data available (HBK not present at build time)");
        return;
    }
    let (analysis, file_id, offset) = setup(fixture);
    let result =
        analysis.hover(file_id, offset).expect("hover must resolve when platform data is present");
    assert!(
        result.markup.contains(expected_substring),
        "hover markup did not contain `{}`; got:\n{}",
        expected_substring,
        result.markup,
    );
}

// ---------- InformationRegisterRecordSet ----------

#[test]
fn hover_record_set_read_resolves_via_composite_prefix() {
    // Original bug repro: `Набор = РегистрыСведений.<X>.СоздатьНаборЗаписей()`
    // followed by `Набор.Прочитать()`. Receiver type is
    // `Ty::MetadataRef { InformationRegisterRecordSet, "РегистрСведений1" }`,
    // which has composite `platform_prefix() = Some("InformationRegisterRecordSet")`.
    // Without the fix, hover returned None.
    assert_hover_contains(
        r#"//- /test.bsl
Процедура Тест()
    Набор = РегистрыСведений.РегистрСведений1.СоздатьНаборЗаписей();
    Набор.Проч$0итать();
КонецПроцедуры
"#,
        "Прочитать",
    );
}

#[test]
fn hover_record_set_write_resolves_via_composite_prefix() {
    // Same shape as Прочитать, different method.
    assert_hover_contains(
        r#"//- /test.bsl
Процедура Тест()
    Набор = РегистрыСведений.РегистрСведений1.СоздатьНаборЗаписей();
    Набор.Запис$0ать();
КонецПроцедуры
"#,
        "Записать",
    );
}

#[test]
fn hover_record_set_load_resolves_via_composite_prefix() {
    // `Загрузить` was the test case pinned in
    // `platform_manager_lookup::tests::metadata_ref_information_register_record_set_resolves_load`
    // for the resolution side; this is the IDE-side companion.
    assert_hover_contains(
        r#"//- /test.bsl
Процедура Тест()
    Набор = РегистрыСведений.РегистрСведений1.СоздатьНаборЗаписей();
    Набор.Загр$0узить(Неопределено);
КонецПроцедуры
"#,
        "Загрузить",
    );
}

// ---------- InformationRegisterRecordManager ----------

#[test]
fn hover_record_manager_read_resolves_via_composite_prefix() {
    // `СоздатьМенеджерЗаписи()` returns
    // `Ty::MetadataRef { InformationRegisterRecordManager, "РегистрСведений1" }`.
    // Same composite-prefix routing — pin so a regression in either the
    // manager-method `СоздатьМенеджерЗаписи` rebinding or the prefixed
    // dispatch surfaces here, not in hand-traced production behaviour.
    assert_hover_contains(
        r#"//- /test.bsl
Процедура Тест()
    Менеджер = РегистрыСведений.РегистрСведений1.СоздатьМенеджерЗаписи();
    Менеджер.Проч$0итать();
КонецПроцедуры
"#,
        "Прочитать",
    );
}

// ---------- Bilingual ----------

#[test]
fn hover_record_set_english_method_name_resolves() {
    // English method name (`Read`) must hit the same handle as the
    // Russian (`Прочитать`) — `find_prefixed_method` matches via
    // `english_name.rsplit_once('.')`.
    assert_hover_contains(
        r#"//- /test.bsl
Процедура Тест()
    Набор = РегистрыСведений.РегистрСведений1.СоздатьНаборЗаписей();
    Набор.Re$0ad();
КонецПроцедуры
"#,
        "Read",
    );
}
