use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::ReservedWordAsMethodName;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Имя \"{}\" является зарезервированным словом и не может использоваться как имя процедуры/функции",
            name
        ),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;

    #[test]
    fn test_procedure_with_reserved_word_execute() {
        let code = r#"Процедура Выполнить(Команда)
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ReservedWordAsMethodName,
            expect![[r#"
            ReservedWordAsMethodName @ 1:11..1:20
              message: Имя "Выполнить" является зарезервированным словом и не может использоваться как имя процедуры/функции
              severity: Blocker"#]],
        );
    }

    #[test]
    fn test_function_with_reserved_word_new() {
        let code = r#"Функция Новый()
    Возврат 1;
КонецФункции"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ReservedWordAsMethodName,
            expect![[r#"
            ReservedWordAsMethodName @ 1:9..1:14
              message: Имя "Новый" является зарезервированным словом и не может использоваться как имя процедуры/функции
              severity: Blocker"#]],
        );
    }

    #[test]
    fn test_procedure_with_reserved_word_if() {
        let code = r#"Процедура Если()
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ReservedWordAsMethodName,
            expect![[r#"
            ReservedWordAsMethodName @ 1:11..1:15
              message: Имя "Если" является зарезервированным словом и не может использоваться как имя процедуры/функции
              severity: Blocker"#]],
        );
    }

    #[test]
    fn test_procedure_with_reserved_word_english() {
        let code = r#"Procedure Execute(Command)
EndProcedure"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ReservedWordAsMethodName,
            expect![[r#"
            ReservedWordAsMethodName @ 1:11..1:18
              message: Имя "Execute" является зарезервированным словом и не может использоваться как имя процедуры/функции
              severity: Blocker"#]],
        );
    }

    #[test]
    fn test_normal_procedure_name_ok() {
        let code = r#"Процедура МояПроцедура()
КонецПроцедуры"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ReservedWordAsMethodName,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_normal_function_name_ok() {
        let code = r#"Функция ПолучитьЗначение()
    Возврат 1;
КонецФункции"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::ReservedWordAsMethodName,
            expect![[r#""#]],
        );
    }
}
