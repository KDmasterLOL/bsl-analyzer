use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;

pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::TempFilesDir) {
        return None;
    }

    let message = get_message(name);

    Some(Diagnostic {
        code: DiagnosticCode::TempFilesDir,
        message,
        severity: Severity::Warning,
        range,
        tags: vec![],
        fixes: vec![],
    })
}

fn get_message(method_name: &str) -> String {
    let lower = method_name.to_lowercase();
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

        assert_eq!(diags.len(), 1);
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

        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("TempFilesDir"));
    }

    #[test]
    fn test_object_method_excluded() {
        let code = r#"
Процедура Тест()
    Каталог = Модуль.КаталогВременныхФайлов();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::TempFilesDir).collect();

        assert_eq!(diags.len(), 0);
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
        let diagnostics = check_hir_diagnostic(code);
        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::TempFilesDir).collect();

        assert_eq!(diags.len(), 3);
    }

    #[test]
    fn test_from_java_fixture() {
        let input = include_str!("../../test_data/TempFilesDirDiagnostic.bsl");
        let diagnostics = check_hir_diagnostic(input);

        let diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::TempFilesDir).collect();

        assert_eq!(diags.len(), 2, "Expected 2 diagnostics");

        assert_diagnostic_range(input, diags[0], 1, 14, 36);
        assert_diagnostic_range(input, diags[1], 8, 14, 26);
    }
}
