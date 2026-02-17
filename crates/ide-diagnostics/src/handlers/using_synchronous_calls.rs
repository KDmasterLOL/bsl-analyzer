//! UsingSynchronousCalls diagnostic
//!
//! Detects usage of synchronous (blocking) calls in client code.
//!
//! Synchronous calls are blocking and not compatible with web client.
//! Each synchronous method has an asynchronous replacement.
//!
//! ## Implementation
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! The diagnostic is emitted in `hir-def/body/lower/expr.rs` when a global
//! synchronous method call is encountered (not in server context).

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 15,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_3,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(
    method_name: &str,
    replacement: &str,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::UsingSynchronousCalls;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let message = format!(
        "Вместо синхронного вызова \"{}\" необходимо использовать \"{}\"",
        method_name, replacement
    );

    Some(Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    #[test]
    fn test_using_synchronous_calls() {
        let code = include_str!("../../test_data/UsingSynchronousCallsDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(code);

        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();

        assert_eq!(sync_diags.len(), 28, "Expected 28 synchronous call diagnostics");

        // Вопрос (multiline call)
        assert_diagnostic_range_multiline(code, sync_diags[0], 2, 12, 3, 57);
        // Предупреждение
        assert_diagnostic_range(code, sync_diags[1], 21, 4, 84);
        // ОткрытьЗначение
        assert_diagnostic_range(code, sync_diags[2], 29, 4, 26);
        // ВвестиДату
        assert_diagnostic_range(code, sync_diags[3], 43, 9, 58);
        // ВвестиЗначение
        assert_diagnostic_range(code, sync_diags[4], 72, 9, 67);
        // ВвестиСтроку
        assert_diagnostic_range(code, sync_diags[5], 103, 9, 50);
        // ВвестиЧисло
        assert_diagnostic_range(code, sync_diags[6], 122, 9, 61);
        // УстановитьВнешнююКомпоненту
        assert_diagnostic_range(code, sync_diags[7], 138, 4, 50);
        // ОткрытьФормуМодально
        assert_diagnostic_range(code, sync_diags[8], 148, 4, 33);
        // УстановитьРасширениеРаботыСФайлами
        assert_diagnostic_range(code, sync_diags[9], 159, 20, 56);
        // УстановитьРасширениеРаботыСКриптографией
        assert_diagnostic_range(code, sync_diags[10], 172, 20, 62);
        // ПодключитьРасширениеРаботыСКриптографией
        assert_diagnostic_range(code, sync_diags[11], 184, 12, 54);
        // Предупреждение
        assert_diagnostic_range(code, sync_diags[12], 185, 8, 129);
        // ПодключитьРасширениеРаботыСФайлами
        assert_diagnostic_range(code, sync_diags[13], 198, 12, 48);
        // Предупреждение
        assert_diagnostic_range(code, sync_diags[14], 199, 8, 109);
        // ПоместитьФайл
        assert_diagnostic_range(code, sync_diags[15], 214, 4, 88);
        // КопироватьФайл
        assert_diagnostic_range(code, sync_diags[16], 225, 4, 68);
        // ПереместитьФайл
        assert_diagnostic_range(code, sync_diags[17], 236, 4, 69);
        // НайтиФайлы
        assert_diagnostic_range(code, sync_diags[18], 247, 21, 51);
        // УдалитьФайлы
        assert_diagnostic_range(code, sync_diags[19], 260, 8, 37);
        // СоздатьКаталог
        assert_diagnostic_range(code, sync_diags[20], 274, 4, 29);
        // КаталогВременныхФайлов
        assert_diagnostic_range(code, sync_diags[21], 285, 16, 40);
        // КаталогДокументов
        assert_diagnostic_range(code, sync_diags[22], 296, 16, 35);
        // РабочийКаталогДанныхПользователя
        assert_diagnostic_range(code, sync_diags[23], 307, 16, 50);
        // ПолучитьФайлы
        assert_diagnostic_range(code, sync_diags[24], 318, 16, 89);
        // ПоместитьФайлы
        assert_diagnostic_range(code, sync_diags[25], 344, 16, 64);
        // ЗапроситьРазрешениеПользователя
        assert_diagnostic_range(code, sync_diags[26], 368, 12, 59);
        // ЗапуститьПриложение
        assert_diagnostic_range(code, sync_diags[27], 391, 4, 38);
    }

    #[test]
    fn test_synchronous_calls_in_server_context() {
        let code = r#"
&НаСервере
Процедура СерверныйМетод()
    ЗапуститьПриложение("app.exe");
КонецПроцедуры

&НаСервереБезКонтекста
Процедура БезКонтекстаМетод()
    КопироватьФайл("source", "dest");
КонецПроцедуры

&AtServer
Procedure AtServerMethod()
    RunApp("app.exe");
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 0, "Server methods should not trigger UsingSynchronousCalls");
    }

    #[test]
    fn test_no_synchronous_calls() {
        let code = r#"
Процедура Тест()
    // Async methods should not trigger diagnostic
    ПоказатьВопрос(Оповещение, "Текст?", РежимДиалогаВопрос.ДаНет);
    ПоказатьПредупреждение(, "Текст");
    НачатьКопированиеФайла(Оповещение, "source", "dest");
    НачатьЗапускПриложения(Оповещение, "app.exe");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let sync_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UsingSynchronousCalls)
            .collect();
        assert_eq!(sync_diags.len(), 0);
    }
}
