//! NestedConstructorsInStructureDeclaration diagnostic.
//!
//! Detects when Structure/FixedStructure constructors contain nested constructors
//! with parameters, which reduces code readability.
//!
//! ## Why?
//! Nested constructors in structure declarations make code harder to read and understand.
//! It's better to create nested structures as separate variables.
//!
//! ## Bad practice
//! ```bsl
//! Результат = Новый Структура("ДанныеНоменклатуры, Количество",
//!                              Новый Структура("Код, Наименование"),
//!                              10);
//! ```
//!
//! ## Good practice
//! ```bsl
//! ДанныеНоменклатуры = Новый Структура("Код, Наименование");
//! Результат = Новый Структура("ДанныеНоменклатуры, Количество",
//!                              ДанныеНоменклатуры,
//!                              10);
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** Minor (Warning)
//! - **Tags:** BADPRACTICE, BRAINOVERLOAD
//! - **Minutes to fix:** 10
//!
//! ## Implementation
//! Ported from:
//! - NestedConstructorsInStructureDeclarationDiagnostic.java (bsl-language-server)
//!
//! **HIR-based implementation** using semantic analysis instead of AST traversal.
//!
//! Migrated from AST to HIR for:
//! - Type-safe expression handling via Expr enum
//! - Automatic Salsa caching via module_bodies()
//! - Module-level code coverage (not just methods)
//! - Cleaner recursive checking via ExprId references

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use hir::ModuleId;
use hir_def::{Body, BodySourceMap, Expr, Name};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::NestedConstructorsInStructureDeclaration) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    // Get module bodies from HIR (cached by Salsa)
    let module_id = ModuleId::new(ctx.file_id);
    let module_bodies = ctx.db.module_bodies(module_id);

    // Check module-level code (code outside procedures/functions)
    if let Some(module_code) = module_bodies.module_code_result() {
        check_body(&module_code.body, &module_code.source_map, &mut diagnostics);
    }

    // Check all method bodies (procedures and functions)
    for (_, body, source_map) in module_bodies.method_bodies() {
        check_body(body, source_map, &mut diagnostics);
    }

    // Sort diagnostics by position (HIR expressions are stored in arena, not source order)
    diagnostics.sort_by_key(|d| (d.range.start(), d.range.end()));

    diagnostics
}

/// Check a single Body for nested constructors in structure declarations.
///
/// HIR-based approach: iterates over expressions and checks New expressions semantically.
fn check_body(body: &Body, source_map: &BodySourceMap, diagnostics: &mut Vec<Diagnostic>) {
    // Walk all expressions in the body
    for (expr_id, expr) in body.exprs.iter() {
        // Only check New expressions
        let Expr::New { type_name, args } = expr else {
            continue;
        };

        // Check if this is Structure or FixedStructure constructor
        if !is_structure_or_fixed_structure(type_name) {
            continue;
        }

        // Must have more than 1 argument to potentially have nested constructors
        if args.len() <= 1 {
            continue;
        }

        // Check if any argument is a New expression with non-empty parameters
        let has_nested_constructor_with_params = args.iter().any(|&arg_id| {
            matches!(
                body.expr(arg_id),
                Expr::New { args: nested_args, .. } if !nested_args.is_empty()
            )
        });

        if !has_nested_constructor_with_params {
            continue;
        }

        // Get range from source map
        let Some(range) = source_map.expr_range(expr_id) else {
            continue;
        };

        diagnostics.push(Diagnostic {
            code: DiagnosticCode::NestedConstructorsInStructureDeclaration,
            message: "Не используйте конструкторы с параметрами при объявлении структуры"
                .to_string(),
            severity: Severity::Warning,
            range,
            tags: vec![],
            fixes: vec![],
        });
    }
}

/// Check if type name is Structure or FixedStructure (case-insensitive, bilingual).
fn is_structure_or_fixed_structure(type_name: &Option<Name>) -> bool {
    let Some(name) = type_name else {
        return false;
    };

    let text = name.as_str().to_lowercase();
    matches!(text.as_str(), "структура" | "structure" | "фиксированнаяструктура" | "fixedstructure")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_diagnostic_range_multiline;
    use crate::DiagnosticsConfig;
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::sync::Arc;
    use test_fixture::Fixture;

    fn check_diagnostic(code: &str) -> (Vec<Diagnostic>, String) {
        use ide_db::base_db::{SourceRoot, SourceRootId};
        use vfs::{FileSet, VfsPath};

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().unwrap();

        let mut db = RootDatabaseImpl::new();

        // Set up source root for HIR (module_bodies) to work
        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
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

        let diagnostics = check(&ctx);
        (diagnostics, file_content)
    }

    #[test]
    fn test_no_diagnostic_for_empty_structure() {
        let code = r#"
Результат = Новый Структура;
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_for_single_param() {
        let code = r#"
А = Новый Структура(Новый ФиксированнаяСтруктура(Мок_ПараметрыПроцедуры));
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_no_diagnostic_for_nested_without_params() {
        let code = r#"
Результат = Новый Структура("МВТ, ТекстЗапроса, Параметры",
                             Новый МенеджерВременныхТаблиц,
                             ТекстЗапроса,
                             Новый Структура);
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_diagnostic_for_nested_with_params() {
        let code = r#"
Результат = Новый Структура("ДанныеНоменклатуры, Количество",
                             Новый Структура("Код, Наименование"),
                             10);
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_diagnostic_for_english_keywords() {
        let code = r#"
Result = New Structure("GoodsData, Count",
                        New Structure("Code, Name"),
                        10);
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_diagnostic_for_non_structure() {
        let code = r#"
Result = New Structure("field1, field2, field3", New Array(), New Array(), New Array());
"#;
        let (diagnostics, _) = check_diagnostic(code);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_comprehensive() {
        let code =
            include_str!("../../test_data/NestedConstructorsInStructureDeclarationDiagnostic.bsl");
        let (diagnostics, file_content) = check_diagnostic(code);

        assert_eq!(
            diagnostics.len(),
            8,
            "Should find exactly 8 diagnostics (matching Java implementation)"
        );

        // Verify exact positions matching bsl-language-server (Java) implementation
        // Java uses 0-indexed lines
        assert_diagnostic_range_multiline(&file_content, &diagnostics[0], 10, 16, 12, 36);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[1], 14, 16, 23, 62);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[2], 25, 16, 27, 96);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[3], 26, 32, 27, 95);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[4], 38, 13, 40, 31);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[5], 42, 13, 51, 50);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[6], 53, 13, 55, 79);
        assert_diagnostic_range_multiline(&file_content, &diagnostics[7], 54, 28, 55, 78);
    }
}
