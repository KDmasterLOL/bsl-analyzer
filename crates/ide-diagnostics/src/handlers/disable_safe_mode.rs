use std::sync::Arc;

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::dataflow::security_state::{open_events, Category};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Vulnerability,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::DisableSafeMode;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let module_security: Arc<ide_db::effects::ModuleSecurityState> = ctx.module_security_state();
    if module_security.is_empty() {
        return Vec::new();
    }
    let module_bodies = ctx.module_bodies();

    let mut diagnostics = Vec::new();
    for (local_id, _body) in module_bodies.iter_bodies() {
        let Some(result) = module_security.get(local_id) else { continue };
        let Some(source_map) = module_bodies.source_map(local_id) else { continue };
        emit_for_result(&result, source_map, code, ctx, &mut diagnostics);
    }
    if let Some(result) = module_security.module_level() {
        if let Some(lower_result) = module_bodies.module_code_result() {
            emit_for_result(&result, &lower_result.source_map, code, ctx, &mut diagnostics);
        }
    }
    diagnostics.sort_by_key(|d| (d.range.start(), d.range.end()));
    diagnostics
}

fn emit_for_result(
    result: &hir::dataflow::DataflowResult<hir::dataflow::security_state::SecurityModeState>,
    source_map: &hir::BodySourceMap,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    out: &mut Vec<Diagnostic>,
) {
    let body = result.body();
    for event in open_events(result) {
        if !matches!(event.category, Category::SafeMode) {
            continue;
        }
        let Some(range) = source_map.expr_range(event.callee) else { continue };
        let method_name = match body.expr(event.callee) {
            hir::Expr::Path(name) => name.as_str(),
            _ => continue,
        };
        out.push(Diagnostic {
            code,
            message: get_message(method_name),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

fn get_message(method_name: &str) -> String {
    let lower = method_name.to_lowercase();
    match lower.as_str() {
        "установитьбезопасныйрежим" | "setsafemode" => {
            "Отключение безопасного режима создает уязвимость безопасности. \
             Используйте УстановитьБезопасныйРежим(Истина) / SetSafeMode(True)"
                .to_string()
        }
        "установитьотключениебезопасногорежима" | "setsafemodedisabled" => {
            "Отключение безопасного режима через УстановитьОтключениеБезопасногоРежима \
             создает уязвимость безопасности"
                .to_string()
        }
        _ => "Отключение безопасного режима создает уязвимость безопасности".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_set_safe_mode_false() {
        let code = r#"
Процедура Тест()
    УстановитьБезопасныйРежим(Ложь);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::DisableSafeMode,
            expect![[r#"
            DisableSafeMode @ 3:5..3:30
              message: Отключение безопасного режима создает уязвимость безопасности. Используйте УстановитьБезопасныйРежим(Истина) / SetSafeMode(True)
              severity: Major"#]],
        );
    }

    #[test]
    fn test_set_safe_mode_true() {
        let code = r#"
Процедура Тест()
    УстановитьБезопасныйРежим(Истина);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::DisableSafeMode, expect![[r#""#]]);
    }

    #[test]
    fn test_set_safe_mode_variable() {
        let code = r#"
Процедура Тест()
    Значение = Ложь;
    УстановитьБезопасныйРежим(Значение);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::DisableSafeMode,
            expect![[r#"
            DisableSafeMode @ 4:5..4:30
              message: Отключение безопасного режима создает уязвимость безопасности. Используйте УстановитьБезопасныйРежим(Истина) / SetSafeMode(True)
              severity: Major"#]],
        );
    }

    #[test]
    fn test_set_disabled_true() {
        let code = r#"
Процедура Тест()
    УстановитьОтключениеБезопасногоРежима(Истина);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::DisableSafeMode,
            expect![[r#"
            DisableSafeMode @ 3:5..3:42
              message: Отключение безопасного режима через УстановитьОтключениеБезопасногоРежима создает уязвимость безопасности
              severity: Major"#]],
        );
    }

    #[test]
    fn test_set_disabled_false() {
        let code = r#"
Процедура Тест()
    УстановитьОтключениеБезопасногоРежима(Ложь);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::DisableSafeMode, expect![[r#""#]]);
    }

    #[test]
    fn test_object_method_excluded() {
        let code = r#"
Процедура Тест()
    Модуль.УстановитьБезопасныйРежим(Ложь);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::DisableSafeMode, expect![[r#""#]]);
    }

    #[test]
    fn test_bilingual() {
        let code = r#"
Процедура Тест()
    SetSafeMode(False);
    SetSafeModeDisabled(True);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::DisableSafeMode,
            expect![[r#"
            DisableSafeMode @ 3:5..3:16
              message: Отключение безопасного режима создает уязвимость безопасности. Используйте УстановитьБезопасныйРежим(Истина) / SetSafeMode(True)
              severity: Major
            DisableSafeMode @ 4:5..4:24
              message: Отключение безопасного режима через УстановитьОтключениеБезопасногоРежима создает уязвимость безопасности
              severity: Major"#]],
        );
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    УСТАНОВИТЬБЕЗОПАСНЫЙРЕЖИМ(ЛОЖЬ);
    установитьбезопасныйрежим(ложь);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::DisableSafeMode,
            expect![[r#"
            DisableSafeMode @ 3:5..3:30
              message: Отключение безопасного режима создает уязвимость безопасности. Используйте УстановитьБезопасныйРежим(Истина) / SetSafeMode(True)
              severity: Major
            DisableSafeMode @ 4:5..4:30
              message: Отключение безопасного режима создает уязвимость безопасности. Используйте УстановитьБезопасныйРежим(Истина) / SetSafeMode(True)
              severity: Major"#]],
        );
    }

    #[test]
    fn test_call_in_if_condition_emits() {
        let code = r#"
Процедура Тест()
    Если УстановитьБезопасныйРежим(Ложь) Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::DisableSafeMode,
            expect![[r#"
            DisableSafeMode @ 3:10..3:35
              message: Отключение безопасного режима создает уязвимость безопасности. Используйте УстановитьБезопасныйРежим(Истина) / SetSafeMode(True)
              severity: Major"#]],
        );
    }

    #[test]
    fn test_nested_call_in_argument_emits() {
        let code = r#"
Процедура Тест()
    Сообщить(УстановитьБезопасныйРежим(Ложь));
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::DisableSafeMode,
            expect![[r#"
            DisableSafeMode @ 3:14..3:39
              message: Отключение безопасного режима создает уязвимость безопасности. Используйте УстановитьБезопасныйРежим(Истина) / SetSafeMode(True)
              severity: Major"#]],
        );
    }

    #[test]
    fn test_all_four_patterns_in_procedure() {
        let code = r#"&НаСервере
Процедура Метод()
    УстановитьБезопасныйРежим(Ложь);

    Значение = Ложь;
    УстановитьБезопасныйРежим(Значение);

    УстановитьБезопасныйРежим(Истина);

    УстановитьОтключениеБезопасногоРежима(Истина);

    Значение = Истина;
    УстановитьОтключениеБезопасногоРежима(Значение);

    УстановитьОтключениеБезопасногоРежима(Ложь);
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::DisableSafeMode,
            expect![[r#"
            DisableSafeMode @ 3:5..3:30
              message: Отключение безопасного режима создает уязвимость безопасности. Используйте УстановитьБезопасныйРежим(Истина) / SetSafeMode(True)
              severity: Major
            DisableSafeMode @ 6:5..6:30
              message: Отключение безопасного режима создает уязвимость безопасности. Используйте УстановитьБезопасныйРежим(Истина) / SetSafeMode(True)
              severity: Major
            DisableSafeMode @ 10:5..10:42
              message: Отключение безопасного режима через УстановитьОтключениеБезопасногоРежима создает уязвимость безопасности
              severity: Major
            DisableSafeMode @ 13:5..13:42
              message: Отключение безопасного режима через УстановитьОтключениеБезопасногоРежима создает уязвимость безопасности
              severity: Major"#]],
        );
    }
}
