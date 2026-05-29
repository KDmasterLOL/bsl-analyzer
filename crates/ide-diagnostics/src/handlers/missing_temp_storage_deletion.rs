use hir::cfg::CfgBuilder;
use hir::dataflow::temp_resource::{analyze_open_resources, ResourceEvent, ResourceProvider};
use hir::{Body, BodySourceMap, Expr, ExprId, ExprIdx, IdConversion};

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 3,
    activated_by_default: false,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Performance, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::MissingTempStorageDeletion;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let module_bodies = ctx.module_bodies();
    let module_cfgs = ctx.module_cfgs();
    let mut diagnostics = Vec::new();

    for (local_id, body) in module_bodies.iter_bodies() {
        let Some(source_map) = module_bodies.source_map(local_id) else { continue };
        let Some(cfg) = module_cfgs.get(local_id) else { continue };
        diagnostics.extend(check_body(body, source_map, cfg.as_ref(), code, ctx));
    }

    if let Some(module_result) = module_bodies.module_code_result() {
        let body = &module_result.body;
        let source_map = &module_result.source_map;
        let cfg =
            CfgBuilder::new().build_graph_from_hir(body.body_stmts_typed(), body, Some(source_map));
        diagnostics.extend(check_body(body, source_map, &cfg, code, ctx));
    }

    diagnostics.sort_by_key(|d| d.range.start());
    diagnostics
}

fn check_body(
    body: &Body,
    source_map: &BodySourceMap,
    cfg: &hir::cfg::ControlFlowGraph,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Vec<Diagnostic> {
    let Some(result) = analyze_open_resources::<_, CanonExpr>(body, cfg, TempStorageProvider)
    else {
        return Vec::new();
    };

    let mut ranges: Vec<ide_db::TextRange> = Vec::new();
    for sites in result.open_at_exit().values() {
        for &expr_idx in sites {
            if let Some(range) = source_map.expr_range(ExprId::from_idx(expr_idx)) {
                ranges.push(range);
            }
        }
    }

    ranges.sort_by_key(|r| r.start());
    ranges.dedup();

    ranges
        .into_iter()
        .map(|range| Diagnostic {
            code,
            message: "Нужно добавить удаление данных из временного хранилища после использования, вызвав \"УдалитьИзВременногоХранилища\"".to_string(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        })
        .collect()
}

struct TempStorageProvider;

impl ResourceProvider<CanonExpr> for TempStorageProvider {
    fn classify(&self, body: &Body, expr_idx: ExprIdx) -> Option<ResourceEvent<CanonExpr>> {
        let Expr::Call { callee, args } = body.expr_idx(expr_idx) else {
            return None;
        };
        let Expr::Path(name) = body.expr_idx(*callee) else {
            return None;
        };
        let name_str = name.as_str();
        let is_open = is_get_from_temp_storage(name_str);
        let is_close = is_delete_from_temp_storage(name_str);
        if !is_open && !is_close {
            return None;
        }
        let &first_arg = args.first()?;
        let canon =
            canonicalize_expr(body, first_arg).unwrap_or_else(|| format!("?:{:?}", first_arg));
        if is_open {
            Some(ResourceEvent::Open(canon))
        } else {
            Some(ResourceEvent::Close(canon))
        }
    }
}

type CanonExpr = String;

fn canonicalize_expr(body: &Body, expr_idx: ExprIdx) -> Option<CanonExpr> {
    match body.expr_idx(expr_idx) {
        Expr::Path(name) => Some(format!("p:{}", name.as_str().to_lowercase())),
        Expr::Field { base, field } => {
            let base_canon = canonicalize_expr(body, *base)?;
            Some(format!("f:{}.{}", base_canon, field.as_str().to_lowercase()))
        }
        Expr::Index { base, index } => {
            let base_canon = canonicalize_expr(body, *base)?;
            let index_canon = canonicalize_expr(body, *index)?;
            Some(format!("i:{}[{}]", base_canon, index_canon))
        }
        Expr::Call { callee, args } => {
            let callee_canon = canonicalize_expr(body, *callee)?;
            let arg_canons =
                args.iter().map(|&a| canonicalize_expr(body, a)).collect::<Option<Vec<_>>>()?;
            Some(format!("c:{}({})", callee_canon, arg_canons.join(",")))
        }
        Expr::MethodCall { receiver, method, args } => {
            let receiver_canon = canonicalize_expr(body, *receiver)?;
            let arg_canons =
                args.iter().map(|&a| canonicalize_expr(body, a)).collect::<Option<Vec<_>>>()?;
            Some(format!(
                "m:{}.{}({})",
                receiver_canon,
                method.as_str().to_lowercase(),
                arg_canons.join(",")
            ))
        }
        Expr::Literal(lit) => Some(format!("l:{:?}", lit)),
        Expr::Missing
        | Expr::QualifiedPath(_)
        | Expr::BinaryOp { .. }
        | Expr::UnaryOp { .. }
        | Expr::Ternary { .. }
        | Expr::New { .. }
        | Expr::Array(_)
        | Expr::Await { .. } => None,
    }
}

fn is_get_from_temp_storage(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "получитьизвременногохранилища" || lower == "getfromtempstorage"
}

fn is_delete_from_temp_storage(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "удалитьизвременногохранилища" || lower == "deletefromtempstorage"
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::*;
    use crate::DiagnosticsConfig;
    use expect_test::expect;

    #[test]
    fn test_missing_temp_storage_deletion() {
        let code = "&НаСервере\nПроцедура ПолучитьТоварыИзХранилища_Ошибка(АдресТоваровВХранилище)\n\n    ПодобранныеТовары = ПолучитьИзВременногоХранилища(АдресТоваровВХранилище); // ошибка\n    Объект.Товары.Загрузить(ПодобранныеТовары);\n\nКонецПроцедуры \n\n&НаСервере\nПроцедура ПолучитьТоварыИзХранилища_Ошибка2(АдресТоваровВХранилище)\n\n    УдалитьИзВременногоХранилища(АдресТоваровВХранилище);\n\n    ПодобранныеТовары = ПолучитьИзВременногоХранилища(АдресТоваровВХранилище); // ошибка\n    Объект.Товары.Загрузить(ПодобранныеТовары);\n\nКонецПроцедуры\n\n&НаСервере\nПроцедура ПолучитьТоварыИзХранилища_Ошибка3(АдресТоваровВХранилище)\n\n    ПодобранныеТовары = ПолучитьИзВременногоХранилища(АдресТоваровВХранилище); //ошибка\n    Объект.Товары.Загрузить(ПодобранныеТовары);\n\n    Если Истина Тогда\n        УдалитьИзВременногоХранилища(ДругойАдрес);\n    КонецЕсли;\n\nКонецПроцедуры\n\n&НаСервере\nПроцедура ЕдинственныйВызовВМетоде(АдресТоваровВХранилище)\n\n    ПодобранныеТовары = ПолучитьИзВременногоХранилища(АдресТоваровВХранилище); //ошибка\n\nКонецПроцедуры\n\n&НаСервере\nПроцедура ПолучитьТоварыИзХранилища_Успешно()\n\n    Адрес = \"\";\n    ОбщийМодуль.ПолучитьАдрес(Адрес);\n\n    ПодобранныеТовары = ПолучитьИзВременногоХранилища(Адрес); // не ошибка\n    Результат = ПодобранныеТовары.ВыгрузитьКолонку(\"Наименование\");\n\n    УдалитьИзВременногоХранилища(Адрес);\n\nКонецПроцедуры\n\n&НаСервере\nПроцедура ПолучитьТоварыИзХранилища_Успешно1()\n\n    Адрес = \"\";\n    Попытка\n        ОбщийМодуль.ПолучитьАдрес(Адрес);\n\n        ПодобранныеТовары = ПолучитьИзВременногоХранилища(Адрес); // не ошибка\n        Результат = ПодобранныеТовары.ВыгрузитьКолонку(\"Наименование\");\n\n        УдалитьИзВременногоХранилища(Адрес);\n    Исключение\n    КонецПопытки;\n\nКонецПроцедуры\n\n&НаСервере\nПроцедура ПолучитьТоварыИзХранилища_Успешно2(АдресТоваровВХранилище)\n\n    Если Истина Тогда\n\t    ПодобранныеТовары = ПолучитьИзВременногоХранилища(АдресТоваровВХранилище); // не ошибка\n        Объект.Товары.Загрузить(ПодобранныеТовары);\n    КонецЕсли;\n\n    УдалитьИзВременногоХранилища(АдресТоваровВХранилище);\n\nКонецПроцедуры\n\n&НаСервере\nПроцедура ПолучитьТоварыИзХранилища_Успешно3(АдресТоваровВХранилище)\n\n    Если Истина Тогда\n\t    ПодобранныеТовары = ПолучитьИзВременногоХранилища(АдресТоваровВХранилище); // не ошибка\n        Объект.Товары.Загрузить(ПодобранныеТовары);\n    КонецЕсли;\n\n    Если Истина Тогда\n        УдалитьИзВременногоХранилища(АдресТоваровВХранилище);\n    КонецЕсли;\n\nКонецПроцедуры\n\n&НаКлиенте\nПроцедура ПриЗавершенииПоискаНастроек(Результат, ДополнительныеПараметры) Экспорт\n\n\tЕсли Результат = Неопределено Тогда // Пользователь отменил задание.\n\t\tВозврат;\n\tКонецЕсли;\n\n\tЕсли Результат.Статус = \"Ошибка\" Тогда\n\t\tВызватьИсключение Результат.КраткоеПредставлениеОшибки;\n\tКонецЕсли;\n\n\tНастройки = ПолучитьИзВременногоХранилища(Результат.АдресРезультата); // не ошибка\n\tУдалитьИзВременногоХранилища(Результат.АдресРезультата);\n\tУстановитьНастройкиУчетнойЗаписи(Настройки);\n\nКонецПроцедуры\n";
        let config = DiagnosticsConfig::all_enabled();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        expect![[r#"
            MissingTempStorageDeletion @ 4:25..4:78
              message: Нужно добавить удаление данных из временного хранилища после использования, вызвав "УдалитьИзВременногоХранилища"
              severity: Warning
            MissingTempStorageDeletion @ 14:25..14:78
              message: Нужно добавить удаление данных из временного хранилища после использования, вызвав "УдалитьИзВременногоХранилища"
              severity: Warning
            MissingTempStorageDeletion @ 22:25..22:78
              message: Нужно добавить удаление данных из временного хранилища после использования, вызвав "УдалитьИзВременногоХранилища"
              severity: Warning
            MissingTempStorageDeletion @ 34:25..34:78
              message: Нужно добавить удаление данных из временного хранилища после использования, вызвав "УдалитьИзВременногоХранилища"
              severity: Warning
            MissingTempStorageDeletion @ 83:26..83:79
              message: Нужно добавить удаление данных из временного хранилища после использования, вызвав "УдалитьИзВременногоХранилища"
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_structural_equality() {
        let code = r#"
Процедура Тест()
    Настройки = ПолучитьИзВременногоХранилища(Результат.АдресРезультата);
    УдалитьИзВременногоХранилища(Результат.АдресРезультата);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_different_parameters() {
        let code = r#"
Процедура Тест()
    Данные = ПолучитьИзВременногоХранилища(АдресТоваров);
    УдалитьИзВременногоХранилища(ДругойАдрес);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MissingTempStorageDeletion @ 3:14..3:57
              message: Нужно добавить удаление данных из временного хранилища после использования, вызвав "УдалитьИзВременногоХранилища"
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_bilingual() {
        let code = r#"
Procedure Test()
    Data = GetFromTempStorage(Address);
    DeleteFromTempStorage(Address);
EndProcedure
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_simple_valid_case() {
        let code = r#"
Процедура Тест()
    Адрес = "";
    Данные = ПолучитьИзВременногоХранилища(Адрес);
    ОбработатьДанные(Данные);
    УдалитьИзВременногоХранилища(Адрес);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_simple_invalid_case() {
        let code = r#"
Процедура Тест()
    Адрес = "";
    Данные = ПолучитьИзВременногоХранилища(Адрес);
    ОбработатьДанные(Данные);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MissingTempStorageDeletion @ 4:14..4:50
              message: Нужно добавить удаление данных из временного хранилища после использования, вызвав "УдалитьИзВременногоХранилища"
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_delete_before_get() {
        let code = r#"
Процедура Тест()
    Адрес = "";
    УдалитьИзВременногоХранилища(Адрес);
    Данные = ПолучитьИзВременногоХранилища(Адрес);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MissingTempStorageDeletion @ 5:14..5:50
              message: Нужно добавить удаление данных из временного хранилища после использования, вызвав "УдалитьИзВременногоХранилища"
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Адрес = "";
    Данные = ПОЛУЧИТЬИЗВРЕМЕННОГОХРАНИЛИЩА(адрес);
    ОбработатьДанные(Данные);
    удалитьизвременногохранилища(АДРЕС);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Should handle case-insensitive method names and parameters"
        );
    }

    #[test]
    fn test_get_in_branch_delete_in_separate_branch_leaks() {
        let code = r#"
Процедура Тест(Адрес, Условие1, Условие2)
    Если Условие1 Тогда
        Данные = ПолучитьИзВременногоХранилища(Адрес);
        ОбработатьДанные(Данные);
    КонецЕсли;

    Если Условие2 Тогда
        УдалитьИзВременногоХранилища(Адрес);
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MissingTempStorageDeletion @ 4:18..4:54
              message: Нужно добавить удаление данных из временного хранилища после использования, вызвав "УдалитьИзВременногоХранилища"
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_get_then_return_then_delete_leaks() {
        let code = r#"
Процедура Тест(Адрес)
    Данные = ПолучитьИзВременногоХранилища(Адрес);
    ОбработатьДанные(Данные);
    Возврат;
    УдалитьИзВременногоХранилища(Адрес);
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MissingTempStorageDeletion @ 3:14..3:50
              message: Нужно добавить удаление данных из временного хранилища после использования, вызвав "УдалитьИзВременногоХранилища"
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_nested_get_inside_outer_call_leaks() {
        let code = r#"
Процедура Тест(Адрес)
    СохранитьВ(ПолучитьИзВременногоХранилища(Адрес));
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MissingTempStorageDeletion @ 3:16..3:52
              message: Нужно добавить удаление данных из временного хранилища после использования, вызвав "УдалитьИзВременногоХранилища"
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_get_inside_if_condition_leaks() {
        let code = r#"
Процедура Тест(Адрес)
    Если ПолучитьИзВременногоХранилища(Адрес) <> Неопределено Тогда
        ВыполнитьДействие();
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MissingTempStorageDeletion @ 3:10..3:46
              message: Нужно добавить удаление данных из временного хранилища после использования, вызвав "УдалитьИзВременногоХранилища"
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_get_with_unsupported_arg_shape_leaks() {
        let code = r#"
Процедура Тест(Условие, А, Б)
    Данные = ПолучитьИзВременногоХранилища(?(Условие, А, Б));
    ОбработатьДанные(Данные);
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#"
            MissingTempStorageDeletion @ 3:14..3:61
              message: Нужно добавить удаление данных из временного хранилища после использования, вызвав "УдалитьИзВременногоХранилища"
              severity: Warning"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_get_with_call_arg_pairs_with_matching_delete() {
        let code = r#"
Процедура Тест()
    Данные = ПолучитьИзВременногоХранилища(ПолучитьАдрес());
    ОбработатьДанные(Данные);
    УдалитьИзВременногоХранилища(ПолучитьАдрес());
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    #[ignore = "requires constant-folding of `Если Истина Тогда` guards (Plan §7 risk #3, deferred track)"]
    fn test_constant_true_guarded_cleanup_no_false_positive() {
        let code = r#"
Процедура Тест(Адрес)
    Если Истина Тогда
        Данные = ПолучитьИзВременногоХранилища(Адрес);
        ОбработатьДанные(Данные);
    КонецЕсли;
    Если Истина Тогда
        УдалитьИзВременногоХранилища(Адрес);
    КонецЕсли;
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "constant-true guards must be folded — both branches always execute at runtime, \
             so the Delete is unconditionally reachable from the Get",
        );
    }
}
