use std::sync::Arc;

use bsl_metadata::Configuration;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use crate::diagnostics::SdblDiagnostic;
use crate::scope::Scope;
use crate::source_map::{SdblSourceMap, TokenCategory, TokenInfo};

pub struct LoweringContext {
    pub(super) metadata: Option<Arc<Configuration>>,

    pub(super) scope: Scope,

    pub(super) diagnostics: Vec<SdblDiagnostic>,

    pub(super) source_map: SdblSourceMap,
}

impl LoweringContext {
    pub fn new(metadata: Option<Arc<Configuration>>) -> Self {
        Self {
            scope: Scope::new_with_metadata(metadata.clone()),
            metadata,
            diagnostics: Vec::new(),
            source_map: SdblSourceMap::new(),
        }
    }

    pub(super) fn record_token(&mut self, token: &SyntaxToken, category: TokenCategory) {
        let info = TokenInfo::new(token.text_range(), token.kind(), token.text().to_string());
        self.source_map.add_token(info, category);
    }

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
                        return;
                    }
                }
            }
        }
    }
}
