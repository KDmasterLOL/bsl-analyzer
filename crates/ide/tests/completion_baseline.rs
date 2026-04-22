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
    // `Новый Массив` gives the expression type `Массив`. Completion after
    // the dot exercises the full pipeline: parser wraps the bare `А.` in
    // an ERROR node (not a valid statement), HIR lowering's recovery
    // path (see `hir-def::body::lower::stmt::try_lower_recovered_expr_stmt`)
    // salvages the FIELD_EXPR so `Semantics::type_of_expr` can return
    // `Ty::Array`, and platform completion surfaces the methods.
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    А = Новый Массив;
    А.$0
КонецПроцедуры
"#,
    );

    assert!(
        !items.is_empty(),
        "platform methods must be offered after `А.` where `А` is `Новый Массив`"
    );
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        has_label(&items, "Добавить"),
        "Массив method `Добавить` must be offered; got: {:?}",
        labels
    );
    for item in &items {
        assert!(
            !item.label.is_empty(),
            "completion items must have non-empty labels; got {:?}",
            item
        );
    }
}

#[test]
fn completion_after_dot_on_array_variable_with_prefix() {
    // `Сп.Доб|` — cursor on IDENT whose previous non-trivia token is DOT.
    // `platform_completions` must walk back to the anchor DOT, use the
    // receiver's type, and filter methods by the partial-typed IDENT.
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    Сп = Новый Массив;
    Сп.Доб$0
КонецПроцедуры
"#,
    );

    assert!(!items.is_empty(), "prefix-filtered completion must not be empty; got empty");
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().all(|l| l.to_lowercase().starts_with("доб")),
        "every label must start with `Доб`; got: {:?}",
        labels
    );
    assert!(has_label(&items, "Добавить"), "`Добавить` must match prefix `Доб`; got: {:?}", labels);
}

#[test]
fn completion_after_dot_on_array_variable_typed_prefix_full_ident() {
    // `Сп.В|` — user has typed just the first letter. Recovery kicks in;
    // prefix filter narrows by `В`.
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    Сп = Новый Массив;
    Сп.В$0
КонецПроцедуры
"#,
    );

    assert!(!items.is_empty(), "methods starting with `В` must be offered after `Сп.В`");
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.iter().all(|l| l.to_lowercase().starts_with('в')),
        "every label must start with `В`; got: {:?}",
        labels
    );
    assert!(
        has_label(&items, "Вставить"),
        "`Вставить` must be offered after `Сп.В`; got: {:?}",
        labels
    );
}

#[test]
fn completion_after_chained_dot_on_array_variable() {
    // Chain with a valid call in between: the first `Сп.Добавить(1)` is
    // a normal CALL_STMT, the trailing `Сп.` goes through recovery.
    // Completion must still surface methods for the second `Сп.` — the
    // recovery marker is per-expression, not per-body.
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    Сп = Новый Массив;
    Сп.Добавить(1);
    Сп.$0
КонецПроцедуры
"#,
    );

    assert!(
        !items.is_empty(),
        "trailing `Сп.` must still produce method completions after a successful call"
    );
    assert!(
        has_label(&items, "Добавить"),
        "methods must still be available on the second `Сп.`; labels: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[test]
fn completion_after_dot_on_number_variable_does_not_offer_array_methods() {
    // Negative: `Сп = 42` makes `Сп` a Number. A trailing `Сп.` must not
    // surface Массив methods — recovery doesn't hallucinate types.
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    Сп = 42;
    Сп.$0
КонецПроцедуры
"#,
    );

    // We don't assert emptiness (Number has a handful of methods in
    // platform data); we assert the Array-specific ones aren't there.
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        !has_label(&items, "Добавить"),
        "Number receiver must not surface Массив.Добавить; got: {:?}",
        labels
    );
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
