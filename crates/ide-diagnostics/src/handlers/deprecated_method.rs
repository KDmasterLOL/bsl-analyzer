//! DeprecatedMethod diagnostic (HIR-based)
//!
//! Detects usage of deprecated methods (8.3.10 and 8.3.17).
//!
//! **This is a HIR-based diagnostic** - collected during AST→HIR lowering.
//!
//! ## Why?
//!
//! Since 1C:Enterprise 8.3.10 and 8.3.17, several global methods were deprecated:
//!
//! ### 8.3.10 - Client application methods
//! Replaced with `КлиентскоеПриложение` / `ClientApplication` object:
//! - `УстановитьКраткийЗаголовокПриложения` → `КлиентскоеПриложение.УстановитьКраткийЗаголовок`
//! - `ПолучитьКраткийЗаголовокПриложения` → `КлиентскоеПриложение.ПолучитьКраткийЗаголовок`
//! - `УстановитьЗаголовокКлиентскогоПриложения` → `КлиентскоеПриложение.УстановитьЗаголовок`
//! - `ПолучитьЗаголовокКлиентскогоПриложения` → `КлиентскоеПриложение.ПолучитьЗаголовок`
//! - `ТекущийВариантОсновногоШрифтаКлиентскогоПриложения` → `КлиентскоеПриложение.ТекущийВариантОсновногоШрифта`
//! - `ТекущийВариантИнтерфейсаКлиентскогоПриложения` → `КлиентскоеПриложение.ТекущийВариантИнтерфейса`
//!
//! ### 8.3.17 - Error handling methods
//! Replaced with `МенеджерОбработкиОшибок` / `ErrorProcessingManager` object:
//! - `КраткоеПредставлениеОшибки` → `МенеджерОбработкиОшибок.КраткоеПредставлениеОшибки`
//! - `ПодробноеПредставлениеОшибки` → `МенеджерОбработкиОшибок.ПодробноеПредставлениеОшибки`
//! - `ПоказатьИнформациюОбОшибке` → `МенеджерОбработкиОшибок.ПоказатьИнформациюОбОшибке`
//!
//! ### Common (various versions)
//! - `ПолучитьФорму` / `GetForm` → Use other methods for form retrieval
//!
//! ## Bad practice
//! ```bsl
//! Процедура Тест()
//!     Заголовок = ПолучитьКраткийЗаголовокПриложения(); // ❌ Deprecated
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//! ```bsl
//! Процедура Тест()
//!     Заголовок = КлиентскоеПриложение.ПолучитьКраткийЗаголовок(); // ✅
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes (both 8310 and 8317)
//! - **Severity:** Information (INFO)
//! - **Tags:** DEPRECATED
//! - **Minutes to fix:** 1-5
//!
//! ## Implementation
//! - Diagnostic is emitted during HIR lowering in `lower_call_expr()`
//! - This handler converts `BodyDiagnostic::DeprecatedMethod` to `Diagnostic`
//! - Replaces separate AST-based handlers: `deprecated_methods_8310` and `deprecated_methods_8317`

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use std::collections::HashMap;

pub const DEPRECATED_METHODS_8310: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_10,
    tags: &[MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};
pub const DEPRECATED_METHODS_8317: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::CompatibilityMode8_3_17,
    tags: &[MetadataTag::Deprecated],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::DeprecatedMethod` is encountered.
pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    // Determine which diagnostic code this belongs to
    let (code, replacement) = get_diagnostic_code_and_replacement(name)?;

    // Check if the specific diagnostic is disabled
    if ctx.config.is_disabled(code) {
        return None;
    }

    let message = get_message(name, replacement);

    Some(Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

/// Determine diagnostic code and replacement text for a deprecated method.
///
/// Returns (DiagnosticCode, replacement_text) or None if not found.
fn get_diagnostic_code_and_replacement(name: &str) -> Option<(DiagnosticCode, &'static str)> {
    let lower = name.to_lowercase();

    // Check 8.3.10 methods
    if let Some(replacement) = get_8310_replacement(lower.as_str()) {
        return Some((DiagnosticCode::DeprecatedMethods8310, replacement));
    }

    // Check 8.3.17 methods
    if let Some(replacement) = get_8317_replacement(lower.as_str()) {
        return Some((DiagnosticCode::DeprecatedMethods8317, replacement));
    }

    None
}

/// Get replacement for 8.3.10 deprecated methods.
fn get_8310_replacement(method_lower: &str) -> Option<&'static str> {
    let map = get_8310_replacement_map();
    map.get(method_lower).copied()
}

/// Get replacement for 8.3.17 deprecated methods.
fn get_8317_replacement(method_lower: &str) -> Option<&'static str> {
    match method_lower {
        "краткоепредставлениеошибки" => {
            Some("МенеджерОбработкиОшибок.КраткоеПредставлениеОшибки")
        }
        "подробноепредставлениеошибки" => {
            Some("МенеджерОбработкиОшибок.ПодробноеПредставлениеОшибки")
        }
        "показатьинформациюобошибке" => {
            Some("МенеджерОбработкиОшибок.ПоказатьИнформациюОбОшибке")
        }
        "brieferrorrepresentation" => Some("ErrorProcessingManager.BriefErrorRepresentation"),
        "detailederrorrepresentation" => Some("ErrorProcessingManager.DetailedErrorRepresentation"),
        "showerrorinformation" => Some("ErrorProcessingManager.ShowErrorInformation"),
        "получитьформу" => Some("Use alternative form retrieval methods"),
        "getform" => Some("Use alternative form retrieval methods"),
        _ => None,
    }
}

/// Get replacement map for 8.3.10 deprecated methods.
fn get_8310_replacement_map() -> HashMap<&'static str, &'static str> {
    let mut map = HashMap::new();

    map.insert(
        "установитькраткийзаголовокприложения",
        "КлиентскоеПриложение.УстановитьКраткийЗаголовок",
    );
    map.insert(
        "получитькраткийзаголовокприложения",
        "КлиентскоеПриложение.ПолучитьКраткийЗаголовок",
    );
    map.insert(
        "установитьзаголовокклиентскогоприложения",
        "КлиентскоеПриложение.УстановитьЗаголовок",
    );
    map.insert("получитьзаголовокклиентскогоприложения", "КлиентскоеПриложение.ПолучитьЗаголовок");
    map.insert(
        "текущийвариантосновногошрифтаклиентскогоприложения",
        "КлиентскоеПриложение.ТекущийВариантОсновногоШрифта",
    );
    map.insert(
        "текущийвариантинтерфейсаклиентскогоприложения",
        "КлиентскоеПриложение.ТекущийВариантИнтерфейса",
    );

    map.insert("setshortapplicationcaption", "ClientApplication.SetShortCaption");
    map.insert("getshortapplicationcaption", "ClientApplication.GetShortCaption");
    map.insert("setclientapplicationcaption", "ClientApplication.SetCaption");
    map.insert("getclientapplicationcaption", "ClientApplication.GetCaption");
    map.insert(
        "clientapplicationbasefontcurrentvariant",
        "ClientApplication.CurrentBaseFontVariant",
    );
    map.insert(
        "clientapplicationinterfacecurrentvariant",
        "ClientApplication.CurrentInterfaceVariant",
    );

    map
}

/// Generate diagnostic message based on method name and replacement.
fn get_message(method_name: &str, replacement: &str) -> String {
    let lower = method_name.to_lowercase();
    let is_russian = lower.chars().any(|c| c as u32 > 127);

    if is_russian {
        format!("Метод \"{}\" устарел. Следует использовать \"{}\".", method_name, replacement)
    } else {
        format!("Method \"{}\" is deprecated. You should use \"{}\".", method_name, replacement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::check_hir_diagnostic;
    #[test]
    fn test_deprecated_8310_russian() {
        let code = r#"
Процедура Тест()
    УстановитьКраткийЗаголовокПриложения("Заголовок");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMethods8310)
            .collect();

        assert_eq!(deprecated_diags.len(), 1);
        assert!(deprecated_diags[0].message.contains("КлиентскоеПриложение"));
    }

    #[test]
    fn test_deprecated_8310_english() {
        let code = r#"
Procedure Test()
    Caption = GetShortApplicationCaption();
EndProcedure
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMethods8310)
            .collect();

        assert_eq!(deprecated_diags.len(), 1);
        assert!(deprecated_diags[0].message.contains("ClientApplication"));
    }

    #[test]
    fn test_deprecated_8317_russian() {
        let code = r#"
Процедура Тест()
    Описание = КраткоеПредставлениеОшибки(ИнформацияОбОшибке());
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeprecatedMethods8317)
            .collect();

        assert_eq!(deprecated_diags.len(), 1);
        assert!(deprecated_diags[0].message.contains("МенеджерОбработкиОшибок"));
    }

    #[test]
    fn test_not_triggered_for_method_calls() {
        let code = r#"
Процедура Тест()
    // Метод объекта - не должен триггериться
    Модуль.ПолучитьКраткийЗаголовокПриложения();
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let deprecated_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.code == DiagnosticCode::DeprecatedMethods8310
                    || d.code == DiagnosticCode::DeprecatedMethods8317
            })
            .collect();

        assert_eq!(deprecated_diags.len(), 0);
    }
}
