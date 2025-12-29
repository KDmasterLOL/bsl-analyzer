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
    /// AST structure:
    /// CALL_STMT
    ///   EXPR
    ///     IDENT "ОбщегоНазначения"
    ///     DOT "."
    ///     IDENT "Метод"
    ///     ARG_LIST ...
    ///
    /// We extract the first IDENT before the first DOT.
    fn process_call_stmt(&mut self, call_stmt: &SyntaxNode) {
        // Find EXPR child
        for child in call_stmt.children() {
            if child.kind() == SyntaxKind::EXPR {
                if let Some(module_name) = self.extract_module_from_expr(&child) {
                    debug!("Found module reference: {}", module_name);
                    self.module_refs.insert(module_name);
                }
            }
        }
    }

    /// Extracts module name from an expression.
    ///
    /// For `ОбщегоНазначения.Метод()`, returns "ОбщегоНазначения".
    /// For `Module.SubModule.Method()`, returns "Module".
    ///
    /// Implementation: Find the first IDENT token, check if followed by DOT.
    ///
    /// # Limitations (TODO: Iteration 11 - Metadata)
    ///
    /// Currently extracts ONLY the first component, which works for CommonModules:
    /// - `ОбщегоНазначения.Метод()` → "ОбщегоНазначения" ✅
    ///
    /// But does NOT handle metadata-based references correctly:
    /// - `Справочники.Номенклатура.Метод()` → "Справочники" ❌ (should be "Номенклатура")
    /// - `Документы.ПриходТовара.Создать()` → "Документы" ❌ (should be "ПриходТовара")
    /// - `Обработки.МояОбработка.Создать()` → "Обработки" ❌ (should be "МояОбработка")
    ///
    /// Proper support requires:
    /// 1. Pattern recognition: `<MetadataClass>.<ObjectName>.<Method>()`
    /// 2. Metadata knowledge: mapping to ManagerModule/ObjectModule paths
    /// 3. Configuration.xml parsing
    ///
    /// This will be implemented in Iteration 11 (Metadata Infrastructure).
    fn extract_module_from_expr(&self, expr: &SyntaxNode) -> Option<String> {
        // Look for pattern: IDENT followed by DOT
        // This indicates a module reference
        let tokens: Vec<_> = expr.children_with_tokens().collect();
        for i in 0..tokens.len().saturating_sub(1) {
            if let Some(token) = tokens[i].as_token() {
                if token.kind() == SyntaxKind::IDENT {
                    // Check if next element is DOT
                    if let Some(next_token) = tokens.get(i + 1).and_then(|t| t.as_token()) {
                        if next_token.kind() == SyntaxKind::DOT {
                            // This is a module reference
                            return Some(token.text().to_string());
                        }
                    }
                }
            }
        }
        None
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
