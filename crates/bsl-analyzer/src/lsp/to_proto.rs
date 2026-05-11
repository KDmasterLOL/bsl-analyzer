//! Convert internal types to LSP protocol types.
//!
//! This module provides conversions from our internal representation
//! (Diagnostic, TextRange, etc.) to LSP types (lsp_types).

use ide::{Diagnostic as IdeDiagnostic, HlMod, HlRange, HlTag, Severity};
use ide::{DiagnosticTag as IdeTag, TextRange};
use line_index::{LineIndex, TextSize};
use lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, DiagnosticTag, Location,
    NumberOrString, Position, Range, SemanticToken, SemanticTokenModifier, SemanticTokenType,
    SemanticTokensLegend, Url,
};

use crate::lsp::PositionEncoding;

/// Converts a TextRange to an LSP Range.
///
/// Uses UTF-16 code units for positions as required by LSP protocol.
/// This is critical for non-ASCII text (e.g., Cyrillic) where byte positions differ from UTF-16 positions.
///
/// # Errors
/// Returns an error if the range is out of bounds.
pub fn range(line_index: &LineIndex, text: &str, range: TextRange) -> Option<Range> {
    range_with_encoding(line_index, text, range, PositionEncoding::Utf16)
}

pub fn range_with_encoding(
    line_index: &LineIndex,
    text: &str,
    range: TextRange,
    encoding: PositionEncoding,
) -> Option<Range> {
    if encoding == PositionEncoding::Utf8 {
        let start = position(line_index, range.start())?;
        let end = position(line_index, range.end())?;
        return Some(Range { start, end });
    }

    let start = position_utf16(line_index, text, range.start())?;
    let end = position_utf16(line_index, text, range.end())?;
    Some(Range { start, end })
}

/// Converts a TextSize to an LSP Position.
///
/// **IMPORTANT**: LSP requires character positions in UTF-16 code units, not bytes!
/// This function is a helper - you must use `position_utf16()` which takes text parameter.
pub fn position(line_index: &LineIndex, offset: TextSize) -> Option<Position> {
    let line_col = line_index.try_line_col(offset)?;
    Some(Position { line: line_col.line, character: line_col.col })
}

/// Converts a TextSize to an LSP Position with UTF-16 character offset.
///
/// **USE THIS** instead of `position()` for all LSP protocol conversions.
/// LSP requires positions in UTF-16 code units, not bytes.
pub fn position_utf16(line_index: &LineIndex, text: &str, offset: TextSize) -> Option<Position> {
    let line_col = line_index.try_line_col(offset)?;
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
    diagnostic_with_encoding(line_index, text, diag, PositionEncoding::Utf16)
}

pub fn diagnostic_with_encoding(
    line_index: &LineIndex,
    text: &str,
    diag: &IdeDiagnostic,
    encoding: PositionEncoding,
) -> Option<Diagnostic> {
    let range = range_with_encoding(line_index, text, diag.range, encoding)?;
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
    diagnostics_with_encoding(line_index, text, diags, PositionEncoding::Utf16)
}

pub fn diagnostics_with_encoding(
    line_index: &LineIndex,
    text: &str,
    diags: &[IdeDiagnostic],
    encoding: PositionEncoding,
) -> Vec<Diagnostic> {
    diags.iter().filter_map(|d| diagnostic_with_encoding(line_index, text, d, encoding)).collect()
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
    location_with_encoding(line_index, text, url, text_range, PositionEncoding::Utf16)
}

pub fn location_with_encoding(
    line_index: &LineIndex,
    text: &str,
    url: &Url,
    text_range: TextRange,
    encoding: PositionEncoding,
) -> Option<Location> {
    let lsp_range = range_with_encoding(line_index, text, text_range, encoding)?;
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
    let loc = location_with_encoding(line_index, text, url, text_range, PositionEncoding::Utf16)?;
    Some(DiagnosticRelatedInformation { location: loc, message })
}

/// Converts a diagnostic Fix to an LSP CodeAction.
pub fn code_action(
    line_index: &LineIndex,
    text: &str,
    uri: &Url,
    diag: &IdeDiagnostic,
    fix: &ide::Fix,
) -> Option<lsp_types::CodeAction> {
    code_action_with_encoding(line_index, text, uri, diag, fix, PositionEncoding::Utf16)
}

pub fn code_action_with_encoding(
    line_index: &LineIndex,
    text: &str,
    uri: &Url,
    diag: &IdeDiagnostic,
    fix: &ide::Fix,
    encoding: PositionEncoding,
) -> Option<lsp_types::CodeAction> {
    let edits: Vec<lsp_types::TextEdit> = fix
        .edits
        .iter()
        .filter_map(|edit| {
            let edit_range = range_with_encoding(line_index, text, edit.range, encoding)?;
            Some(lsp_types::TextEdit { range: edit_range, new_text: edit.new_text.clone() })
        })
        .collect();

    if edits.is_empty() {
        return None;
    }

    let mut changes = std::collections::HashMap::new();
    changes.insert(uri.clone(), edits);

    Some(lsp_types::CodeAction {
        title: fix.label.clone(),
        kind: Some(lsp_types::CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diagnostic_with_encoding(line_index, text, diag, encoding)?]),
        edit: Some(lsp_types::WorkspaceEdit { changes: Some(changes), ..Default::default() }),
        is_preferred: Some(true),
        ..Default::default()
    })
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
        SemanticTokenType::TYPE,
        SemanticTokenType::ENUM_MEMBER,
        SemanticTokenType::NAMESPACE,
        SemanticTokenType::CLASS,
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
        HlTag::Type => 12,
        HlTag::EnumMember => 13,
        HlTag::Namespace => 14,
        HlTag::Class => 15,
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
///
/// Contract: `ide::highlight()` returns highlights sorted by `range.start()`
/// and pairwise non-overlapping. This adapter is a dumb encoder: it does not
/// make priority decisions about which highlight wins. As a transitional
/// safety-net it skips any incoming highlight that violates the sort/non-overlap
/// invariant and logs at WARN, rather than emitting an LSP-invalid token list
/// (the delta encoder would underflow on a backwards-sorted token). The skip
/// path is to be tightened to a `debug_assert!` once the IDE-layer contract has
/// soaked in production.
pub fn semantic_tokens(
    line_index: &LineIndex,
    text: &str,
    highlights: &[HlRange],
) -> Vec<SemanticToken> {
    semantic_tokens_with_encoding(line_index, text, highlights, PositionEncoding::Utf16)
}

pub fn semantic_tokens_with_encoding(
    line_index: &LineIndex,
    text: &str,
    highlights: &[HlRange],
    encoding: PositionEncoding,
) -> Vec<SemanticToken> {
    let mut tokens = Vec::with_capacity(highlights.len());
    let mut prev_line = 0;
    let mut prev_start = 0;
    // Tracks the largest range-end we have already emitted. Any incoming
    // highlight whose start sits below this value violates either the
    // sortedness contract or the non-overlap contract (or both).
    let mut prev_max_end: Option<TextSize> = None;

    for hl in highlights {
        if let Some(prev_end) = prev_max_end {
            if hl.range.start() < prev_end {
                tracing::warn!(
                    target: "bsl_analyzer::lsp::semantic_tokens",
                    range = ?hl.range,
                    tag = ?hl.tag,
                    prev_max_end = ?prev_end,
                    "ide::highlight() returned an out-of-order or overlapping HlRange — skipping (contract violation)"
                );
                continue;
            }
        }

        let start_pos = match position_for_encoding(line_index, text, hl.range.start(), encoding) {
            Some(pos) => pos,
            None => continue,
        };

        let length = token_len_for_encoding(text, hl.range, encoding);

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
        prev_max_end = Some(prev_max_end.map_or(hl.range.end(), |p| p.max(hl.range.end())));
    }

    tokens
}

fn position_for_encoding(
    line_index: &LineIndex,
    text: &str,
    offset: TextSize,
    encoding: PositionEncoding,
) -> Option<Position> {
    match encoding {
        PositionEncoding::Utf8 => position(line_index, offset),
        PositionEncoding::Utf16 => position_utf16(line_index, text, offset),
    }
}

fn token_len_for_encoding(text: &str, range: TextRange, encoding: PositionEncoding) -> u32 {
    match encoding {
        PositionEncoding::Utf8 => u32::from(range.len()),
        PositionEncoding::Utf16 => LineIndex::utf16_len(text, range),
    }
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

    /// Encoder behavior: tokens on different lines round-trip correctly via
    /// delta encoding. Pure encoder test, no dedup involved.
    #[test]
    fn test_semantic_tokens_encodes_disjoint_tokens() {
        let text = "abc\ndef\n";
        let line_index = LineIndex::new(text);

        let highlights = vec![
            HlRange {
                range: TextRange::new(0.into(), 3.into()),
                tag: HlTag::Variable,
                modifiers: HlMod::new(),
            },
            HlRange {
                range: TextRange::new(4.into(), 7.into()),
                tag: HlTag::Function,
                modifiers: HlMod::new(),
            },
        ];

        let tokens = semantic_tokens(&line_index, text, &highlights);

        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].delta_line, 0);
        assert_eq!(tokens[1].delta_line, 1);
    }

    #[test]
    fn test_semantic_tokens_encode_cyrillic_identifier_span_as_utf16() {
        let text = "    НаборЗаписей = НаборЗаписей\n";
        let line_index = LineIndex::new(text);
        let start = text.find("НаборЗаписей").unwrap() as u32;
        let end = start + "НаборЗаписей".len() as u32;

        let highlights = vec![HlRange {
            range: TextRange::new(start.into(), end.into()),
            tag: HlTag::Variable,
            modifiers: HlMod::new(),
        }];

        let tokens = semantic_tokens(&line_index, text, &highlights);

        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].delta_line, 0);
        assert_eq!(tokens[0].delta_start, 4);
        assert_eq!(tokens[0].length, "НаборЗаписей".encode_utf16().count() as u32);
    }

    #[test]
    fn test_semantic_tokens_utf8_encoding_does_not_shift_record_set_identifier() {
        let text = "\t\t\tНаборЗаписей = НаборЗаписей;\n";
        let line_index = LineIndex::new(text);
        let start = text.rfind("НаборЗаписей").unwrap() as u32;
        let end = start + "НаборЗаписей".len() as u32;

        let highlights = vec![HlRange {
            range: TextRange::new(start.into(), end.into()),
            tag: HlTag::Variable,
            modifiers: HlMod::new(),
        }];

        let utf8_tokens =
            semantic_tokens_with_encoding(&line_index, text, &highlights, PositionEncoding::Utf8);

        assert_eq!(utf8_tokens.len(), 1);
        assert_eq!(utf8_tokens[0].delta_start, start);
        assert_eq!(utf8_tokens[0].length, "НаборЗаписей".len() as u32);
        assert!(text[start as usize..].starts_with("НаборЗаписей"));

        let utf16_tokens =
            semantic_tokens_with_encoding(&line_index, text, &highlights, PositionEncoding::Utf16);
        let utf16_col_as_byte_col = utf16_tokens[0].delta_start as usize;
        let shifted_prefix = text.find("писей = НаборЗаписей").unwrap();
        assert_eq!(
            utf16_col_as_byte_col - 1,
            shifted_prefix,
            "the old UTF-16 token column lands in the observed shifted highlight"
        );
    }

    /// End-to-end check: `ide::highlight()` on the Zed-regression example must
    /// itself return overlap-free highlights (use-case-layer contract), and
    /// the encoded LSP tokens must inherit that property.
    #[test]
    fn test_semantic_tokens_end_to_end_no_overlap_on_procedure_name() {
        use ide::highlight;
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{FileId, FileSet, VfsPath};

        let code = "Процедура Тест()\n    Форма = ПолучитьФорму(\"Обработка.Тест.Форма\");\nКонецПроцедуры\n";

        let mut db = RootDatabaseImpl::default();
        let file_id = FileId(0);
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, code);

        let result = highlight(&db, file_id);

        // 1. Use-case-layer contract: highlight() output is sorted and pairwise non-overlapping.
        for window in result.highlights.windows(2) {
            assert!(
                window[0].range.start() <= window[1].range.start(),
                "ide::highlight() must return highlights sorted by start; got {:?} then {:?}",
                window[0],
                window[1]
            );
            assert!(
                window[0].range.end() <= window[1].range.start(),
                "ide::highlight() must return non-overlapping highlights; got {:?} overlapping with {:?}",
                window[0],
                window[1]
            );
        }

        // 2. Adapter passes the contract through to LSP wire format.
        let line_index = LineIndex::new(code);
        let tokens = semantic_tokens(&line_index, code, &result.highlights);

        let mut absolute = Vec::with_capacity(tokens.len());
        let (mut line, mut col) = (0u32, 0u32);
        for tok in &tokens {
            line += tok.delta_line;
            if tok.delta_line != 0 {
                col = 0;
            }
            col += tok.delta_start;
            absolute.push((line, col, col + tok.length));
        }

        for window in absolute.windows(2) {
            let (l1, _, e1) = window[0];
            let (l2, s2, _) = window[1];
            assert!(
                l1 != l2 || e1 <= s2,
                "LSP semantic tokens must not overlap; got {:?} then {:?}",
                window[0],
                window[1]
            );
        }

        // 3. Procedure name 'Тест' occupies UTF-16 columns 10..14 on line 0; exactly one token.
        let proc_name_tokens: Vec<_> = absolute
            .iter()
            .filter(|(line, start, end)| *line == 0 && *start == 10 && *end == 14)
            .collect();
        assert_eq!(
            proc_name_tokens.len(),
            1,
            "expected exactly one token covering the procedure name range, got {proc_name_tokens:?}"
        );
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
