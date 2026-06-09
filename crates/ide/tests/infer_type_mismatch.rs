use hir::{Builders, HirDatabase, InferenceDiagnostic, TypeId};
use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
use ide_db::RootDatabaseImpl;
use std::path::PathBuf;
use test_fixture::Fixture;
use vfs::FileId;

fn designer_fixture_path() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer"))
}

fn setup(fixture_text: &str) -> (RootDatabaseImpl, FileId) {
    setup_impl(fixture_text, true)
}

fn setup_vfs_only(fixture_text: &str) -> (RootDatabaseImpl, FileId) {
    setup_impl(fixture_text, false)
}

fn setup_impl(fixture_text: &str, attach_designer_config: bool) -> (RootDatabaseImpl, FileId) {
    let fixture = Fixture::parse(fixture_text);
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
    if attach_designer_config {
        db.set_all_config_paths(vec![(None, designer_fixture_path())]);
    }

    let test_file = fixture
        .files
        .iter()
        .find(|(_, f)| f.path.as_path().to_string_lossy().ends_with("/test.bsl"))
        .map(|(id, _)| *id)
        .expect("fixture must contain /test.bsl");
    (db, test_file)
}

fn mismatches(db: &RootDatabaseImpl, file_id: FileId) -> Vec<(TypeId, TypeId)> {
    db.infer(file_id)
        .diagnostics
        .iter()
        .chain(db.arg_diagnostics(file_id).iter())
        .filter_map(|(_, d)| match d {
            InferenceDiagnostic::TypeMismatch { expected, actual, .. } => {
                Some((*expected, *actual))
            }
            _ => None,
        })
        .collect()
}

const COMMON_MODULE_TYPED_NUMBER_PARAM: &str = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Параметры:
//   П - Число - численный аргумент
// Возвращаемое значение:
//   Строка - описание
Функция Привет(П) Экспорт
    Возврат "привет";
КонецФункции
"#;

#[test]
fn type_mismatch_fires_on_concrete_mismatch() {
    let fixture = format!(
        "{COMMON_MODULE_TYPED_NUMBER_PARAM}\n\
//- /test.bsl\n\
Процедура Тест()\n\
    А = ПервыйОбщийМодуль.Привет(\"строка\");\n\
КонецПроцедуры\n"
    );
    let (db, file_id) = setup(&fixture);

    let mm = mismatches(&db, file_id);
    assert_eq!(
        mm,
        vec![(db.number(None, None), db.string(None, false))],
        "concrete Number param + String arg must fire exactly one mismatch"
    );
}

#[test]
fn type_mismatch_silent_on_matching_arg() {
    let fixture = format!(
        "{COMMON_MODULE_TYPED_NUMBER_PARAM}\n\
//- /test.bsl\n\
Процедура Тест()\n\
    А = ПервыйОбщийМодуль.Привет(42);\n\
КонецПроцедуры\n"
    );
    let (db, file_id) = setup(&fixture);
    assert!(mismatches(&db, file_id).is_empty(), "matching arg must not produce any TypeMismatch");
}

#[test]
fn type_mismatch_silent_when_param_type_is_unknown() {
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
Функция Привет(П) Экспорт
    Возврат П;
КонецФункции

//- /test.bsl
Процедура Тест()
    А = ПервыйОбщийМодуль.Привет("строка");
    Б = ПервыйОбщийМодуль.Привет(42);
    В = ПервыйОбщийМодуль.Привет(Истина);
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        mismatches(&db, file_id).is_empty(),
        "untyped param (Unknown) must accept any arg type — gradual bottom rule"
    );
}

#[test]
fn type_mismatch_silent_when_arg_type_is_unknown() {
    let fixture = format!(
        "{COMMON_MODULE_TYPED_NUMBER_PARAM}\n\
//- /test.bsl\n\
Процедура Тест()\n\
    Х = Опаковать();\n\
    А = ПервыйОбщийМодуль.Привет(Х);\n\
КонецПроцедуры\n"
    );
    let (db, file_id) = setup(&fixture);
    assert!(
        mismatches(&db, file_id).is_empty(),
        "Unknown arg must not trigger TypeMismatch — gradual top rule"
    );
}

#[test]
fn type_mismatch_respects_null_to_ref_rule() {
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Параметры:
//   Ссылка - СправочникСсылка.Справочник1 - ссылка
Процедура СохранитьСсылку(Ссылка) Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    ПервыйОбщийМодуль.СохранитьСсылку(NULL);
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        mismatches(&db, file_id).is_empty(),
        "Null arg to a ref-typed param must not fire TypeMismatch — matches `Null ≤ ref-type` subtype rule"
    );
}

#[test]
fn type_mismatch_fires_on_three_level_manager_call() {
    let fixture = r#"
//- /Documents/ПКО/Ext/ManagerModule.bsl
// Параметры:
//   Код - Число - первый
Функция ПолучитьСсылку(Код) Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = Документы.ПКО.ПолучитьСсылку("not a number");
КонецПроцедуры
"#;
    let (db, file_id) = setup_vfs_only(fixture);
    assert_eq!(
        mismatches(&db, file_id),
        vec![(db.number(None, None), db.string(None, false))],
        "3-level manager call must emit TypeMismatch through infer_three_level_call"
    );
}

#[test]
fn type_mismatch_fires_on_fluent_method_call() {
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    ТЗ = Новый ТаблицаЗначений();
    ТЗ.Вставить("не число");
КонецПроцедуры
"#;
    let (db, file_id) = setup_vfs_only(fixture);
    assert_eq!(
        mismatches(&db, file_id),
        vec![(db.number(None, None), db.string(None, false))],
        "fluent method call with wrong-typed arg must fire TypeMismatch via Expr::Field callee branch"
    );
}

#[test]
fn type_mismatch_silent_on_fluent_method_call_matching_arg() {
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    ТЗ = Новый ТаблицаЗначений();
    ТЗ.Вставить(0);
КонецПроцедуры
"#;
    let (db, file_id) = setup_vfs_only(fixture);
    assert!(
        mismatches(&db, file_id).is_empty(),
        "fluent method call with matching arg must not produce any TypeMismatch"
    );
}

#[test]
fn type_mismatch_silent_on_union_receiver_when_one_arm_accepts() {
    // `Знач` is `Массив | Структура` at the call site. `Структура.Вставить`
    // accepts a String key even though `Массив.Вставить` wants a numeric index.
    // A union receiver is an over-approximation (at most one arm is the runtime
    // type), so an argument accepted by ANY arm must not fire.
    let fixture = r#"
//- /test.bsl
Процедура Тест(Условие)
    Если Условие Тогда
        Знач = Новый Массив;
    Иначе
        Знач = Новый Структура;
    КонецЕсли;
    Знач.Вставить("Ключ", 1);
КонецПроцедуры
"#;
    let (db, file_id) = setup_vfs_only(fixture);
    assert!(
        mismatches(&db, file_id).is_empty(),
        "union receiver Массив | Структура: a String key is valid via Структура.Вставить, \
         got {:?}",
        mismatches(&db, file_id)
    );
}

#[test]
fn type_mismatch_fires_on_array_insert_string_index() {
    // Guard against over-suppression: a non-union Массив receiver must still
    // flag a String passed where a numeric index is required.
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    М = Новый Массив;
    М.Вставить("Ключ", 1);
КонецПроцедуры
"#;
    let (db, file_id) = setup_vfs_only(fixture);
    assert_eq!(
        mismatches(&db, file_id),
        vec![(db.number(None, None), db.string(None, false))],
        "single Массив receiver must still fire on a String index"
    );
}

#[test]
fn type_mismatch_does_not_double_fire_on_arg_count_mismatch() {
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Параметры:
//   П - Число - первый
//   С - Строка - второй
Процедура Двойной(П, С) Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    ПервыйОбщийМодуль.Двойной(42);
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let infer_diags = db.infer(file_id).diagnostics.clone();
    let arg_diags = db.arg_diagnostics(file_id);

    let count_mismatches: Vec<_> = infer_diags
        .iter()
        .filter(|(_, d)| matches!(d, InferenceDiagnostic::MismatchedArgCount { .. }))
        .collect();
    assert_eq!(count_mismatches.len(), 1, "exactly one MismatchedArgCount expected");

    let type_mismatches: Vec<_> = infer_diags
        .iter()
        .chain(arg_diags.iter())
        .filter(|(_, d)| matches!(d, InferenceDiagnostic::TypeMismatch { .. }))
        .collect();
    assert!(
        type_mismatches.is_empty(),
        "no TypeMismatch must fire for the paired position or for the missing tail — \
         got {type_mismatches:?}"
    );
}

#[test]
fn type_mismatch_silent_on_coercion_to_string_param() {
    let fixture = format!(
        "{COMMON_MODULE_TYPED_NUMBER_PARAM}\n\
//- /CommonModules/ВторойОбщийМодуль/Ext/Module.bsl\n\
// Параметры:\n\
//   С - Строка - строковый аргумент\n\
Процедура Принимает(С) Экспорт\n\
КонецПроцедуры\n\
\n\
//- /test.bsl\n\
Процедура Тест()\n\
    ВторойОбщийМодуль.Принимает(42);\n\
    ВторойОбщийМодуль.Принимает(Истина);\n\
КонецПроцедуры\n"
    );
    let (db, file_id) = setup(&fixture);
    assert!(
        mismatches(&db, file_id).is_empty(),
        "non-String arg flowing into a String param must coerce silently — got {:?}",
        mismatches(&db, file_id)
    );
}

#[test]
fn type_mismatch_still_fires_on_string_to_number() {
    let fixture = format!(
        "{COMMON_MODULE_TYPED_NUMBER_PARAM}\n\
//- /test.bsl\n\
Процедура Тест()\n\
    ПервыйОбщийМодуль.Привет(\"не число\");\n\
КонецПроцедуры\n"
    );
    let (db, file_id) = setup(&fixture);
    assert_eq!(
        mismatches(&db, file_id),
        vec![(db.number(None, None), db.string(None, false))],
        "String arg flowing into a Number param must still fire — coercion is one-way"
    );
}

#[test]
fn type_mismatch_silent_on_any_ref_param() {
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Параметры:
//   Значение - Строка, ЛюбаяСсылка - строка или любая ссылка
Процедура Записать(Значение) Экспорт
КонецПроцедуры

// Возвращаемое значение:
//   СправочникСсылка.Справочник1 - ссылка на элемент
Функция ПолучитьСсылку() Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Процедура Тест()
    Ссылка = ПервыйОбщийМодуль.ПолучитьСсылку();
    ПервыйОбщийМодуль.Записать(Ссылка);
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let rendered: Vec<(String, String)> = mismatches(&db, file_id)
        .into_iter()
        .map(|(expected, actual)| {
            (
                hir::Type::from_id(&db, file_id, expected)
                    .display_name(ide_db::base_db::Locale::Ru),
                hir::Type::from_id(&db, file_id, actual).display_name(ide_db::base_db::Locale::Ru),
            )
        })
        .collect();
    assert!(
        rendered.is_empty(),
        "concrete `СправочникСсылка.X` is a subtype of `ЛюбаяСсылка` — passing it \
         into a `Строка | ЛюбаяСсылка` param must not fire TypeMismatch; got {rendered:?}"
    );
}

#[test]
fn type_mismatch_still_fires_on_number_to_any_ref_param() {
    let fixture = r#"
//- /CommonModules/ПервыйОбщийМодуль/Ext/Module.bsl
// Параметры:
//   Значение - ЛюбаяСсылка - любая ссылка
Процедура Записать(Значение) Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    ПервыйОбщийМодуль.Записать(42);
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        mismatches(&db, file_id),
        vec![(db.any_ref(), db.number(None, None))],
        "Число into a `ЛюбаяСсылка` param must still fire — AnyRef is a ref supertype, not gradual"
    );
}

#[test]
fn bare_identifier_matching_global_function_is_not_function_typed() {
    let fixture = r#"
//- /test.bsl
Функция ИнформационнаяБазаФайловая(Знач СтрокаСоединенияИнформационнойБазы = "") Экспорт
    Если ПустаяСтрока(СтрокаСоединенияИнформационнойБазы) Тогда
        Возврат Истина;
    КонецЕсли;
    Возврат Ложь;
КонецФункции
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        mismatches(&db, file_id).is_empty(),
        "bare identifier shadowing a platform global must not produce TypeMismatch — got {:?}",
        mismatches(&db, file_id)
    );
}
