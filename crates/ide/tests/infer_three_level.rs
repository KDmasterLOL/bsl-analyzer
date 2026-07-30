use std::sync::Arc;

use hir::{Builders, HirDatabase, TypeKernelDb, TypeKind, UnresolvedMethodKind};
use ide_db::base_db::SourceDatabase;

#[path = "infer_three_level/support.rs"]
mod support;

use support::{
    mismatched_arg_counts, setup, setup_with_designer_config, unresolved_fields, unresolved_kinds,
};

const MANAGER_FIXTURE: &str = r#"
//- /Documents/ПКО/Ext/ManagerModule.bsl
Функция ПолучитьСсылку(Код, Имя) Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = Документы.ПКО.ПолучитьСсылку("001", "Первый");
КонецПроцедуры
"#;

#[test]
fn three_level_call_resolves_against_manager_module() {
    let (db, file_id) = setup(MANAGER_FIXTURE);
    assert!(
        unresolved_kinds(&db, file_id).is_empty(),
        "three-level call must resolve cleanly, got {:?}",
        unresolved_kinds(&db, file_id)
    );
    assert!(
        mismatched_arg_counts(&db, file_id).is_empty(),
        "call passes 2 args to a 2-param method, arity must match; got {:?}",
        mismatched_arg_counts(&db, file_id)
    );
}

#[test]
fn three_level_arity_mismatch_emits_diagnostic() {
    let fixture = r#"
//- /Documents/ПКО/Ext/ManagerModule.bsl
Функция ПолучитьСсылку(Код, Имя) Экспорт
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = Документы.ПКО.ПолучитьСсылку();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        mismatched_arg_counts(&db, file_id),
        vec![(2, 2, 0)],
        "expected a single MismatchedArgCount(required=2, total=2, found=0) for the 3-level call"
    );
    assert!(
        unresolved_kinds(&db, file_id).is_empty(),
        "method exists — UnresolvedMethodCall must not be emitted"
    );
}

/// Промах по объекту — это промах СРЕДНЕГО звена, и диагностика теперь называет
/// именно его. Пока цепочка сворачивалась в один узел, сказать было нечего кроме
/// «метод не найден» на всём вызове: узел не различал, какое из трёх звеньев
/// подвело.
#[test]
fn three_level_missing_mdo_reports_the_middle_segment() {
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Результат = Документы.НетТакогоДокумента.ПолучитьСсылку();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        unresolved_fields(&db, file_id),
        vec!["НетТакогоДокумента".to_string()],
        "the collection has no such member — that is the defect, and its place is the middle"
    );
    assert!(
        unresolved_kinds(&db, file_id).is_empty(),
        "and the method is not the defect: got {:?}",
        unresolved_kinds(&db, file_id)
    );
    assert!(
        mismatched_arg_counts(&db, file_id).is_empty(),
        "resolution failed before arity check; got {:?}",
        mismatched_arg_counts(&db, file_id)
    );
}

/// Принятая потеря, названная прямо. `ОбщиеМодули` — узнаваемый plural, но
/// менеджерной коллекции за ним нет (`manager_type_prefix` пуст), поэтому корень
/// цепочки не становится `ManagerCollection` и промах не сообщается ничем. До
/// снятия свёртки форма давала `UnresolvedMethodCall{MethodNotFound}`.
///
/// Потеря принята потому, что форма в BSL бессмысленна: общий модуль зовут
/// `ОбщийМодуль.Метод()`, а не через коллекцию с объектом.
#[test]
fn common_modules_chain_is_no_longer_diagnosed() {
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    ОбщиеМодули.Утилиты.Метод();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(
        db.infer(file_id).diagnostics.is_empty(),
        "declared loss: got {:?}",
        db.infer(file_id).diagnostics
    );
}

/// Предсуществующая дыра, а не следствие снятия свёртки: нераспознанный plural не
/// сворачивался и раньше, поэтому цепочка молчала и до, и после.
#[test]
fn an_unknown_plural_chain_stays_silent() {
    let fixture = r#"
//- /test.bsl
Процедура Тест()
    Неизвестные.Х.Метод();
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert!(db.infer(file_id).diagnostics.is_empty(), "got {:?}", db.infer(file_id).diagnostics);
}

#[test]
fn config_less_collection_member_promotes_to_object_manager() {
    let fixture = r#"
//- /Documents/ПКО/Ext/ManagerModule.bsl
Процедура Метод() Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    М = Документы.ПКО;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    // An unknown type is not recorded at all, so a missing key means the promotion failed.
    let ty = db.infer(file_id).var_types.get("м").copied();
    let shape = ty.map(|ty| format!("{:?}", db.lookup_type(ty)));
    assert!(
        ty.is_some_and(|ty| matches!(db.lookup_type(ty), TypeKind::ObjectManager(_))),
        "without a visible config the manager module found by path proves the object exists, \
         exactly as locate_manager_module already decides; got {shape:?}"
    );
}

#[test]
fn visible_config_outranks_a_manager_module_on_disk() {
    let fixture = r#"
//- /Documents/ПКО/Ext/ManagerModule.bsl
Процедура Метод() Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    М = Документы.ПКО;
КонецПроцедуры
"#;
    let (mut db, file_id) = setup(fixture);
    assert!(db.infer(file_id).var_types.contains_key("м"), "config-less baseline must promote");

    db.set_all_config_paths(vec![(None, std::path::PathBuf::from("/does-not-exist"))]);
    let ty = db.infer(file_id).var_types.get("м").copied();
    let shape = ty.map(|ty| format!("{:?}", db.lookup_type(ty)));
    assert!(
        ty.is_none(),
        "with configs visible they alone declare what exists — a module file must not \
         resurrect an undeclared object; got {shape:?}"
    );
}

#[test]
fn self_qualified_call_in_report_manager_still_checks_the_method() {
    let fixture = r#"
//- /Reports/ТестовыйОтчёт/Ext/ManagerModule.bsl
Процедура Тест()
    ТестовыйОтчёт.НетТакогоМетода();
КонецПроцедуры
"#;
    let (db, file_id) =
        setup_with_designer_config(fixture, "/Reports/ТестовыйОтчёт/Ext/ManagerModule.bsl");
    assert_eq!(
        unresolved_kinds(&db, file_id),
        vec![UnresolvedMethodKind::MethodNotFound],
        "a self-qualified call is re-resolved as the collection-qualified one, so a \
         misspelled method keeps its diagnostic"
    );
}

#[test]
fn self_qualified_call_in_constant_manager_still_checks_the_method() {
    let fixture = r#"
//- /Constants/СтрокаКонст/Ext/ManagerModule.bsl
Процедура Тест()
    СтрокаКонст.НетТакогоМетода();
КонецПроцедуры
"#;
    let (db, file_id) =
        setup_with_designer_config(fixture, "/Constants/СтрокаКонст/Ext/ManagerModule.bsl");
    assert_eq!(
        unresolved_kinds(&db, file_id),
        vec![UnresolvedMethodKind::MethodNotFound],
        "constants own a manager module like any other manager-backed kind — the \
         self-qualified call must be checked the same way"
    );
}

#[test]
fn config_less_object_named_like_a_collection_promotes() {
    let fixture = r#"
//- /Documents/Constants/Ext/ManagerModule.bsl
Процедура Метод() Экспорт
КонецПроцедуры

//- /test.bsl
Процедура Тест()
    М = Документы.Constants;
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    let ty = db.infer(file_id).var_types.get("м").copied();
    let shape = ty.map(|ty| format!("{:?}", db.lookup_type(ty)));
    assert!(
        ty.is_some_and(|ty| matches!(db.lookup_type(ty), TypeKind::ObjectManager(_))),
        "an object may be named after a collection; the index must still find its \
         manager module by position; got {shape:?}"
    );
}

#[test]
fn three_level_non_exported_method_emits_method_not_export() {
    let fixture = r#"
//- /Documents/ПКО/Ext/ManagerModule.bsl
Функция ПолучитьСсылку(Код, Имя)
    Возврат Неопределено;
КонецФункции

//- /test.bsl
Процедура Тест()
    Результат = Документы.ПКО.ПолучитьСсылку("001", "Первый");
КонецПроцедуры
"#;
    let (db, file_id) = setup(fixture);
    assert_eq!(
        unresolved_kinds(&db, file_id),
        vec![UnresolvedMethodKind::MethodNotExport],
        "non-exported method must emit MethodNotExport, not MethodNotFound"
    );
    assert!(
        mismatched_arg_counts(&db, file_id).is_empty(),
        "non-exported is a visibility issue, not an arity issue"
    );
}

#[test]
fn three_level_invalidates_on_config_change() {
    let (mut db, file_id) = setup(MANAGER_FIXTURE);

    assert!(unresolved_kinds(&db, file_id).is_empty(), "baseline must be clean");

    db.set_all_config_paths(vec![(None, std::path::PathBuf::from("/does-not-exist"))]);
    assert_eq!(
        unresolved_fields(&db, file_id),
        vec!["ПКО".to_string()],
        "bogus config must hide the MDO, and the miss lands on the object segment"
    );

    db.set_all_config_paths(vec![]);
    assert!(
        unresolved_kinds(&db, file_id).is_empty(),
        "clearing configs must restore the baseline — invalidation fires on input removal"
    );
}

#[test]
fn three_level_candidate_invalidation() {
    const FIXTURE: &str = r#"
//- /valid.bsl
Процедура Тест()
    Отбор = Новый Структура;
    ДатаОтбора = ТекущаяДата();
    Результат = РегистрыСведений.РегистрСведений1.Выбрать(Отбор);
КонецПроцедуры
"#;
    const AFTER: &str = r#"Процедура Тест()
    Отбор = Новый Структура;
    ДатаОтбора = ТекущаяДата();
    Результат = РегистрыСведений.РегистрСведений1.Выбрать(ДатаОтбора);
КонецПроцедуры
"#;

    let (mut db, file_id) = setup_with_designer_config(FIXTURE, "/valid.bsl");
    let db_address = std::ptr::from_ref(&db);
    let before_text = db.file_text(file_id);
    assert!(before_text.contains("Выбрать(Отбор)"));
    let before = db.infer(file_id);
    let before_binding = before
        .call_arg_bindings
        .iter()
        .find(|binding| {
            let candidates = binding.candidate.candidates.as_slice();
            candidates.len() == 3 && candidates.iter().all(|signature| signature.id.is_platform())
        })
        .expect("three-level call must have a candidate-backed binding");
    let before_candidate = &before_binding.candidate;
    let before_selected = before_candidate
        .resolution
        .unique_candidate()
        .expect("structure argument must select one platform candidate");
    assert!(before_selected.is_platform());
    assert_eq!(before_candidate.candidates.as_slice().len(), 3);
    let before_result = *before.var_types.get("результат").expect("result must be inferred");
    assert_eq!(before_candidate.resolution.return_ty, before_result);
    assert!(!matches!(db.lookup_type(before_result), TypeKind::Unknown));
    let before_arg_ty = before
        .type_id_of_expr_in(before_binding.owner, before_binding.args[0])
        .expect("argument type must feed diagnostics");
    assert_eq!(before_arg_ty, db.structure(None));
    assert!(before_candidate.resolution.is_survivor(before_selected));

    db.set_file_text(file_id, AFTER);

    let after_text = db.file_text(file_id);
    assert_ne!(after_text, before_text);
    assert!(after_text.contains("Выбрать(ДатаОтбора)"));
    let after = db.infer(file_id);
    assert_eq!(std::ptr::from_ref(&db), db_address, "the same database instance must be reused");
    assert!(!Arc::ptr_eq(&before, &after), "the source edit must recompute inference");
    let after_binding = after
        .call_arg_bindings
        .iter()
        .find(|binding| {
            let candidates = binding.candidate.candidates.as_slice();
            candidates.len() == 3 && candidates.iter().all(|signature| signature.id.is_platform())
        })
        .expect("edited three-level call must remain candidate-backed");
    assert_eq!(after_binding.call_expr, before_binding.call_expr);
    let after_candidate = &after_binding.candidate;
    let after_selected = after_candidate
        .resolution
        .unique_candidate()
        .expect("date argument must select one platform candidate");
    assert_ne!(after_selected, before_selected);
    assert_eq!(
        after_candidate
            .candidates
            .as_slice()
            .iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>(),
        before_candidate
            .candidates
            .as_slice()
            .iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>(),
        "the complete stable candidate set must survive the edit"
    );
    let after_result = *after.var_types.get("результат").expect("result must be inferred");
    assert_eq!(after_candidate.resolution.return_ty, after_result);
    assert_eq!(after_result, before_result);
    let after_arg_ty = after
        .type_id_of_expr_in(after_binding.owner, after_binding.args[0])
        .expect("edited argument type must feed diagnostics");
    assert!(matches!(db.lookup_type(after_arg_ty), TypeKind::Date(_)));
    assert_ne!(after_arg_ty, before_arg_ty);
    assert!(!after_candidate.resolution.is_survivor(before_selected));
    assert!(after_candidate.resolution.is_survivor(after_selected));
}
