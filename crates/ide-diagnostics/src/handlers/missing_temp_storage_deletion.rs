//! MissingTempStorageDeletion diagnostic.
//!
//! Detects temporary storage data retrieved with GetFromTempStorage() that is not properly deleted.
//!
//! ## Why?
//!
//! Temporary storage data retrieved with `ПолучитьИзВременногоХранилища()` / `GetFromTempStorage()` must be
//! explicitly deleted after use with `УдалитьИзВременногоХранилища()` / `DeleteFromTempStorage()`.
//! Failure to delete temporary storage data can:
//! - Exhaust memory in temporary storage
//! - Cause performance degradation in 1C:Enterprise applications
//! - Leave sensitive data in memory longer than necessary
//!
//! ## Bad practice
//!
//! ```bsl
//! Процедура ОбработатьДанные(АдресТоваров)
//!     Товары = ПолучитьИзВременногоХранилища(АдресТоваров);
//!     // ... use data ...
//! КонецПроцедуры  // ❌ Temporary storage not cleaned up!
//! ```
//!
//! ## Good practice
//!
//! ```bsl
//! Процедура ОбработатьДанные(АдресТоваров)
//!     Товары = ПолучитьИзВременногоХранилища(АдресТоваров);
//!     Попытка
//!         // ... use data ...
//!     Исключение
//!         УдалитьИзВременногоХранилища(АдресТоваров);  // ✅ Clean up on error
//!         ВызватьИсключение;
//!     КонецПопытки;
//!     УдалитьИзВременногоХранилища(АдресТоваров);  // ✅ Clean up on success
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//!
//! This diagnostic has NO configuration parameters (unlike MissingTemporaryFileDeletion).
//! It can only be enabled/disabled:
//!
//! ```json
//! {
//!   "diagnostics": {
//!     "MissingTempStorageDeletion": true
//!   }
//! }
//! ```
//!
//! - **Enabled by default:** No (false)
//! - **Severity:** Critical
//! - **Tags:** STANDARD, PERFORMANCE, BADPRACTICE
//! - **Minutes to fix:** 3
//!
//! ## Implementation
//!
//! Ported from:
//! - MissingTempStorageDeletionDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//!
//! Key difference from MissingTemporaryFileDeletion:
//! - Uses STRUCTURAL HIR EQUALITY for parameter comparison (not string matching)
//! - This allows matching `Результат.АдресРезультата` correctly

use hir_def::hir::{Expr, ExprIdx};
use hir_def::{Body, BodySourceMap};

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};

/// HIR-based entry point for MissingTempStorageDeletion diagnostic.
///
/// Uses Salsa-cached module_bodies instead of raw AST traversal.
///
/// ## Algorithm
///
/// For each method in the module:
/// 1. Walk HIR expressions to find GetFromTempStorage() calls
/// 2. Extract the address parameter (first argument)
/// 3. Search for DeleteFromTempStorage() calls AFTER the get call
/// 4. Check if any deletion uses the SAME address (structural HIR equality)
/// 5. Create diagnostic if no matching deletion found
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::MissingTempStorageDeletion;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let module_bodies = ctx.module_bodies();
    let mut diagnostics = Vec::new();

    for (local_id, body) in module_bodies.iter_bodies() {
        let Some(source_map) = module_bodies.source_map(local_id) else { continue };
        diagnostics.extend(check_body(body, source_map, code, ctx));
    }

    if let Some(module_result) = module_bodies.module_code_result() {
        diagnostics.extend(check_body(&module_result.body, &module_result.source_map, code, ctx));
    }

    diagnostics.sort_by_key(|d| d.range.start());
    diagnostics
}

/// Check if name is GetFromTempStorage (case-insensitive, bilingual).
fn is_get_from_temp_storage(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "получитьизвременногохранилища" || lower == "getfromtempstorage"
}

/// Check if name is DeleteFromTempStorage (case-insensitive, bilingual).
fn is_delete_from_temp_storage(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "удалитьизвременногохранилища" || lower == "deletefromtempstorage"
}

/// Check a single HIR body for MissingTempStorageDeletion.
fn check_body(
    body: &Body,
    source_map: &BodySourceMap,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Collect all GetFromTempStorage calls: (ExprId, first_arg ExprIdx, call range)
    let mut get_calls: Vec<(ide_db::TextRange, ExprIdx)> = Vec::new();

    for (expr_id, expr) in body.exprs_iter() {
        let Expr::Call { callee, args } = expr else { continue };
        let Expr::Path(name) = body.expr_idx(*callee) else { continue };
        if !is_get_from_temp_storage(name.as_str()) {
            continue;
        }
        // Must have at least one argument (the address)
        let Some(&first_arg) = args.first() else { continue };
        let Some(range) = source_map.expr_range(expr_id) else { continue };
        get_calls.push((range, first_arg));
    }

    // Collect all DeleteFromTempStorage calls: (range, first_arg ExprIdx)
    let mut delete_calls: Vec<(ide_db::TextRange, ExprIdx)> = Vec::new();

    for (expr_id, expr) in body.exprs_iter() {
        let Expr::Call { callee, args } = expr else { continue };
        let Expr::Path(name) = body.expr_idx(*callee) else { continue };
        if !is_delete_from_temp_storage(name.as_str()) {
            continue;
        }
        let Some(&first_arg) = args.first() else { continue };
        let Some(range) = source_map.expr_range(expr_id) else { continue };
        delete_calls.push((range, first_arg));
    }

    // For each get call, check if there's a matching delete AFTER it
    for (get_range, get_arg) in &get_calls {
        let has_matching_delete = delete_calls.iter().any(|(del_range, del_arg)| {
            del_range.start() > get_range.end()
                && exprs_structurally_equal(body, *get_arg, *del_arg)
        });

        if !has_matching_delete {
            diagnostics.push(Diagnostic {
                code,
                message: "Нужно добавить удаление данных из временного хранилища после использования, вызвав \"УдалитьИзВременногоХранилища\"".to_string(),
                severity: ctx.severity(code),
                range: *get_range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    diagnostics
}

/// Structural equality of HIR expressions (case-insensitive for identifiers).
///
/// This is the CRITICAL function that differentiates this diagnostic
/// from MissingTemporaryFileDeletion. Allows matching complex expressions like:
/// - `Результат.АдресРезультата` (member access)
/// - Simple identifiers like `Адрес`
fn exprs_structurally_equal(body: &Body, a: ExprIdx, b: ExprIdx) -> bool {
    match (body.expr_idx(a), body.expr_idx(b)) {
        (Expr::Path(n1), Expr::Path(n2)) => {
            n1.as_str().to_lowercase() == n2.as_str().to_lowercase()
        }
        (Expr::Field { base: b1, field: f1 }, Expr::Field { base: b2, field: f2 }) => {
            f1.as_str().to_lowercase() == f2.as_str().to_lowercase()
                && exprs_structurally_equal(body, *b1, *b2)
        }
        (Expr::Call { callee: c1, args: a1 }, Expr::Call { callee: c2, args: a2 }) => {
            a1.len() == a2.len()
                && exprs_structurally_equal(body, *c1, *c2)
                && a1.iter().zip(a2.iter()).all(|(x, y)| exprs_structurally_equal(body, *x, *y))
        }
        (
            Expr::MethodCall { receiver: r1, method: m1, args: a1 },
            Expr::MethodCall { receiver: r2, method: m2, args: a2 },
        ) => {
            m1.as_str().to_lowercase() == m2.as_str().to_lowercase()
                && a1.len() == a2.len()
                && exprs_structurally_equal(body, *r1, *r2)
                && a1.iter().zip(a2.iter()).all(|(x, y)| exprs_structurally_equal(body, *x, *y))
        }
        (Expr::Index { base: b1, index: i1 }, Expr::Index { base: b2, index: i2 }) => {
            exprs_structurally_equal(body, *b1, *b2) && exprs_structurally_equal(body, *i1, *i2)
        }
        (Expr::Literal(l1), Expr::Literal(l2)) => l1 == l2,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::*;
    use crate::DiagnosticsConfig;

    #[test]
    fn test_missing_temp_storage_deletion() {
        let code = include_str!("../../test_data/MissingTempStorageDeletionDiagnostic.bsl");
        let config = DiagnosticsConfig::all_enabled();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Expect 4 diagnostics
        assert_eq!(diagnostics.len(), 4, "Expected 4 diagnostics");

        // 0-indexed lines and columns (character positions)
        assert_diagnostic_range(code, &diagnostics[0], 3, 24, 77); // Line 4
        assert_diagnostic_range(code, &diagnostics[1], 13, 24, 77); // Line 14
        assert_diagnostic_range(code, &diagnostics[2], 21, 24, 77); // Line 22
        assert_diagnostic_range(code, &diagnostics[3], 33, 24, 77); // Line 34
    }

    #[test]
    fn test_structural_equality() {
        // Test that member access parameters work correctly
        let code = r#"
Процедура Тест()
    Настройки = ПолучитьИзВременногоХранилища(Результат.АдресРезультата);
    УдалитьИзВременногоХранилища(Результат.АдресРезультата);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Member access should match structurally");
    }

    #[test]
    fn test_different_parameters() {
        let code = r#"
Процедура Тест()
    Данные = ПолучитьИзВременногоХранилища(АдресТоваров);
    УдалитьИзВременногоХранилища(ДругойАдрес);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Different parameters should trigger error");
    }

    #[test]
    fn test_bilingual() {
        // Test both Russian and English
        let code = r#"
Procedure Test()
    Data = GetFromTempStorage(Address);
    DeleteFromTempStorage(Address);
EndProcedure
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "English keywords should work");
    }

    #[test]
    fn test_simple_valid_case() {
        let code = r#"
Процедура Тест()
    Адрес = "";
    Данные = ПолучитьИзВременногоХранилища(Адрес);
    ОбработатьДанные(Данные);
    УдалитьИзВременногоХранилища(Адрес);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should not report when data is deleted");
    }

    #[test]
    fn test_simple_invalid_case() {
        let code = r#"
Процедура Тест()
    Адрес = "";
    Данные = ПолучитьИзВременногоХранилища(Адрес);
    ОбработатьДанные(Данные);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should report when data is not deleted");
    }

    #[test]
    fn test_delete_before_get() {
        // Delete before get should trigger error (wrong order)
        let code = r#"
Процедура Тест()
    Адрес = "";
    УдалитьИзВременногоХранилища(Адрес);
    Данные = ПолучитьИзВременногоХранилища(Адрес);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Delete before get should trigger error");
    }

    #[test]
    fn test_case_insensitive() {
        // Test case-insensitive matching
        let code = r#"
Процедура Тест()
    Адрес = "";
    Данные = ПОЛУЧИТЬИЗВРЕМЕННОГОХРАНИЛИЩА(адрес);
    ОбработатьДанные(Данные);
    удалитьизвременногохранилища(АДРЕС);
КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(
            diagnostics.len(),
            0,
            "Should handle case-insensitive method names and parameters"
        );
    }
}
