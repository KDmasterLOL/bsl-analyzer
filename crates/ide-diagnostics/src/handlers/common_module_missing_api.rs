//! CommonModuleMissingAPI diagnostic
//!
//! CommonModule should contain export methods AND API regions.
//!
//! Ported from: CommonModuleMissingAPIDiagnostic.java

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use syntax::ast::{AstNode, FunctionDef, PreRegionDir, ProcedureDef};

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CommonModuleMissingAPI) {
        return Vec::new();
    }

    let parse = ctx.db.parse(ctx.file_id);
    let root = parse.syntax_node();

    let has_methods = has_any_methods(&root);
    if !has_methods {
        return Vec::new();
    }

    let has_export = has_export_methods(&root);
    let has_api_region = has_api_regions(&root);

    if !has_export || !has_api_region {
        vec![Diagnostic {
            code: DiagnosticCode::CommonModuleMissingAPI,
            message: "Общий модуль должен содержать экспортные методы и области API".to_string(),
            severity: Severity::Information,
            range: TextRange::empty(0.into()),
            tags: vec![],
            fixes: vec![],
        }]
    } else {
        Vec::new()
    }
}

fn has_any_methods(root: &syntax::SyntaxNode) -> bool {
    root.descendants()
        .any(|node| ProcedureDef::cast(node.clone()).is_some() || FunctionDef::cast(node).is_some())
}

fn has_export_methods(root: &syntax::SyntaxNode) -> bool {
    root.descendants().any(|node| {
        if let Some(proc) = ProcedureDef::cast(node.clone()) {
            proc.export_keyword().is_some()
        } else if let Some(func) = FunctionDef::cast(node) {
            func.export_keyword().is_some()
        } else {
            false
        }
    })
}

fn has_api_regions(root: &syntax::SyntaxNode) -> bool {
    const API_REGIONS: &[&str] =
        &["программныйинтерфейс", "public", "служебныйпрограммныйинтерфейс", "internal"];

    root.descendants()
        .filter_map(PreRegionDir::cast)
        .filter_map(|r| r.name())
        .any(|name| API_REGIONS.contains(&name.to_lowercase().as_str()))
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_ast_diagnostic;

    #[test]
    fn test_with_export_and_api() {
        let code = r#"
#Область ПрограммныйИнтерфейс
Процедура Тест() Экспорт
КонецПроцедуры
#КонецОбласти
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_without_export() {
        let code = r#"
#Область ПрограммныйИнтерфейс
Процедура Тест()
КонецПроцедуры
#КонецОбласти
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_without_api_region() {
        let code = r#"
#Область Служебный
Процедура Тест() Экспорт
КонецПроцедуры
#КонецОбласти
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_no_methods() {
        let code = r#"
#Область ПрограммныйИнтерфейс
#КонецОбласти
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }
}
