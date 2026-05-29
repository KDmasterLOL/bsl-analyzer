pub mod ast;
pub mod ast_utils;
pub mod sdbl_query;
mod syntax_kind;
mod syntax_node;

use std::marker::PhantomData;

use parser_error::ParseError;
use rowan::GreenNode;

pub use crate::{
    ast_utils::{
        extract_leading_comments, extract_leading_comments_at_offset,
        extract_variable_comments_at_offset, has_trailing_comment, has_variable_description,
    },
    sdbl_query::{extract_sdbl_with_corrections, SdblQueryInfo},
    syntax_kind::SyntaxKind,
    syntax_node::{
        with_shared_node_cache, BslLanguage, SyntaxElement, SyntaxNode, SyntaxNodePtr, SyntaxToken,
        SyntaxTreeBuilder,
    },
};
pub use rowan::{Direction, NodeCache, NodeOrToken, TextRange, TextSize, TokenAtOffset, WalkEvent};

pub const MODULE_RANGE: TextRange = TextRange::empty(TextSize::new(0));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse<T> {
    green: GreenNode,
    errors: Vec<SyntaxError>,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Parse<T> {
    pub(crate) fn new(green: GreenNode, errors: Vec<SyntaxError>) -> Self {
        Self { green, errors, _marker: PhantomData }
    }

    pub fn syntax_node(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    pub fn errors(&self) -> &[SyntaxError] {
        &self.errors
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

impl Parse<SyntaxNode> {
    pub fn tree(&self) -> SyntaxNode {
        self.syntax_node()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SyntaxError {
    message: String,
    range: TextRange,
    error: ParseError,
}

impl SyntaxError {
    pub fn new(range: TextRange, err: ParseError) -> Self {
        Self { message: err.format_ru(), range, error: err }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn range(&self) -> TextRange {
        self.range
    }

    pub fn structured(&self) -> &ParseError {
        &self.error
    }
}

impl std::fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {:?}", self.message, self.range)
    }
}

impl std::error::Error for SyntaxError {}

#[cfg(test)]
mod tests {
    use parser_error::{ParseError, RecoveryKind};

    use super::*;

    #[test]
    fn syntax_error_carries_structured_payload() {
        let range = TextRange::new(0.into(), 5.into());
        let err = ParseError::Unexpected { found: None, recovery: RecoveryKind::MissingToken };
        let syntax_err = SyntaxError::new(range, err.clone());

        assert_eq!(syntax_err.range(), range);
        assert_eq!(syntax_err.structured(), &err);
        assert!(syntax_err.message().contains("конец файла"));
    }
}
