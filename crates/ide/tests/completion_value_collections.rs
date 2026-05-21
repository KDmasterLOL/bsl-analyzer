//! Completion coverage for platform value-collection types:
//! `ТаблицаЗначений` (ValueTable), `Структура` (Structure), and
//! `ДеревоЗначений` (ValueTree).
//!
//! Two surfaces are pinned:
//!
//! 1. **Direct `Новый T` construction** — assigning the result of
//!    `Новый ТаблицаЗначений` / `Новый Структура` / `Новый ДеревоЗначений`
//!    to a local variable, then completing on the variable dot, must
//!    surface the platform members for the corresponding `Ty::*` (members
//!    sourced from `platform_data.json` via
//!    `method_lookup`/`platform_property_lookup`).
//!
//! 2. **Cascade typing through same-module function-call**
//!    (`Х = F(); Х.|`) — Phase J `method_return_type_query` +
//!    `materialise_signature_enriched` (commit O.11) propagate the
//!    `Новый ТаблицаЗначений` return type back to the caller so
//!    completion on `Х.` offers ValueTable members without a docstring.
//!    This is the C3 follow-up captured in
//!    `project_phase_o_c3_completion_followup.md`.

use ide::{Analysis, CompletionItem};
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

fn labels(items: &[CompletionItem]) -> Vec<&str> {
    items.iter().map(|i| i.label.as_str()).collect()
}

// ---------- direct `Новый T` construction ----------

#[test]
fn completion_after_dot_on_new_value_table() {
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    ТЗ = Новый ТаблицаЗначений;
    ТЗ.$0
КонецПроцедуры
"#,
    );

    let ls = labels(&items);
    assert!(!items.is_empty(), "ТаблицаЗначений members must be offered; got empty");
    for expected in ["Добавить", "Очистить", "Количество", "НайтиСтроки", "Колонки"]
    {
        assert!(
            has_label(&items, expected),
            "ValueTable member `{expected}` must be offered; got: {ls:?}"
        );
    }
}

#[test]
fn completion_after_dot_on_new_structure() {
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    Стр = Новый Структура;
    Стр.$0
КонецПроцедуры
"#,
    );

    let ls = labels(&items);
    assert!(!items.is_empty(), "Структура members must be offered; got empty");
    for expected in ["Вставить", "Свойство", "Очистить", "Количество", "Удалить"]
    {
        assert!(
            has_label(&items, expected),
            "Structure member `{expected}` must be offered; got: {ls:?}"
        );
    }
}

#[test]
fn completion_after_dot_on_new_value_tree() {
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    ДЗ = Новый ДеревоЗначений;
    ДЗ.$0
КонецПроцедуры
"#,
    );

    let ls = labels(&items);
    assert!(!items.is_empty(), "ДеревоЗначений members must be offered; got empty");
    for expected in ["Колонки", "Строки", "Скопировать"] {
        assert!(
            has_label(&items, expected),
            "ValueTree member `{expected}` must be offered; got: {ls:?}"
        );
    }
}

// ---------- prefix-filtered completion ----------

#[test]
fn completion_after_dot_on_new_value_table_with_prefix() {
    // Прибавляем префикс `Доб` — фильтр должен оставить только
    // `Добавить` среди методов ValueTable.
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    ТЗ = Новый ТаблицаЗначений;
    ТЗ.Доб$0
КонецПроцедуры
"#,
    );

    let ls = labels(&items);
    assert!(!items.is_empty(), "ТЗ.Доб — at least `Добавить` must remain; got empty");
    assert!(
        ls.iter().all(|l| l.to_lowercase().starts_with("доб")),
        "every label must start with `Доб`; got: {ls:?}"
    );
    assert!(has_label(&items, "Добавить"), "`Добавить` must be in the filtered set; got: {ls:?}");
}

// ---------- C3: cascade typing through same-module function-call ----------

#[test]
fn completion_after_dot_on_local_from_same_module_fn_returning_value_table() {
    // Pin for the C3 follow-up: same-module function with no docstring
    // returns `Новый ТаблицаЗначений`. Phase J cascade typing
    // (method_return_type_query + materialise_signature_enriched,
    // commit 224a2559 — Phase O.11) infers the return type from the
    // body; Phase L narrow-infer routes the caller's local through
    // InferOwnerResult; completion on `Х.` must offer ValueTable members.
    let items = complete(
        r#"//- /test.bsl
Функция Получить()
    Возврат Новый ТаблицаЗначений;
КонецФункции

Процедура Тест()
    Х = Получить();
    Х.$0
КонецПроцедуры
"#,
    );

    let ls = labels(&items);
    assert!(
        !items.is_empty(),
        "cascade typing must propagate ТаблицаЗначений through `Получить()` to `Х`; got empty"
    );
    for expected in ["Добавить", "Колонки", "Количество"] {
        assert!(
            has_label(&items, expected),
            "cascade-typed local `Х` must surface ValueTable member `{expected}`; got: {ls:?}"
        );
    }
}

#[test]
fn completion_after_dot_on_local_from_same_module_fn_returning_structure() {
    let items = complete(
        r#"//- /test.bsl
Функция СоздатьОтбор()
    Возврат Новый Структура;
КонецФункции

Процедура Тест()
    Отбор = СоздатьОтбор();
    Отбор.$0
КонецПроцедуры
"#,
    );

    let ls = labels(&items);
    assert!(
        !items.is_empty(),
        "cascade typing must propagate Структура through `СоздатьОтбор()` to `Отбор`; got empty"
    );
    for expected in ["Вставить", "Свойство"] {
        assert!(
            has_label(&items, expected),
            "cascade-typed local `Отбор` must surface Structure member `{expected}`; got: {ls:?}"
        );
    }
}

// ---------- 2-hop cascade through method-graph ----------

#[test]
fn completion_two_hop_same_module_cascade_value_table() {
    // `Outer()` calls `Inner()` which returns `Новый ТаблицаЗначений`.
    // `method_return_type_query(Outer)` must recursively trigger
    // `method_return_type_query(Inner)` to infer Outer's return type
    // as `Ty::ValueTable`. Two method-graph hops; cycle_fn keeps it
    // safe even though there's no actual cycle here.
    let items = complete(
        r#"//- /test.bsl
Функция Inner()
    Возврат Новый ТаблицаЗначений;
КонецФункции

Функция Outer()
    Возврат Inner();
КонецФункции

Процедура Тест()
    Х = Outer();
    Х.$0
КонецПроцедуры
"#,
    );

    let ls = labels(&items);
    assert!(
        !items.is_empty(),
        "2-hop cascade Inner→Outer must propagate ТаблицаЗначений; got empty"
    );
    for expected in ["Добавить", "Колонки", "Количество"] {
        assert!(
            has_label(&items, expected),
            "2-hop cascade must surface ValueTable member `{expected}`; got: {ls:?}"
        );
    }
}

#[test]
fn completion_two_hop_same_module_cascade_structure() {
    let items = complete(
        r#"//- /test.bsl
Функция СоздатьБазу()
    Возврат Новый Структура;
КонецФункции

Функция Прочитать()
    Возврат СоздатьБазу();
КонецФункции

Процедура Тест()
    Отбор = Прочитать();
    Отбор.$0
КонецПроцедуры
"#,
    );

    let ls = labels(&items);
    assert!(!items.is_empty(), "2-hop cascade Прочитать→СоздатьБазу must propagate Структура");
    for expected in ["Вставить", "Свойство"] {
        assert!(
            has_label(&items, expected),
            "2-hop cascade must surface Structure member `{expected}`; got: {ls:?}"
        );
    }
}

// ---------- cross-module qualified call (Phase J via CommonModule) ----------

#[test]
fn completion_qualified_call_cross_module_value_table() {
    // `Util.СоздатьТЗ()` — qualified call on a CommonModule receiver.
    // Resolver finds the CommonModule, picks the exported method,
    // and `method_return_type_query` infers its return type as
    // `Ty::ValueTable` from the body. Single Phase J hop across the
    // CommonModule boundary.
    let items = complete(
        r#"//- /CommonModules/Util/Ext/Module.bsl
Функция СоздатьТЗ() Экспорт
    Возврат Новый ТаблицаЗначений;
КонецФункции

//- /test.bsl
Процедура Тест()
    Х = Util.СоздатьТЗ();
    Х.$0
КонецПроцедуры
"#,
    );

    let ls = labels(&items);
    assert!(
        !items.is_empty(),
        "cross-module qualified call `Util.СоздатьТЗ()` must propagate ТаблицаЗначений; got empty"
    );
    for expected in ["Добавить", "Колонки"] {
        assert!(
            has_label(&items, expected),
            "cross-module cascade must surface ValueTable member `{expected}`; got: {ls:?}"
        );
    }
}

#[test]
fn completion_two_hop_cross_module_cascade_structure() {
    // Two Phase J hops across a CommonModule:
    //   Util.Outer() → Util.Inner() → Новый Структура
    // method_return_type_query(Outer) needs to recurse into Inner via
    // the same query — Salsa cycle_fn keeps it safe across the
    // CommonModule boundary.
    let items = complete(
        r#"//- /CommonModules/Util/Ext/Module.bsl
Функция Inner() Экспорт
    Возврат Новый Структура;
КонецФункции

Функция Outer() Экспорт
    Возврат Inner();
КонецФункции

//- /test.bsl
Процедура Тест()
    Отбор = Util.Outer();
    Отбор.$0
КонецПроцедуры
"#,
    );

    let ls = labels(&items);
    assert!(
        !items.is_empty(),
        "2-hop cross-module cascade `Util.Outer()→Util.Inner()` must propagate Структура; got empty"
    );
    for expected in ["Вставить", "Свойство"] {
        assert!(
            has_label(&items, expected),
            "2-hop cross-module cascade must surface Structure member `{expected}`; got: {ls:?}"
        );
    }
}

// ---------- mixed: method-graph hop + property hop ----------

#[test]
fn completion_method_graph_hop_then_property_hop_value_table_columns() {
    // Hop 1: `Получить()` → Phase J method_return_type_query → Ty::ValueTable.
    // Hop 2: `.Колонки` → platform_property_lookup on ValueTable →
    //         Ty::ValueTableColumnCollection.
    // Completion at the second dot must surface column-collection
    // methods (`Добавить`, `Найти`, etc.) — proving cascade typing
    // chains a Phase J hop with a platform property hop.
    let items = complete(
        r#"//- /test.bsl
Функция Получить()
    Возврат Новый ТаблицаЗначений;
КонецФункции

Процедура Тест()
    Х = Получить();
    Y = Х.Колонки;
    Y.$0
КонецПроцедуры
"#,
    );

    let ls = labels(&items);
    assert!(
        !items.is_empty(),
        "method-graph + property hop must reach ValueTableColumnCollection; got empty"
    );
    for expected in ["Добавить", "Найти", "Количество"] {
        assert!(
            has_label(&items, expected),
            "mixed-hop cascade must surface column-collection member `{expected}`; got: {ls:?}"
        );
    }
}

// ---------- Массив (Array) cascade with callee-below-caller ordering ----------

#[test]
fn completion_after_dot_on_local_from_same_module_fn_returning_array_callee_below() {
    // Воспроизводит реальный сценарий из ERP: caller объявлен ВЫШЕ callee,
    // callee возвращает `Новый Массив`. Method-graph closure должен видеть
    // вызываемую функцию независимо от порядка объявления; cascade typing
    // через `method_return_type_query` обязан вывести `Ty::Array` и
    // выдать на `Пар.` методы массива (`Добавить`, `Количество`, `Найти`).
    let items = complete(
        r#"//- /test.bsl
Функция ТестированиеТипов()
    Пар = СписокПараметров();
    Пар.$0
КонецФункции

Функция СписокПараметров()
    Возврат Новый Массив;
КонецФункции
"#,
    );

    let ls = labels(&items);
    assert!(
        !items.is_empty(),
        "cascade typing must propagate Массив through `СписокПараметров()` to `Пар`; got empty"
    );
    for expected in ["Добавить", "Количество", "Найти"] {
        assert!(
            has_label(&items, expected),
            "cascade-typed local `Пар` must surface Array member `{expected}`; got: {ls:?}"
        );
    }
}

// ---------- diagnostic: isolate caller-above-callee vs Array-specific ----------

#[test]
fn completion_after_dot_on_local_from_same_module_fn_value_table_callee_below() {
    // Тот же ValueTable, что и в зелёном тесте выше, но caller ВЫШЕ callee.
    // Если падает — виноват порядок объявления (forward-reference в
    // method-graph closure). Если зелёный — ordering ОК, баг в Массиве.
    let items = complete(
        r#"//- /test.bsl
Функция Тест()
    Х = Получить();
    Х.$0
КонецФункции

Функция Получить()
    Возврат Новый ТаблицаЗначений;
КонецФункции
"#,
    );

    let ls = labels(&items);
    assert!(!items.is_empty(), "ValueTable callee-below cascade must work; got empty: {ls:?}");
    for expected in ["Добавить", "Колонки", "Количество"] {
        assert!(
            has_label(&items, expected),
            "ValueTable callee-below cascade must surface `{expected}`; got: {ls:?}"
        );
    }
}

#[test]
fn completion_after_dot_on_local_from_same_module_fn_array_callee_above() {
    // Массив с callee ВЫШЕ caller — обратный к failing-case порядок.
    // Если зелёный — баг в forward-reference. Если красный —
    // баг конкретно в выводе `Ty::Array` для `Новый Массив`
    // (cascade через method_return_type_query не достаёт Array).
    let items = complete(
        r#"//- /test.bsl
Функция СписокПараметров()
    Возврат Новый Массив;
КонецФункции

Функция ТестированиеТипов()
    Пар = СписокПараметров();
    Пар.$0
КонецФункции
"#,
    );

    let ls = labels(&items);
    assert!(
        !items.is_empty(),
        "Array callee-above cascade must propagate Ty::Array; got empty: {ls:?}"
    );
    for expected in ["Добавить", "Количество", "Найти"] {
        assert!(
            has_label(&items, expected),
            "Array callee-above cascade must surface `{expected}`; got: {ls:?}"
        );
    }
}

// ---------- chained dot through ValueTable.Колонки ----------

#[test]
fn completion_after_chained_dot_on_value_table_columns() {
    // ValueTable.Колонки : ValueTableColumnCollection — chain hop
    // through `Ty::ValueTable -> property "Колонки" -> Ty::?` must
    // surface column-collection methods (`Добавить`, `Найти`, etc).
    let items = complete(
        r#"//- /test.bsl
Процедура Тест()
    ТЗ = Новый ТаблицаЗначений;
    ТЗ.Колонки.$0
КонецПроцедуры
"#,
    );

    let ls = labels(&items);
    assert!(
        !items.is_empty(),
        "ТЗ.Колонки.| must surface ValueTableColumnCollection methods; got empty"
    );
    for expected in ["Добавить", "Найти", "Количество"] {
        assert!(
            has_label(&items, expected),
            "ValueTableColumnCollection member `{expected}` must be offered; got: {ls:?}"
        );
    }
}
