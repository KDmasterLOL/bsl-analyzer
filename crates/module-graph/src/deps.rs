//! Dependency extraction from BSL syntax trees.

use rustc_hash::FxHashSet;
use syntax::{SyntaxKind, SyntaxNode};
use tracing::debug;

/// Extracts module dependencies from a syntax tree.
///
/// Finds:
/// - Direct function calls: `ОбщегоНазначения.Метод()` → "ОбщегоНазначения"
///
/// Note: #Использовать is a OneScript construct, not 1C, so we don't support it.
/// In 1C, CommonModules and ManagerModules are always available in the configuration.
///
/// Returns a list of module names (case-preserved, caller should normalize).
pub struct DependencyExtractor {
    module_refs: FxHashSet<String>,
}

impl DependencyExtractor {
    /// Creates a new extractor.
    pub fn new() -> Self {
        Self { module_refs: FxHashSet::default() }
    }

    /// Extracts all module dependencies from a syntax tree.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use module_graph::deps::DependencyExtractor;
    ///
    /// let parse = db.parse(file_id);
    /// let deps = DependencyExtractor::extract(parse.syntax_node());
    /// // deps = ["ОбщегоНазначения", "РаботаСФайлами", ...]
    /// ```
    pub fn extract(root: &SyntaxNode) -> Vec<String> {
        let mut extractor = Self::new();
        extractor.walk(root);
        extractor.module_refs.into_iter().collect()
    }

    /// Recursively walks the syntax tree.
    fn walk(&mut self, node: &SyntaxNode) {
        // Check for call statements (CALL_STMT contains EXPR with IDENT DOT IDENT pattern)
        if node.kind() == SyntaxKind::CALL_STMT {
            self.process_call_stmt(node);
        }

        // Recursively process children
        for child in node.children() {
            self.walk(&child);
        }
    }

    /// Processes a call statement.
    ///
    /// Pattern: `ОбщегоНазначения.Метод()` or `Module.SubModule.Method()`
    ///
    /// AST structure (after parser refactoring with CALL_EXPR/FIELD_EXPR):
    /// CALL_STMT
    ///   CALL_EXPR
    ///     FIELD_EXPR
    ///       IDENT(node) "ОбщегоНазначения"
    ///         IDENT(token) "ОбщегоНазначения"
    ///       DOT "."
    ///       IDENT(node) "Метод"
    ///     ARG_LIST ...
    ///
    /// We extract the first IDENT before the first DOT from FIELD_EXPR.
    fn process_call_stmt(&mut self, call_stmt: &SyntaxNode) {
        // Find FIELD_EXPR within the call (indicates qualified call like Module.Method())
        let field_expr = call_stmt.descendants().find(|n| n.kind() == SyntaxKind::FIELD_EXPR);

        let Some(field_expr) = field_expr else {
            return; // Not a qualified call, no module reference
        };

        // Collect IDENT tokens and DOT position from FIELD_EXPR
        let mut first_ident: Option<String> = None;

        for element in field_expr.descendants_with_tokens() {
            if let Some(token) = element.as_token() {
                if token.kind() == SyntaxKind::IDENT && first_ident.is_none() {
                    first_ident = Some(token.text().to_string());
                } else if token.kind() == SyntaxKind::DOT {
                    // Found DOT, if we have an IDENT before it, that's the module name
                    if let Some(module_name) = first_ident {
                        debug!("Found module reference: {}", module_name);
                        self.module_refs.insert(module_name);
                        return; // Only extract first module reference
                    }
                }
            }
        }
    }
}

impl Default for DependencyExtractor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(code: &str) -> SyntaxNode {
        parser::parse(code).syntax_node()
    }

    #[test]
    fn test_extract_direct_call() {
        let code = r#"
Процедура Тест()
    ОбщегоНазначения.СообщитьПользователю("Привет");
КонецПроцедуры
"#;

        let tree = parse(code);
        let deps = DependencyExtractor::extract(&tree);

        assert_eq!(deps.len(), 1);
        assert!(deps.contains(&"ОбщегоНазначения".to_string()));
    }

    #[test]
    fn test_extract_multiple_calls() {
        let code = r#"
Процедура Тест()
    ОбщегоНазначения.Сообщить("1");
    РаботаСФайлами.СохранитьФайл("2");
КонецПроцедуры
"#;

        let tree = parse(code);
        let deps = DependencyExtractor::extract(&tree);

        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&"ОбщегоНазначения".to_string()));
        assert!(deps.contains(&"РаботаСФайлами".to_string()));
    }

    #[test]
    fn test_no_dependencies() {
        let code = r#"
Процедура Тест()
    Локальная = 123;
КонецПроцедуры
"#;

        let tree = parse(code);
        let deps = DependencyExtractor::extract(&tree);

        assert_eq!(deps.len(), 0);
    }

    #[test]
    fn test_deduplicate_dependencies() {
        let code = r#"
Процедура Тест()
    ОбщегоНазначения.Метод1();
    ОбщегоНазначения.Метод2();
    ОбщегоНазначения.Метод3();
КонецПроцедуры
"#;

        let tree = parse(code);
        let deps = DependencyExtractor::extract(&tree);

        // Should have only 1 unique dependency
        assert_eq!(deps.len(), 1);
        assert!(deps.contains(&"ОбщегоНазначения".to_string()));
    }
}
