use std::cell::RefCell;

use parser_error::ParseError;
use rowan::{GreenNode, GreenNodeBuilder, Language, NodeCache};

use crate::{Parse, SyntaxError, SyntaxKind};

thread_local! {
    static SHARED_NODE_CACHE: RefCell<NodeCache> = RefCell::new(NodeCache::default());
}

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

pub type SyntaxNode = rowan::SyntaxNode<BslLanguage>;

pub type SyntaxToken = rowan::SyntaxToken<BslLanguage>;

pub type SyntaxElement = rowan::SyntaxElement<BslLanguage>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SyntaxNodePtr {
    kind: SyntaxKind,
    range: rowan::TextRange,
}

impl SyntaxNodePtr {
    pub fn new(node: &SyntaxNode) -> Self {
        Self { kind: node.kind(), range: node.text_range() }
    }

    pub fn to_node(&self, root: &SyntaxNode) -> Option<SyntaxNode> {
        if root.kind() == self.kind && root.text_range() == self.range {
            return Some(root.clone());
        }

        root.descendants().find(|n| n.kind() == self.kind && n.text_range() == self.range)
    }

    pub fn try_to_node<N: crate::ast::AstNode>(&self, root: &SyntaxNode) -> Option<N> {
        self.to_node(root).and_then(N::cast)
    }

    pub fn kind(&self) -> SyntaxKind {
        self.kind
    }

    pub fn range(&self) -> rowan::TextRange {
        self.range
    }
}

impl From<&SyntaxNode> for SyntaxNodePtr {
    fn from(node: &SyntaxNode) -> Self {
        Self::new(node)
    }
}

pub fn with_shared_node_cache<T>(f: impl FnOnce(&mut NodeCache) -> T) -> T {
    SHARED_NODE_CACHE.with_borrow_mut(f)
}

/// Drop the calling thread's shared green-node cache. The cache holds strong
/// `GreenNode`/`GreenToken` references for cross-parse subtree deduplication and
/// never evicts, so on a thread that parses tens of thousands of unrelated files
/// (a whole-workspace graph build) it grows unbounded and pins every parsed tree's
/// green storage long after the `Parse` itself is dropped. Resetting it releases
/// those references; nodes still held by a live `Parse` survive (green storage is
/// refcounted). Call only between parses, never mid-tree-build.
pub fn clear_shared_node_cache() {
    SHARED_NODE_CACHE.with_borrow_mut(|cache| *cache = NodeCache::default());
}

pub struct SyntaxTreeBuilder<'cache> {
    errors: Vec<SyntaxError>,
    inner: GreenNodeBuilder<'cache>,
}

impl Default for SyntaxTreeBuilder<'static> {
    fn default() -> Self {
        Self { errors: Vec::new(), inner: GreenNodeBuilder::new() }
    }
}

impl SyntaxTreeBuilder<'static> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<'cache> SyntaxTreeBuilder<'cache> {
    pub fn with_cache(cache: &'cache mut NodeCache) -> Self {
        Self { errors: Vec::new(), inner: GreenNodeBuilder::with_cache(cache) }
    }

    pub fn token(&mut self, kind: SyntaxKind, text: &str) {
        let kind = BslLanguage::kind_to_raw(kind);
        self.inner.token(kind, text);
    }

    pub fn start_node(&mut self, kind: SyntaxKind) {
        let kind = BslLanguage::kind_to_raw(kind);
        self.inner.start_node(kind);
    }

    pub fn finish_node(&mut self) {
        self.inner.finish_node();
    }

    pub fn error(&mut self, range: rowan::TextRange, err: ParseError) {
        self.errors.push(SyntaxError::new(range, err));
    }

    pub(crate) fn finish_raw(self) -> (GreenNode, Vec<SyntaxError>) {
        let green = self.inner.finish();
        (green, self.errors)
    }

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
        use parser_error::{ParseError, RecoveryKind};

        let mut builder = SyntaxTreeBuilder::new();

        builder.start_node(SyntaxKind::SOURCE_FILE);
        builder.error(
            rowan::TextRange::empty(rowan::TextSize::from(0)),
            ParseError::Custom { message: "unexpected token", recovery: RecoveryKind::Custom },
        );
        builder.finish_node();

        let parse = builder.finish();
        assert!(parse.has_errors());
        assert_eq!(parse.errors().len(), 1);
        assert_eq!(parse.errors()[0].message(), "Unexpected token");
    }

    #[test]
    fn test_syntax_node_ptr() {
        let mut builder = SyntaxTreeBuilder::new();

        builder.start_node(SyntaxKind::SOURCE_FILE);
        builder.start_node(SyntaxKind::PROCEDURE_DEF);
        builder.token(SyntaxKind::KW_PROCEDURE, "Процедура");
        builder.token(SyntaxKind::IDENT, "Тест");
        builder.finish_node();
        builder.finish_node();

        let parse = builder.finish();
        let root = parse.syntax_node();

        let proc_node = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::PROCEDURE_DEF)
            .expect("procedure node should exist");

        let ptr = SyntaxNodePtr::new(&proc_node);

        assert_eq!(ptr.kind(), SyntaxKind::PROCEDURE_DEF);

        let resolved = ptr.to_node(&root).expect("should resolve successfully");
        assert_eq!(resolved.kind(), SyntaxKind::PROCEDURE_DEF);
        assert_eq!(resolved.text_range(), proc_node.text_range());

        let ptr2 = SyntaxNodePtr::from(&proc_node);
        assert_eq!(ptr, ptr2);
    }
}
