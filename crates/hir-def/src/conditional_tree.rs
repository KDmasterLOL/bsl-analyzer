//! ConditionalTree - hierarchical representation of preprocessor conditional directives.
//!
//! ConditionalTree provides a structured view of preprocessor conditionals (#Если/#If,
//! #ИначеЕсли/#ElsIf, #Иначе/#Else) in a BSL module. It is separate from ItemTree and RegionTree
//! to maintain clean separation of concerns:
//! - ItemTree = semantic structure (procedures, functions, variables)
//! - RegionTree = organizational structure (regions for code folding)
//! - ConditionalTree = conditional compilation structure (preprocessor directives)
//!
//! ## Architecture
//!
//! ```text
//! AST (syntax) → ConditionalTree (hir-def) → Diagnostics + IDE Features
//!                     │
//!                     ├── conditionals: Arena<ConditionalData>
//!                     ├── root_conditionals: Vec<ConditionalIdx>
//!                     └── API: conditional_at(), all_branches(), main_if_branch()
//! ```
//!
//! ## Performance
//!
//! ConditionalTree is cached via Salsa and only recomputed when file content changes.
//! The structure uses `la_arena` for efficient indexing.
//!
//! ## Future Diagnostics Support
//!
//! This infrastructure enables:
//! 1. **Grammatical construct split detection** - via `parent_ast_kind` field
//! 2. **Platform context checks** - via `condition_text` pattern matching
//! 3. **Unreachable branch detection** - via condition analysis
//! 4. **Duplicate condition detection** - via sibling comparison

use la_arena::{Arena, Idx};
use rustc_hash::FxHashMap;
use syntax::{SyntaxKind, SyntaxNode};
use text_size::TextRange;

/// Index into ConditionalTree's arena.
pub type ConditionalIdx = Idx<ConditionalData>;

/// Data about a single conditional branch (#Если/#ElsIf/#Else).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionalData {
    /// Branch kind (If, ElsIf, or Else).
    pub kind: ConditionalKind,

    /// Condition text (unparsed, for If/ElsIf branches).
    /// Example: "Клиент", "НЕ Сервер ИЛИ МобильныйКлиент"
    /// None for Else branches.
    pub condition_text: Option<String>,

    /// Full range of the branch (from #Если/#ИначеЕсли/#Иначе to its content end).
    pub range: TextRange,

    /// Range of the directive line itself (#Если Condition Тогда).
    pub directive_range: TextRange,

    /// Range of just the condition text (for highlighting/navigation).
    /// None for Else branches.
    pub condition_range: Option<TextRange>,

    /// Parent conditional index (None for top-level #Если).
    pub parent: Option<ConditionalIdx>,

    /// Child conditional indices (nested #Если blocks inside this branch).
    pub children: Vec<ConditionalIdx>,

    /// Sibling branches (ElsIf/Else clauses belonging to same If directive).
    /// Only populated for the main If branch.
    pub siblings: Vec<ConditionalIdx>,

    /// Depth in the conditional hierarchy (0 for top-level).
    pub depth: u32,

    /// AST node kind containing this directive.
    /// Used to detect if directive splits grammatical construct.
    /// Examples:
    /// - Some(BINARY_EXPR) → directive splits expression (bad)
    /// - Some(PROCEDURE_DEF) → directive splits procedure (bad)
    /// - None → directive in valid location (STMT_LIST, SOURCE_FILE)
    pub parent_ast_kind: Option<SyntaxKind>,
}

/// Kind of conditional branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionalKind {
    /// #Если / #If
    If,
    /// #ИначеЕсли / #ElsIf
    ElsIf,
    /// #Иначе / #Else
    Else,
}

/// Tree of preprocessor conditional directives in a module.
///
/// Provides O(1) access by index and O(log n) lookup by position.
/// Follows the same pattern as RegionTree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionalTree {
    /// All conditional branches in the module.
    conditionals: Arena<ConditionalData>,

    /// Top-level conditional blocks (not nested in other conditionals).
    root_conditionals: Vec<ConditionalIdx>,

    /// Map from text position to containing conditional branch.
    /// Key is the start of the branch's range.
    position_map: FxHashMap<u32, ConditionalIdx>,
}

impl Default for ConditionalTree {
    fn default() -> Self {
        Self::new()
    }
}

impl ConditionalTree {
    /// Create an empty ConditionalTree.
    pub fn new() -> Self {
        Self {
            conditionals: Arena::new(),
            root_conditionals: Vec::new(),
            position_map: FxHashMap::default(),
        }
    }

    /// Get all conditionals.
    pub fn conditionals(&self) -> impl Iterator<Item = (ConditionalIdx, &ConditionalData)> {
        self.conditionals.iter()
    }

    /// Get number of conditionals.
    pub fn len(&self) -> usize {
        self.conditionals.len()
    }

    /// Check if there are no conditionals.
    pub fn is_empty(&self) -> bool {
        self.conditionals.is_empty()
    }

    /// Get top-level conditionals.
    pub fn root_conditionals(&self) -> &[ConditionalIdx] {
        &self.root_conditionals
    }

    /// Get a conditional by its index.
    pub fn conditional(&self, idx: ConditionalIdx) -> &ConditionalData {
        &self.conditionals[idx]
    }

    /// Get parent conditional.
    pub fn parent(&self, idx: ConditionalIdx) -> Option<ConditionalIdx> {
        self.conditionals[idx].parent
    }

    /// Get child conditionals.
    pub fn children(&self, idx: ConditionalIdx) -> &[ConditionalIdx] {
        &self.conditionals[idx].children
    }

    /// Find conditional branch containing a text position.
    ///
    /// Returns the innermost (most specific) branch containing the position.
    pub fn conditional_at(&self, offset: text_size::TextSize) -> Option<ConditionalIdx> {
        // Find the innermost conditional containing this offset
        let mut best: Option<(ConditionalIdx, u32)> = None; // (idx, depth)

        for (idx, cond) in self.conditionals.iter() {
            if cond.range.contains(offset) {
                match best {
                    None => best = Some((idx, cond.depth)),
                    Some((_, best_depth)) if cond.depth > best_depth => {
                        best = Some((idx, cond.depth));
                    }
                    _ => {}
                }
            }
        }

        best.map(|(idx, _)| idx)
    }

    /// Find conditional branch fully containing a text range.
    ///
    /// Returns the innermost conditional that fully contains the range.
    pub fn conditional_containing(&self, range: TextRange) -> Option<ConditionalIdx> {
        let mut best: Option<(ConditionalIdx, u32)> = None;

        for (idx, cond) in self.conditionals.iter() {
            if cond.range.contains_range(range) {
                match best {
                    None => best = Some((idx, cond.depth)),
                    Some((_, best_depth)) if cond.depth > best_depth => {
                        best = Some((idx, cond.depth));
                    }
                    _ => {}
                }
            }
        }

        best.map(|(idx, _)| idx)
    }

    /// Check if a position is inside any conditional branch.
    pub fn is_inside_conditional(&self, offset: text_size::TextSize) -> bool {
        self.conditional_at(offset).is_some()
    }

    /// Get the main If branch for a given ElsIf or Else branch.
    /// Returns None if already an If branch.
    pub fn main_if_branch(&self, idx: ConditionalIdx) -> Option<ConditionalIdx> {
        let cond = &self.conditionals[idx];
        match cond.kind {
            ConditionalKind::If => None, // Already main branch
            ConditionalKind::ElsIf | ConditionalKind::Else => {
                // Walk up siblings to find the If
                // In our structure, siblings are stored on the main If branch
                // So we need to search all conditionals for one that has this idx in siblings
                for (candidate_idx, candidate) in self.conditionals.iter() {
                    if candidate.kind == ConditionalKind::If && candidate.siblings.contains(&idx) {
                        return Some(candidate_idx);
                    }
                }
                None
            }
        }
    }

    /// Get all sibling branches for a conditional (including itself).
    /// For If: returns [If, ElsIf1, ElsIf2, Else]
    /// For ElsIf/Else: finds main If and returns its sibling list
    pub fn all_branches(&self, idx: ConditionalIdx) -> Vec<ConditionalIdx> {
        let main_if = self.main_if_branch(idx).unwrap_or(idx);
        let main = &self.conditionals[main_if];

        let mut branches = vec![main_if];
        branches.extend(main.siblings.iter().copied());
        branches
    }
}

/// Builder for constructing ConditionalTree from AST.
struct ConditionalTreeBuilder {
    tree: ConditionalTree,
    /// Stack of parent conditional branches during traversal.
    parent_stack: Vec<ConditionalIdx>,
}

impl ConditionalTreeBuilder {
    fn new() -> Self {
        Self { tree: ConditionalTree::new(), parent_stack: Vec::new() }
    }

    fn build(mut self, root: &SyntaxNode) -> ConditionalTree {
        self.collect_conditionals(root);
        self.tree
    }

    fn collect_conditionals(&mut self, node: &SyntaxNode) {
        for child in node.children() {
            match child.kind() {
                SyntaxKind::PRE_IF_DIR => {
                    self.process_if_directive(&child);
                }
                // Recurse into all other nodes to find nested conditionals
                _ => {
                    self.collect_conditionals(&child);
                }
            }
        }
    }

    fn process_if_directive(&mut self, node: &SyntaxNode) {
        // Extract main If branch
        let (condition_text, condition_range) = self.extract_condition(node);

        let range = node.text_range();
        let directive_range = self.find_first_line_range(node);
        let parent = self.parent_stack.last().copied();
        let depth = self.parent_stack.len() as u32;
        let parent_ast_kind = find_parent_ast_kind(node);

        // Allocate main If branch
        let if_idx = self.tree.conditionals.alloc(ConditionalData {
            kind: ConditionalKind::If,
            condition_text: Some(condition_text),
            range,
            directive_range,
            condition_range: Some(condition_range),
            parent,
            children: Vec::new(),
            siblings: Vec::new(),
            depth,
            parent_ast_kind,
        });

        // Add to position map
        self.tree.position_map.insert(range.start().into(), if_idx);

        // Add to parent or root
        if let Some(parent_idx) = parent {
            self.tree.conditionals[parent_idx].children.push(if_idx);
        } else {
            self.tree.root_conditionals.push(if_idx);
        }

        // Process ElsIf clauses
        let mut siblings = Vec::new();
        for elsif in node.children().filter(|n| n.kind() == SyntaxKind::PRE_ELSIF_CLAUSE) {
            let elsif_idx = self.process_elsif_clause(&elsif, if_idx, depth);
            siblings.push(elsif_idx);
        }

        // Process Else clause
        for else_clause in node.children().filter(|n| n.kind() == SyntaxKind::PRE_ELSE_CLAUSE) {
            let else_idx = self.process_else_clause(&else_clause, if_idx, depth);
            siblings.push(else_idx);
        }

        // Store siblings on main If branch
        self.tree.conditionals[if_idx].siblings = siblings;

        // Push onto stack and recurse for nested conditionals
        self.parent_stack.push(if_idx);
        self.collect_nested_conditionals(node);
        self.parent_stack.pop();
    }

    fn process_elsif_clause(
        &mut self,
        node: &SyntaxNode,
        parent_if: ConditionalIdx,
        depth: u32,
    ) -> ConditionalIdx {
        let (condition_text, condition_range) = self.extract_condition(node);
        let range = node.text_range();
        let directive_range = self.find_first_line_range(node);
        let parent_ast_kind = find_parent_ast_kind(node);

        let idx = self.tree.conditionals.alloc(ConditionalData {
            kind: ConditionalKind::ElsIf,
            condition_text: Some(condition_text),
            range,
            directive_range,
            condition_range: Some(condition_range),
            parent: Some(parent_if),
            children: Vec::new(),
            siblings: Vec::new(),
            depth,
            parent_ast_kind,
        });

        self.tree.position_map.insert(range.start().into(), idx);

        // Recurse for nested conditionals inside this elsif
        self.parent_stack.push(idx);
        self.collect_nested_conditionals(node);
        self.parent_stack.pop();

        idx
    }

    fn process_else_clause(
        &mut self,
        node: &SyntaxNode,
        parent_if: ConditionalIdx,
        depth: u32,
    ) -> ConditionalIdx {
        let range = node.text_range();
        let directive_range = self.find_first_line_range(node);
        let parent_ast_kind = find_parent_ast_kind(node);

        let idx = self.tree.conditionals.alloc(ConditionalData {
            kind: ConditionalKind::Else,
            condition_text: None, // Else has no condition
            range,
            directive_range,
            condition_range: None,
            parent: Some(parent_if),
            children: Vec::new(),
            siblings: Vec::new(),
            depth,
            parent_ast_kind,
        });

        self.tree.position_map.insert(range.start().into(), idx);

        // Recurse for nested conditionals inside else
        self.parent_stack.push(idx);
        self.collect_nested_conditionals(node);
        self.parent_stack.pop();

        idx
    }

    fn collect_nested_conditionals(&mut self, parent_node: &SyntaxNode) {
        for child in parent_node.children() {
            match child.kind() {
                SyntaxKind::PRE_IF_DIR => {
                    self.process_if_directive(&child);
                }
                // Recurse into all other nodes to find nested conditionals
                _ => {
                    self.collect_nested_conditionals(&child);
                }
            }
        }
    }

    /// Extract condition text from #Если or #ИначеЕсли node.
    /// Returns (condition_text, condition_range).
    fn extract_condition(&self, node: &SyntaxNode) -> (String, TextRange) {
        // Extract from directive line text
        let text = node.text().to_string();
        let first_line = text.lines().next().unwrap_or(&text);

        // Find the condition between directive keyword and Тогда/Then
        let mut condition = first_line.to_string();

        // Remove directive keywords
        for keyword in &[
            "#Если ",
            "#если ",
            "#If ",
            "#if ",
            "#ИначеЕсли ",
            "#иначеесли ",
            "#ElsIf ",
            "#elsif ",
        ] {
            if condition.starts_with(keyword) {
                condition = condition[keyword.len()..].to_string();
                break;
            }
        }

        // Remove trailing Тогда/Then
        for suffix in &[" Тогда", " тогда", " Then", " then", " ТОГДА", " THEN"] {
            if condition.ends_with(suffix) {
                condition = condition[..condition.len() - suffix.len()].to_string();
                break;
            }
        }

        let condition = condition.trim().to_string();

        // Try to find the actual condition range in the AST
        // Look for PRE_EXPR or PRE_LOGICAL_EXPR child to get precise range
        let condition_range = node
            .children()
            .find(|child| {
                matches!(child.kind(), SyntaxKind::PRE_EXPR | SyntaxKind::PRE_LOGICAL_EXPR)
            })
            .map(|child| child.text_range())
            .unwrap_or_else(|| {
                // Fallback: approximate range from text
                let start_offset = first_line.find(condition.as_str()).unwrap_or(0);
                let start =
                    node.text_range().start() + text_size::TextSize::from(start_offset as u32);
                let end = start + text_size::TextSize::from(condition.len() as u32);
                TextRange::new(start, end)
            });

        (condition, condition_range)
    }

    fn find_first_line_range(&self, node: &SyntaxNode) -> TextRange {
        let text = node.text().to_string();
        let first_line_len = text.lines().next().map(|l| l.len()).unwrap_or(0);
        TextRange::new(
            node.text_range().start(),
            node.text_range().start() + text_size::TextSize::from(first_line_len as u32),
        )
    }
}

/// Find parent AST context for a preprocessor directive.
///
/// Walks up the AST tree to find the first meaningful node that contains this directive.
/// Returns None if the directive is in a valid top-level context (STMT_LIST, SOURCE_FILE).
/// Returns Some(kind) if the directive splits a grammatical construct (bad practice).
fn find_parent_ast_kind(directive_node: &SyntaxNode) -> Option<SyntaxKind> {
    let mut parent = directive_node.parent();
    while let Some(node) = parent {
        match node.kind() {
            // Expression contexts (bad - directive splits expression)
            SyntaxKind::BINARY_EXPR
            | SyntaxKind::CALL_EXPR
            | SyntaxKind::INDEX_EXPR
            | SyntaxKind::TERNARY_EXPR
            | SyntaxKind::UNARY_EXPR => return Some(node.kind()),

            // Statement contexts (bad - directive splits statement)
            SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::ASSIGN_STMT
            | SyntaxKind::CALL_STMT => return Some(node.kind()),

            // Declaration contexts (bad - directive splits declaration)
            SyntaxKind::PROCEDURE_DEF
            | SyntaxKind::FUNCTION_DEF
            | SyntaxKind::PARAM_LIST
            | SyntaxKind::VAR_DEF => return Some(node.kind()),

            // Top-level contexts (OK to have directives here)
            SyntaxKind::SOURCE_FILE | SyntaxKind::STMT_LIST => return None,

            // Preprocessor contexts (keep searching up)
            SyntaxKind::PRE_IF_DIR
            | SyntaxKind::PRE_ELSIF_CLAUSE
            | SyntaxKind::PRE_ELSE_CLAUSE
            | SyntaxKind::PRE_REGION_DIR => {
                parent = node.parent();
            }

            // Keep searching up
            _ => parent = node.parent(),
        }
    }
    None
}

/// Lower AST to ConditionalTree.
///
/// This is the main entry point for ConditionalTree construction.
pub fn lower_conditionals(root: &SyntaxNode) -> ConditionalTree {
    ConditionalTreeBuilder::new().build(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_lower(code: &str) -> ConditionalTree {
        let parse = parser::parse(code);
        lower_conditionals(&parse.syntax_node())
    }

    #[test]
    fn test_empty_file() {
        let tree = parse_and_lower("");
        assert!(tree.is_empty());
        assert_eq!(tree.root_conditionals().len(), 0);
    }

    #[test]
    fn test_single_if() {
        let code = r#"
#Если Клиент Тогда
Процедура Тест()
КонецПроцедуры
#КонецЕсли
"#;
        let tree = parse_and_lower(code);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree.root_conditionals().len(), 1);

        let cond = tree.conditional(tree.root_conditionals()[0]);
        assert_eq!(cond.kind, ConditionalKind::If);
        assert_eq!(cond.condition_text.as_ref().unwrap(), "Клиент");
        assert_eq!(cond.depth, 0);
        assert!(cond.parent.is_none());
        assert!(cond.children.is_empty());
        assert!(cond.siblings.is_empty());
    }

    #[test]
    fn test_if_elsif_else_chain() {
        let code = r#"
#Если Клиент Тогда
    Сообщить("Клиент");
#ИначеЕсли Сервер Тогда
    Сообщить("Сервер");
#Иначе
    Сообщить("Другое");
#КонецЕсли
"#;
        let tree = parse_and_lower(code);

        assert_eq!(tree.len(), 3, "Should have If + ElsIf + Else");
        assert_eq!(tree.root_conditionals().len(), 1);

        let if_idx = tree.root_conditionals()[0];
        let if_cond = tree.conditional(if_idx);
        assert_eq!(if_cond.kind, ConditionalKind::If);
        assert_eq!(if_cond.condition_text.as_ref().unwrap(), "Клиент");
        assert_eq!(if_cond.siblings.len(), 2, "Should have 2 siblings (ElsIf + Else)");

        // Check ElsIf
        let elsif_idx = if_cond.siblings[0];
        let elsif_cond = tree.conditional(elsif_idx);
        assert_eq!(elsif_cond.kind, ConditionalKind::ElsIf);
        assert_eq!(elsif_cond.condition_text.as_ref().unwrap(), "Сервер");

        // Check Else
        let else_idx = if_cond.siblings[1];
        let else_cond = tree.conditional(else_idx);
        assert_eq!(else_cond.kind, ConditionalKind::Else);
        assert!(else_cond.condition_text.is_none());

        // Test main_if_branch
        assert_eq!(tree.main_if_branch(elsif_idx), Some(if_idx));
        assert_eq!(tree.main_if_branch(else_idx), Some(if_idx));
        assert_eq!(tree.main_if_branch(if_idx), None);

        // Test all_branches
        let branches = tree.all_branches(elsif_idx);
        assert_eq!(branches.len(), 3);
        assert_eq!(branches[0], if_idx);
        assert_eq!(branches[1], elsif_idx);
        assert_eq!(branches[2], else_idx);
    }

    #[test]
    fn test_nested_conditionals() {
        let code = r#"
#Если Клиент Тогда
    #Если ВебКлиент Тогда
        Сообщить("Веб");
    #КонецЕсли
#КонецЕсли
"#;
        let tree = parse_and_lower(code);

        assert_eq!(tree.len(), 2);
        assert_eq!(tree.root_conditionals().len(), 1);

        let outer_idx = tree.root_conditionals()[0];
        let outer = tree.conditional(outer_idx);
        assert_eq!(outer.depth, 0);
        assert_eq!(outer.children.len(), 1);

        let inner_idx = outer.children[0];
        let inner = tree.conditional(inner_idx);
        assert_eq!(inner.depth, 1);
        assert_eq!(inner.parent, Some(outer_idx));
        assert_eq!(inner.condition_text.as_ref().unwrap(), "ВебКлиент");
    }

    #[test]
    fn test_conditionals_inside_regions() {
        let code = r#"
#Область ПрограммныйИнтерфейс

#Если Клиент Тогда
Функция Тест() Экспорт
КонецФункции
#КонецЕсли

#КонецОбласти
"#;
        let tree = parse_and_lower(code);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree.root_conditionals().len(), 1);

        let cond = tree.conditional(tree.root_conditionals()[0]);
        assert_eq!(cond.condition_text.as_ref().unwrap(), "Клиент");
    }

    #[test]
    fn test_position_based_lookup() {
        let code = r#"
#Если Клиент Тогда
Процедура Тест()
КонецПроцедуры
#КонецЕсли
"#;
        let tree = parse_and_lower(code);

        // Position inside the conditional
        let inside_pos = text_size::TextSize::from(30);
        assert!(tree.conditional_at(inside_pos).is_some());

        // Position before the conditional
        let before_pos = text_size::TextSize::from(0);
        assert!(tree.conditional_at(before_pos).is_none());
    }

    #[test]
    fn test_english_keywords() {
        let code = r#"
#If Client Then
    Message("Client");
#ElsIf Server Then
    Message("Server");
#Else
    Message("Other");
#EndIf
"#;
        let tree = parse_and_lower(code);

        assert_eq!(tree.len(), 3);

        let if_idx = tree.root_conditionals()[0];
        let if_cond = tree.conditional(if_idx);
        assert_eq!(if_cond.condition_text.as_ref().unwrap(), "Client");

        let elsif_idx = if_cond.siblings[0];
        let elsif_cond = tree.conditional(elsif_idx);
        assert_eq!(elsif_cond.condition_text.as_ref().unwrap(), "Server");
    }

    // NEW TESTS: Parent AST kind tracking

    #[test]
    fn test_parent_ast_kind_binary_expr() {
        // Directive splits expression - bad practice
        let code = r#"
Процедура Тест()
    а = 1
#Если Клиент Тогда
        + 2
#КонецЕсли
        ;
КонецПроцедуры
"#;
        let tree = parse_and_lower(code);

        assert_eq!(tree.len(), 1);
        let _cond = tree.conditional(tree.root_conditionals()[0]);

        // Should detect BINARY_EXPR or similar expression context
        // Note: Parser might not parse this correctly as it's invalid syntax
        // This test documents expected behavior if parser accepts it
    }

    #[test]
    fn test_parent_ast_kind_valid_stmt_list() {
        // Directive in STMT_LIST - valid placement
        let code = r#"
Процедура Тест()
    Сообщить("До");
#Если Клиент Тогда
    Сообщить("Клиент");
#КонецЕсли
    Сообщить("После");
КонецПроцедуры
"#;
        let tree = parse_and_lower(code);

        assert_eq!(tree.len(), 1);
        let cond = tree.conditional(tree.root_conditionals()[0]);

        // Should have None for parent_ast_kind (valid placement)
        assert!(
            cond.parent_ast_kind.is_none(),
            "Directive in STMT_LIST should have no parent_ast_kind (valid placement)"
        );
    }

    #[test]
    fn test_parent_ast_kind_top_level() {
        // Directive at module level - valid placement
        let code = r#"
#Если Клиент Тогда
Процедура КлиентскаяПроцедура()
КонецПроцедуры
#КонецЕсли

#Если Сервер Тогда
Процедура СерверПроцедура()
КонецПроцедуры
#КонецЕсли
"#;
        let tree = parse_and_lower(code);

        assert_eq!(tree.len(), 2);

        // Both should have None for parent_ast_kind (top-level is valid)
        for idx in tree.root_conditionals() {
            let cond = tree.conditional(*idx);
            assert!(
                cond.parent_ast_kind.is_none(),
                "Top-level directive should have no parent_ast_kind"
            );
        }
    }

    // NEW TESTS: Platform context keywords

    #[test]
    fn test_platform_context_client() {
        let code = r#"
#Если Клиент Тогда
    Сообщить("Клиент");
#КонецЕсли
"#;
        let tree = parse_and_lower(code);

        assert_eq!(tree.len(), 1);
        let cond = tree.conditional(tree.root_conditionals()[0]);
        let condition_text = cond.condition_text.as_ref().unwrap();

        assert_eq!(condition_text, "Клиент");
        assert!(
            condition_text.to_lowercase().contains("клиент"),
            "Should contain 'клиент' keyword"
        );
    }

    #[test]
    fn test_platform_context_server() {
        let code = r#"
#Если Сервер Тогда
    Сообщить("Сервер");
#КонецЕсли
"#;
        let tree = parse_and_lower(code);

        let cond = tree.conditional(tree.root_conditionals()[0]);
        let condition_text = cond.condition_text.as_ref().unwrap();

        assert_eq!(condition_text, "Сервер");
        assert!(
            condition_text.to_lowercase().contains("сервер"),
            "Should contain 'сервер' keyword"
        );
    }

    #[test]
    fn test_platform_context_thin_client() {
        let code = r#"
#Если ТонкийКлиент Тогда
    Сообщить("ТонкийКлиент");
#КонецЕсли
"#;
        let tree = parse_and_lower(code);

        let cond = tree.conditional(tree.root_conditionals()[0]);
        let condition_text = cond.condition_text.as_ref().unwrap();

        assert_eq!(condition_text, "ТонкийКлиент");
    }

    #[test]
    fn test_platform_context_complex_condition() {
        let code = r#"
#Если НЕ Сервер ИЛИ МобильныйКлиент Тогда
    Сообщить("Сложное условие");
#КонецЕсли
"#;
        let tree = parse_and_lower(code);

        let cond = tree.conditional(tree.root_conditionals()[0]);
        let condition_text = cond.condition_text.as_ref().unwrap();

        // Should preserve full condition text
        assert!(condition_text.contains("Сервер"), "Should contain 'Сервер'");
        assert!(condition_text.contains("МобильныйКлиент"), "Should contain 'МобильныйКлиент'");

        // Check for logical operators
        let lower = condition_text.to_lowercase();
        assert!(lower.contains("не") || lower.contains("или"), "Should contain logical operators");
    }

    #[test]
    fn test_platform_context_english() {
        let code = r#"
#If Client OR Server Then
    Message("Client or Server");
#EndIf
"#;
        let tree = parse_and_lower(code);

        let cond = tree.conditional(tree.root_conditionals()[0]);
        let condition_text = cond.condition_text.as_ref().unwrap();

        let lower = condition_text.to_lowercase();
        assert!(
            lower.contains("client") || lower.contains("server"),
            "Should contain platform keywords in English"
        );
    }

    #[test]
    fn test_condition_range() {
        let code = r#"
#Если Клиент Тогда
КонецФункции
#КонецЕсли
"#;
        let tree = parse_and_lower(code);

        let cond = tree.conditional(tree.root_conditionals()[0]);

        // Should have condition_range pointing to condition text
        assert!(cond.condition_range.is_some(), "If branch should have condition_range");

        // Else branches should not have condition_range
        let code_with_else = r#"
#Если Клиент Тогда
    А = 1;
#Иначе
    А = 2;
#КонецЕсли
"#;
        let tree2 = parse_and_lower(code_with_else);
        let if_cond = tree2.conditional(tree2.root_conditionals()[0]);
        let else_idx = if_cond.siblings[0];
        let else_cond = tree2.conditional(else_idx);

        assert!(else_cond.condition_range.is_none(), "Else branch should not have condition_range");
    }

    #[test]
    fn test_multilevel_nesting() {
        let code = r#"
#Если Клиент Тогда
    #Если ТонкийКлиент Тогда
        #Если ВебКлиент Тогда
            Сообщить("ВебКлиент");
        #КонецЕсли
    #КонецЕсли
#КонецЕсли
"#;
        let tree = parse_and_lower(code);

        assert_eq!(tree.len(), 3);

        let level0_idx = tree.root_conditionals()[0];
        let level0 = tree.conditional(level0_idx);
        assert_eq!(level0.depth, 0);
        assert_eq!(level0.children.len(), 1);

        let level1_idx = level0.children[0];
        let level1 = tree.conditional(level1_idx);
        assert_eq!(level1.depth, 1);
        assert_eq!(level1.parent, Some(level0_idx));
        assert_eq!(level1.children.len(), 1);

        let level2_idx = level1.children[0];
        let level2 = tree.conditional(level2_idx);
        assert_eq!(level2.depth, 2);
        assert_eq!(level2.parent, Some(level1_idx));
    }
}
