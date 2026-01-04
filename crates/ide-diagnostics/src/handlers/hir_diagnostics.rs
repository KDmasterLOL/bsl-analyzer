//! HIR-based diagnostics.
//!
//! This module exposes diagnostics collected during HIR lowering.
//! These diagnostics are collected as a byproduct of AST→HIR transformation,
//! following rust-analyzer's `*_with_diagnostics` pattern.
//!
//! ## Available diagnostics
//!
//! - `FunctionShouldHaveReturn` - function without any return statement
//! - `EmptyCodeBlock` - empty if/while/for/try body
//! - `MagicNumber` - hardcoded numeric literals (future)
//! - `SelfAssign` - self-assignment like `a = a` (future)
//! - `UnusedVariable` - declared but unused variables (future)
//!
//! ## Architecture
//!
//! ```text
//! AST (parse) → HIR (module_bodies) → Diagnostics
//!                    │
//!                    └── Collected during lowering, cached by Salsa
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use hir::{BodyDiagnostic, ModuleId};

/// Runs HIR-based diagnostics.
///
/// This retrieves diagnostics collected during HIR lowering via `module_bodies()`.
/// These diagnostics are cached and only recomputed when file content changes.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let module_id = ModuleId::new(ctx.file_id);
    let module_bodies = ctx.db.module_bodies(module_id);

    let mut diagnostics = Vec::new();

    for (_method_id, body_diag) in module_bodies.all_diagnostics() {
        if let Some(diag) = convert_body_diagnostic(body_diag, ctx) {
            diagnostics.push(diag);
        }
    }

    diagnostics
}

/// Convert a BodyDiagnostic to our Diagnostic type.
fn convert_body_diagnostic(
    body_diag: &BodyDiagnostic,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    match body_diag {
        BodyDiagnostic::FunctionShouldHaveReturn { range } => {
            if ctx.config.is_disabled(DiagnosticCode::FunctionShouldHaveReturn) {
                return None;
            }
            Some(Diagnostic {
                code: DiagnosticCode::FunctionShouldHaveReturn,
                message: "Функция должна содержать хотя бы один оператор Возврат".to_string(),
                severity: Severity::Major,
                range: *range,
                tags: vec![],
                fixes: vec![],
            })
        }

        BodyDiagnostic::EmptyCodeBlock { range } => {
            if ctx.config.is_disabled(DiagnosticCode::EmptyCodeBlock) {
                return None;
            }
            Some(Diagnostic {
                code: DiagnosticCode::EmptyCodeBlock,
                message: "Пустой блок кода".to_string(),
                severity: Severity::Major,
                range: *range,
                tags: vec![],
                fixes: vec![],
            })
        }

        BodyDiagnostic::MagicNumber { value, range } => {
            if ctx.config.is_disabled(DiagnosticCode::MagicNumber) {
                return None;
            }
            Some(Diagnostic {
                code: DiagnosticCode::MagicNumber,
                message: format!("Магическое число: {}", value),
                severity: Severity::Warning,
                range: *range,
                tags: vec![],
                fixes: vec![],
            })
        }

        BodyDiagnostic::SelfAssign { range } => {
            if ctx.config.is_disabled(DiagnosticCode::SelfAssign) {
                return None;
            }
            Some(Diagnostic {
                code: DiagnosticCode::SelfAssign,
                message: "Присваивание переменной самой себе".to_string(),
                severity: Severity::Major,
                range: *range,
                tags: vec![],
                fixes: vec![],
            })
        }

        BodyDiagnostic::UnusedVariable { name, range } => {
            if ctx.config.is_disabled(DiagnosticCode::UnusedLocalVariable) {
                return None;
            }
            Some(Diagnostic {
                code: DiagnosticCode::UnusedLocalVariable,
                message: format!("Неиспользуемая переменная: {}", name),
                severity: Severity::Warning,
                range: *range,
                tags: vec![crate::DiagnosticTag::Unnecessary],
                fixes: vec![],
            })
        }

        BodyDiagnostic::UnreachableCode { range } => {
            if ctx.config.is_disabled(DiagnosticCode::UnreachableCode) {
                return None;
            }
            Some(Diagnostic {
                code: DiagnosticCode::UnreachableCode,
                message: "Недостижимый код".to_string(),
                severity: Severity::Warning,
                range: *range,
                tags: vec![crate::DiagnosticTag::Unnecessary],
                fixes: vec![],
            })
        }

        BodyDiagnostic::MissingReturn { range } => {
            if ctx.config.is_disabled(DiagnosticCode::AllFunctionPathMustHaveReturn) {
                return None;
            }
            Some(Diagnostic {
                code: DiagnosticCode::AllFunctionPathMustHaveReturn,
                message: "Не все пути выполнения функции содержат возврат".to_string(),
                severity: Severity::Major,
                range: *range,
                tags: vec![],
                fixes: vec![],
            })
        }

        BodyDiagnostic::DeprecatedMethod { name, range } => {
            // Map to appropriate deprecated diagnostic
            Some(Diagnostic {
                code: DiagnosticCode::DeprecatedMethods8310,
                message: format!("Устаревший метод: {}", name),
                severity: Severity::Warning,
                range: *range,
                tags: vec![crate::DiagnosticTag::Deprecated],
                fixes: vec![],
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    use ide_db::RootDatabase;
    use std::sync::Arc;

    fn check_diagnostic(code: &str) -> Vec<Diagnostic> {
        use ide_db::base_db::SourceDatabase;
        use ide_db::RootDatabaseImpl;
        use test_fixture::Fixture;

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;
        let config = DiagnosticsConfig::default();
        let ctx = DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        check(&ctx)
    }

    #[test]
    fn test_function_without_return_hir() {
        let code = r#"Функция БезВозврата()
    Перем Х;
    Х = 42;
КонецФункции"#;

        let diagnostics = check_diagnostic(code);

        // Filter only FunctionShouldHaveReturn diagnostics
        let return_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::FunctionShouldHaveReturn)
            .collect();
        assert_eq!(return_diags.len(), 1, "Expected 1 FunctionShouldHaveReturn diagnostic");
    }

    #[test]
    fn test_function_with_return_hir() {
        let code = r#"Функция СВозвратом()
    Возврат 42;
КонецФункции"#;

        let diagnostics = check_diagnostic(code);

        // No FunctionShouldHaveReturn diagnostic
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::FunctionShouldHaveReturn),
            "Function with return should not trigger diagnostic"
        );
    }

    #[test]
    fn test_procedure_no_return_needed() {
        let code = r#"Процедура БезВозврата()
    Сообщить("Привет");
КонецПроцедуры"#;

        let diagnostics = check_diagnostic(code);

        // Procedures don't need return statements
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::FunctionShouldHaveReturn),
            "Procedures should not trigger FunctionShouldHaveReturn"
        );
    }

    #[test]
    fn test_empty_code_block_hir() {
        let code = r#"Процедура Тест()
    Если Истина Тогда
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics = check_diagnostic(code);

        assert!(
            diagnostics.iter().any(|d| d.code == DiagnosticCode::EmptyCodeBlock),
            "Empty if block should trigger EmptyCodeBlock diagnostic"
        );
    }
}
