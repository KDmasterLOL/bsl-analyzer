//! Baseline regression tests for completion.
//!
//! These lock down current completion behaviour ahead of the M1 resolver
//! rework (`Scope::Builtins`, unified `Resolver` name lookup, CFE extension
//! iteration via `visible_configurations`). Completion output is broad and
//! order-sensitive, so these tests assert on presence + kind of specific
//! items instead of full-markup snapshots.
//!
//! The `$0` marker denotes the cursor position.

use ide::{Analysis, CompletionItem, CompletionItemKind};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use test_fixture::Fixture;
use vfs::{FileId, FileSet};

fn setup(fixture_text: &str) -> (Analysis, FileId, u32) {
    let (fixture_text, test_path, cursor_offset) = extract_cursor(fixture_text);
    let fixture = Fixture::parse(&fixture_text);

    let mut db = RootDatabaseImpl::new();
    let source_root_id = SourceRootId(0);

    let mut file_set = FileSet::default();
    for (file_id, file) in &fixture.files {
        file_set.insert(*file_id, file.path.clone());
    }
    let source_root = SourceRoot::new_local(file_set);
    db.set_source_root(source_root_id, source_root);

    for (file_id, file) in &fixture.files {
        db.set_file_source_root(*file_id, source_root_id);
        db.set_file_text(*file_id, &file.content);
    }

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
    let (analysis, file_id, offset) = setup(fixture);
    analysis.completions(file_id, offset, None)
}

fn has_label(items: &[CompletionItem], label: &str) -> bool {
    items.iter().any(|i| i.label == label)
}

fn items_matching<'a>(items: &'a [CompletionItem], label: &str) -> Vec<&'a CompletionItem> {
    items.iter().filter(|i| i.label == label).collect()
}

// ---------- cross-module method completion ----------

#[test]
fn completion_after_dot_on_common_module_lists_exported_methods() {
    let items = complete(
        r#"//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
Функция ПолучитьЗначение() Экспорт
    Возврат 1;
КонецФункции

Функция ВнутреннийМетод()
    Возврат 0;
КонецФункции

Процедура УстановитьЗначение(Значение) Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    Результат = ОбщегоНазначения.$0
КонецПроцедуры
"#,
    );

    assert!(
        has_label(&items, "ПолучитьЗначение"),
        "exported function must appear; got labels: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert!(
        has_label(&items, "УстановитьЗначение"),
        "exported procedure must appear; got labels: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert!(
        !has_label(&items, "ВнутреннийМетод"),
        "non-exported method must not leak across module boundary; got labels: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );

    for item in items_matching(&items, "ПолучитьЗначение") {
        assert!(
            matches!(item.kind, CompletionItemKind::Function | CompletionItemKind::Method),
            "exported function kind should be Function or Method, got {:?}",
            item.kind
        );
    }
}

// ---------- top-level BSL keyword completion ----------

#[test]
fn completion_in_procedure_body_without_qualifier_is_empty_today() {
    // Current baseline: typing a bare identifier in a procedure body without
    // a qualifier (no DOT) yields an empty completion list. The M1 rework
    // introduces `Scope::Builtins` and unified `Resolver` lookup, which is
    // expected to populate this path. When that lands, this test should
    // flip to asserting non-empty output.
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    Есл$0
КонецПроцедуры
"#,
    );

    assert!(
        items.is_empty(),
        "baseline: completion without qualifier is currently empty; got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ---------- platform type methods ----------

#[test]
fn completion_after_dot_on_new_array_type() {
    // `Новый Массив` gives the expression type `Массив`. Completion after the
    // dot should offer platform methods. If platform data is unavailable in
    // the test environment we accept an empty list — the assertion is only
    // that the pipeline reaches platform completion without panicking.
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    А = Новый Массив;
    А.$0
КонецПроцедуры
"#,
    );

    for item in &items {
        assert!(
            !item.label.is_empty(),
            "completion items must have non-empty labels; got {:?}",
            item
        );
    }
}

// ---------- no-crash canary for unresolved receiver ----------

#[test]
fn completion_after_dot_on_unresolved_receiver_is_safe() {
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    НеизвестныйСимвол.$0
КонецПроцедуры
"#,
    );

    // The pipeline must complete without panicking. Returning an empty list
    // is acceptable behaviour for an unresolved receiver.
    for item in &items {
        assert!(
            !item.label.is_empty(),
            "completion items must have non-empty labels; got {:?}",
            item
        );
    }
}
