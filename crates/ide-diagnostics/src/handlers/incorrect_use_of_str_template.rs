//! IncorrectUseOfStrTemplate diagnostic
//!
//! Detects incorrect usage of СтрШаблон/StrTemplate method.
//!
//! **Source (Java):** bsl-language-server/IncorrectUseOfStrTemplateDiagnostic.java
//! **Source (Rust tree-sitter):** bsl-language-server-rust/rules/incorrect_use_of_str_template.rs
//!
//! ## Implementation
//!
//! Two-phase detection (rust-analyzer pattern):
//!
//! **Phase 1: HIR Lowering** (75% coverage)
//! - Detects errors in string literal templates during AST→HIR lowering
//! - Validated in `lower_call_expr()` (hir-def/body/lower/expr.rs:524-545)
//!
//! **Phase 2: Post-HIR Check** (95%+ coverage)
//! - Uses ReachingDefs dataflow analysis to resolve variables
//! - Recursive resolution with depth limit (max 10 levels)
//! - Handles transitive assignments: `var1 = "template"; var2 = var1`
//! - Handles control flow (if/else, loops)
//! - Multiple definitions: if all resolve to same value → accepted
//! - Salsa-cached via `reaching_definitions()` query
//!
//! - Detects invalid placeholders (%0, %11+)
//! - Validates parameter count matches placeholders
//! - Handles %% escape sequences correctly
//!
//! ## Why?
//! StrTemplate requires proper parameter matching:
//! - Number of %N placeholders must match number of arguments
//! - Only %1 to %10 are supported (also %(1) to %(10))
//! - %0 is invalid
//! - %11, %12, etc. are invalid
//! - %% escapes to single % (not a parameter)
//!
//! ## Bad practice
//! ```bsl
//! // Missing parameter value
//! А = СтрШаблон("Наименование (версия %1)");
//!
//! // Insufficient arguments
//! Б = СтрШаблон("%1 (версия %2)", Наименование);
//!
//! // Invalid parameter number
//! К = СтрШаблон("Наименование %11", Наименование);
//!
//! // Invalid %0
//! К = СтрШаблон("Наименование %0", Наименование);
//! ```
//!
//! ## Good practice
//! ```bsl
//! // Correct usage
//! Г = СтрШаблон("Наименование (версия %1)", Версия());
//!
//! // Multiple parameters
//! Е = СтрШаблон("Наименование %1 (версия %2)", Наименование, Версия);
//!
//! // Escaped %% (not a parameter)
//! З = СтрШаблон("Наименование %%1 (версия %%2)");
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use hir_def::{
    hir::{Expr, Literal, Stmt},
    MethodId, ModuleId,
};
use ide_db::TextRange;

/// Post-HIR check for variable resolution cases.
///
/// This function complements the HIR lowering validation by resolving variables
/// to their string literal definitions using reaching definitions analysis with
/// recursive resolution support.
///
/// ## Coverage
/// - HIR lowering: detects errors in string literals (75% coverage)
/// - This check: resolves variables to literals using dataflow (95%+ coverage)
///   - Supports transitive assignments (var1 = "x"; var2 = var1)
///   - Handles multiple definitions if all resolve to same value
///   - Depth limit: 10 levels for cycle protection
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::IncorrectUseOfStrTemplate) {
        return vec![];
    }

    let mut diagnostics = Vec::new();
    let module_id = ModuleId { file_id: ctx.file_id };

    // Get module bodies
    let module_bodies = ctx.db.module_bodies(module_id);

    // Check each method
    for (local_id, body, source_map) in module_bodies.method_bodies() {
        let method_id = MethodId { module: module_id, local_id };

        // Get reaching definitions for this method
        let reaching_defs = match ctx.db.reaching_definitions(method_id) {
            Some(defs) => defs,
            None => continue, // Analysis didn't converge, skip
        };

        // Scan for StrTemplate calls with variable arguments
        for (stmt_id, stmt) in body.stmts_iter() {
            // Check both Expr and Assign statements (assignment RHS can have Call)
            let expr_id = match stmt {
                Stmt::Expr(id) => Some(*id),
                Stmt::Assign { value, .. } => Some(*value),
                _ => None,
            };

            if let Some(expr_id) = expr_id {
                let expr = body.expr(expr_id);

                // Check if this is a Call expression (function call like СтрШаблон(...))
                let (method_name, args) = match expr {
                    Expr::Call { callee, args } => {
                        // For Call, the callee should be a Path (function name)
                        if let Expr::Path(name) = body.expr(*callee) {
                            (name.as_str().to_lowercase(), args)
                        } else {
                            continue; // Not a simple function call
                        }
                    }
                    Expr::MethodCall { method, args, .. } => (method.as_str().to_lowercase(), args),
                    _ => continue, // Not a call expression
                };

                // Check if this is StrTemplate call
                if !matches!(method_name.as_str(), "strtemplate" | "стршаблон") {
                    continue;
                }

                // Need at least one argument (template)
                if args.is_empty() {
                    continue;
                }

                let template_expr_id = args[0];
                let param_count = args.len() - 1; // Excluding template itself

                // Skip string literals - they're already validated by HIR lowering
                if matches!(body.expr(template_expr_id), Expr::Literal(Literal::String(_))) {
                    continue;
                }

                // Try to resolve template to string literal
                if let Some(template_string) =
                    resolve_expr_to_string(template_expr_id, body, &reaching_defs, stmt_id)
                {
                    // Validate template (reuse logic from lowering)
                    if is_wrong_str_template(&template_string, param_count) {
                        // Get source range for diagnostic
                        if let Some(range) = source_map.expr_range(template_expr_id) {
                            diagnostics.push(Diagnostic {
                                code: DiagnosticCode::IncorrectUseOfStrTemplate,
                                message: format!(
                                    "Template '{}' requires {} parameters but {} provided",
                                    template_string.chars().take(50).collect::<String>(),
                                    count_required_params(&template_string),
                                    param_count
                                ),
                                severity: Severity::Error,
                                range,
                                tags: vec![],
                                fixes: vec![],
                            });
                        }
                    }
                }
            }
        }
    }

    diagnostics
}

/// Resolve expression to string literal using reaching definitions.
///
/// Handles:
/// - Direct string literals: `"template %1"` → Some("template %1")
/// - Variables: `var = "template %1"; StrTemplate(var, ...)` → Some("template %1")
/// - Transitive assignments: `var1 = "template"; var2 = var1; StrTemplate(var2)` → Some("template")
/// - Multiple definitions: if all resolve to same value → Some(value), otherwise None
///
/// Uses recursive resolution with depth limit to prevent infinite loops.
fn resolve_expr_to_string(
    expr_id: hir_def::hir::ExprId,
    body: &hir_def::Body,
    reaching_defs: &dataflow::reaching_defs::ReachingDefsResult,
    stmt_id: hir_def::hir::StmtId,
) -> Option<String> {
    resolve_expr_to_string_impl(expr_id, body, reaching_defs, stmt_id, 0)
}

/// Internal implementation with depth tracking for cycle protection.
fn resolve_expr_to_string_impl(
    expr_id: hir_def::hir::ExprId,
    body: &hir_def::Body,
    reaching_defs: &dataflow::reaching_defs::ReachingDefsResult,
    stmt_id: hir_def::hir::StmtId,
    depth: u32,
) -> Option<String> {
    const MAX_DEPTH: u32 = 10;

    // Cycle protection
    if depth > MAX_DEPTH {
        return None;
    }

    match body.expr(expr_id) {
        // Direct string literal - base case
        Expr::Literal(Literal::String(s)) => Some(s.to_string()),

        // Variable reference - resolve using reaching definitions
        Expr::Path(var_name) => {
            let defs = reaching_defs.defs_for_var_at_stmt(var_name.as_str(), stmt_id)?;

            // Try to resolve all reaching definitions
            let mut resolved_values = std::collections::HashSet::new();

            for def in defs {
                if let Some(value) =
                    resolve_definition(&def, body, reaching_defs, stmt_id, depth + 1)
                {
                    resolved_values.insert(value);
                }
            }

            // If all definitions resolve to the same string literal, return it
            // Otherwise, None (ambiguous or couldn't resolve)
            if resolved_values.len() == 1 {
                resolved_values.into_iter().next()
            } else {
                None
            }
        }

        // Other expressions (method calls, field access, etc.) - not resolvable
        _ => None,
    }
}

/// Resolve a definition to its string literal value.
///
/// Handles assignment statements and recursively resolves transitive assignments.
fn resolve_definition(
    def: &dataflow::reaching_defs::Definition,
    body: &hir_def::Body,
    reaching_defs: &dataflow::reaching_defs::ReachingDefsResult,
    _current_stmt: hir_def::hir::StmtId,
    depth: u32,
) -> Option<String> {
    match def.def_site {
        dataflow::reaching_defs::DefSite::Assignment(assign_raw_idx) => {
            let assign_stmt_id = hir_def::hir::StmtId::from_raw(assign_raw_idx);

            if let Stmt::Assign { value, .. } = body.stmt(assign_stmt_id) {
                // Recursively resolve the assigned value
                // This handles both direct literals and transitive assignments
                resolve_expr_to_string_impl(*value, body, reaching_defs, assign_stmt_id, depth)
            } else {
                None
            }
        }
        // Parameters, var declarations, loop variables - can't resolve to string literals
        _ => None,
    }
}

/// Validate template string against parameter count.
///
/// Reused from hir-def lowering logic.
fn is_wrong_str_template(template_string: &str, used_params_count: usize) -> bool {
    use once_cell::sync::Lazy;
    use regex::Regex;

    static TWO_PERCENT_PATTERN: Lazy<Regex> = Lazy::new(|| Regex::new("%%").unwrap());

    let is_wrong_call = compare_template_and_params(template_string, used_params_count);
    if !is_wrong_call {
        return false;
    }

    // Remove %% escapes and check again
    let str = TWO_PERCENT_PATTERN.replace_all(template_string, "");
    compare_template_and_params(&str, used_params_count)
}

#[allow(clippy::nonminimal_bool)]
fn compare_template_and_params(template_string: &str, used_params_count: usize) -> bool {
    use once_cell::sync::Lazy;
    use regex::Regex;

    static PARAMS_PATTERN_INNER: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"%(?:(10|[1-9])|\((10|[1-9])\))").unwrap());

    static WRONG_NUMBERS_PATTERN_INNER: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"%(?:(1[1-9]\d*|[2-9]\d+|0|10\d+)|\((1[1-9]\d*|[2-9]\d+|0|10\d+)\))").unwrap()
    });

    let have_params = used_params_count > 0;
    let matches = PARAMS_PATTERN_INNER.is_match(template_string);

    (matches && !have_params)
        || (!matches && have_params)
        || (matches && various_params(used_params_count, template_string))
        || WRONG_NUMBERS_PATTERN_INNER.is_match(template_string)
}

fn various_params(used_params_count: usize, template_string: &str) -> bool {
    use once_cell::sync::Lazy;
    use regex::Regex;
    use std::collections::HashSet;

    static PARAMS_PATTERN: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"%(?:(10|[1-9])|\((10|[1-9])\))").unwrap());

    let mut template_params = HashSet::new();
    let bytes = template_string.as_bytes();

    for cap in PARAMS_PATTERN.captures_iter(template_string) {
        let match_obj = cap.get(0).unwrap();
        let pos = match_obj.start();

        // Skip if this is part of %% escape sequence
        if pos > 0 && bytes.get(pos - 1) == Some(&b'%') {
            continue;
        }

        let group = cap.get(1).or_else(|| cap.get(2));
        if let Some(g) = group {
            if let Ok(index) = g.as_str().parse::<usize>() {
                if index > used_params_count {
                    return true;
                }
                template_params.insert(index);
            }
        }
    }

    for i in 1..=used_params_count {
        if !template_params.contains(&i) {
            return true;
        }
    }

    false
}

fn count_required_params(template_string: &str) -> usize {
    use once_cell::sync::Lazy;
    use regex::Regex;

    static PARAMS_PATTERN: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"%(?:(10|[1-9])|\((10|[1-9])\))").unwrap());

    let mut max_param = 0;
    let bytes = template_string.as_bytes();

    for cap in PARAMS_PATTERN.captures_iter(template_string) {
        let match_obj = cap.get(0).unwrap();
        let pos = match_obj.start();

        // Skip if this is part of %% escape sequence
        if pos > 0 && bytes.get(pos - 1) == Some(&b'%') {
            continue;
        }

        let group = cap.get(1).or_else(|| cap.get(2));
        if let Some(g) = group {
            if let Ok(index) = g.as_str().parse::<usize>() {
                max_param = max_param.max(index);
            }
        }
    }

    max_param
}

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when IncorrectUseOfStrTemplate diagnostic is emitted during lowering.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::IncorrectUseOfStrTemplate) {
        return None;
    }

    Some(Diagnostic {
        code: DiagnosticCode::IncorrectUseOfStrTemplate,
        message: "Incorrect use of StrTemplate".to_string(),
        severity: Severity::Error,
        range,
        tags: vec![],
        fixes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use crate::test_utils::*;
    use crate::DiagnosticCode;

    #[test]
    fn test_correct_usage() {
        let code = r#"
Процедура Тест()
    Г = СтрШаблон("Наименование (версия %1)", Версия());
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let filtered: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        assert_eq!(filtered.len(), 0, "Should not detect correct usage");
    }

    #[test]
    fn test_missing_parameter() {
        let code = r#"
Процедура Тест()
    А = СтрШаблон("Наименование (версия %1)");
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let filtered: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        assert_eq!(filtered.len(), 1, "Should detect missing parameter");
        assert_diagnostic_range(code, filtered[0], 2, 8, 45);
    }

    #[test]
    fn test_insufficient_arguments() {
        let code = r#"
Процедура Тест()
    Б = СтрШаблон("%1 (версия %2)", Наименование);
КонецПроцедуры
"#;
        let diagnostics = check_hir_diagnostic(code);
        let filtered: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        assert_eq!(filtered.len(), 1, "Should detect insufficient arguments");
    }

    #[test]
    fn test_comprehensive() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabase, RootDatabaseImpl,
        };
        use std::sync::Arc;
        use test_fixture::Fixture;

        let code = include_str!("../../test_data/IncorrectUseOfStrTemplateDiagnostic.bsl");
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        // Set up source root for module_bodies to work
        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        let config = crate::DiagnosticsConfig::default();
        let ctx = crate::DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        // Call full diagnostic pipeline (includes both HIR lowering + post-HIR check())
        let diagnostics = crate::diagnostics(&ctx);

        let filtered: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();

        // Java expects 12 diagnostics total
        // Now detecting all 12:
        // - 9 direct string literals (from HIR lowering)
        // - 3 variable resolution cases (lines 17, 21, 25) - now supported via ReachingDefs
        assert_eq!(
            filtered.len(),
            12,
            "Should detect all 12 errors (100% coverage with ReachingDefs)"
        );
    }

    #[test]
    fn test_variable_resolution_simple() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabase, RootDatabaseImpl,
        };
        use std::sync::Arc;
        use test_fixture::Fixture;

        let code = r#"
Процедура Тест()
    НовыйШаблон = "123";
    А = СтрШаблон(НовыйШаблон, Наименование); // ошибка: "123" не содержит %1
КонецПроцедуры
"#;
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        let config = crate::DiagnosticsConfig::default();
        let ctx = crate::DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = crate::diagnostics(&ctx);
        let filtered: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        assert_eq!(filtered.len(), 1, "Should detect unused parameter");
    }

    #[test]
    fn test_variable_resolution_with_template() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabase, RootDatabaseImpl,
        };
        use std::sync::Arc;
        use test_fixture::Fixture;

        let code = r#"
Процедура Тест()
    НовыйШаблон = "%1";
    А = СтрШаблон(НовыйШаблон, Наименование); // OK
КонецПроцедуры
"#;
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        let config = crate::DiagnosticsConfig::default();
        let ctx = crate::DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = crate::diagnostics(&ctx);
        let filtered: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        assert_eq!(filtered.len(), 0, "Should not report valid template");
    }

    #[test]
    fn test_variable_resolution_conditional() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabase, RootDatabaseImpl,
        };
        use std::sync::Arc;
        use test_fixture::Fixture;

        let code = r#"
Процедура Тест()
    Если Условие Тогда
        Шаблон = "123";
    Иначе
        Шаблон = "%1";
    КонецЕсли;
    А = СтрШаблон(Шаблон, Наименование);
КонецПроцедуры
"#;
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        let config = crate::DiagnosticsConfig::default();
        let ctx = crate::DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = crate::diagnostics(&ctx);
        let filtered: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        // ReachingDefs handles multiple definitions - no false positive
        assert_eq!(filtered.len(), 0, "Should handle multiple reaching definitions");
    }

    #[test]
    fn test_transitive_assignment() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabase, RootDatabaseImpl,
        };
        use std::sync::Arc;
        use test_fixture::Fixture;

        let code = r#"
Процедура Тест()
    Шаблон1 = "template %1";
    Шаблон2 = Шаблон1;
    А = СтрШаблон(Шаблон2, Наименование); // OK - resolves through transitive assignment
КонецПроцедуры
"#;
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        let config = crate::DiagnosticsConfig::default();
        let ctx = crate::DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = crate::diagnostics(&ctx);
        let filtered: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        assert_eq!(filtered.len(), 0, "Should resolve transitive assignment");
    }

    #[test]
    fn test_transitive_assignment_error() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabase, RootDatabaseImpl,
        };
        use std::sync::Arc;
        use test_fixture::Fixture;

        let code = r#"
Процедура Тест()
    Шаблон1 = "no placeholders";
    Шаблон2 = Шаблон1;
    А = СтрШаблон(Шаблон2, Наименование); // Error - resolves to "no placeholders"
КонецПроцедуры
"#;
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        let config = crate::DiagnosticsConfig::default();
        let ctx = crate::DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = crate::diagnostics(&ctx);
        let filtered: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        assert_eq!(filtered.len(), 1, "Should detect error through transitive assignment");
    }

    #[test]
    fn test_deep_transitive_chain() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabase, RootDatabaseImpl,
        };
        use std::sync::Arc;
        use test_fixture::Fixture;

        let code = r#"
Процедура Тест()
    Ш1 = "template %1";
    Ш2 = Ш1;
    Ш3 = Ш2;
    Ш4 = Ш3;
    А = СтрШаблон(Ш4, Наименование); // OK - resolves through chain
КонецПроцедуры
"#;
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        let config = crate::DiagnosticsConfig::default();
        let ctx = crate::DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = crate::diagnostics(&ctx);
        let filtered: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        assert_eq!(filtered.len(), 0, "Should resolve deep transitive chain");
    }

    #[test]
    fn test_multiple_defs_same_value() {
        use ide_db::{
            base_db::{SourceDatabase, SourceRoot, SourceRootId},
            RootDatabase, RootDatabaseImpl,
        };
        use std::sync::Arc;
        use test_fixture::Fixture;

        let code = r#"
Процедура Тест()
    Если Условие Тогда
        Шаблон = "template %1";
    Иначе
        Шаблон = "template %1";  // Same value as then branch
    КонецЕсли;
    А = СтрШаблон(Шаблон, Наименование); // OK - both branches give same value
КонецПроцедуры
"#;
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();

        let mut file_set = vfs::FileSet::default();
        file_set.insert(file_id, vfs::VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        #[allow(clippy::arc_with_non_send_sync)]
        let db = Arc::new(db) as Arc<dyn RootDatabase>;

        let config = crate::DiagnosticsConfig::default();
        let ctx = crate::DiagnosticsContext {
            db: db.as_ref(),
            config: &config,
            file_id,
            workspace_root: None,
            configuration_path: None,
            configuration_path_input: None,
            file_set: None,
        };

        let diagnostics = crate::diagnostics(&ctx);
        let filtered: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IncorrectUseOfStrTemplate)
            .collect();
        assert_eq!(filtered.len(), 0, "Should accept when all definitions resolve to same value");
    }
}
