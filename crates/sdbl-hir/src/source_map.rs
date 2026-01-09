//! Source mapping for SDBL HIR to syntax tokens.
//!
//! This module provides bidirectional mapping between SDBL HIR and source tokens
//! for semantic highlighting. Unlike BSL's `BodySourceMap` which maps HIR node IDs
//! to ranges, `SdblSourceMap` stores explicit token positions for keywords and operators
//! that are implicit in HIR structure.
//!
//! ## Architecture
//!
//! ```text
//! SDBL AST (syntax tokens)
//!       │
//!       ▼ lower() + collect_token_positions()
//! ┌─────────────────────────────────────┐
//! │         SdblHir                     │  Semantic structure
//! │  (ExprHir with ranges)              │
//! └─────────────────────────────────────┘
//!       +
//! ┌─────────────────────────────────────┐
//! │      SdblSourceMap                  │  Token positions
//! │  ┌──────────────────────────────┐   │
//! │  │ keywords: Vec<TokenInfo>     │   │  SELECT, FROM, WHERE, etc.
//! │  │ operators: Vec<TokenInfo>    │   │  =, <>, AND, OR, etc.
//! │  │ special: Vec<TokenInfo>      │   │  CASE, WHEN, THEN, etc.
//! │  └──────────────────────────────┘   │
//! └─────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! Used by semantic highlighting to assign token types to keywords/operators
//! that don't have explicit HIR nodes.

use rustc_hash::FxHashMap;
use smol_str::SmolStr;
use syntax::SyntaxKind;
use text_size::TextRange;

/// Bidirectional mapping between SDBL HIR and source tokens for semantic highlighting.
///
/// Stores token positions for keywords and operators that are implicit in HIR structure.
/// Tokens are grouped by category for efficient queries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SdblSourceMap {
    /// SDBL clause keywords (SELECT, FROM, WHERE, GROUP BY, ORDER BY, etc.)
    /// Sorted by TextRange start for efficient lookup.
    pub(crate) clause_keywords: Vec<TokenInfo>,

    /// Logical/comparison operators (AND, OR, NOT, =, <>, <, >, etc.)
    pub(crate) operators: Vec<TokenInfo>,

    /// Special keywords (IN, BETWEEN, LIKE, IS NULL, CASE, WHEN, THEN, ELSE, END)
    pub(crate) special_keywords: Vec<TokenInfo>,

    /// JOIN-related keywords (JOIN, INNER, LEFT, RIGHT, FULL, OUTER, ON)
    pub(crate) join_keywords: Vec<TokenInfo>,

    /// Query modifiers (DISTINCT, TOP, UNION, ALL)
    pub(crate) modifiers: Vec<TokenInfo>,

    /// Aggregate function names (SUM, AVG, COUNT, MIN, MAX)
    /// Stored separately because they need different highlighting than regular functions.
    pub(crate) aggregate_functions: Vec<TokenInfo>,

    /// Range lookup: TextRange → TokenCategory (for reverse lookup).
    /// Used by "find token at position" queries.
    range_to_category: FxHashMap<TextRange, TokenCategory>,
}

impl SdblSourceMap {
    /// Create a new empty source map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Get all tokens for semantic highlighting.
    /// Returns iterator over all tokens sorted by range.
    pub fn all_tokens(&self) -> impl Iterator<Item = (&TokenInfo, TokenCategory)> {
        let clause_iter = self.clause_keywords.iter().map(|t| (t, TokenCategory::ClauseKeyword));
        let op_iter = self.operators.iter().map(|t| (t, TokenCategory::Operator));
        let special_iter = self.special_keywords.iter().map(|t| (t, TokenCategory::SpecialKeyword));
        let join_iter = self.join_keywords.iter().map(|t| (t, TokenCategory::JoinKeyword));
        let modifier_iter = self.modifiers.iter().map(|t| (t, TokenCategory::Modifier));
        let agg_iter =
            self.aggregate_functions.iter().map(|t| (t, TokenCategory::AggregateFunction));

        clause_iter
            .chain(op_iter)
            .chain(special_iter)
            .chain(join_iter)
            .chain(modifier_iter)
            .chain(agg_iter)
    }

    /// Find token at a given position.
    pub fn token_at_position(
        &self,
        offset: text_size::TextSize,
    ) -> Option<(&TokenInfo, TokenCategory)> {
        self.all_tokens().find(|(info, _)| info.range.contains(offset))
    }

    /// Find token by exact range.
    pub fn token_at_range(&self, range: TextRange) -> Option<(&TokenInfo, TokenCategory)> {
        self.range_to_category.get(&range).and_then(|cat| {
            let tokens = self.tokens_by_category(*cat);
            tokens.iter().find(|info| info.range == range).map(|t| (t, *cat))
        })
    }

    /// Get all tokens in a specific category.
    pub fn tokens_by_category(&self, category: TokenCategory) -> &[TokenInfo] {
        match category {
            TokenCategory::ClauseKeyword => &self.clause_keywords,
            TokenCategory::Operator => &self.operators,
            TokenCategory::SpecialKeyword => &self.special_keywords,
            TokenCategory::JoinKeyword => &self.join_keywords,
            TokenCategory::Modifier => &self.modifiers,
            TokenCategory::AggregateFunction => &self.aggregate_functions,
        }
    }

    /// Add a token to the map.
    pub(crate) fn add_token(&mut self, info: TokenInfo, category: TokenCategory) {
        self.range_to_category.insert(info.range, category);

        match category {
            TokenCategory::ClauseKeyword => self.clause_keywords.push(info),
            TokenCategory::Operator => self.operators.push(info),
            TokenCategory::SpecialKeyword => self.special_keywords.push(info),
            TokenCategory::JoinKeyword => self.join_keywords.push(info),
            TokenCategory::Modifier => self.modifiers.push(info),
            TokenCategory::AggregateFunction => self.aggregate_functions.push(info),
        }
    }

    /// Sort all token lists by range start (call after lowering completes).
    pub(crate) fn finalize(&mut self) {
        self.clause_keywords.sort_by_key(|t| t.range.start());
        self.operators.sort_by_key(|t| t.range.start());
        self.special_keywords.sort_by_key(|t| t.range.start());
        self.join_keywords.sort_by_key(|t| t.range.start());
        self.modifiers.sort_by_key(|t| t.range.start());
        self.aggregate_functions.sort_by_key(|t| t.range.start());
    }
}

/// Information about a single token in SDBL source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenInfo {
    /// Source range of the token.
    pub range: TextRange,

    /// Syntax kind (from lexer/parser).
    pub kind: SyntaxKind,

    /// Original text (case-preserved, for display).
    /// Stored as SmolStr for memory efficiency (most keywords are <16 chars).
    pub text: SmolStr,
}

impl TokenInfo {
    /// Create a new TokenInfo.
    pub fn new(range: TextRange, kind: SyntaxKind, text: impl Into<SmolStr>) -> Self {
        Self { range, kind, text: text.into() }
    }
}

/// Token category for reverse lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenCategory {
    /// Clause keywords (SELECT, FROM, WHERE, GROUP BY, ORDER BY).
    ClauseKeyword,
    /// Operators (=, <>, AND, OR, +, -, *, /).
    Operator,
    /// Special keywords (IN, BETWEEN, LIKE, IS NULL, CASE, WHEN, THEN, ELSE, END).
    SpecialKeyword,
    /// JOIN keywords (JOIN, INNER, LEFT, RIGHT, FULL, OUTER, ON).
    JoinKeyword,
    /// Modifiers (DISTINCT, TOP, UNION, ALL).
    Modifier,
    /// Aggregate functions (SUM, AVG, COUNT, MIN, MAX).
    AggregateFunction,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_map_empty() {
        let map = SdblSourceMap::new();
        assert_eq!(map.clause_keywords.len(), 0);
        assert_eq!(map.operators.len(), 0);
        assert_eq!(map.all_tokens().count(), 0);
    }

    #[test]
    fn test_add_token() {
        let mut map = SdblSourceMap::new();

        let token = TokenInfo::new(TextRange::new(0.into(), 6.into()), SyntaxKind::IDENT, "SELECT");
        map.add_token(token.clone(), TokenCategory::ClauseKeyword);

        assert_eq!(map.clause_keywords.len(), 1);
        assert_eq!(map.clause_keywords[0].text, "SELECT");
        assert_eq!(map.all_tokens().count(), 1);
    }

    #[test]
    fn test_token_at_position() {
        let mut map = SdblSourceMap::new();

        let token = TokenInfo::new(TextRange::new(0.into(), 6.into()), SyntaxKind::IDENT, "SELECT");
        map.add_token(token, TokenCategory::ClauseKeyword);

        // Position inside SELECT
        let result = map.token_at_position(3.into());
        assert!(result.is_some());
        let (info, category) = result.unwrap();
        assert_eq!(info.text, "SELECT");
        assert_eq!(category, TokenCategory::ClauseKeyword);

        // Position outside
        let result = map.token_at_position(10.into());
        assert!(result.is_none());
    }

    #[test]
    fn test_token_at_range() {
        let mut map = SdblSourceMap::new();

        let range = TextRange::new(0.into(), 6.into());
        let token = TokenInfo::new(range, SyntaxKind::IDENT, "SELECT");
        map.add_token(token, TokenCategory::ClauseKeyword);

        let result = map.token_at_range(range);
        assert!(result.is_some());
        let (info, category) = result.unwrap();
        assert_eq!(info.text, "SELECT");
        assert_eq!(category, TokenCategory::ClauseKeyword);
    }

    #[test]
    fn test_finalize_sorts_tokens() {
        let mut map = SdblSourceMap::new();

        // Add tokens in reverse order
        map.add_token(
            TokenInfo::new(TextRange::new(20.into(), 25.into()), SyntaxKind::IDENT, "WHERE"),
            TokenCategory::ClauseKeyword,
        );
        map.add_token(
            TokenInfo::new(TextRange::new(0.into(), 6.into()), SyntaxKind::IDENT, "SELECT"),
            TokenCategory::ClauseKeyword,
        );
        map.add_token(
            TokenInfo::new(TextRange::new(10.into(), 14.into()), SyntaxKind::IDENT, "FROM"),
            TokenCategory::ClauseKeyword,
        );

        map.finalize();

        // Should be sorted by range start
        assert_eq!(map.clause_keywords[0].text, "SELECT");
        assert_eq!(map.clause_keywords[1].text, "FROM");
        assert_eq!(map.clause_keywords[2].text, "WHERE");
    }

    #[test]
    fn test_tokens_by_category() {
        let mut map = SdblSourceMap::new();

        map.add_token(
            TokenInfo::new(TextRange::new(0.into(), 6.into()), SyntaxKind::IDENT, "SELECT"),
            TokenCategory::ClauseKeyword,
        );
        map.add_token(
            TokenInfo::new(TextRange::new(20.into(), 21.into()), SyntaxKind::EQ, "="),
            TokenCategory::Operator,
        );

        let keywords = map.tokens_by_category(TokenCategory::ClauseKeyword);
        assert_eq!(keywords.len(), 1);
        assert_eq!(keywords[0].text, "SELECT");

        let operators = map.tokens_by_category(TokenCategory::Operator);
        assert_eq!(operators.len(), 1);
        assert_eq!(operators[0].text, "=");
    }
}
