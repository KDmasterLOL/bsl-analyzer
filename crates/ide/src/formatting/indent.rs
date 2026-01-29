//! Indentation level tracking.
//!
//! Based on RDT1C algorithm: tracks instruction level (block depth)
//! and expression level (for continuation lines).

// These functions are prepared for future AST-based formatting improvements
#![allow(dead_code)]

use syntax::{SyntaxKind, SyntaxNode};

/// Tracks indentation state during formatting.
#[derive(Debug, Clone, Default)]
pub struct IndentState {
    /// Base indentation level (from first non-empty line).
    pub base: u32,
    /// Current instruction level (block depth).
    pub instruction: u32,
    /// Current expression level (for multi-line expressions).
    pub expression: u32,
    /// Offset adjustment for current line only.
    pub current_offset: i32,
}

impl IndentState {
    /// Creates a new indent state with the given base level.
    pub fn with_base(base: u32) -> Self {
        Self { base, instruction: 0, expression: 0, current_offset: 0 }
    }

    /// Returns the total indent level for the current line.
    pub fn total(&self) -> u32 {
        let base = self.base as i32 + self.instruction as i32 + self.current_offset;
        let total = base + self.expression as i32;
        total.max(0) as u32
    }

    /// Enter a block (increment instruction level).
    pub fn enter_block(&mut self) {
        self.instruction += 1;
    }

    /// Leave a block (decrement instruction level).
    pub fn leave_block(&mut self) {
        self.instruction = self.instruction.saturating_sub(1);
    }

    /// Set temporary offset for current line (e.g., for Иначе, Исключение).
    pub fn set_current_offset(&mut self, offset: i32) {
        self.current_offset = offset;
    }

    /// Reset current line offset.
    pub fn reset_current_offset(&mut self) {
        self.current_offset = 0;
    }

    /// Enter expression continuation (e.g., unclosed parenthesis).
    pub fn enter_expression(&mut self) {
        self.expression += 1;
    }

    /// Leave expression continuation.
    pub fn leave_expression(&mut self) {
        self.expression = self.expression.saturating_sub(1);
    }

    /// Reset expression level (e.g., at semicolon).
    pub fn reset_expression(&mut self) {
        self.expression = 0;
    }
}

/// Returns the indent adjustment for a syntax node.
///
/// Returns:
/// - `Some(1)` if the node increases indent for its children
/// - `Some(-1)` if the node itself should be dedented (like Else)
/// - `None` if no adjustment needed
pub fn node_indent_adjustment(kind: SyntaxKind) -> Option<i32> {
    match kind {
        // Block constructs that increase indent for their body
        SyntaxKind::PROCEDURE_DEF
        | SyntaxKind::FUNCTION_DEF
        | SyntaxKind::IF_STMT
        | SyntaxKind::WHILE_STMT
        | SyntaxKind::FOR_STMT
        | SyntaxKind::FOR_EACH_STMT
        | SyntaxKind::TRY_STMT
        | SyntaxKind::PRE_REGION_DIR
        | SyntaxKind::PRE_IF_DIR
        | SyntaxKind::PRE_DELETE_DIR
        | SyntaxKind::PRE_INSERT_DIR => Some(1),

        _ => None,
    }
}

/// Checks if a keyword starts a block that increases indent.
pub fn is_block_start_keyword(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::KW_PROCEDURE
            | SyntaxKind::KW_FUNCTION
            | SyntaxKind::KW_IF
            | SyntaxKind::KW_WHILE
            | SyntaxKind::KW_FOR
            | SyntaxKind::KW_TRY
            | SyntaxKind::PRE_REGION
            | SyntaxKind::PRE_IF
            | SyntaxKind::PRE_DELETE
            | SyntaxKind::PRE_INSERT
    )
}

/// Checks if a keyword ends a block (decreases indent).
pub fn is_block_end_keyword(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::KW_END_PROCEDURE
            | SyntaxKind::KW_END_FUNCTION
            | SyntaxKind::KW_END_IF
            | SyntaxKind::KW_END_DO
            | SyntaxKind::KW_END_TRY
            | SyntaxKind::PRE_END_REGION
            | SyntaxKind::PRE_END_IF
            | SyntaxKind::PRE_END_DELETE
            | SyntaxKind::PRE_END_INSERT
    )
}

/// Checks if a keyword is a "middle" keyword (dedent for itself, but indent for content).
/// Examples: Иначе, Исключение, ИначеЕсли, #Иначе, #ИначеЕсли
pub fn is_middle_keyword(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::KW_ELSE
            | SyntaxKind::KW_ELSIF
            | SyntaxKind::KW_EXCEPT
            | SyntaxKind::PRE_ELSE
            | SyntaxKind::PRE_ELSIF
    )
}

/// Calculates the base indent level from the first non-empty line.
pub fn calculate_base_indent(text: &str) -> u32 {
    for line in text.lines() {
        let trimmed = line.trim_start();
        if !trimmed.is_empty() {
            let leading = line.len() - trimmed.len();
            let tabs = line.chars().take(leading).filter(|&c| c == '\t').count();
            let spaces = line.chars().take(leading).filter(|&c| c == ' ').count();
            return (tabs + spaces / 4) as u32;
        }
    }
    0
}

/// Calculates the depth of a node in the syntax tree.
/// This is used to determine the indentation level for nested constructs.
pub fn calculate_node_depth(node: &SyntaxNode) -> u32 {
    let mut depth = 0u32;
    let mut current = node.parent();

    while let Some(parent) = current {
        if node_indent_adjustment(parent.kind()).is_some() {
            depth += 1;
        }
        current = parent.parent();
    }

    depth
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indent_state_total() {
        let mut state = IndentState::with_base(1);
        assert_eq!(state.total(), 1);

        state.enter_block();
        assert_eq!(state.total(), 2);

        state.enter_expression();
        assert_eq!(state.total(), 3);

        state.leave_block();
        assert_eq!(state.total(), 2);
    }

    #[test]
    fn test_indent_state_offset() {
        let mut state = IndentState::with_base(0);
        state.enter_block();
        assert_eq!(state.total(), 1);

        state.set_current_offset(-1);
        assert_eq!(state.total(), 0);

        state.reset_current_offset();
        assert_eq!(state.total(), 1);
    }

    #[test]
    fn test_calculate_base_indent() {
        assert_eq!(calculate_base_indent("Процедура Тест()"), 0);
        assert_eq!(calculate_base_indent("\tПроцедура Тест()"), 1);
        assert_eq!(calculate_base_indent("\t\tПроцедура Тест()"), 2);
        assert_eq!(calculate_base_indent("    Процедура Тест()"), 1);
        assert_eq!(calculate_base_indent("\n\nПроцедура Тест()"), 0);
    }

    #[test]
    fn test_block_keywords() {
        assert!(is_block_start_keyword(SyntaxKind::KW_PROCEDURE));
        assert!(is_block_start_keyword(SyntaxKind::KW_IF));
        assert!(!is_block_start_keyword(SyntaxKind::KW_ELSE));

        assert!(is_block_end_keyword(SyntaxKind::KW_END_PROCEDURE));
        assert!(is_block_end_keyword(SyntaxKind::KW_END_IF));
        assert!(!is_block_end_keyword(SyntaxKind::KW_IF));

        assert!(is_middle_keyword(SyntaxKind::KW_ELSE));
        assert!(is_middle_keyword(SyntaxKind::KW_ELSIF));
        assert!(is_middle_keyword(SyntaxKind::KW_EXCEPT));
        assert!(!is_middle_keyword(SyntaxKind::KW_IF));
    }
}
