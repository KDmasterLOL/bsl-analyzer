//! Convert internal types to LSP protocol types.
//!
//! This module provides conversions from our internal representation
//! (Diagnostic, TextRange, etc.) to LSP types (lsp_types).

use ide::{Diagnostic as IdeDiagnostic, HlMod, HlRange, HlTag, Severity};
use ide_db::TextRange;
use ide_diagnostics::DiagnosticTag as IdeTag;
use line_index::{LineIndex, TextSize};
use lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, DiagnosticTag, Location,
    NumberOrString, Position, Range, SemanticToken, SemanticTokenModifier, SemanticTokenType,
    SemanticTokensLegend, Url,
};

/// Converts a TextRange to an LSP Range.
///
/// Uses UTF-16 code units for positions as required by LSP protocol.
/// This is critical for non-ASCII text (e.g., Cyrillic) where byte positions differ from UTF-16 positions.
///
/// # Errors
/// Returns an error if the range is out of bounds.
pub fn range(line_index: &LineIndex, text: &str, range: TextRange) -> Option<Range> {
    let start = position_utf16(line_index, text, range.start())?;
    let end = position_utf16(line_index, text, range.end())?;
    Some(Range { start, end })
}

/// Converts a TextSize to an LSP Position.
///
/// **IMPORTANT**: LSP requires character positions in UTF-16 code units, not bytes!
/// This function is a helper - you must use `position_utf16()` which takes text parameter.
pub fn position(line_index: &LineIndex, offset: TextSize) -> Option<Position> {
    let line_col = line_index.line_col(offset);
    Some(Position { line: line_col.line, character: line_col.col })
}

/// Converts a TextSize to an LSP Position with UTF-16 character offset.
///
/// **USE THIS** instead of `position()` for all LSP protocol conversions.
/// LSP requires positions in UTF-16 code units, not bytes.
pub fn position_utf16(line_index: &LineIndex, text: &str, offset: TextSize) -> Option<Position> {
    let line_col = line_index.line_col(offset);
    let utf16_col = line_index.utf16_col(text, line_col.line, line_col.col);
    Some(Position { line: line_col.line, character: utf16_col })
}

/// Converts our Severity to LSP DiagnosticSeverity.
pub fn severity(severity: Severity) -> DiagnosticSeverity {
    match severity {
        Severity::Blocker => DiagnosticSeverity::ERROR,
        Severity::Critical => DiagnosticSeverity::ERROR,
        Severity::Major => DiagnosticSeverity::ERROR,
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Information => DiagnosticSeverity::INFORMATION,
        Severity::Hint => DiagnosticSeverity::HINT,
    }
}

/// Converts our DiagnosticTag to LSP DiagnosticTag.
pub fn diagnostic_tags(tags: &[IdeTag]) -> Option<Vec<DiagnosticTag>> {
    if tags.is_empty() {
        return None;
    }

    Some(
        tags.iter()
            .map(|tag| match tag {
                IdeTag::Unnecessary => DiagnosticTag::UNNECESSARY,
                IdeTag::Deprecated => DiagnosticTag::DEPRECATED,
            })
            .collect(),
    )
}

/// Converts an IDE Diagnostic to an LSP Diagnostic.
///
/// # Errors
/// Returns None if the diagnostic range cannot be converted.
pub fn diagnostic(line_index: &LineIndex, text: &str, diag: &IdeDiagnostic) -> Option<Diagnostic> {
    let range = range(line_index, text, diag.range)?;
    let severity = severity(diag.severity);
    let code = Some(NumberOrString::String(diag.code.as_str().to_string()));
    let tags = diagnostic_tags(&diag.tags);

    Some(Diagnostic {
        range,
        severity: Some(severity),
        code,
        code_description: None,
        source: Some("bsl-analyzer".to_string()),
        message: diag.message.clone(),
        related_information: None,
        tags,
        data: None,
    })
}

/// Converts multiple diagnostics to LSP format.
pub fn diagnostics(line_index: &LineIndex, text: &str, diags: &[IdeDiagnostic]) -> Vec<Diagnostic> {
    diags.iter().filter_map(|d| diagnostic(line_index, text, d)).collect()
}

/// Converts a FileId + TextRange to an LSP Location.
///
/// # Errors
/// Returns None if the range cannot be converted or URL cannot be created.
pub fn location(
    line_index: &LineIndex,
    text: &str,
    url: &Url,
    text_range: TextRange,
) -> Option<Location> {
    let lsp_range = range(line_index, text, text_range)?;
    Some(Location { uri: url.clone(), range: lsp_range })
}

/// Converts related information for diagnostics.
pub fn related_information(
    line_index: &LineIndex,
    text: &str,
    url: &Url,
    message: String,
    text_range: TextRange,
) -> Option<DiagnosticRelatedInformation> {
    let loc = location(line_index, text, url, text_range)?;
    Some(DiagnosticRelatedInformation { location: loc, message })
}

/// Returns the semantic tokens legend (token types and modifiers).
pub fn semantic_tokens_legend() -> SemanticTokensLegend {
    let token_types = vec![
        SemanticTokenType::KEYWORD,
        SemanticTokenType::FUNCTION,
        SemanticTokenType::PARAMETER,
        SemanticTokenType::VARIABLE,
        SemanticTokenType::STRING,
        SemanticTokenType::NUMBER,
        SemanticTokenType::COMMENT,
        SemanticTokenType::MACRO,
        SemanticTokenType::DECORATOR,
        SemanticTokenType::PROPERTY,
        SemanticTokenType::OPERATOR,
        SemanticTokenType::new("unresolvedReference"),
    ];

    let token_modifiers = vec![
        SemanticTokenModifier::new("defaultLibrary"),
        SemanticTokenModifier::new("deprecated"),
        SemanticTokenModifier::new("async"),
        SemanticTokenModifier::new("declaration"),
        SemanticTokenModifier::new("definition"),
    ];

    SemanticTokensLegend { token_types, token_modifiers }
}

/// Converts HlTag to token type index.
fn token_type_index(tag: HlTag) -> u32 {
    match tag {
        HlTag::Keyword | HlTag::BooleanLiteral => 0,
        HlTag::Function | HlTag::Procedure | HlTag::BuiltinFunction => 1,
        HlTag::Parameter => 2,
        HlTag::Variable => 3,
        HlTag::StringLiteral => 4,
        HlTag::NumberLiteral => 5,
        HlTag::Comment => 6,
        HlTag::Preprocessor => 7,
        HlTag::Annotation => 8,
        HlTag::Property => 9,
        HlTag::Operator => 10,
        HlTag::UnresolvedReference => 11,
    }
}

/// Converts HlMod to token modifiers bitset.
fn token_modifiers_bitset(mods: HlMod) -> u32 {
    let mut bitset = 0u32;
    if mods.contains(HlMod::EXPORT) {
        bitset |= 1 << 0;
    }
    if mods.contains(HlMod::DEPRECATED) {
        bitset |= 1 << 1;
    }
    if mods.contains(HlMod::ASYNC) {
        bitset |= 1 << 2;
    }
    if mods.contains(HlMod::DECLARATION) {
        bitset |= 1 << 3;
    }
    if mods.contains(HlMod::DEFINITION) {
        bitset |= 1 << 4;
    }
    bitset
}

/// Converts highlighted ranges to LSP semantic tokens (delta encoding).
///
/// Semantic tokens are encoded as a flat array of integers:
/// [deltaLine, deltaStart, length, tokenType, tokenModifiers, ...]
///
/// **IMPORTANT**: The length field must be in UTF-16 code units, not bytes!
/// This is critical for non-ASCII text like Cyrillic where 1 char = 2 bytes but 1 UTF-16 code unit.
pub fn semantic_tokens(
    line_index: &LineIndex,
    text: &str,
    highlights: &[HlRange],
) -> Vec<SemanticToken> {
    let mut tokens = Vec::new();
    let mut prev_line = 0;
    let mut prev_start = 0;

    // Sort by position for delta encoding
    let mut sorted: Vec<_> = highlights.iter().collect();
    sorted.sort_by_key(|hl| hl.range.start());

    for hl in sorted {
        // CRITICAL: Use UTF-16 positions, not byte positions!
        let start_pos = match position_utf16(line_index, text, hl.range.start()) {
            Some(pos) => pos,
            None => continue,
        };

        // CRITICAL: Use UTF-16 length, not byte length!
        // For Cyrillic: "ПрограммныйИнтерфейс" = 40 bytes but only 20 UTF-16 code units
        let length = LineIndex::utf16_len(text, hl.range);

        // Calculate deltas
        let delta_line = start_pos.line - prev_line;
        let delta_start =
            if delta_line == 0 { start_pos.character - prev_start } else { start_pos.character };

        tokens.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type: token_type_index(hl.tag),
            token_modifiers_bitset: token_modifiers_bitset(hl.modifiers),
        });

        prev_line = start_pos.line;
        prev_start = start_pos.character;
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide::DiagnosticCode;

    #[test]
    fn test_range_conversion() {
        let text = "hello\nworld";
        let line_index = LineIndex::new(text);

        // Range covering "world"
        let text_range = TextRange::new(6.into(), 11.into());
        let lsp_range = range(&line_index, text, text_range).unwrap();

        assert_eq!(lsp_range.start.line, 1);
        assert_eq!(lsp_range.start.character, 0);
        assert_eq!(lsp_range.end.line, 1);
        assert_eq!(lsp_range.end.character, 5);
    }

    #[test]
    fn test_severity_conversion() {
        assert_eq!(severity(Severity::Error), DiagnosticSeverity::ERROR);
        assert_eq!(severity(Severity::Warning), DiagnosticSeverity::WARNING);
        assert_eq!(severity(Severity::Hint), DiagnosticSeverity::HINT);
    }

    #[test]
    fn test_diagnostic_conversion() {
        let text = "hello\nworld";
        let line_index = LineIndex::new(text);

        let ide_diag = IdeDiagnostic {
            code: DiagnosticCode::EmptyCodeBlock,
            message: "Empty code block".to_string(),
            severity: Severity::Warning,
            range: TextRange::new(6.into(), 11.into()),
            tags: vec![IdeTag::Unnecessary],
            fixes: vec![],
        };

        let lsp_diag = diagnostic(&line_index, text, &ide_diag).unwrap();

        assert_eq!(lsp_diag.message, "Empty code block");
        assert_eq!(lsp_diag.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(lsp_diag.code, Some(NumberOrString::String("EmptyCodeBlock".to_string())));
        assert_eq!(lsp_diag.source, Some("bsl-analyzer".to_string()));
        assert_eq!(lsp_diag.tags, Some(vec![DiagnosticTag::UNNECESSARY]));
    }

    #[test]
    fn test_range_utf16_cyrillic() {
        // Test case from MissingReturnedValueDescription bug report.
        // Code: "// Описание\nФункция ЗапросВERP(СервисПублика) Экспорт"
        // Diagnostic should highlight "ЗапросВERP" which is at bytes 35..52.
        //
        // IMPORTANT: LSP positions must use UTF-16 code units, not bytes!
        // "// Описание\nФункция " = 11 Cyrillic chars + 10 ASCII = 21 chars total
        // In UTF-16: 11*1 + 10*1 = 21 code units
        // "ЗапросВERP" = 7 Cyrillic + 3 ASCII = 10 chars = 10 UTF-16 code units
        //
        // Expected LSP position: line 1, characters 8..18 (UTF-16 code units)
        // (8 = "Функция " in UTF-16, 18 = 8 + 10)
        let text = "// Описание\nФункция ЗапросВERP(СервисПублика) Экспорт";
        let line_index = LineIndex::new(text);

        // Byte range for "ЗапросВERP" (35..52 in UTF-8)
        let text_range = TextRange::new(35.into(), 52.into());
        let lsp_range = range(&line_index, text, text_range).unwrap();

        // Verify UTF-16 positions
        assert_eq!(lsp_range.start.line, 1, "Start line should be 1");
        assert_eq!(lsp_range.start.character, 8, "Start character should be 8 (UTF-16 code units)");
        assert_eq!(lsp_range.end.line, 1, "End line should be 1");
        assert_eq!(lsp_range.end.character, 18, "End character should be 18 (UTF-16 code units)");
    }
}
