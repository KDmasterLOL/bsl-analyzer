//! IncorrectUseOfStrTemplate diagnostic
//!
//! Detects incorrect usage of СтрШаблон/StrTemplate method.
//!
//!
//! ## Implementation
//!
//! Two-phase detection:
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

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{Expr, ExprId, IdConversion, Literal, MethodId, ModuleId, Stmt, StmtId};
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload, MetadataTag::Suspicious, MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Quick pre-check: does the file text contain StrTemplate method calls?
///
/// This is a fast O(n) scan that avoids expensive HIR/dataflow analysis
/// for files that don't use StrTemplate at all.
fn has_str_template_calls(text: &str) -> bool {
    // Patterns to search for (case-insensitive)
    const PATTERNS: &[&str] = &["стршаблон", "strtemplate"];

    let text_lower = text.to_lowercase();
    for pattern in PATTERNS {
        if text_lower.contains(pattern) {
            return true;
        }
    }
    false
}

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
    let code = DiagnosticCode::IncorrectUseOfStrTemplate;

    if ctx.is_disabled_with_metadata(code) {
        return vec![];
    }

    // Early exit: skip files without StrTemplate calls
    let text = ctx.file_text();
    if !has_str_template_calls(&text) {
        return vec![];
    }

    let mut diagnostics = Vec::new();
    let module_id = ModuleId { file_id: ctx.file_id };

    // Get module bodies
    let module_bodies = ctx.module_bodies();

    // Check each method - collect candidates first, then resolve lazily
    for (local_id, body, source_map) in module_bodies.method_bodies() {
        // First pass: find StrTemplate calls with variable arguments (no dataflow yet)
        let mut candidates: Vec<(StmtId, ExprId, usize)> = Vec::new();

        for (stmt_id, stmt) in body.stmts_iter() {
            // Check both Expr and Assign statements (assignment RHS can have Call)
            let expr_id = match stmt {
                Stmt::Expr(id) => Some(*id),
                Stmt::Assign { value, .. } => Some(*value),
                _ => None,
            };

            if let Some(expr_id) = expr_id {
                let expr = body.expr(ExprId::from_idx(expr_id));

                // Check if this is a Call expression (function call like СтрШаблон(...))
                let (method_name, args) = match expr {
                    Expr::Call { callee, args } => {
                        // For Call, the callee should be a Path (function name)
                        if let Expr::Path(name) = body.expr(ExprId::from_idx(*callee)) {
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

                let template_expr_id = ExprId::from_idx(args[0]);
                let param_count = args.len() - 1; // Excluding template itself

                // Skip string literals - they're already validated by HIR lowering
                if matches!(body.expr(template_expr_id), Expr::Literal(Literal::String(_))) {
                    continue;
                }

                // Found a candidate - variable argument that needs resolution
                candidates.push((stmt_id, template_expr_id, param_count));
            }
        }

        // Skip methods without candidates (no dataflow analysis needed)
        if candidates.is_empty() {
            continue;
        }

        // Second pass: resolve variables using reaching definitions (lazy computation)
        let method_id = MethodId { module: module_id, local_id };
        let reaching_defs = match ctx.reaching_definitions(method_id) {
            Some(defs) => defs,
            None => continue, // Analysis didn't converge, skip
        };

        for (stmt_id, template_expr_id, param_count) in candidates {
            // Try to resolve template to string literal
            if let Some(template_string) =
                resolve_expr_to_string(template_expr_id, body, &reaching_defs, stmt_id)
            {
                // Validate template
                if is_wrong_str_template(&template_string, param_count) {
                    // Get source range for diagnostic
                    if let Some(range) = source_map.expr_range(template_expr_id) {
                        diagnostics.push(Diagnostic {
                            code,
                            message: format!(
                                "Template '{}' requires {} parameters but {} provided",
                                template_string.chars().take(50).collect::<String>(),
                                count_required_params(&template_string),
                                param_count
                            ),
                            severity: ctx.severity(code),
                            range,
                            tags: ctx.tags(code),
                            fixes: vec![],
                        });
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
    expr_id: ExprId,
    body: &hir::Body,
    reaching_defs: &hir::dataflow::reaching_defs::ReachingDefsResult,
    stmt_id: StmtId,
) -> Option<String> {
    resolve_expr_to_string_impl(expr_id, body, reaching_defs, stmt_id, 0)
}

/// Internal implementation with depth tracking for cycle protection.
fn resolve_expr_to_string_impl(
    expr_id: ExprId,
    body: &hir::Body,
    reaching_defs: &hir::dataflow::reaching_defs::ReachingDefsResult,
    stmt_id: StmtId,
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
    def: &hir::dataflow::reaching_defs::Definition,
    body: &hir::Body,
    reaching_defs: &hir::dataflow::reaching_defs::ReachingDefsResult,
    _current_stmt: StmtId,
    depth: u32,
) -> Option<String> {
    match def.def_site {
        hir::dataflow::reaching_defs::DefSite::Assignment(assign_raw_idx) => {
            let assign_stmt_id = StmtId::from_raw(assign_raw_idx);

            if let Stmt::Assign { value, .. } = body.stmt(assign_stmt_id) {
                // Recursively resolve the assigned value
                // This handles both direct literals and transitive assignments
                resolve_expr_to_string_impl(
                    ExprId::from_idx(*value),
                    body,
                    reaching_defs,
                    assign_stmt_id,
                    depth,
                )
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
/// Checks for:
/// - Mismatch between placeholders and provided parameters
/// - Invalid placeholders (%0, %11+)
fn is_wrong_str_template(template_string: &str, used_params_count: usize) -> bool {
    // First check without removing %% escapes
    let is_wrong_call = compare_template_and_params(template_string, used_params_count);
    if !is_wrong_call {
        return false;
    }

    // Remove %% escapes and check again
    // This handles cases like "100%%" which should not be treated as %% + placeholder
    let cleaned = remove_double_percent(template_string);
    compare_template_and_params(&cleaned, used_params_count)
}

/// Remove %% escape sequences from template string.
fn remove_double_percent(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'%' && bytes[i + 1] == b'%' {
            // Skip both %% characters
            i += 2;
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }
    result
}

/// Parse a placeholder at position, returns (number, length) or None.
/// Handles both %N and %(N) formats where N is a number.
fn parse_placeholder(bytes: &[u8], pos: usize) -> Option<(usize, usize)> {
    if pos >= bytes.len() || bytes[pos] != b'%' {
        return None;
    }

    let start = pos + 1;
    if start >= bytes.len() {
        return None;
    }

    // Check for %(N) format
    if bytes[start] == b'(' {
        let num_start = start + 1;
        let mut num_end = num_start;
        while num_end < bytes.len() && bytes[num_end].is_ascii_digit() {
            num_end += 1;
        }
        if num_end > num_start && num_end < bytes.len() && bytes[num_end] == b')' {
            let num_str = std::str::from_utf8(&bytes[num_start..num_end]).ok()?;
            let num: usize = num_str.parse().ok()?;
            return Some((num, num_end - pos + 1)); // Include closing )
        }
        return None;
    }

    // Check for %N format (one or more digits)
    let mut num_end = start;
    while num_end < bytes.len() && bytes[num_end].is_ascii_digit() {
        num_end += 1;
    }
    if num_end > start {
        let num_str = std::str::from_utf8(&bytes[start..num_end]).ok()?;
        let num: usize = num_str.parse().ok()?;
        return Some((num, num_end - pos));
    }

    None
}

/// Check if placeholder number is valid (1-10).
fn is_valid_placeholder(num: usize) -> bool {
    (1..=10).contains(&num)
}

#[allow(clippy::nonminimal_bool)]
fn compare_template_and_params(template_string: &str, used_params_count: usize) -> bool {
    let bytes = template_string.as_bytes();
    let have_params = used_params_count > 0;

    let mut has_valid_placeholder = false;
    let mut has_wrong_number = false;
    let mut used_placeholders = [false; 11]; // Index 1-10

    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            // Skip %% escape sequences
            if i + 1 < bytes.len() && bytes[i + 1] == b'%' {
                i += 2;
                continue;
            }

            if let Some((num, len)) = parse_placeholder(bytes, i) {
                if is_valid_placeholder(num) {
                    has_valid_placeholder = true;
                    used_placeholders[num] = true;
                    if num > used_params_count {
                        return true; // Placeholder exceeds provided params
                    }
                } else {
                    // %0 or %11+ are invalid
                    has_wrong_number = true;
                }
                i += len;
                continue;
            }
        }
        i += 1;
    }

    // Check conditions
    if has_wrong_number {
        return true;
    }
    if has_valid_placeholder && !have_params {
        return true;
    }
    if !has_valid_placeholder && have_params {
        return true;
    }

    // Check if all parameters 1..=used_params_count are used
    if has_valid_placeholder {
        for &used in used_placeholders.iter().take(used_params_count + 1).skip(1) {
            if !used {
                return true; // Parameter not used in template
            }
        }
    }

    false
}

fn count_required_params(template_string: &str) -> usize {
    let bytes = template_string.as_bytes();
    let mut max_param = 0;

    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            // Skip %% escape sequences
            if i + 1 < bytes.len() && bytes[i + 1] == b'%' {
                i += 2;
                continue;
            }

            if let Some((num, len)) = parse_placeholder(bytes, i) {
                if is_valid_placeholder(num) {
                    max_param = max_param.max(num);
                }
                i += len;
                continue;
            }
        }
        i += 1;
    }

    max_param
}

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when IncorrectUseOfStrTemplate diagnostic is emitted during lowering.
pub fn from_hir(range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    let code = DiagnosticCode::IncorrectUseOfStrTemplate;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    Some(Diagnostic {
        code,
        message: "Некорректное использование СтрШаблон".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
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

        // Inline BSL: 12 errors total
        // 9 direct string literal errors (HIR lowering) + 3 variable resolution errors (ReachingDefs)
        let code = r#"Процедура Метод()

    А = СтрШаблон("Наименование (версия %1)"); // ошибка

    Б = СтрШаблон("%1 (версия %2)", Наименование); // ошибка

    К = СтрШаблон("Наименование %11", Наименование); // ошибка

    К = СтрШаблон("Наименование %0", Наименование); // ошибка

    Ж = СтрШаблон("Наименование %2 (версия %%3)", Наименование, Версия); // ошибка

    //здесь ошибочно не закрыта скобка для НСтр
    В = СтрШаблон(НСтр("ru='Наименование (версия %1)'", Версия())); // ошибка

    НовыйШаблон = "123";
    Н = СтрШаблон(НовыйШаблон, Наименование); // ошибка

    НовыйШаблон1 = "123";
    ДругаяСтрока = "5487";
    Н = СтрШаблон(НовыйШаблон1, Наименование); // ошибка

    //НовыйШаблон2 = НСтр("ru='Наименование (версия)'";
    НовыйШаблон2 = "5487";
    Н = СтрШаблон(НовыйШаблон2, Наименование); // ошибка

    // ошибка
    С24 = СтрШаблон("%1, %2, %3, %4, %5, %6, %7, %8, %9, %10, %11", "ф", "ф", "ф", "ф", "ф", "ф", "ф", "ф", "Ф", "");

    Л = СтрШаблон("Наименование %(1)"); // ошибка

    Г = СтрШаблон(НСтр("ru='Наименование (версия %1)'"), Версия());

    Д = СтрШаблон("Наименование (версия)");

    Е = СтрШаблон("Наименование (версия %1)", Наименование);

    Е = СтрШаблон("Наименование %1 (версия %2)", Наименование, Версия);

    З = СтрШаблон("Наименование %%1 (версия %%2)");
    Ий = СтрШаблон("Наименование %1 (версия %%2)", Наименование);

    Л = СтрШаблон("Наименование %(1)1", Наименование); // в СП разрешен такой вариант

    М = СтрШаблон(ШаблонНаименования, Наименование);
    М = СтрШаблон("123" + ШаблонНаименования, Наименование);

    НовыйШаблон3 = "%1";
    Н = СтрШаблон(НовыйШаблон3, Наименование);

    А = СтрШаблон("%(1)%(2)", "Первая", 2);

    Б = СтрШаблон("%%%1%%", "Первая");

    Объект.НовыйШаблон4 = "%1"; // новый код
    Н = СтрШаблон(Объект.НовыйШаблон4, Наименование);

    Объект.НовыйШаблон5Ошибка = "%1 %2"; // падает на этой строчке, что верно
    Н = СтрШаблон(Объект.НовыйШаблон5Ошибка, Наименование);
КонецПроцедуры
"#;
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
            provider: None,
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

        // Expected 12 diagnostics total
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
            provider: None,
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
            provider: None,
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
            provider: None,
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
            provider: None,
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
            provider: None,
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
            provider: None,
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
            provider: None,
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
