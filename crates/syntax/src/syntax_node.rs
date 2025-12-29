//! Concrete Syntax Tree (CST) for BSL language.
//!
//! The CST includes comments and whitespace, provides a single node type,
//! `SyntaxNode`, and a basic traversal API (parent, children, siblings).
//!
//! The real implementation is in the (language-agnostic) `rowan` crate, this
//! module just wraps its API.

use rowan::{GreenNode, GreenNodeBuilder, Language};

use crate::{Parse, SyntaxError, SyntaxKind};

/// BSL language definition for Rowan.
///
/// This is a zero-sized type that implements the `Language` trait,
/// connecting our `SyntaxKind` enum with Rowan's infrastructure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BslLanguage {}

impl Language for BslLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
        SyntaxKind::from(raw.0)
    }

    fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind.into())
    }
}

/// Untyped BSL syntax node.
///
/// This is a thin wrapper around Rowan's `SyntaxNode` with BSL language type.
pub type SyntaxNode = rowan::SyntaxNode<BslLanguage>;

/// Untyped BSL syntax token.
///
/// This represents a single token in the syntax tree (leaf node).
pub type SyntaxToken = rowan::SyntaxToken<BslLanguage>;

/// Untyped BSL syntax element (either a node or a token).
pub type SyntaxElement = rowan::SyntaxElement<BslLanguage>;

/// Iterator over syntax node children.
#[allow(dead_code)]
pub type SyntaxNodeChildren = rowan::SyntaxNodeChildren<BslLanguage>;

/// Iterator over syntax element children (both nodes and tokens).
#[allow(dead_code)]
pub type SyntaxElementChildren = rowan::SyntaxElementChildren<BslLanguage>;

/// Builder for constructing syntax trees.
///
/// The parser generates events, which are then processed into a tree
/// using this builder.
#[derive(Default)]
pub struct SyntaxTreeBuilder {
    errors: Vec<SyntaxError>,
    inner: GreenNodeBuilder<'static>,
}

impl SyntaxTreeBuilder {
    /// Create a new syntax tree builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a token to the current node.
    pub fn token(&mut self, kind: SyntaxKind, text: &str) {
        let kind = BslLanguage::kind_to_raw(kind);
        self.inner.token(kind, text);
    }

    /// Start a new node.
    pub fn start_node(&mut self, kind: SyntaxKind) {
        let kind = BslLanguage::kind_to_raw(kind);
        self.inner.start_node(kind);
    }

    /// Finish the current node.
    pub fn finish_node(&mut self) {
        self.inner.finish_node();
    }

    /// Add an error at a specific text position.
    pub fn error(&mut self, message: impl Into<String>, offset: rowan::TextSize) {
        self.errors.push(SyntaxError::new_at_offset(message, offset));
    }

    /// Finish building and return the green node and errors.
    pub(crate) fn finish_raw(self) -> (GreenNode, Vec<SyntaxError>) {
        let green = self.inner.finish();
        (green, self.errors)
    }

    /// Finish building and return a `Parse` result.
    pub fn finish(self) -> Parse<SyntaxNode> {
        let (green, errors) = self.finish_raw();
        Parse::new(green, errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_simple_tree() {
        let mut builder = SyntaxTreeBuilder::new();

        builder.start_node(SyntaxKind::SOURCE_FILE);
        builder.token(SyntaxKind::KW_PROCEDURE, "Процедура");
        builder.token(SyntaxKind::WHITESPACE, " ");
        builder.token(SyntaxKind::IDENT, "Тест");
        builder.token(SyntaxKind::L_PAREN, "(");
        builder.token(SyntaxKind::R_PAREN, ")");
        builder.finish_node();

        let parse = builder.finish();
        assert!(!parse.has_errors());
        let root = parse.syntax_node();
        assert_eq!(root.kind(), SyntaxKind::SOURCE_FILE);
    }

    #[test]
    fn test_build_with_error() {
        let mut builder = SyntaxTreeBuilder::new();

        builder.start_node(SyntaxKind::SOURCE_FILE);
        builder.error("unexpected token", rowan::TextSize::from(0));
        builder.finish_node();

        let parse = builder.finish();
        assert!(parse.has_errors());
        assert_eq!(parse.errors().len(), 1);
        assert_eq!(parse.errors()[0].message(), "unexpected token");
    }
}
