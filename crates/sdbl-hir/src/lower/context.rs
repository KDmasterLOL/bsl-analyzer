use bsl_metadata::QueryMetadataResolver;
use stdx::case::CaseExt;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

use crate::diagnostics::SdblDiagnostic;
use crate::scope::Scope;
use crate::source_map::{SdblSourceMap, TokenCategory, TokenInfo};

pub struct LoweringContext<'a> {
    pub(super) resolver: Option<&'a dyn QueryMetadataResolver>,

    pub(super) scope: Scope<'a>,

    pub(super) diagnostics: Vec<SdblDiagnostic>,

    pub(super) source_map: SdblSourceMap,
}

impl<'a> LoweringContext<'a> {
    pub fn new(resolver: Option<&'a dyn QueryMetadataResolver>) -> Self {
        Self {
            scope: match resolver {
                Some(resolver) => Scope::new_with_resolver(resolver),
                None => Scope::new(),
            },
            resolver,
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
                    let text_lower = token.text().fold_lower();
                    if text_lower == en_text.fold_lower() || text_lower == ru_text.fold_lower() {
                        self.record_token(token, category);
                        return;
                    }
                }
            }
        }
    }
}
