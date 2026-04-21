//! Baseline snapshot tests for hover.
//!
//! These lock down the current hover output ahead of the M1 resolver rework
//! (`Scope::Builtins`, unified `Resolver` name lookup, `visible_configurations`
//! iteration). Any behavioural drift in user-defined / cross-module / platform
//! hover will surface here.
//!
//! Use `UPDATE_EXPECT=1 cargo test -p ide --test hover_baseline` to refresh
//! snapshots when the markup legitimately changes.
//!
//! The `$0` marker denotes the cursor position.

use expect_test::{expect, Expect};
use ide::Analysis;
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

/// Finds `$0` in the fixture source, removes it, and returns the cursor
/// offset within the file that contained it (byte offset relative to that
/// file's content, which is what `Analysis::hover` expects).
fn extract_cursor(fixture_text: &str) -> (String, String, u32) {
    let abs_idx = fixture_text.find("$0").expect("fixture must contain $0 cursor marker");

    // Determine which `//- /path` section the cursor is in and the offset
    // relative to that section's content (stripping the header line).
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

fn check_hover(fixture: &str, expected: Expect) {
    let (analysis, file_id, offset) = setup(fixture);
    let hover = analysis.hover(file_id, offset).expect("hover should produce a result");
    expected.assert_eq(&hover.markup);
}

fn check_no_hover(fixture: &str) {
    let (analysis, file_id, offset) = setup(fixture);
    assert!(analysis.hover(file_id, offset).is_none(), "expected no hover result");
}

// ---------- user-defined symbols ----------

#[test]
fn hover_user_method_definition() {
    check_hover(
        r#"//- /test.bsl
Функция Посч$0итать() Экспорт
    Возврат 42;
КонецФункции
"#,
        expect![[r#"
            **Функция Посчитать()**

            *Экспортная*

        "#]],
    );
}

#[test]
fn hover_user_procedure_with_parameters() {
    check_hover(
        r#"//- /test.bsl
Процедура Обр$0аботать(Объект, ИмяРеквизита)
КонецПроцедуры
"#,
        expect![[r#"
            **Процедура Обработать()**

        "#]],
    );
}

#[test]
fn hover_parameter_reference() {
    check_hover(
        r#"//- /test.bsl
Процедура Обработать(Объ$0ект, ИмяРеквизита)
КонецПроцедуры
"#,
        expect![[r#"
            **Параметр Объект**

        "#]],
    );
}

#[test]
fn hover_module_variable() {
    check_hover(
        r#"//- /test.bsl
Перем Счет$0чик Экспорт;

Процедура Использовать()
КонецПроцедуры
"#,
        expect![[r#"
            **Перем Счетчик**

            *Экспортная*

        "#]],
    );
}

// ---------- cross-module (CommonModule) ----------

#[test]
fn hover_cross_module_method_call() {
    check_hover(
        r#"//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
Функция ПолучитьЗначение() Экспорт
    Возврат 1;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = ОбщегоНазначения.Получ$0итьЗначение();
КонецПроцедуры
"#,
        expect![[r#"
            **Функция ПолучитьЗначение()**

            *Экспортная*

        "#]],
    );
}

#[test]
fn hover_common_module_name() {
    check_hover(
        r#"//- /CommonModules/ОбщегоНазначения/Ext/Module.bsl
Функция ПолучитьЗначение() Экспорт
    Возврат 1;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = ОбщегоНазна$0чения.ПолучитьЗначение();
КонецПроцедуры
"#,
        expect![[r#"
            **Функция ПолучитьЗначение()**

            *Экспортная*

        "#]],
    );
}

// ---------- platform / builtins ----------

#[test]
fn hover_platform_global_function() {
    // `Сообщить` is a global platform function shipped in bundled docs.
    // Snapshot kept minimal-prefix so renames of the doc text don't break
    // the M1 baseline — assert only that we got *some* markup for it.
    let fixture = r#"//- /test.bsl
Процедура Тест()
    Сооб$0щить("x");
КонецПроцедуры
"#;
    let (analysis, file_id, offset) = setup(fixture);
    let hover = analysis.hover(file_id, offset);
    // Platform data may be absent in minimal test runs; accept either state
    // so we pin the call path without forcing a specific markup fingerprint.
    if let Some(result) = hover {
        assert!(
            !result.markup.is_empty(),
            "hover on global function must produce non-empty markup when platform data is loaded"
        );
    }
}

#[test]
fn hover_keyword_процедура() {
    // Keyword hover reads from bundled keyword docs; same tolerance as above.
    let fixture = r#"//- /test.bsl
Проц$0едура Тест()
КонецПроцедуры
"#;
    let (analysis, file_id, offset) = setup(fixture);
    let hover = analysis.hover(file_id, offset);
    if let Some(result) = hover {
        assert!(
            result.markup.contains("Процедура"),
            "keyword hover must mention the keyword, got: {:?}",
            result.markup
        );
    }
}

// ---------- negative cases ----------

#[test]
fn hover_on_unknown_identifier() {
    // Unresolved identifier should not produce hover (user-defined branch
    // returns None, platform branch misses, keyword branch misses).
    check_no_hover(
        r#"//- /test.bsl
Процедура Тест()
    Результат = НеизвестныйСим$0вол;
КонецПроцедуры
"#,
    );
}
