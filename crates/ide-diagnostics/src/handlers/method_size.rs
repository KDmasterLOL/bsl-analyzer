//! MethodSize diagnostic.
//!
//! Detects functions and procedures with excessive line count.
//!
//! ## Why?
//! Long methods are hard to understand, test, and maintain.
//! They often indicate lack of proper abstraction and responsibility separation.
//!
//! ## Bad practice
//! ```bsl
//! Процедура ОченьДлиннаяПроцедура()
//!     // 300 lines of code...
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! Split into smaller, focused methods:
//! ```bsl
//! Процедура ВыполнитьОперацию()
//!     ПодготовитьДанные();
//!     ВыполнитьОсновнуюЛогику();
//!     ОбработатьРезультат();
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **maxMethodSize** (default: 200) - Maximum allowed method line count
//! - **Enabled by default:** Yes
//! - **Severity:** MAJOR
//! - **Tags:** BADPRACTICE (concept)
//! - **Minutes to fix:** 30
//!
//! ## Implementation
//!
//! Algorithm: Calculates line difference (stop_line - start_line) matching bsl-language-server's ANTLR behavior.
//!
//! ## Performance
//! Uses LineIndex for O(1) line number lookups instead of scanning the entire
//! file text for each method. LineIndex is built once O(n) at the start.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Adaptable,
};

#[derive(Debug, Clone)]
struct Config {
    max_method_size: usize,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let max_method_size =
            ctx.config.get_int(DiagnosticCode::MethodSize, "maxMethodSize").unwrap_or(200) as usize;

        Self { max_method_size }
    }
}

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from hir_dispatch when `BodyDiagnostic::MethodSize` is encountered.
/// Applies configuration filtering (maxMethodSize).
pub fn from_hir(
    method_name: &str,
    size: u32,
    _is_function: bool,
    range: ide_db::TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::MethodSize;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let config = Config::from_context(ctx);
    if (size as usize) <= config.max_method_size {
        return None;
    }

    Some(Diagnostic {
        code,
        message: format!(
            "Длина метода \"{}\" равна {}, что больше установленного лимита в {} строк",
            method_name, size, config.max_method_size
        ),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{check_hir_diagnostic, check_hir_diagnostic_with_config};
    use crate::{DiagnosticCode, DiagnosticsConfig, Severity};

    fn make_method_size_code() -> String {
        // Reproduces MethodSizeDiagnostic.bsl structure:
        //   Line 0:   Процедура ПустаяПроцедура()
        //   Line 1:   (empty)
        //   Line 2:   КонецПроцедуры
        //   Line 3:   (empty)
        //   Line 4:   Функция ФункцияВОднуСтроку() КонецФункции
        //   Line 5:   (empty)
        //   Line 6:   Процедура Процедура201Строка()   <- diagnostic here
        //   Lines 7-208: body (201 lines from start to КонецПроцедуры)
        //   Line 209:   (empty) [after end line 208]
        //   ...
        //   Line 419:  Функция Функция201Строка()       <- diagnostic here
        //
        // Sizes are computed as stop_line - start_line (ANTLR behavior).
        // Процедура201Строка: lines 6-212 → size 206? No — we need size=201.
        // Original has 201 А=0 statements, making the method span 203 lines (start + 201 body + end).
        // bsl-language-server size = stop_line - start_line = 202 lines diff but reports 201.
        // We replicate the exact file structure: same line counts as original .bsl.

        let mut s = String::new();
        // Lines 0-2: ПустаяПроцедура
        s.push_str("Процедура ПустаяПроцедура()\n\n КонецПроцедуры\n\n");
        // Line 4: ФункцияВОднуСтроку
        s.push_str("Функция ФункцияВОднуСтроку() КонецФункции\n\n");
        // Процедура201Строка: size = (end_line - start_line) - 4 must be > 200.
        // Structure: declaration + blank + N body lines + blank + end
        // end_line - start_line = N + 3, size = N + 3 - 4 = N - 1.
        // For size=201: N=202 body lines.
        s.push_str("Процедура Процедура201Строка()\n\n");
        for _ in 0..202 {
            s.push_str("    А = 0;\n");
        }
        s.push_str("\n КонецПроцедуры\n\n");
        // Процедура200Строк: size=200, should NOT trigger at default threshold 200.
        // For size=200: N=201 body lines.
        s.push_str("Процедура Процедура200Строк()\n\n");
        for _ in 0..201 {
            s.push_str("    А = 0;\n");
        }
        s.push_str("\n КонецПроцедуры\n\n");
        // Функция201Строка: size=201, N=202 body lines.
        s.push_str("Функция Функция201Строка()\n\n");
        for _ in 0..202 {
            s.push_str("    А = 0;\n");
        }
        s.push_str("\n КонецФункции\n\n");
        // Функция200Строк: size=200, N=201 body lines.
        s.push_str("Функция Функция200Строк()\n\n");
        for _ in 0..201 {
            s.push_str("    А = 0;\n");
        }
        s.push_str("\n КонецФункции\n\n");
        // А(А=0) — one-liner with default param
        s.push_str("Функция А(А=0)\n\n КонецФункции\n");
        s
    }

    #[test]
    fn test_comprehensive() {
        let code = make_method_size_code();
        let diagnostics = check_hir_diagnostic(&code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::MethodSize).collect();

        assert_eq!(diagnostics.len(), 2, "Should find 2 diagnostics");

        assert_eq!(diagnostics[0].code, DiagnosticCode::MethodSize);
        assert_eq!(diagnostics[0].severity, Severity::Warning); // CodeSmell + Major -> Warning
        assert!(
            diagnostics[0].message.contains("Процедура201Строка"),
            "Message should contain method name, got: {}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("201"),
            "Message should contain size 201, got: {}",
            diagnostics[0].message
        );
        assert!(diagnostics[1].message.contains("Функция201Строка"));
    }

    #[test]
    fn test_configure_threshold_20() {
        let code = make_method_size_code();
        let mut config = DiagnosticsConfig::default();
        let mut params = serde_json::Map::new();
        params.insert("maxMethodSize".to_string(), serde_json::Value::Number(20.into()));
        config.parameters.insert(DiagnosticCode::MethodSize, serde_json::Value::Object(params));

        let diagnostics = check_hir_diagnostic_with_config(&code, config, crate::diagnostics);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::MethodSize).collect();
        assert_eq!(diagnostics.len(), 4, "Should find 4 diagnostics with threshold 20");
    }

    #[test]
    fn test_empty_method() {
        let code = r#"Процедура Пустая()

КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::MethodSize).collect();
        assert_eq!(diagnostics.len(), 0, "Empty method should not trigger");
    }

    #[test]
    fn test_one_liner() {
        let code = r#"Функция Тест() КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let diagnostics: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::MethodSize).collect();
        assert_eq!(diagnostics.len(), 0, "One-liner should not trigger");
    }
}
