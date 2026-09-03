use crate::define_metadata;
use crate::metadata::*;
use crate::{BodyContext, Diagnostic, DiagnosticCode};
use hir::dataflow::security_state::{open_events, Category};
use hir::BodySourceMap;
use hir::LocalRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::SecurityHotspot,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check_body(ctx: &BodyContext, acc: &mut Vec<Diagnostic<LocalRange>>) {
    let code = DiagnosticCode::SetPrivilegedMode;
    if ctx.is_disabled_with_metadata(code) {
        return;
    }
    let Some(result) = ctx.security_state() else {
        return;
    };
    emit_for_result(&result, ctx.body(), ctx.source_map(), code, ctx, acc);
}

fn emit_for_result(
    result: &hir::dataflow::DataflowResult<hir::dataflow::security_state::SecurityModeState>,
    body: &hir::Body,
    source_map: &BodySourceMap,
    code: DiagnosticCode,
    ctx: &BodyContext,
    out: &mut Vec<Diagnostic<LocalRange>>,
) {
    for event in open_events(result, body) {
        if !matches!(event.category, Category::PrivilegedMode) {
            continue;
        }
        let Some(range) = source_map.expr_range(event.callee) else { continue };
        out.push(Diagnostic {
            code,
            message: "Проверьте установку привилегированного режима".to_string(),
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::check_diagnostics_snapshot_for;
    use expect_test::expect;
    #[test]
    fn test_from_java_fixture() {
        let code = r#"&НаСервере
Процедура Метод()
    УстановитьПривилегированныйРежим(Истина); // есть замечание
    Значение = Истина;
    УстановитьПривилегированныйРежим(Значение); // есть замечание

    УстановитьПривилегированныйРежим(Ложь); // нет замечания
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SetPrivilegedMode,
            expect![[r#"
                SetPrivilegedMode @ 3:5..3:37
                  message: Проверьте установку привилегированного режима
                  severity: Warning
                SetPrivilegedMode @ 5:5..5:37
                  message: Проверьте установку привилегированного режима
                  severity: Warning"#]],
        );
    }

    #[test]
    fn test_module_level_call_emits() {
        let code = r#"УстановитьПривилегированныйРежим(Истина);
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::SetPrivilegedMode,
            expect![[r#"
                SetPrivilegedMode @ 1:1..1:33
                  message: Проверьте установку привилегированного режима
                  severity: Warning"#]],
        );
    }
}
