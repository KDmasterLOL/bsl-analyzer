//! ServerCallsInFormEvents diagnostic.
//!
//! Detects server method calls (`&НаСервере`, `&НаСервереБезКонтекста`) inside
//! form event handlers `ПриАктивизацииСтроки` and `НачалоВыбора`.
//!
//! ## Why?
//!
//! These events fire frequently during UI interaction (e.g., when user navigates table rows
//! or opens dropdown). Calling server methods in these events causes excessive network traffic
//! and degrades performance.
//!
//! ## Bad practice
//!
//! ```bsl
//! &НаСервере
//! Процедура СерверныйМетод()
//!     // ...
//! КонецПроцедуры
//!
//! &НаКлиенте
//! Процедура ТаблицаФормыПриАктивизацииСтроки(Элемент)
//!     СерверныйМетод();  // ERROR: server call in form event
//! КонецПроцедуры
//! ```
//!
//! ## Good practice
//!
//! ```bsl
//! &НаКлиенте
//! Процедура ТаблицаФормыПриАктивизацииСтроки(Элемент)
//!     // Use client-side code only
//!     Элементы.ОтображениеДанных.Видимость = Ложь;
//! КонецПроцедуры
//! ```
//!
//! ## Scope
//!
//! - Only triggers in FormModule (form modules)
//! - Only checks unqualified calls (local method calls)
//! - Qualified calls like `CommonModule.ServerMethod()` are NOT checked
//!
//! ## Configuration
//!
//! - **Enabled by default:** Yes
//! - **Severity:** CRITICAL (ERROR)
//! - **Tags:** ERROR, PERFORMANCE

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use hir_def::item_tree::AnnotationKind;
use hir_def::Name;
use ide_db::TextRange;

/// Server annotations that trigger the diagnostic.
const SERVER_ANNOTATIONS: &[AnnotationKind] =
    &[AnnotationKind::AtServer, AnnotationKind::AtServerNoContext];

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from hir_dispatch when `BodyDiagnostic::ServerCallsInFormEvents` is encountered.
///
/// ## Validation steps
///
/// 1. Check if diagnostic is disabled
/// 2. Check if module type is FormModule
/// 3. Find the called method in SymbolTree
/// 4. Check if method has server annotation (AtServer or AtServerNoContext)
pub fn from_hir(callee: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::ServerCallsInFormEvents) {
        return None;
    }

    // Check module type is FormModule
    let metadata = ctx.module_metadata();
    if metadata.module_type != bsl_metadata::ModuleType::FormModule {
        return None;
    }

    // Get SymbolTree and find called method
    let symbol_tree = ctx.symbol_tree();
    let method_name = Name::new(callee);

    let method = symbol_tree.find_method(&method_name)?;

    // Check if method has server annotation
    let has_server_annotation =
        method.annotations.iter().any(|ann| SERVER_ANNOTATIONS.contains(&ann.kind));

    if !has_server_annotation {
        return None;
    }

    Some(Diagnostic {
        code: DiagnosticCode::ServerCallsInFormEvents,
        message: format!(
            "В событиях ПриАктивизацииСтроки и НачалоВыбора не должно быть вызовов \
             серверных процедур. Процедура \"{}\" выполняется на сервере",
            callee
        ),
        severity: Severity::Critical,
        range,
        tags: vec![],
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::check_hir_diagnostic;

    #[test]
    fn test_server_annotations_contains_expected() {
        assert!(SERVER_ANNOTATIONS.contains(&AnnotationKind::AtServer));
        assert!(SERVER_ANNOTATIONS.contains(&AnnotationKind::AtServerNoContext));
        assert!(!SERVER_ANNOTATIONS.contains(&AnnotationKind::AtClient));
        assert!(!SERVER_ANNOTATIONS.contains(&AnnotationKind::AtClientAtServer));
    }

    #[test]
    fn test_no_diagnostic_without_form_module() {
        // Without FormModule metadata, no diagnostics should be emitted
        // (check_hir_diagnostic uses ModuleType::Unknown)
        let code = r#"
&НаСервере
Процедура СерверныйМетод()
КонецПроцедуры

&НаКлиенте
Процедура ПриАктивизацииСтроки(Элемент)
    СерверныйМетод();
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let server_calls_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ServerCallsInFormEvents)
            .collect();

        // Without FormModule context, no diagnostics
        assert_eq!(server_calls_diags.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_for_normal_procedure() {
        // Normal procedures (not form events) should not trigger diagnostics
        let code = r#"
&НаСервере
Процедура СерверныйМетод()
КонецПроцедуры

&НаКлиенте
Процедура ОбычнаяПроцедура()
    СерверныйМетод();
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let server_calls_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ServerCallsInFormEvents)
            .collect();

        // Normal procedure, not a form event - no diagnostics
        assert_eq!(server_calls_diags.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_for_client_method_call() {
        // Calling client methods in form events is OK
        let code = r#"
&НаКлиенте
Процедура КлиентскийМетод()
КонецПроцедуры

&НаКлиенте
Процедура ПриАктивизацииСтроки(Элемент)
    КлиентскийМетод();
КонецПроцедуры
"#;

        let diagnostics = check_hir_diagnostic(code);
        let server_calls_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ServerCallsInFormEvents)
            .collect();

        // Client method call, no diagnostics
        assert_eq!(server_calls_diags.len(), 0);
    }
}
