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

/// A "pointer" to a [`SyntaxNode`], via its absolute offset and text range.
///
/// This is a more compact representation than a full `SyntaxNode` which keeps
/// the entire tree alive. It can be resolved back to a `SyntaxNode` by finding
/// the node at the stored position.
///
/// # Example
///
/// ```no_run
/// use syntax::SyntaxNodePtr;
/// # use syntax::SyntaxNode;
/// # fn example(node: &SyntaxNode, root: &SyntaxNode) {
/// let ptr = SyntaxNodePtr::new(node);
/// // ... later ...
/// let resolved = ptr.to_node(root);
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SyntaxNodePtr {
    kind: SyntaxKind,
    range: rowan::TextRange,
}

impl SyntaxNodePtr {
    /// Create a new pointer to the given node.
    pub fn new(node: &SyntaxNode) -> Self {
        Self { kind: node.kind(), range: node.text_range() }
    }

    /// Resolve this pointer back to a syntax node.
    ///
    /// Returns `None` if the node can't be found (e.g., the tree has been modified).
    pub fn to_node(&self, root: &SyntaxNode) -> Option<SyntaxNode> {
        // Fast path: check if root itself matches
        if root.kind() == self.kind && root.text_range() == self.range {
            return Some(root.clone());
        }

        // Search descendants for matching node
        root.descendants().find(|n| n.kind() == self.kind && n.text_range() == self.range)
    }

    /// Try to cast this pointer to a typed AST node.
    pub fn try_to_node<N: crate::ast::AstNode>(&self, root: &SyntaxNode) -> Option<N> {
        self.to_node(root).and_then(N::cast)
    }

    /// Get the syntax kind this pointer refers to.
    pub fn kind(&self) -> SyntaxKind {
        self.kind
    }

    /// Get the text range this pointer refers to.
    pub fn range(&self) -> rowan::TextRange {
        self.range
    }
}

impl From<&SyntaxNode> for SyntaxNodePtr {
    fn from(node: &SyntaxNode) -> Self {
        Self::new(node)
    }
}

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

    #[test]
    fn test_syntax_node_ptr() {
        let mut builder = SyntaxTreeBuilder::new();

        builder.start_node(SyntaxKind::SOURCE_FILE);
        builder.start_node(SyntaxKind::PROCEDURE_DEF);
        builder.token(SyntaxKind::KW_PROCEDURE, "Процедура");
        builder.token(SyntaxKind::IDENT, "Тест");
        builder.finish_node(); // PROCEDURE_DEF
        builder.finish_node(); // SOURCE_FILE

        let parse = builder.finish();
        let root = parse.syntax_node();

        // Find the procedure node
        let proc_node = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::PROCEDURE_DEF)
            .expect("procedure node should exist");

        // Create a pointer to it
        let ptr = SyntaxNodePtr::new(&proc_node);

        // Verify pointer properties
        assert_eq!(ptr.kind(), SyntaxKind::PROCEDURE_DEF);

        // Resolve the pointer back to a node
        let resolved = ptr.to_node(&root).expect("should resolve successfully");
        assert_eq!(resolved.kind(), SyntaxKind::PROCEDURE_DEF);
        assert_eq!(resolved.text_range(), proc_node.text_range());

        // Test From trait
        let ptr2 = SyntaxNodePtr::from(&proc_node);
        assert_eq!(ptr, ptr2);
    }
}
