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

#[test]
fn hover_platform_global_function() {
    let fixture = r#"//- /test.bsl
Процедура Тест()
    Сооб$0щить("x");
КонецПроцедуры
"#;
    let (analysis, file_id, offset) = setup(fixture);
    let hover = analysis.hover(file_id, offset, ide::Locale::Ru);
    if let Some(result) = hover {
        assert!(
            !result.markup.is_empty(),
            "hover on global function must produce non-empty markup when platform data is loaded"
        );
    }
}

#[test]
fn hover_keyword_процедура() {
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

#[test]
fn hover_implicit_module_var_after_new_constructor() {
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
    // A construction is authoritative: `Новый X` types the variable as `X` even
    // when the corpus does not model `X`, so hover keeps the constructor name.
    // (Only an *annotation* with an unrecognised name degrades to Unknown.)
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
    let fixture = r#"//- /test.bsl
Процедура Тест()
    Х = Новый Мас$0сив;
КонецПроцедуры
"#;
    let (analysis, file_id, offset) = setup(fixture);
    let hover = analysis.hover(file_id, offset, ide::Locale::Ru);
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

#[test]
fn hover_platform_method_renders_unique_semantic_candidate() {
    let fixture = r#"//- /test.bsl
Процедура Тест()
    Значение = Новый Массив;
    Значение.Вста$0вить(0, Неизвестно);
КонецПроцедуры
"#;
    let (analysis, file_id, offset) = setup(fixture);
    let hover = analysis
        .hover(file_id, offset, ide::Locale::Ru)
        .expect("hover on a uniquely resolved platform method must produce a result");
    assert!(hover.markup.contains("Массив"), "got: {}", hover.markup);
    assert!(!hover.markup.contains("\n\n---\n\n"), "got: {}", hover.markup);
}

#[test]
fn hover_platform_method_renders_ambiguous_semantic_candidates() {
    let fixture = r#"//- /CommonModules/ФабрикаКриптографии/Ext/Module.bsl
// Возвращаемое значение:
//   СертификатКриптографии, КонтейнерКлючейКриптографии
Функция Получить() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Процедура Тест(Условие)
    Значение = ФабрикаКриптографии.Получить();
    Значение.Выгру$0зить(Неизвестно, Неизвестно);
КонецПроцедуры
"#;
    let (analysis, file_id, offset) = setup(fixture);
    let hover = analysis
        .hover(file_id, offset, ide::Locale::Ru)
        .expect("hover on an ambiguously resolved platform method must produce a result");
    assert!(hover.markup.contains("СертификатКриптографии"), "got: {}", hover.markup);
    assert!(hover.markup.contains("КонтейнерКлючейКриптографии"), "got: {}", hover.markup);
    assert!(hover.markup.contains("\n\n---\n\n"), "got: {}", hover.markup);
}

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
        assert!(
            !h.markup.contains("РезультатЗапроса"),
            "global Выполнить must not render Query.Execute hover, got: {}",
            h.markup
        );
    }
}

#[test]
fn hover_for_each_loop_var_at_declaration_shows_element_type() {
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

#[test]
fn hover_on_unknown_identifier() {
    check_no_hover(
        r#"//- /test.bsl
Процедура Тест()
    Результат = НеизвестныйСим$0вол;
КонецПроцедуры
"#,
    );
}

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
