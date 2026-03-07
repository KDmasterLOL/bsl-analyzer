//! Lowering context for SDBL HIR.

use std::sync::Arc;

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
pub struct LoweringContext {
    /// 1C Configuration metadata (optional, Arc-wrapped to avoid cloning).
    pub(super) metadata: Option<Arc<Configuration>>,

    /// Current scope for name resolution.
    pub(super) scope: Scope,

    /// Collected semantic diagnostics.
    pub(super) diagnostics: Vec<SdblDiagnostic>,

    /// Source mapping for semantic highlighting.
    pub(super) source_map: SdblSourceMap,
}

impl LoweringContext {
    /// Create a new lowering context.
    ///
    /// If metadata is provided, it will be passed to Scope for resolving nested field references.
    /// Uses Arc to avoid cloning the large Configuration structure.
    pub fn new(metadata: Option<Arc<Configuration>>) -> Self {
        Self {
            scope: Scope::new_with_metadata(metadata.clone()),
            metadata,
            diagnostics: Vec::new(),
            source_map: SdblSourceMap::new(),
        }
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
