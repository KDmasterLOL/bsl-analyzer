//! Typed AST wrappers for syntax nodes.
//!
//! This module provides typed access to syntax nodes.

use crate::{SyntaxKind, SyntaxNode, SyntaxToken};

/// Trait for AST nodes.
pub trait AstNode: Sized {
    fn can_cast(kind: SyntaxKind) -> bool;
    fn cast(node: SyntaxNode) -> Option<Self>;
    fn syntax(&self) -> &SyntaxNode;
}

/// Source file node.
#[derive(Debug, Clone)]
pub struct SourceFile(SyntaxNode);

impl AstNode for SourceFile {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::SOURCE_FILE
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

/// Procedure definition.
#[derive(Debug, Clone)]
pub struct ProcedureDef(SyntaxNode);

impl AstNode for ProcedureDef {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::PROCEDURE_DEF
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl ProcedureDef {
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == SyntaxKind::IDENT)
    }
}

/// Function definition.
#[derive(Debug, Clone)]
pub struct FunctionDef(SyntaxNode);

impl AstNode for FunctionDef {
    fn can_cast(kind: SyntaxKind) -> bool {
        kind == SyntaxKind::FUNCTION_DEF
    }

    fn cast(node: SyntaxNode) -> Option<Self> {
        if Self::can_cast(node.kind()) {
            Some(Self(node))
        } else {
            None
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        &self.0
    }
}

impl FunctionDef {
    pub fn name(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(|it| it.into_token())
            .find(|it| it.kind() == SyntaxKind::IDENT)
    }
}

// TODO: Add more AST nodes
