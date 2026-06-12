use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use stdx::case::CaseExt;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::TempFilesDir;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let message = get_message(name);

    Some(Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

fn get_message(method_name: &str) -> String {
    let lower = method_name.fold_lower();
    if lower == "каталогвременныхфайлов" {
        "Не рекомендуемый вызов функции КаталогВременныхФайлов()".to_string()
    } else {
        "Not recommended TempFilesDir() call".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use crate::Severity;
    use expect_test::expect;
    #[test]
    fn test_temp_files_dir_russian() {
        let code = r#"
Процедура Тест()
    Каталог = КаталогВременныхФайлов();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::TempFilesDir).collect();

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TempFilesDir,
            expect![[r#"
            TempFilesDir @ 3:15..3:37
              message: Не рекомендуемый вызов функции КаталогВременныхФайлов()
              severity: Warning"#]],
        );
        assert_eq!(diags[0].severity, Severity::Warning);
        assert!(diags[0].message.contains("КаталогВременныхФайлов"));
    }

    #[test]
    fn test_temp_files_dir_english() {
        let code = r#"
Procedure Test()
    Catalog = TempFilesDir();
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::TempFilesDir).collect();

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TempFilesDir,
            expect![[r#"
            TempFilesDir @ 3:15..3:27
              message: Not recommended TempFilesDir() call
              severity: Warning"#]],
        );
        assert!(diags[0].message.contains("TempFilesDir"));
    }

    #[test]
    fn test_object_method_excluded() {
        let code = r#"
Процедура Тест()
    Каталог = Модуль.КаталогВременныхФайлов();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(code, DiagnosticCode::TempFilesDir, expect![[r#""#]]);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    К1 = КАТАЛОГВРЕМЕННЫХФАЙЛОВ();
    К2 = каталогвременныхфайлов();
    К3 = КаталогВременныхФайлов();
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::TempFilesDir,
            expect![[r#"
            TempFilesDir @ 3:10..3:32
              message: Не рекомендуемый вызов функции КаталогВременныхФайлов()
              severity: Warning
            TempFilesDir @ 4:10..4:32
              message: Не рекомендуемый вызов функции КаталогВременныхФайлов()
              severity: Warning
            TempFilesDir @ 5:10..5:32
              message: Не рекомендуемый вызов функции КаталогВременныхФайлов()
              severity: Warning"#]],
        );
    }

    #[test]
    fn test_from_java_fixture() {
        let input = r#"Функция Тест()
    Каталог = КаталогВременныхФайлов();  // Срабатывание здесь
    ИмяФайла = Строка(Новый УникальныйИдентификатор) + ".xml";
    ИмяПромежуточногоФайла = Каталог + ИмяФайла;
    Данные.Записать(ИмяПромежуточногоФайла);
КонецФункции

Function Test()
    Catalog = TempFilesDir(); // Срабатывание здесь
    FileName = Str(New UUID);
EndFunction"#;
        check_diagnostics_snapshot_for(
            input,
            DiagnosticCode::TempFilesDir,
            expect![[r#"
            TempFilesDir @ 2:15..2:37
              message: Не рекомендуемый вызов функции КаталогВременныхФайлов()
              severity: Warning
            TempFilesDir @ 9:15..9:27
              message: Not recommended TempFilesDir() call
              severity: Warning"#]],
        );
    }
}
