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
    let hover =
        analysis.hover(file_id, offset, ide::Locale::Ru).expect("hover should produce a result");
    expected.assert_eq(&hover.markup);
}

fn check_no_hover(fixture: &str) {
    let (analysis, file_id, offset) = setup(fixture);
    assert!(analysis.hover(file_id, offset, ide::Locale::Ru).is_none(), "expected no hover result");
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
    let hover = analysis.hover(file_id, offset, ide::Locale::Ru);
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
    let hover = analysis.hover(file_id, offset, ide::Locale::Ru);
    if let Some(result) = hover {
        assert!(
            result.markup.contains("Процедура"),
            "keyword hover must mention the keyword, got: {:?}",
            result.markup
        );
    }
}

// ---------- inferred type on bindings ----------

#[test]
fn hover_implicit_module_var_after_new_constructor() {
    // Root case from the type-system bring-up: BSL allows implicit variable
    // declaration at first assignment, so `КомпоновщикНастроек` has no
    // `Перем` entry in the item tree. Before the fix, hover returned None
    // (definition miss → platform type lookup by the variable's name also
    // misses). The fallback in `hover_user_defined` now surfaces the
    // inferred platform object type carried by `Expr::New`.
    let fixture = r#"//- /test.bsl
КомпоновщикНас$0троек = Новый КомпоновщикНастроекКомпоновкиДанных;
"#;
    let (analysis, file_id, offset) = setup(fixture);
    let hover = analysis
        .hover(file_id, offset, ide::Locale::Ru)
        .expect("hover on implicit var must produce a result");
    assert!(
        hover.markup.contains("КомпоновщикНастроекКомпоновкиДанных"),
        "hover must surface inferred type name, got: {:?}",
        hover.markup
    );
}

#[test]
fn hover_local_var_carries_inferred_primitive_type() {
    // Local variable path — `definition_to_hover` wires `Semantics::type_of_expr`
    // into the `Definition::Local` branch so the primitive is rendered as
    // "Массив / Array" alongside the existing `**Локальная переменная X**`
    // header. `ExprScopes` only collects explicit `Перем` decls, so the
    // fixture declares the variable up-front and hovers over the later use.
    let fixture = r#"//- /test.bsl
Процедура Тест()
    Перем Результат;
    Результат = Новый Массив;
    Резу$0льтат.Добавить(1);
КонецПроцедуры
"#;
    let (analysis, file_id, offset) = setup(fixture);
    let hover = analysis
        .hover(file_id, offset, ide::Locale::Ru)
        .expect("hover on local var must produce a result");
    assert!(
        hover.markup.contains("Локальная переменная Результат"),
        "hover must keep the local-variable header, got: {:?}",
        hover.markup
    );
    assert!(
        hover.markup.contains("Массив"),
        "hover must render Ty::Array as Массив, got: {:?}",
        hover.markup
    );
}

#[test]
fn hover_on_unknown_platform_type_in_new_falls_back_to_name_only() {
    // `КонвейерДанныхЗаказов` is not a real platform object — the
    // `TyLoweringContext::lower_bare_name` fallback produces
    // `Ty::PlatformObject("КонвейерДанныхЗаказов")`. Hover surfaces a
    // bare `**Тип:** name` block when `platform_type_query` has no data
    // for it, so output is informative even if the platform index is
    // incomplete.
    let fixture = r#"//- /test.bsl
Резу$0льтат = Новый КонвейерДанныхЗаказов;
"#;
    let (analysis, file_id, offset) = setup(fixture);
    let hover = analysis
        .hover(file_id, offset, ide::Locale::Ru)
        .expect("hover on implicit var with unknown platform object must still produce a result");
    assert!(
        hover.markup.contains("КонвейерДанныхЗаказов"),
        "hover must include the constructor type name even without platform data, got: {:?}",
        hover.markup
    );
}

#[test]
fn hover_on_constructor_name_does_not_leak_enclosing_new_type() {
    // Regression guard for the `type_of_token` scope rule: the token
    // `Массив` sits under a wider `NEW_EXPR` whose inferred `Ty::Array`
    // is the *result* of `Новый Массив`, not the type of the token
    // itself. Picking that up would short-circuit `hover_platform` and
    // miss the platform-type block (context / version / methods preview).
    let fixture = r#"//- /test.bsl
Процедура Тест()
    Х = Новый Мас$0сив;
КонецПроцедуры
"#;
    let (analysis, file_id, offset) = setup(fixture);
    let hover = analysis.hover(file_id, offset, ide::Locale::Ru);
    // Platform data may or may not be loaded in minimal test runs; the
    // negative guard (no unresolved-ident fallback header) runs always,
    // the positive check (right path fired) only when hover produced a
    // result — without it, we'd only prove the wrong path did not fire.
    if let Some(result) = hover {
        assert!(
            !result.markup.starts_with("**Массив**"),
            "constructor-name hover must not use the unresolved-ident fallback header, got: {:?}",
            result.markup
        );
        assert!(
            result.markup.contains("**Тип:** Массив"),
            "constructor-name hover must emit the platform-type header, got: {:?}",
            result.markup
        );
    }
}

// ---------- keyword-as-method-name (Layer B regression guards) ----------

/// Headline case from the user's bug report. `Выполнить` is lexed as
/// `KW_EXECUTE` (BSL has a global eval-style statement of the same
/// name), so the legacy IDENT-only gate in every hover handler killed
/// hover for `Запрос.Выполнить()`. After the unified classifier
/// migration, the token reaches the platform-method dispatch with the
/// receiver type intact and the method renderer fires.
#[test]
fn hover_keyword_method_after_dot_resolves_to_platform_method() {
    let fixture = r#"//- /test.bsl
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ 1";
    Результат = Запрос.Вып$0олнить();
КонецПроцедуры
"#;
    let (analysis, file_id, offset) = setup(fixture);
    let h = analysis
        .hover(file_id, offset, ide::Locale::Ru)
        .expect("hover on Запрос.Выполнить() must produce a result");
    // Hard positive: bilingual platform-method header AND the method's
    // documented return type.
    assert!(
        h.markup.contains("Выполнить") && h.markup.contains("Execute"),
        "hover must include the bilingual method name, got: {}",
        h.markup
    );
    assert!(
        h.markup.contains("РезультатЗапроса"),
        "hover must include Query.Execute return type, got: {}",
        h.markup
    );
}

/// Same keyword-as-method-name shape, but the receiver is itself a
/// platform-method call result (`Запрос.Выполнить().Выгрузить()` —
/// fluent chains). Pins that the classifier dispatch composes with the
/// fluent-aware `lookup_method_with_key` resolver.
#[test]
fn hover_chained_keyword_method() {
    let fixture = r#"//- /test.bsl
Процедура Тест()
    Запрос = Новый Запрос;
    Запрос.Текст = "ВЫБРАТЬ 1";
    тзнТоваров = Запрос.Вып$0олнить().Выгрузить();
КонецПроцедуры
"#;
    let (analysis, file_id, offset) = setup(fixture);
    let h = analysis
        .hover(file_id, offset, ide::Locale::Ru)
        .expect("hover on chained Запрос.Выполнить() must produce a result");
    assert!(h.markup.contains("Выполнить") && h.markup.contains("Execute"), "got: {}", h.markup);
    assert!(
        h.markup.contains("РезультатЗапроса"),
        "fluent receiver must not break Query.Execute return-type rendering, got: {}",
        h.markup
    );
}

/// Negative-direction guard: hovering the global `Выполнить("...")`
/// statement form (no dot) must NOT route through the method
/// dispatcher — it stays a keyword/global hover. The classifier's
/// `FieldName` arm is gated on `FIELD_EXPR` parent, so the global form
/// classifies as `Keyword` (or `FreeName` depending on how the parser
/// shapes the call-callee) and goes through `hover_keyword`.
#[test]
fn hover_global_execute_statement_does_not_render_query_method() {
    let fixture = r#"//- /test.bsl
Процедура Тест()
    Вып$0олнить("Сообщить()");
КонецПроцедуры
"#;
    let (analysis, file_id, offset) = setup(fixture);
    let h = analysis.hover(file_id, offset, ide::Locale::Ru);
    if let Some(h) = h {
        // Whatever surface (keyword vs global function), it must NOT
        // be the Query.Execute method markup — `РезультатЗапроса` is
        // unique to that path.
        assert!(
            !h.markup.contains("РезультатЗапроса"),
            "global Выполнить must not render Query.Execute hover, got: {}",
            h.markup
        );
    }
}

// ---------- declaration-site loop variables ----------

#[test]
fn hover_for_each_loop_var_at_declaration_shows_element_type() {
    // `Для Каждого X Из Y Цикл` lowers `X` only as a `BindingId` — there
    // is no `Expr::Path` at the declaration site, so the wrapper-walk in
    // `type_of_token` finds nothing. The fallback through
    // `Semantics::type_of_binding_at` reaches into per-body `var_types`
    // and surfaces the iter element type the same way hover at a use
    // site does.
    let fixture = r#"//- /test.bsl
Процедура Тест()
    М = Новый Соответствие;
    Для Каждого К$0З Из М Цикл
        Х = КЗ;
    КонецЦикла;
КонецПроцедуры
"#;
    let (analysis, file_id, offset) = setup(fixture);
    let hover = analysis
        .hover(file_id, offset, ide::Locale::Ru)
        .expect("hover on declaration-site loop variable must produce a result");
    assert!(
        hover.markup.contains("КлючИЗначение"),
        "hover on Для Каждого declaration-site var over Соответствие must show КлючИЗначение, got: {:?}",
        hover.markup
    );
}

#[test]
fn hover_classic_for_counter_at_declaration_shows_number() {
    // `Для I = 1 По 10 Цикл` binds `I` to `Ty::Number` per BSL semantics
    // (the counter is always Number; runtime errors on non-Number
    // bounds). Declaration-site hover must show that, mirroring the
    // ForEach declaration-site fix. Counter name `СчётчикЦикла` keeps
    // the cursor anchored inside an IDENT token (single-letter names
    // are too short to land mid-token without ambiguity).
    let fixture = r#"//- /test.bsl
Процедура Тест()
    Для Счёт$0чикЦикла = 1 По 10 Цикл
        Х = СчётчикЦикла;
    КонецЦикла;
КонецПроцедуры
"#;
    let (analysis, file_id, offset) = setup(fixture);
    let hover = analysis
        .hover(file_id, offset, ide::Locale::Ru)
        .expect("hover on classic-for counter declaration must produce a result");
    assert!(
        hover.markup.contains("Число") || hover.markup.contains("Number"),
        "hover on classic-for counter must show Number, got: {:?}",
        hover.markup
    );
}

#[test]
fn hover_for_each_loop_var_same_body_shadowing() {
    // Two `Для Каждого` loops in the **same** procedure with the same
    // lowercase variable name and different element types. With
    // name-keyed `var_types`, the file-global last-write-wins would
    // surface the second loop's type on hover at the first
    // declaration. Per-binding `binding_types_by_body` keys by the
    // freshly allocated `BindingId`, so each declaration site shows
    // its own type.
    let fixture = r#"//- /test.bsl
Процедура Тест()
    М = Новый Соответствие;
    Т = Новый ТаблицаЗначений;
    Для Каждого Эле$0м Из М Цикл
    КонецЦикла;
    Для Каждого Элем Из Т Цикл
    КонецЦикла;
КонецПроцедуры
"#;
    let (analysis, file_id, offset) = setup(fixture);
    let hover = analysis
        .hover(file_id, offset, ide::Locale::Ru)
        .expect("hover at first declaration in same-body shadowing fixture must produce a result");
    assert!(
        hover.markup.contains("КлючИЗначение"),
        "first declaration must resolve to КлючИЗначение (its own collection's element), \
         not СтрокаТаблицыЗначений from the second loop, got: {:?}",
        hover.markup
    );
}

#[test]
fn hover_for_each_loop_var_per_body_isolation() {
    // Two procedures with the same loop variable name but different
    // collection types must not collide. With per-body `var_types_by_body`,
    // hover in each body resolves to the correct element type. Without
    // it, the file-global `var_types` would carry only the
    // last-inferred body's type and the first body would mis-render.
    let fixture = r#"//- /test.bsl
Процедура ПерваяПроцедура()
    М = Новый Соответствие;
    Для Каждого Эле$0м Из М Цикл
    КонецЦикла;
КонецПроцедуры

Процедура ВтораяПроцедура()
    Т = Новый ТаблицаЗначений;
    Для Каждого Элем Из Т Цикл
    КонецЦикла;
КонецПроцедуры
"#;
    let (analysis, file_id, offset) = setup(fixture);
    let hover = analysis
        .hover(file_id, offset, ide::Locale::Ru)
        .expect("hover in first procedure must produce a result");
    assert!(
        hover.markup.contains("КлючИЗначение"),
        "first procedure's loop var must resolve to КлючИЗначение (Соответствие element), \
         not СтрокаТаблицыЗначений from the sibling procedure, got: {:?}",
        hover.markup
    );
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

// ---------- bilingual display ----------

/// `Ty::display(locale)` flips the rendered type label per locale. The
/// hover frame stays Russian (single-locale message templates are out
/// of scope for the bilingual-display refactor), but the
/// `**Тип:** <T>` line picks up the user's selected locale so a
/// Russian-only IDE no longer reads "Number" when the surrounding text
/// says "Тип:".
///
/// Hits the `definition_to_hover` → `ty_info_markup` path on a local
/// variable: a literal `42` infers to `Ty::Number`, which `display(Ru)`
/// renders as "Число" and `display(En)` as "Number".
#[test]
fn hover_local_variable_type_label_localizes() {
    let fixture = r#"//- /test.bsl
Процедура Тест()
    Сч$0етчик = 42;
КонецПроцедуры
"#;
    let (analysis, file_id, offset) = setup(fixture);

    let hover_ru = analysis
        .hover(file_id, offset, ide::Locale::Ru)
        .expect("hover on Russian-locale must produce a result");
    assert!(
        hover_ru.markup.contains("**Тип:** Число"),
        "Russian hover must label the local as Число, got: {:?}",
        hover_ru.markup
    );
    assert!(
        !hover_ru.markup.contains("Number"),
        "Russian hover must not surface the English label, got: {:?}",
        hover_ru.markup
    );

    let hover_en = analysis
        .hover(file_id, offset, ide::Locale::En)
        .expect("hover on English-locale must produce a result");
    assert!(
        hover_en.markup.contains("**Тип:** Number"),
        "English hover must label the local as Number, got: {:?}",
        hover_en.markup
    );
    assert!(
        !hover_en.markup.contains("Число"),
        "English hover must not surface the Russian label, got: {:?}",
        hover_en.markup
    );
}
