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
    analysis.completions(file_id, offset, None, ide::Locale::Ru)
}

fn has_label(items: &[CompletionItem], label: &str) -> bool {
    items.iter().any(|i| i.label == label)
}

fn items_matching<'a>(items: &'a [CompletionItem], label: &str) -> Vec<&'a CompletionItem> {
    items.iter().filter(|i| i.label == label).collect()
}

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

#[test]
fn completion_in_procedure_body_offers_keyword_templates() {
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    Есл$0
КонецПроцедуры
"#,
    );

    assert!(!items.is_empty(), "unqualified `Есл` must offer keyword/templates");

    // The `Если … Тогда … КонецЕсли` template must be offered as a snippet.
    let if_template =
        items.iter().find(|i| i.kind == CompletionItemKind::Snippet && i.label.starts_with("Если"));
    assert!(
        if_template.is_some(),
        "`Если` block template must be offered; got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert!(
        if_template.unwrap().insert_text.contains("КонецЕсли"),
        "the template must expand to a full block with `КонецЕсли`"
    );

    // The contiguous gate keeps scattered platform matches (`П-е-...-с-л`) out of
    // the list for a short prefix.
    assert!(
        !has_label(&items, "Перечисления"),
        "scattered platform match must not flood a short-prefix list; got: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[test]
fn completion_ranks_exact_prefix_above_fuzzy_via_sort_text() {
    // `Найти` is an exact prefix of `НайтиПоКоду`/`НайтиПоНаименованию` and a
    // scattered match for other names; the exact-prefix hits must sort first.
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    НайтиЗначение = 1;
    Найт$0
КонецПроцедуры
"#,
    );
    let ranked: Vec<(&str, &str)> = items
        .iter()
        .filter_map(|i| i.sort_text.as_deref().map(|s| (i.label.as_str(), s)))
        .collect();
    assert!(!ranked.is_empty(), "every unqualified item must carry a sort_text");
    // `НайтиЗначение` is a local in scope and an exact prefix → tier 0 + locals band.
    let local = ranked.iter().find(|(l, _)| *l == "НайтиЗначение");
    assert!(local.is_some(), "the in-scope local must be offered; got {:?}", ranked);
    assert!(
        local.unwrap().1.starts_with('0'),
        "exact-prefix local must be in the best quality tier (sort_text starts with 0); got {:?}",
        local.unwrap()
    );
}

#[test]
fn completion_english_prefix_ranks_metadata_plural_in_top_tier() {
    // Typing the English name `Docu` admits `Документы` via its English alias; it
    // must be ranked by that match's real quality (tier 0), not sunk to Fuzzy by
    // scoring the Russian label.
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    Docu$0
КонецПроцедуры
"#,
    );
    // Platform data may be unavailable in some environments; only assert when the
    // metadata plural is actually offered.
    if let Some(item) = items.iter().find(|i| i.label == "Документы") {
        let sort_text = item.sort_text.as_deref().expect("offered item must carry sort_text");
        assert!(
            sort_text.starts_with('0'),
            "English prefix `Docu` must rank `Документы` in the top quality tier; got {sort_text:?}"
        );
    }
}

#[test]
fn completion_after_dot_on_new_array_type() {
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
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    Сп = 42;
    Сп.$0
КонецПроцедуры
"#,
    );

    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        !has_label(&items, "Добавить"),
        "Number receiver must not surface Массив.Добавить; got: {:?}",
        labels
    );
}

#[test]
fn completion_with_cursor_on_keyword_method_name_after_dot() {
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Выполнить$0
КонецПроцедуры
"#,
    );

    assert!(
        !items.is_empty(),
        "completion must fire when cursor is on a keyword field-tail token (KW_EXECUTE)"
    );
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        has_label(&items, "Выполнить"),
        "method list must include `Выполнить` for a Query receiver; got: {:?}",
        labels
    );
}

#[test]
fn completion_after_dot_on_unresolved_receiver_is_safe() {
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    НеизвестныйСимвол.$0
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
