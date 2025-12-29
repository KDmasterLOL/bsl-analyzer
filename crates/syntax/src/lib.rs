//! Syntax trees for BSL language.
//!
//! This crate provides syntax tree infrastructure based on Rowan.
//!
//! ## Architecture
//!
//! - [`SyntaxKind`] - enum of all syntactic constructs (tokens + nodes)
//! - [`BslLanguage`] - language definition for Rowan
//! - [`SyntaxNode`], [`SyntaxToken`], [`SyntaxElement`] - untyped tree types
//! - [`ast`] - typed AST wrappers over untyped syntax tree
//!
//! ## References
//!
//! Based on rust-analyzer's syntax crate architecture.

pub mod ast;
mod syntax_kind;
mod syntax_node;

use std::marker::PhantomData;

use rowan::GreenNode;

pub use crate::{
    syntax_kind::SyntaxKind,
    syntax_node::{BslLanguage, SyntaxElement, SyntaxNode, SyntaxToken, SyntaxTreeBuilder},
};
pub use rowan::{Direction, NodeOrToken, TextRange, TextSize, TokenAtOffset, WalkEvent};

/// Result of parsing BSL source code.
///
/// Contains a syntax tree (green node) and a list of errors.
/// Note that we always produce a syntax tree, even for completely invalid files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse<T> {
    green: GreenNode,
    errors: Vec<SyntaxError>,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Parse<T> {
    /// Create a new parse result.
    pub(crate) fn new(green: GreenNode, errors: Vec<SyntaxError>) -> Self {
        Self { green, errors, _marker: PhantomData }
    }

    /// Get the syntax tree root.
    pub fn syntax_node(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    /// Get parsing errors.
    pub fn errors(&self) -> &[SyntaxError] {
        &self.errors
    }

    /// Check if there are any errors.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
}

impl Parse<SyntaxNode> {
    /// Cast this parse result to a typed AST node.
    pub fn tree(&self) -> SyntaxNode {
        self.syntax_node()
    }
}

/// A syntax error.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SyntaxError {
    message: String,
    range: TextRange,
}

impl SyntaxError {
    /// Create a new syntax error.
    pub fn new(message: impl Into<String>, range: TextRange) -> Self {
        Self { message: message.into(), range }
    }

    /// Create a syntax error at a specific offset.
    pub fn new_at_offset(message: impl Into<String>, offset: TextSize) -> Self {
        Self::new(message, TextRange::empty(offset))
    }

    /// Get the error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Get the text range of the error.
    pub fn range(&self) -> TextRange {
        self.range
    }
}

impl std::fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {:?}", self.message, self.range)
    }
}

impl std::error::Error for SyntaxError {}
