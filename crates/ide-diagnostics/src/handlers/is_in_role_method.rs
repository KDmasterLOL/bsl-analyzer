//! IsInRoleMethod diagnostic.
//!
//! Detects incorrect usage of `IsInRole()` / `РольДоступна()` method for access checking
//! without proper `PrivilegedMode()` / `ПривилегированныйРежим()` protection.
//!
//! ## Why?
//! The `IsInRole()` method should be used ONLY when a role does not grant access rights to
//! metadata objects and serves only to define an additional access right. When used, it
//! MUST be combined with a check for `PrivilegedMode()`.
//!
//! Using `IsInRole()` without `PrivilegedMode()` check may lead to security vulnerabilities
//! where access control checks can be bypassed.
//!
//! ## Bad practice
//! ```bsl
//! Если РольДоступна("ТребуемаяРоль") Тогда
//!     // Выполнение кода
//! КонецЕсли;
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Option 1: Combined check with PrivilegedMode
//! Если РольДоступна("ТребуемаяРоль") ИЛИ ПривилегированныйРежим() Тогда
//!     // Выполнение кода
//! КонецЕсли;
//!
//! // Option 2: Use AccessRight instead
//! Если ПравоДоступа("Добавление", Метаданные.Справочники.Номенклатура) Тогда
//!     // Выполнение кода
//! КонецЕсли;
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Major
//! - **Tags:** ERROR
//! - **Minutes to fix:** 5
//!
//! ## Implementation
//! Ported from:
//! - IsInRoleMethodDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET
//!
//! **Architecture:** HIR-based diagnostic (migrated from AST).
//!
//! ### HIR approach
//! - Two-pass analysis per method/module:
//!   - Pass 1: Track variables containing `IsInRole()` or `PrivilegedMode()` results
//!   - Pass 2: Check if-statements for unprotected usage
//! - Uses `Stmt::Assign` for variable tracking
//! - Uses `Expr::Call` for method call detection
//! - Uses `Stmt::If` for condition checking
//!
//! ### Advantages over AST
//! - Semantic analysis - operates on lowered HIR representation
//! - Salsa caching - benefits from automatic invalidation
//! - Cleaner code - no token-level parsing
//! - Better error recovery - HIR handles parse errors gracefully

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use hir_def::{
    hir::{Expr, ExprId, Stmt},
    ModuleId,
};
use ide_db::TextRange;
use std::collections::HashSet;

/// HIR-based check for IsInRole() usage without PrivilegedMode() protection.
///
/// Processes each method and module-level code independently to maintain proper scoping.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::IsInRoleMethod) {
        return Vec::new();
    }

    let mut all_diagnostics = Vec::new();
    let module_id = ModuleId { file_id: ctx.file_id };
    let module_bodies = ctx.db.module_bodies(module_id);

    // Check method bodies
    for (_local_id, body, source_map) in module_bodies.method_bodies() {
        let mut checker = IsInRoleChecker {
            is_in_role_vars: HashSet::new(),
            privileged_mode_vars: HashSet::new(),
            diagnostics: Vec::new(),
            body,
            source_map,
        };

        // Two-pass approach within each method:
        // Pass 1: Process all assignments to build variable tracking
        checker.collect_variables();

        // Pass 2: Check if-statements for diagnostics
        checker.check_statements();

        all_diagnostics.extend(checker.diagnostics);
    }

    // Check module-level code
    if let Some(lower_result) = module_bodies.module_code_result() {
        let mut checker = IsInRoleChecker {
            is_in_role_vars: HashSet::new(),
            privileged_mode_vars: HashSet::new(),
            diagnostics: Vec::new(),
            body: &lower_result.body,
            source_map: &lower_result.source_map,
        };

        checker.collect_variables();
        checker.check_statements();

        all_diagnostics.extend(checker.diagnostics);
    }

    all_diagnostics.sort_by_key(|d| d.range.start());
    all_diagnostics
}

struct IsInRoleChecker<'a> {
    is_in_role_vars: HashSet<String>,
    privileged_mode_vars: HashSet<String>,
    diagnostics: Vec<Diagnostic>,
    body: &'a hir_def::Body,
    source_map: &'a hir_def::body::BodySourceMap,
}

impl<'a> IsInRoleChecker<'a> {
    /// Pass 1: Collect variables containing IsInRole() or PrivilegedMode() results.
    fn collect_variables(&mut self) {
        for (_stmt_id, stmt) in self.body.stmts_iter() {
            if let Stmt::Assign { target, value } = stmt {
                self.handle_assignment(*target, *value);
            }
        }
    }

    /// Pass 2: Check if-statements for unprotected IsInRole() usage.
    fn check_statements(&mut self) {
        for (_stmt_id, stmt) in self.body.stmts_iter() {
            if let Stmt::If { condition, then_branch: _, elsif_branches, else_branch: _ } = stmt {
                // Check main if condition
                self.check_expression(*condition);

                // Check elsif conditions
                for (elsif_condition, _elsif_stmts) in elsif_branches.iter() {
                    self.check_expression(*elsif_condition);
                }
            }
        }
    }

    /// Handle assignment statement - track variables containing method call results.
    fn handle_assignment(&mut self, target: ExprId, value: ExprId) {
        // Get variable name from target
        let var_name = if let Expr::Path(name) = self.body.expr(target) {
            Some(name.as_str().to_lowercase())
        } else {
            None
        };

        // CRITICAL: Remove variable from BOTH sets (reassignment clears tracking)
        if let Some(ref var) = var_name {
            self.is_in_role_vars.remove(var);
            self.privileged_mode_vars.remove(var);
        }

        // Check if RHS is IsInRole() or PrivilegedMode() call
        if let Some(ref var) = var_name {
            if self.is_is_in_role_call(value) {
                self.is_in_role_vars.insert(var.clone());
            } else if self.is_privileged_mode_call(value) {
                self.privileged_mode_vars.insert(var.clone());
            }
        }
    }

    /// Check expression for unprotected IsInRole() usage.
    ///
    /// This recursively scans the expression tree looking for IsInRole() calls or variables,
    /// but protection check is done at the root level (if-condition).
    fn check_expression(&mut self, root_expr_id: ExprId) {
        // Protection is checked at root level (if-condition contains PrivilegedMode somewhere)
        let has_protection = self.has_privileged_mode_protection(root_expr_id);

        // Recursively find all IsInRole usages in the expression tree
        self.find_is_in_role_usages(root_expr_id, has_protection);
    }

    /// Recursively find IsInRole() calls and variables in expression tree.
    fn find_is_in_role_usages(&mut self, expr_id: ExprId, has_protection: bool) {
        // Check for direct IsInRole() calls
        if self.is_is_in_role_call(expr_id) && !has_protection {
            if let Some(range) = self.source_map.expr_range(expr_id) {
                self.diagnostics.push(create_diagnostic(range));
            }
        }

        // Check for variable references to IsInRole() results
        if let Expr::Path(name) = self.body.expr(expr_id) {
            let var_name = name.as_str().to_lowercase();
            if self.is_in_role_vars.contains(&var_name) && !has_protection {
                if let Some(range) = self.source_map.expr_range(expr_id) {
                    self.diagnostics.push(create_diagnostic(range));
                }
            }
        }

        // Recursively check subexpressions
        match self.body.expr(expr_id) {
            Expr::BinaryOp { lhs, rhs, .. } => {
                self.find_is_in_role_usages(*lhs, has_protection);
                self.find_is_in_role_usages(*rhs, has_protection);
            }
            Expr::UnaryOp { expr, .. } => {
                self.find_is_in_role_usages(*expr, has_protection);
            }
            Expr::Ternary { condition, then_expr, else_expr } => {
                self.find_is_in_role_usages(*condition, has_protection);
                self.find_is_in_role_usages(*then_expr, has_protection);
                self.find_is_in_role_usages(*else_expr, has_protection);
            }
            _ => {}
        }
    }

    /// Check if expression is a call to IsInRole() / РольДоступна().
    fn is_is_in_role_call(&self, expr_id: ExprId) -> bool {
        if let Expr::Call { callee, .. } = self.body.expr(expr_id) {
            if let Expr::Path(name) = self.body.expr(*callee) {
                return is_is_in_role_method(name.as_str());
            }
        }
        false
    }

    /// Check if expression is a call to PrivilegedMode() / ПривилегированныйРежим().
    fn is_privileged_mode_call(&self, expr_id: ExprId) -> bool {
        if let Expr::Call { callee, .. } = self.body.expr(expr_id) {
            if let Expr::Path(name) = self.body.expr(*callee) {
                return is_privileged_mode_method(name.as_str());
            }
        }
        false
    }

    /// Check if expression has PrivilegedMode() protection.
    ///
    /// Protection means the expression contains:
    /// - Direct call to PrivilegedMode()
    /// - Variable reference to PrivilegedMode() result
    /// - OR operator with PrivilegedMode() in either operand
    fn has_privileged_mode_protection(&self, expr_id: ExprId) -> bool {
        self.contains_privileged_mode(expr_id)
    }

    /// Recursively check if expression contains PrivilegedMode() call or variable.
    fn contains_privileged_mode(&self, expr_id: ExprId) -> bool {
        match self.body.expr(expr_id) {
            // Direct PrivilegedMode() call
            Expr::Call { callee, .. } => {
                if let Expr::Path(name) = self.body.expr(*callee) {
                    if is_privileged_mode_method(name.as_str()) {
                        return true;
                    }
                }
                false
            }

            // Variable reference to PrivilegedMode() result
            Expr::Path(name) => {
                let var_name = name.as_str().to_lowercase();
                self.privileged_mode_vars.contains(&var_name)
            }

            // Binary operations - recursively check both sides
            Expr::BinaryOp { lhs, rhs, .. } => {
                self.contains_privileged_mode(*lhs) || self.contains_privileged_mode(*rhs)
            }

            // Unary operations
            Expr::UnaryOp { expr, .. } => self.contains_privileged_mode(*expr),

            // Ternary operations
            Expr::Ternary { condition, then_expr, else_expr } => {
                self.contains_privileged_mode(*condition)
                    || self.contains_privileged_mode(*then_expr)
                    || self.contains_privileged_mode(*else_expr)
            }

            _ => false,
        }
    }
}

/// Check if method name is IsInRole() / РольДоступна().
fn is_is_in_role_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "рольдоступна" | "isinrole")
}

/// Check if method name is PrivilegedMode() / ПривилегированныйРежим().
fn is_privileged_mode_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "привилегированныйрежим" | "privilegedmode")
}

fn create_diagnostic(range: TextRange) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::IsInRoleMethod,
        message: "Для проверки прав доступа в коде следует использовать метод ПравоДоступа"
            .to_string(),
        severity: Severity::Major,
        range,
        tags: vec![],
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_diagnostic_range;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();

        // Set up source root for HIR-based diagnostics
        use ide_db::base_db::{SourceRoot, SourceRootId};
        use vfs::VfsPath;

        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        // Set file content
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        let db = Rc::new(db) as Rc<dyn RootDatabase>;
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

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/IsInRoleMethodDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(diagnostics.len(), 3, "Should match Java: 3 diagnostics");

        // Line 33 (0-indexed 32), cols 9-35: Direct РольДоступна() in if
        assert_diagnostic_range(&file_content, &diagnostics[0], 32, 9, 35);

        // Line 39 (0-indexed 38), cols 9-23: Variable ДоступРазрешен in if
        assert_diagnostic_range(&file_content, &diagnostics[1], 38, 9, 23);

        // Line 57 (0-indexed 56), cols 14-40: Direct РольДоступна() in elsif
        assert_diagnostic_range(&file_content, &diagnostics[2], 56, 14, 40);
    }

    #[test]
    fn test_direct_call_without_protection() {
        let code = r#"
Процедура Тест()
    Если РольДоступна("Роль") Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_direct_call_with_protection() {
        let code = r#"
Процедура Тест()
    Если РольДоступна("Роль") ИЛИ ПривилегированныйРежим() Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_variable_without_protection() {
        let code = r#"
Процедура Тест()
    Доступ = РольДоступна("Роль");
    Если Доступ Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_variable_with_protection() {
        let code = r#"
Процедура Тест()
    Доступ = РольДоступна("Роль");
    Если Доступ ИЛИ ПривилегированныйРежим() Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_variable_reassignment_clears_tracking() {
        let code = r#"
Процедура Тест()
    Доступ = РольДоступна("Роль");
    Доступ = Ложь;
    Если Доступ Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "Reassignment should clear tracking");
    }

    #[test]
    fn test_elsif_clause() {
        let code = r#"
Процедура Тест()
    Если Ложь Тогда
    ИначеЕсли РольДоступна("Роль") Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    Если РОЛЬДОСТУПНА("Роль") Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
Procedure Test()
    If IsInRole("Role") Then
    EndIf;
EndProcedure
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_privileged_mode_variable() {
        let code = r#"
Процедура Тест()
    ПР = ПривилегированныйРежим();
    Если РольДоступна("Роль") ИЛИ ПР Тогда
    КонецЕсли;
КонецПроцедуры
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0, "PrivilegedMode variable should protect");
    }
}
