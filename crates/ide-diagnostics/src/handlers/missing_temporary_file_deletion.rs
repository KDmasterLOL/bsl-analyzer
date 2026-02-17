//! MissingTemporaryFileDeletion diagnostic.
//!
//! Detects temporary files created with GetTempFileName() that are not properly deleted.
//!
//! ## Why?
//!
//! Temporary files created with `ПолучитьИмяВременногоФайла()` / `GetTempFileName()` must be
//! explicitly deleted after use. Failure to delete temporary files can:
//! - Exhaust disk space in temp directory
//! - Leave sensitive data on disk
//! - Cause issues in long-running server processes
//!
//! ## Bad practice
//!
//! ```bsl
//! Процедура ОбработатьДанные()
//!     ИмяФайла = ПолучитьИмяВременногоФайла("xml");
//!     // ... use file ...
//! КонецПроцедуры  // ❌ Temporary file not deleted!
//! ```
//!
//! ## Good practice
//!
//! ```bsl
//! Процедура ОбработатьДанные()
//!     ИмяФайла = ПолучитьИмяВременногоФайла("xml");
//!     Попытка
//!         // ... use file ...
//!     Исключение
//!         УдалитьФайлы(ИмяФайла);  // ✅ Clean up on error
//!         ВызватьИсключение;
//!     КонецПопытки;
//!     УдалитьФайлы(ИмяФайла);  // ✅ Clean up on success
//! КонецПроцедуры
//! ```
//!
//! ## Configuration
//!
//! The diagnostic supports one configuration parameter:
//!
//! - **searchDeleteFileMethod** (string, regex pattern):
//!   Pipe-separated list of method names that delete/move files.
//!   Default: `"УдалитьФайлы|DeleteFiles|НачатьУдалениеФайлов|BeginDeletingFiles|ПереместитьФайл|MoveFile"`
//!
//! Example configuration:
//!
//! ```json
//! {
//!   "diagnostics": {
//!     "MissingTemporaryFileDeletion": {
//!       "searchDeleteFileMethod": "УдалитьФайлы|DeleteFiles|РаботаСФайламиКлиент.УдалитьФайл"
//!     }
//!   }
//! }
//! ```
//!
//! ## Configuration
//!
//! - **Enabled by default:** Yes
//! - **Severity:** Major (ERROR)
//! - **Tags:** BADPRACTICE, STANDARD
//! - **Minutes to fix:** 5
//!
//! ## Implementation
//!
//! Ported from:
//! - MissingTemporaryFileDeletionDiagnostic.java (bsl-language-server) - COMPATIBILITY TARGET

use cfg_types::{IdConversion, StmtId};
use hir_def::hir::{Expr, ExprIdx, Stmt};
use hir_def::{Body, BodySourceMap};

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use regex::Regex;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice, MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Default deletion methods pattern
const DEFAULT_SEARCH_DELETE_FILE_METHOD: &str =
    "УдалитьФайлы|DeleteFiles|НачатьУдалениеФайлов|BeginDeletingFiles|ПереместитьФайл|MoveFile";

/// Configuration for MissingTemporaryFileDeletion diagnostic
#[derive(Debug, Clone)]
struct Config {
    /// Regex pattern for deletion/move methods (case-insensitive)
    deletion_methods: Regex,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let pattern = ctx
            .config
            .get_string(DiagnosticCode::MissingTemporaryFileDeletion, "searchDeleteFileMethod")
            .unwrap_or(DEFAULT_SEARCH_DELETE_FILE_METHOD);

        // Create case-insensitive regex with error handling
        // Anchor with ^ and $ to match full method names only (not substrings)
        let regex_pattern = format!("(?i)^({})$", pattern);
        let deletion_methods = Regex::new(&regex_pattern).unwrap_or_else(|e| {
            tracing::warn!(
                error = %e,
                pattern = %pattern,
                "Invalid searchDeleteFileMethod regex, using default"
            );
            Regex::new(&format!("(?i)^({})$", DEFAULT_SEARCH_DELETE_FILE_METHOD))
                .expect("Default regex must be valid")
        });

        tracing::debug!(pattern = %pattern, "MissingTemporaryFileDeletion config loaded");
        Self { deletion_methods }
    }
}

/// HIR + CFG based entry point for MissingTemporaryFileDeletion diagnostic.
///
/// Uses Salsa-cached module_bodies and module_cfgs instead of raw AST traversal.
///
/// ## Algorithm
///
/// For each method in the module:
/// 1. Walk HIR expressions to find GetTempFileName() calls
/// 2. Determine if call is assigned to a variable (Assign stmt) or inline
/// 3. For assigned calls: check if any deletion call in the body uses that variable
/// 4. Use CFG to verify the deletion call is reachable from the GetTempFileName call
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::MissingTemporaryFileDeletion;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let module_bodies = ctx.module_bodies();
    let module_cfgs = ctx.module_cfgs();
    let mut diagnostics = Vec::new();

    // Check each method body
    for (local_id, body) in module_bodies.iter_bodies() {
        let Some(source_map) = module_bodies.source_map(local_id) else { continue };
        let cfg = module_cfgs.get(local_id);

        diagnostics.extend(check_body(
            body,
            source_map,
            cfg.map(|c| c.as_ref()),
            &config,
            code,
            ctx,
        ));
    }

    // Check module-level code
    if let Some(module_result) = module_bodies.module_code_result() {
        let body = &module_result.body;
        let source_map = &module_result.source_map;
        let cfg = cfg::CfgBuilder::new().build_graph_from_hir(
            body.body_stmts_typed(),
            body,
            Some(source_map),
        );

        diagnostics.extend(check_body(body, source_map, Some(&cfg), &config, code, ctx));
    }

    diagnostics.sort_by_key(|d| d.range.start());
    diagnostics
}

/// Check a single HIR body for MissingTemporaryFileDeletion.
fn check_body(
    body: &Body,
    source_map: &BodySourceMap,
    cfg: Option<&cfg::ControlFlowGraph>,
    config: &Config,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Build set of ExprIdx that are direct RHS of Assign statements
    // Maps: value_expr → target_expr (for extracting variable name)
    let mut assigned_values: rustc_hash::FxHashMap<ExprIdx, ExprIdx> =
        rustc_hash::FxHashMap::default();
    // Also map value_expr → StmtId (for CFG lookup)
    let mut value_to_stmt: rustc_hash::FxHashMap<ExprIdx, StmtId> =
        rustc_hash::FxHashMap::default();

    for (stmt_id, stmt) in body.stmts_iter() {
        if let Stmt::Assign { target, value } = stmt {
            assigned_values.insert(*value, *target);
            value_to_stmt.insert(*value, stmt_id);
        }
    }

    // Build stmt→block mapping for CFG reachability (if CFG available)
    let stmt_to_block = cfg.map(build_stmt_to_block_map);

    // Find all GetTempFileName calls in HIR expressions
    for (expr_id, expr) in body.exprs_iter() {
        let Expr::Call { callee, .. } = expr else { continue };
        let Expr::Path(name) = body.expr_idx(*callee) else { continue };
        if !is_get_temp_filename(name.as_str()) {
            continue;
        }

        let call_expr_idx: ExprIdx = expr_id.to_idx();

        // Check if this call is the direct RHS of an Assign
        if let Some(&target_idx) = assigned_values.get(&call_expr_idx) {
            let Expr::Path(var_name) = body.expr_idx(target_idx) else { continue };

            // Check if there's a deletion call for this variable
            let has_deletion = has_deletion_in_body(
                body,
                var_name.as_str(),
                config,
                value_to_stmt.get(&call_expr_idx).copied(),
                stmt_to_block.as_ref(),
                cfg,
            );

            if !has_deletion {
                if let Some(range) = source_map.expr_range(expr_id) {
                    diagnostics.push(Diagnostic {
                        code,
                        message: format!(
                            "Нужно добавить удаление временного файла '{}' после использования",
                            var_name.as_str()
                        ),
                        severity: ctx.severity(code),
                        range,
                        tags: ctx.tags(code),
                        fixes: vec![],
                    });
                }
            }
        } else {
            // Inline usage (not assigned) → always error
            if let Some(range) = source_map.expr_range(expr_id) {
                diagnostics.push(Diagnostic {
                    code,
                    message: "Нужно добавить удаление временного файла после использования"
                        .to_string(),
                    severity: ctx.severity(code),
                    range,
                    tags: ctx.tags(code),
                    fixes: vec![],
                });
            }
        }
    }

    diagnostics
}

/// Check if token text is GetTempFileName (case-insensitive).
fn is_get_temp_filename(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "получитьимявременногофайла" || lower == "gettempfilename"
}

/// Check if body contains a deletion call for the given variable.
///
/// Uses CFG to verify the deletion call is reachable from the GetTempFileName statement.
fn has_deletion_in_body(
    body: &Body,
    var_name: &str,
    config: &Config,
    get_temp_stmt: Option<StmtId>,
    stmt_to_block: Option<&rustc_hash::FxHashMap<StmtId, cfg::NodeIndex>>,
    cfg: Option<&cfg::ControlFlowGraph>,
) -> bool {
    // Find the CFG block containing GetTempFileName (for reachability check)
    let get_temp_block = get_temp_stmt.and_then(|s| stmt_to_block.and_then(|m| m.get(&s).copied()));

    for (_, stmt) in body.stmts_iter() {
        // Only check expression statements and assignments that could contain deletion calls
        let call_expr_ids = match stmt {
            Stmt::Expr(expr_idx) => vec![*expr_idx],
            Stmt::Assign { value, .. } => vec![*value],
            _ => continue,
        };

        for call_idx in call_expr_ids {
            if check_expr_for_deletion(body, call_idx, var_name, config) {
                // If CFG is available, verify reachability
                if let (Some(from_block), Some(cfg_ref)) = (get_temp_block, cfg) {
                    let deletion_reachable =
                        is_deletion_reachable_from(body, cfg_ref, from_block, var_name, config);
                    return deletion_reachable;
                }
                return true;
            }
        }
    }

    false
}

/// Check if an expression (or its subexpressions) is a deletion call for the variable.
fn check_expr_for_deletion(
    body: &Body,
    expr_idx: ExprIdx,
    var_name: &str,
    config: &Config,
) -> bool {
    match body.expr_idx(expr_idx) {
        Expr::Call { callee, args } => {
            let method_path = extract_call_path(body, *callee);
            if config.deletion_methods.is_match(&method_path)
                && args.iter().any(|&a| expr_contains_var(body, a, var_name))
            {
                return true;
            }
            // Check nested calls in arguments
            args.iter().any(|&a| check_expr_for_deletion(body, a, var_name, config))
        }
        Expr::MethodCall { receiver, method, args } => {
            // Check method name for deletion pattern
            if config.deletion_methods.is_match(method.as_str())
                && args.iter().any(|&a| expr_contains_var(body, a, var_name))
            {
                return true;
            }
            // Check receiver and args for nested deletion calls
            check_expr_for_deletion(body, *receiver, var_name, config)
                || args.iter().any(|&a| check_expr_for_deletion(body, a, var_name, config))
        }
        _ => false,
    }
}

/// Extract method path from a Call expression's callee.
fn extract_call_path(body: &Body, callee: ExprIdx) -> String {
    match body.expr_idx(callee) {
        Expr::Path(name) => name.as_str().to_string(),
        Expr::Field { base, field } => {
            let base_path = extract_call_path(body, *base);
            if base_path.is_empty() {
                field.as_str().to_string()
            } else {
                format!("{}.{}", base_path, field.as_str())
            }
        }
        _ => String::new(),
    }
}

/// Check if expression tree contains a reference to the variable (case-insensitive).
fn expr_contains_var(body: &Body, expr_idx: ExprIdx, var_name: &str) -> bool {
    match body.expr_idx(expr_idx) {
        Expr::Path(name) => name.as_str().eq_ignore_ascii_case(var_name),
        Expr::Call { callee, args } => {
            expr_contains_var(body, *callee, var_name)
                || args.iter().any(|&a| expr_contains_var(body, a, var_name))
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_contains_var(body, *receiver, var_name)
                || args.iter().any(|&a| expr_contains_var(body, a, var_name))
        }
        Expr::Field { base, .. } => expr_contains_var(body, *base, var_name),
        Expr::Index { base, index } => {
            expr_contains_var(body, *base, var_name) || expr_contains_var(body, *index, var_name)
        }
        Expr::BinaryOp { lhs, rhs, .. } => {
            expr_contains_var(body, *lhs, var_name) || expr_contains_var(body, *rhs, var_name)
        }
        Expr::UnaryOp { expr, .. } => expr_contains_var(body, *expr, var_name),
        Expr::New { args, .. } => args.iter().any(|&a| expr_contains_var(body, a, var_name)),
        _ => false,
    }
}

/// Build mapping from StmtId → CFG NodeIndex (basic block).
fn build_stmt_to_block_map(
    cfg: &cfg::ControlFlowGraph,
) -> rustc_hash::FxHashMap<StmtId, cfg::NodeIndex> {
    let mut map = rustc_hash::FxHashMap::default();
    for (node_idx, vertex) in cfg.vertices() {
        if let cfg::CfgVertex::BasicBlock(block) = vertex {
            for &stmt_id in block.statements() {
                map.insert(stmt_id, node_idx);
            }
        }
    }
    map
}

/// Check if any deletion call is reachable from the GetTempFileName block via CFG.
fn is_deletion_reachable_from(
    body: &Body,
    cfg: &cfg::ControlFlowGraph,
    from_block: cfg::NodeIndex,
    var_name: &str,
    config: &Config,
) -> bool {
    // BFS from from_block to find any block containing a matching deletion call
    let mut visited = rustc_hash::FxHashSet::default();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(from_block);
    visited.insert(from_block);

    while let Some(current) = queue.pop_front() {
        // Check if current block contains a deletion call
        if let Some(cfg::CfgVertex::BasicBlock(block)) = cfg.vertex(current) {
            for &stmt_id in block.statements() {
                let stmt = body.stmt(stmt_id);
                let call_exprs = match stmt {
                    Stmt::Expr(e) => vec![*e],
                    Stmt::Assign { value, .. } => vec![*value],
                    _ => continue,
                };
                for call_idx in call_exprs {
                    if check_expr_for_deletion(body, call_idx, var_name, config) {
                        return true;
                    }
                }
            }
        }

        // Enqueue successors
        for (succ, _edge_type) in cfg.outgoing_edges(current) {
            if visited.insert(succ) {
                queue.push_back(succ);
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::*;
    use crate::{DiagnosticCode, DiagnosticsConfig};

    #[test]
    fn test_default_config() {
        let code = include_str!("../../test_data/MissingTemporaryFileDeletionDiagnostic.bsl");
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Expect 7 diagnostics with default configuration
        assert_eq!(diagnostics.len(), 7, "Expected 7 diagnostics with default config");

        // Verify exact positions match Java implementation
        // Line 6: ИмяПромежуточногоФайла = ПолучитьИмяВременногоФайла("xml")
        assert_diagnostic_range(code, &diagnostics[0], 6, 29, 62);

        // Line 19: ИмяПромежуточногоФайла4 = ПолучитьИмяВременногоФайла("xml")
        assert_diagnostic_range(code, &diagnostics[1], 19, 30, 63);

        // Line 25: ИмяПромежуточногоФайла5 = ПолучитьИмяВременногоФайла("xml")
        assert_diagnostic_range(code, &diagnostics[2], 25, 30, 63);

        // Line 45: ИмяПромежуточногоФайла = ПолучитьИмяВременногоФайла("xml")
        assert_diagnostic_range(code, &diagnostics[3], 45, 29, 62);

        // Line 49: ИмяПромежуточногоФайла2 = ПолучитьИмяВременногоФайла("txt")
        assert_diagnostic_range(code, &diagnostics[4], 49, 30, 63);

        // Line 64: ИмяПромежуточногоФайла3 = ПолучитьИмяВременногоФайла()
        assert_diagnostic_range(code, &diagnostics[5], 64, 30, 58);

        // Line 71: ИмяФайлаНаДиске = ПолучитьИмяВременногоФайла()
        assert_diagnostic_range(code, &diagnostics[6], 71, 26, 54);
    }

    #[test]
    fn test_extended_config() {
        let code = include_str!("../../test_data/MissingTemporaryFileDeletionDiagnostic.bsl");

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingTemporaryFileDeletion,
            serde_json::json!({
                "searchDeleteFileMethod": "УдалитьФайлы|DeleteFiles|НачатьУдалениеФайлов|BeginDeletingFiles|ПереместитьФайл|MoveFile|РаботаСФайламиСлужебныйКлиент.УдалитьФайл|Справочники.ОбщийМодуль.УдалитьВсеФайлы"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Expect 5 diagnostics with extended configuration
        assert_eq!(diagnostics.len(), 5, "Expected 5 diagnostics with extended config");

        // Lines 19 and 45 should no longer trigger (custom methods recognized)
        assert_diagnostic_range(code, &diagnostics[0], 6, 29, 62);
        assert_diagnostic_range(code, &diagnostics[1], 25, 30, 63);
        assert_diagnostic_range(code, &diagnostics[2], 49, 30, 63);
        assert_diagnostic_range(code, &diagnostics[3], 64, 30, 58);
        assert_diagnostic_range(code, &diagnostics[4], 71, 26, 54);
    }

    #[test]
    fn test_restrictive_config() {
        let code = include_str!("../../test_data/MissingTemporaryFileDeletionDiagnostic.bsl");

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingTemporaryFileDeletion,
            serde_json::json!({
                "searchDeleteFileMethod": "УдалитьФайл|DeleteFile|НачатьУдалениеФайловВсех|ОбщийМодуль.УдалитьВсеФайлы"
            }),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        // Debug: print all diagnostics
        println!("Got {} diagnostics:", diagnostics.len());
        for (i, d) in diagnostics.iter().enumerate() {
            let (line, start_col, _end_line, end_col) = range_to_line_col(code, d.range);
            println!("  {}: line {} col {}..{}", i + 1, line + 1, start_col, end_col);
        }

        // Expect 12 diagnostics with restrictive configuration
        assert_eq!(diagnostics.len(), 12, "Expected 12 diagnostics with restrictive config");
    }

    #[test]
    fn test_range_debug() {
        let code = r#"
Процедура Тест()
    ИмяПромежуточногоФайла = ПолучитьИмяВременногоФайла("xml"); // ошибка
КонецПроцедуры
        "#;
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        assert_eq!(diagnostics.len(), 1);
        let d = &diagnostics[0];
        println!("Range: {:?}", d.range);
        println!("Range start: {:?}, end: {:?}", d.range.start(), d.range.end());

        // Find the line
        let lines: Vec<&str> = code.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.contains("ПолучитьИмяВременногоФайла") {
                println!("Line {}: {}", i + 1, line);
                println!("Line length: {}", line.len());
            }
        }
    }

    #[test]
    fn test_debug() {
        use ide_db::base_db::{RootQueryDb, SourceDatabase};
        use ide_db::{RootDatabase, RootDatabaseImpl};
        use std::rc::Rc;
        use test_fixture::Fixture;
        // Debug test to see what tokens we're getting
        let code = r#"
            Процедура Тест()
                ИмяФайла = ПолучитьИмяВременногоФайла("xml");
            КонецПроцедуры
        "#;
        let fixture = Fixture::parse(&format!("//- /test.bsl\n{}", code));
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let parse = db.parse(file_id);
        let root = parse.syntax_node();

        // Print all tokens to see what we have
        let tokens: Vec<_> =
            root.descendants_with_tokens().filter_map(|el| el.into_token()).collect();

        println!("\n=== TOKENS ===");
        for (i, token) in tokens.iter().enumerate() {
            println!("{}: {:?} = '{}'", i, token.kind(), token.text());
        }

        // Now run the actual diagnostic
        let db = Rc::new(db) as Rc<dyn RootDatabase>;
        let config = DiagnosticsConfig::default();
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

        let diagnostics = super::check(&ctx);
        println!("\n=== DIAGNOSTICS ===");
        println!("Found {} diagnostics", diagnostics.len());
        for diag in &diagnostics {
            println!("{:?}", diag);
        }
    }

    #[test]
    fn test_inline_usage() {
        // Java version creates diagnostic for inline GetTempFileName usage (without assignment)
        // Example: Func(GetTempFileName("xml"))
        // This is ALWAYS an error because deletion cannot be tracked

        // Test 1: Pure inline usage without any assignment
        let code = r#"
            Процедура Тест()
                Записать(GetTempFileName("txt"));
                ПолучитьИмяВременногоФайла("xml");  // standalone call
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(
            diagnostics.len(),
            2,
            "Should create diagnostic for inline usage (matches Java behavior)"
        );

        // Both should have generic message (no variable name)
        for d in &diagnostics {
            assert_eq!(d.message, "Нужно добавить удаление временного файла после использования");
        }

        // Test 2: Inline usage inside expression
        let code2 = r#"
            Процедура Тест()
                Файл = Новый Файл(ПолучитьИмяВременногоФайла("xml"));
            КонецПроцедуры
        "#;
        let diagnostics2 = check_ast_diagnostic(code2, check);

        // This creates ONE diagnostic for the GetTempFileName call (inline usage)
        // Note: "Файл" is assigned but GetTempFileName itself has no assignment
        assert_eq!(diagnostics2.len(), 1, "Should create diagnostic for inline GetTempFileName");
    }

    #[test]
    fn test_comprehensive_java_compatibility() {
        // Comprehensive test to ensure 100% compatibility with Java implementation
        let code = r#"
            Процедура ТестВсехКейсов()
                // Case 1: Normal assignment with deletion - OK
                Файл1 = ПолучитьИмяВременногоФайла("xml");
                УдалитьФайлы(Файл1);

                // Case 2: Normal assignment without deletion - ERROR
                Файл2 = ПолучитьИмяВременногоФайла("xml");

                // Case 3: Inline usage in function call - ERROR
                Записать(GetTempFileName("txt"));

                // Case 4: Inline usage in expression - ERROR
                Файл3 = Новый Файл(ПолучитьИмяВременногоФайла("doc"));

                // Case 5: Standalone call - ERROR
                ПолучитьИмяВременногоФайла("tmp");

                // Case 6: Assignment with move (not deletion) - OK with default config
                Файл4 = ПолучитьИмяВременногоФайла("xml");
                ПереместитьФайл(Файл4, "новое_имя.xml");
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);

        // Expected errors:
        // - Файл2 (no deletion)
        // - GetTempFileName in Записать() (inline)
        // - ПолучитьИмяВременногоФайла in Новый Файл() (inline)
        // - Standalone ПолучитьИмяВременногоФайла (inline)
        // Total: 4 diagnostics

        println!("Found {} diagnostics:", diagnostics.len());
        for (i, d) in diagnostics.iter().enumerate() {
            println!("  {}: {}", i + 1, d.message);
        }

        assert_eq!(diagnostics.len(), 4, "Should find exactly 4 errors (3 inline + 1 no deletion)");

        // Verify messages
        let inline_count = diagnostics
            .iter()
            .filter(|d| d.message == "Нужно добавить удаление временного файла после использования")
            .count();

        let var_count = diagnostics.iter().filter(|d| d.message.contains("Файл")).count();

        assert_eq!(inline_count, 3, "Should have 3 inline usage errors");
        assert_eq!(var_count, 1, "Should have 1 variable without deletion error");
    }

    #[test]
    fn test_simple_cases() {
        // Valid: file is deleted
        let code = r#"
            Процедура Тест()
                ИмяФайла = ПолучитьИмяВременногоФайла("xml");
                УдалитьФайлы(ИмяФайла);
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should not report when file is deleted");

        // Invalid: file not deleted
        let code = r#"
            Процедура Тест()
                ИмяФайла = ПолучитьИмяВременногоФайла("xml");
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should report when file is not deleted");

        // Valid: file moved
        let code = r#"
            Процедура Тест()
                ИмяФайла = ПолучитьИмяВременногоФайла("xml");
                ПереместитьФайл(ИмяФайла, "новое_имя.xml");
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should not report when file is moved");
    }

    #[test]
    fn test_case_insensitive() {
        // Test case-insensitive matching for GetTempFileName
        let code = r#"
            Процедура Тест()
                Файл1 = ПОЛУЧИТЬИМЯВРЕМЕННОГОФАЙЛА("xml");
                Файл2 = получитьимявременногофайла("xml");
                Файл3 = ПолучитьИмяВременногоФайла("xml");
                УдалитьФайлы(Файл3);
            КонецПроцедуры
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        // Only Файл1 and Файл2 should trigger (Файл3 is deleted)
        assert_eq!(diagnostics.len(), 2, "Should handle case-insensitive GetTempFileName");
    }

    #[test]
    fn test_english_keywords() {
        // Test English keywords
        let code = r#"
            Procedure Test()
                TempFile = GetTempFileName("xml");
            EndProcedure
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1, "Should detect English GetTempFileName");

        // Test English deletion method
        let code = r#"
            Procedure Test()
                TempFile = GetTempFileName("xml");
                DeleteFiles(TempFile);
            EndProcedure
        "#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Should recognize English DeleteFiles");
    }

    #[test]
    fn test_module_qualified_calls() {
        // Test module-qualified deletion methods
        let code = r#"
            Процедура Тест()
                ИмяФайла = ПолучитьИмяВременногоФайла("xml");
                РаботаСФайламиКлиент.УдалитьФайл(Неопределено, ИмяФайла);
            КонецПроцедуры
        "#;
        let config = DiagnosticsConfig::default();
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        // Should report error - РаботаСФайламиКлиент.УдалитьФайл not in default config
        assert_eq!(diagnostics.len(), 1, "Custom method not in default config");

        // Now add it to config
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::MissingTemporaryFileDeletion,
            serde_json::json!({
                "searchDeleteFileMethod": "УдалитьФайлы|DeleteFiles|РаботаСФайламиКлиент.УдалитьФайл"
            }),
        );
        let diagnostics = check_ast_diagnostic_with_config(code, config, check);
        assert_eq!(diagnostics.len(), 0, "Custom method recognized in config");
    }
}
