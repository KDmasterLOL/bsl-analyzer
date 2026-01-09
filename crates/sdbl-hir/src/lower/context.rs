//! Lowering context for SDBL HIR.

use bsl_metadata::Configuration;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use crate::diagnostics::SdblDiagnostic;
use crate::scope::Scope;
use crate::source_map::{SdblSourceMap, TokenCategory, TokenInfo};

/// Context for lowering SDBL AST to HIR.
///
/// Maintains:
/// - Metadata for table/field resolution
/// - Scope for name resolution
/// - Collected diagnostics
/// - Source map for semantic highlighting
pub struct LoweringContext<'a> {
    /// 1C Configuration metadata (optional).
    pub(super) metadata: Option<&'a Configuration>,

    /// Current scope for name resolution.
    pub(super) scope: Scope,

    /// Collected semantic diagnostics.
    pub(super) diagnostics: Vec<SdblDiagnostic>,

    /// Source mapping for semantic highlighting.
    pub(super) source_map: SdblSourceMap,
}

impl<'a> LoweringContext<'a> {
    /// Create a new lowering context.
    pub fn new(metadata: Option<&'a Configuration>) -> Self {
        Self {
            metadata,
            scope: Scope::new(),
            diagnostics: Vec::new(),
            source_map: SdblSourceMap::new(),
        }
    }

    /// Push a new scope frame (for subqueries).
    #[allow(dead_code)]
    pub fn push_scope(&mut self) {
        self.scope.push_frame();
    }

    /// Pop the current scope frame.
    #[allow(dead_code)]
    pub fn pop_scope(&mut self) {
        self.scope.pop_frame();
    }

    /// Add a diagnostic.
    #[allow(dead_code)]
    pub fn add_diagnostic(&mut self, diagnostic: SdblDiagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Record a token directly in the source map.
    pub(super) fn record_token(&mut self, token: &SyntaxToken, category: TokenCategory) {
        let info = TokenInfo::new(token.text_range(), token.kind(), token.text().to_string());
        self.source_map.add_token(info, category);
    }

    /// Record keyword by case-insensitive text matching (for SDBL keywords mapped to IDENT).
    ///
    /// SDBL keywords like SELECT, FROM, WHERE are converted to TokenKind::Ident by the parser,
    /// so we need to match by text instead of SyntaxKind.
    pub(super) fn record_keyword_by_text(
        &mut self,
        node: &SyntaxNode,
        en_text: &str,
        ru_text: &str,
        category: TokenCategory,
    ) {
        for element in node.descendants_with_tokens() {
            if let Some(token) = element.as_token() {
                if token.kind() == SyntaxKind::IDENT {
                    let text_lower = token.text().to_lowercase();
                    if text_lower == en_text.to_lowercase() || text_lower == ru_text.to_lowercase()
                    {
                        self.record_token(token, category);
                        return; // Only record first match
                    }
                }
            }
        }
    }
}
