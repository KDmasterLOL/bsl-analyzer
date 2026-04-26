//! LatinAndCyrillicSymbolInWord diagnostic
//!
//! Detects identifiers that mix Latin and Cyrillic characters in the same word.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use syntax::SyntaxKind;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload, MetadataTag::Suspicious],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Consistent,
};

const DEFAULT_EXCLUDE_WORDS: &str = "ЧтениеXML, ЧтениеJSON, ЗаписьXML, ЗаписьJSON, ComОбъект, \
    ФабрикаXDTO, ОбъектXDTO, СоединениеFTP, HTTPСоединение, HTTPЗапрос, HTTPСервисОтвет, \
    SMSСообщение, WSПрокси";

const DEFAULT_ALLOW_TRAILING_PARTS: bool = true;

/// Configuration for the diagnostic
#[derive(Debug, Clone)]
struct Config {
    exclude_words: Vec<String>,
    allow_trailing_parts: bool,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let exclude_str = ctx.config_string(
            DiagnosticCode::LatinAndCyrillicSymbolInWord,
            "excludeWords",
            DEFAULT_EXCLUDE_WORDS,
        );

        let exclude_words: Vec<String> = exclude_str
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        let allow_trailing_parts = ctx.config_bool(
            DiagnosticCode::LatinAndCyrillicSymbolInWord,
            "allowTrailingPartsInAnotherLanguage",
            DEFAULT_ALLOW_TRAILING_PARTS,
        );

        Self { exclude_words, allow_trailing_parts }
    }
}

/// Information about an identifier found in the code
#[derive(Debug, Clone)]
struct IdentifierInfo {
    text: String,
    range: TextRange,
}

/// Check if character is Cyrillic (Russian alphabet)
#[inline]
fn is_cyrillic(c: char) -> bool {
    matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')
}

/// Check if character is Latin
#[inline]
fn is_latin(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// Check if character is uppercase Cyrillic
#[inline]
fn is_cyrillic_upper(c: char) -> bool {
    matches!(c, 'А'..='Я' | 'Ё')
}

/// Check if character is uppercase Latin
#[inline]
fn is_latin_upper(c: char) -> bool {
    c.is_ascii_uppercase()
}

/// Quick byte-level check to detect pure scripts without char iteration.
/// Returns Some(false) if definitely not mixed (pure Cyrillic or pure Latin).
/// Returns None if we need a full char-level check.
///
/// Optimized for BSL where most identifiers are pure Cyrillic.
#[inline]
fn quick_mixed_check(text: &str) -> Option<bool> {
    let bytes = text.as_bytes();

    // Fast path 1: Pure Cyrillic (most common in BSL)
    // Cyrillic letters are multi-byte UTF-8 (bytes >= 0x80)
    // If no ASCII letters present, can't be mixed
    let has_ascii_letter = bytes.iter().any(|&b| b.is_ascii_alphabetic());
    if !has_ascii_letter {
        return Some(false); // Pure Cyrillic (or no letters at all)
    }

    // Fast path 2: Pure Latin
    // If no high bytes (>= 0x80), no multi-byte chars, so no Cyrillic
    let has_high_byte = bytes.iter().any(|&b| b >= 0x80);
    if !has_high_byte {
        return Some(false); // Pure ASCII/Latin
    }

    // Has both ASCII letters and high bytes - need full check
    None
}

/// Checks if text contains both Latin and Cyrillic characters
fn has_mixed_scripts(text: &str) -> bool {
    // Try quick byte-level check first
    if let Some(result) = quick_mixed_check(text) {
        return result;
    }

    // Full char-level check for ambiguous cases
    let mut has_cyrillic = false;
    let mut has_latin = false;

    for c in text.chars() {
        if is_cyrillic(c) {
            has_cyrillic = true;
        } else if is_latin(c) {
            has_latin = true;
        }
        if has_cyrillic && has_latin {
            return true;
        }
    }

    false
}

/// Check if identifier is in exclusion list (case-insensitive)
fn is_excluded(text: &str, exclude_words: &[String]) -> bool {
    let text_lower = text.to_lowercase();
    exclude_words.contains(&text_lower)
}

/// Check if identifier matches trailing pattern (one language at start, another at end).
/// Pattern: starts with >=2 chars of one script (uppercase start), ends with >=2 chars of another.
/// Examples: HTTPСоединение, ВИмениEnglish
fn matches_trailing_pattern(text: &str) -> bool {
    if text.len() < 4 {
        return false;
    }

    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 4 {
        return false;
    }

    let first = chars[0];

    // Pattern 1: Latin start -> Cyrillic end (e.g., HTTPСоединение)
    if is_latin_upper(first) {
        // Find where Latin ends and Cyrillic begins
        let mut latin_end = 0;
        for (i, &c) in chars.iter().enumerate() {
            if is_latin(c) || c.is_ascii_digit() || c == '_' {
                latin_end = i + 1;
            } else {
                break;
            }
        }

        // Need at least 2 Latin chars at start
        if latin_end < 2 {
            return false;
        }

        // Check remaining is Cyrillic (at least 2 chars)
        let remaining = &chars[latin_end..];
        if remaining.len() < 2 {
            return false;
        }

        // First char of Cyrillic part must be uppercase
        if !is_cyrillic_upper(remaining[0]) {
            return false;
        }

        // All remaining must be Cyrillic or digits/underscore
        return remaining.iter().all(|&c| is_cyrillic(c) || c.is_ascii_digit() || c == '_');
    }

    // Pattern 2: Cyrillic start -> Latin end (e.g., ВИмениEnglish)
    if is_cyrillic_upper(first) {
        // Find where Cyrillic ends and Latin begins
        let mut cyrillic_end = 0;
        for (i, &c) in chars.iter().enumerate() {
            if is_cyrillic(c) || c.is_ascii_digit() || c == '_' {
                cyrillic_end = i + 1;
            } else {
                break;
            }
        }

        // Need at least 2 Cyrillic chars at start
        if cyrillic_end < 2 {
            return false;
        }

        // Check remaining is Latin (at least 2 chars)
        let remaining = &chars[cyrillic_end..];
        if remaining.len() < 2 {
            return false;
        }

        // First char of Latin part must be uppercase
        if !is_latin_upper(remaining[0]) {
            return false;
        }

        // All remaining must be Latin or digits/underscore
        return remaining.iter().all(|&c| is_latin(c) || c.is_ascii_digit() || c == '_');
    }

    false
}

/// Determines if identifier should be reported based on configuration
fn should_report(id: &IdentifierInfo, config: &Config) -> bool {
    if !config.allow_trailing_parts {
        return true; // Strict mode: report all mixed scripts
    }

    // Check trailing pattern exception
    !matches_trailing_pattern(&id.text)
}

/// Extracts region name from PRE_REGION_DIR node
fn extract_region_name(node: &syntax::SyntaxNode) -> Option<IdentifierInfo> {
    let text = node.text().to_string();
    let first_line = text.lines().next()?;

    // Try all variants (case-insensitive prefix matching)
    let (name, prefix_len) = if let Some(n) = first_line.strip_prefix("#Область") {
        (n, "#Область".len())
    } else if let Some(n) = first_line.strip_prefix("#область") {
        (n, "#область".len())
    } else if let Some(n) = first_line.strip_prefix("#Region") {
        (n, "#Region".len())
    } else if let Some(n) = first_line.strip_prefix("#region") {
        (n, "#region".len())
    } else {
        return None;
    };

    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Extract only the identifier (before any comment '//')
    let identifier = if let Some(comment_pos) = trimmed.find("//") {
        trimmed[..comment_pos].trim()
    } else {
        trimmed.split_whitespace().next().unwrap_or(trimmed)
    };

    if identifier.is_empty() {
        return None;
    }

    // Calculate exact range for the identifier
    let name_start = prefix_len + (name.len() - trimmed.len());
    let byte_start: u32 = node.text_range().start().into();
    let byte_start = byte_start + name_start as u32;

    Some(IdentifierInfo {
        text: identifier.to_string(),
        range: TextRange::new(byte_start.into(), (byte_start + identifier.len() as u32).into()),
    })
}

/// Extracts label from GOTO_STMT node (IDENT after TILDE)
fn extract_goto_label(node: &syntax::SyntaxNode) -> Option<IdentifierInfo> {
    let mut found_tilde = false;

    for element in node.children_with_tokens() {
        if let Some(token) = element.as_token() {
            if token.kind() == SyntaxKind::TILDE {
                found_tilde = true;
            } else if found_tilde && token.kind() == SyntaxKind::IDENT {
                return Some(IdentifierInfo {
                    text: token.text().to_string(),
                    range: token.text_range(),
                });
            }
        }
    }

    None
}

/// Extracts lvalue from ASSIGN_STMT node (first IDENT token)
fn extract_assign_lvalue(node: &syntax::SyntaxNode) -> Option<IdentifierInfo> {
    for element in node.descendants_with_tokens() {
        if let Some(token) = element.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                return Some(IdentifierInfo {
                    text: token.text().to_string(),
                    range: token.text_range(),
                });
            }
        }
    }

    None
}
/// Check if token text is a candidate for mixed scripts (quick pre-filter).
/// Only allocates String if this returns true.
#[inline]
fn is_mixed_candidate(text: &str) -> bool {
    text.len() >= 2 && has_mixed_scripts(text)
}

/// Process a single IDENT token: check if mixed and add to results
#[inline]
fn process_ident_token(
    token: &syntax::SyntaxToken,
    config: &Config,
    diagnostics: &mut Vec<Diagnostic>,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) {
    let text = token.text();
    if !is_mixed_candidate(text) {
        return;
    }
    // Only allocate String after passing quick check
    let text_owned = text.to_string();
    if is_excluded(&text_owned, &config.exclude_words) {
        return;
    }
    let id = IdentifierInfo { text: text_owned, range: token.text_range() };
    if should_report(&id, config) {
        diagnostics.push(Diagnostic {
            code,
            message: format!(
                "Identifier '{}' contains mixed Latin and Cyrillic characters",
                id.text
            ),
            severity: ctx.severity(code),
            range: id.range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }
}

/// Collects and filters identifiers in a single pass (avoids intermediate allocations)
fn collect_and_check(
    ctx: &DiagnosticsContext,
    config: &Config,
    code: DiagnosticCode,
) -> Vec<Diagnostic> {
    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    for node in root.descendants() {
        match node.kind() {
            // 1. Function names
            SyntaxKind::FUNCTION_DEF | SyntaxKind::PROCEDURE_DEF => {
                for element in node.children_with_tokens() {
                    if let Some(token) = element.as_token() {
                        if token.kind() == SyntaxKind::IDENT {
                            process_ident_token(token, config, &mut diagnostics, code, ctx);
                            break;
                        }
                    }
                }
            }

            // 3. Variable declarations
            SyntaxKind::VAR_DEF => {
                for element in node.descendants_with_tokens() {
                    if let Some(token) = element.as_token() {
                        if token.kind() == SyntaxKind::IDENT {
                            process_ident_token(token, config, &mut diagnostics, code, ctx);
                        }
                    }
                }
            }

            // 4. Parameters
            SyntaxKind::PARAM => {
                for element in node.children_with_tokens() {
                    if let Some(token) = element.as_token() {
                        if token.kind() == SyntaxKind::IDENT {
                            process_ident_token(token, config, &mut diagnostics, code, ctx);
                            break;
                        }
                    }
                }
            }

            // 5. Annotations (both name and params)
            SyntaxKind::ANNOTATION | SyntaxKind::COMPILER_DIRECTIVE => {
                for element in node.children_with_tokens() {
                    if let Some(token) = element.as_token() {
                        if token.kind() == SyntaxKind::ANN_CUSTOM {
                            let text = token.text();
                            if let Some(name) = text.strip_prefix('&') {
                                if is_mixed_candidate(name) {
                                    let text_owned = name.to_string();
                                    if !is_excluded(&text_owned, &config.exclude_words) {
                                        let range = TextRange::new(
                                            (u32::from(token.text_range().start()) + 1).into(),
                                            token.text_range().end(),
                                        );
                                        let id = IdentifierInfo { text: text_owned, range };
                                        if should_report(&id, config) {
                                            diagnostics.push(Diagnostic {
                                                code,
                                                message: format!(
                                                    "Identifier '{}' contains mixed Latin and Cyrillic characters",
                                                    id.text
                                                ),
                                                severity: ctx.severity(code),
                                                range: id.range,
                                                tags: ctx.tags(code),
                                                fixes: vec![],
                                            });
                                        }
                                    }
                                }
                            }
                        } else if token.kind() == SyntaxKind::IDENT {
                            process_ident_token(token, config, &mut diagnostics, code, ctx);
                        }
                    }
                }
                // Annotation parameters
                for child in node.descendants() {
                    if child.kind() == SyntaxKind::ANNOTATION_PARAM {
                        for element in child.children_with_tokens() {
                            if let Some(token) = element.as_token() {
                                if token.kind() == SyntaxKind::IDENT {
                                    process_ident_token(token, config, &mut diagnostics, code, ctx);
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            // 6. Region names
            SyntaxKind::PRE_REGION_DIR => {
                if let Some(id) = extract_region_name(&node) {
                    if id.text.len() >= 2
                        && has_mixed_scripts(&id.text)
                        && !is_excluded(&id.text, &config.exclude_words)
                        && should_report(&id, config)
                    {
                        diagnostics.push(Diagnostic {
                            code,
                            message: format!(
                                "Identifier '{}' contains mixed Latin and Cyrillic characters",
                                id.text
                            ),
                            severity: ctx.severity(code),
                            range: id.range,
                            tags: ctx.tags(code),
                            fixes: vec![],
                        });
                    }
                }
            }

            // 7. Goto labels
            SyntaxKind::GOTO_STMT => {
                if let Some(id) = extract_goto_label(&node) {
                    if id.text.len() >= 2
                        && has_mixed_scripts(&id.text)
                        && !is_excluded(&id.text, &config.exclude_words)
                        && should_report(&id, config)
                    {
                        diagnostics.push(Diagnostic {
                            code,
                            message: format!(
                                "Identifier '{}' contains mixed Latin and Cyrillic characters",
                                id.text
                            ),
                            severity: ctx.severity(code),
                            range: id.range,
                            tags: ctx.tags(code),
                            fixes: vec![],
                        });
                    }
                }
            }

            // 8. Assignment left-hand side
            SyntaxKind::ASSIGN_STMT => {
                if let Some(id) = extract_assign_lvalue(&node) {
                    if id.text.len() >= 2
                        && has_mixed_scripts(&id.text)
                        && !is_excluded(&id.text, &config.exclude_words)
                        && should_report(&id, config)
                    {
                        diagnostics.push(Diagnostic {
                            code,
                            message: format!(
                                "Identifier '{}' contains mixed Latin and Cyrillic characters",
                                id.text
                            ),
                            severity: ctx.severity(code),
                            range: id.range,
                            tags: ctx.tags(code),
                            fixes: vec![],
                        });
                    }
                }
            }

            _ => {}
        }
    }

    diagnostics
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::LatinAndCyrillicSymbolInWord;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);

    // Single-pass collection and filtering to minimize allocations
    collect_and_check(ctx, &config, code)
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};
    #[test]
    fn test_comprehensive() {
        // Inline fixture: covers all 8 identifier categories with known mixed-script identifiers.
        // Line numbers (0-based) must match position assertions below.
        // Uses concat! to preserve exact per-line indentation without Rust's \n\ stripping.
        let code = concat!(
            "Перем Namе;                 // <- ошибка\n",
            "\n",
            "Процедура ВИмениEnglish()   // <- Не ошибка (начинается на кириллице, заканчивается на латинице)\n",
            "    Перем а;\n",
            "    Перем ии, вв;\n",
            "    перем ССС, ccc, сcс;    // <- ошибка в последнем имени\n",
            "    Переменная = 1;\n",
            "КонецПроцедуры\n",
            "\n",
            "Функция InNameРусский()     // <- Не ошибка (начинается на латинице, заканчивается на кириллице)\n",
            "    Перем тьфу;\n",
            "    Перем name;\n",
            "    перем Аg;               // <- ошибка\n",
            "    перем _Аg09;            // <- ошибка\n",
            "    Переменная = \"engру\";   // <- ошибки нет\n",
            "    ComОбъект2 = _Аg09;     // <- Не ошибка (начинается на латинице, заканчивается на кириллице)\n",
            "    _3C_omRRRО_5__бъект = 1;// <- ошибка\n",
            "КонецФункции\n",
            "\n",
            "&Аnotation                  // <- ошибка\n",
            "Функция __t1est()           // <- ошибки нет\n",
            "КонецФункции\n",
            "\n",
            "&Аннотация(Парaметр = 1)    // <- ошибка в параметре\n",
            "Функция _тест12()           // <- ошибки нет\n",
            "КонецФункции\n",
            "\n",
            "#Область Regiоn             // <- ошибка\n",
            "#КонецОбласти\n",
            "\n",
            "Процедура Тест12(ПараметрY, Знач ParamЫ) // <- ошибка в именах параметров\n",
            "    Перейти ~Lаbell;        // <- ошибка\n",
            "КонецПроцедуры\n",
            "\n",
            "Процедура Tутошибка()       // <- ошибка в имени метода\n",
            "    Перем ПеременнаяA;      // <- ошибка, т.к. должно быть минимум 2 в конце\n",
            "    Перем ПеременнаяAМ;     // <- ошибка\n",
            "    Перем XПириенс;         // <- ошибка, т.к. должно быть минимум 2 в начале\n",
            "КонецПроцедуры",
        );
        let diagnostics = check_ast_diagnostic(code, check);

        // Expected 15 diagnostics
        assert_eq!(diagnostics.len(), 15, "Expected 15 diagnostics");

        // Verify all positions
        // Methods
        assert_diagnostic_range(code, &diagnostics[8], 30, 17, 26); // ПараметрY
        assert_diagnostic_range(code, &diagnostics[9], 30, 33, 39); // ParamЫ
        assert_diagnostic_range(code, &diagnostics[11], 34, 10, 19); // Tутошибка

        // Variables
        assert_diagnostic_range(code, &diagnostics[0], 0, 6, 10); // Namе
        assert_diagnostic_range(code, &diagnostics[1], 5, 20, 23); // сcс
        assert_diagnostic_range(code, &diagnostics[2], 12, 10, 12); // Аg
        assert_diagnostic_range(code, &diagnostics[3], 13, 10, 15); // _Аg09
        assert_diagnostic_range(code, &diagnostics[4], 16, 4, 23); // _3C_omRRRО_5__бъект
        assert_diagnostic_range(code, &diagnostics[12], 35, 10, 21); // ПеременнаяA
        assert_diagnostic_range(code, &diagnostics[13], 36, 10, 22); // ПеременнаяAМ
        assert_diagnostic_range(code, &diagnostics[14], 37, 10, 18); // XПириенс

        // Annotations
        assert_diagnostic_range(code, &diagnostics[5], 19, 1, 10); // Аnotation
        assert_diagnostic_range(code, &diagnostics[6], 23, 11, 19); // Парaметр

        // Other (region, goto)
        assert_diagnostic_range(code, &diagnostics[7], 27, 9, 15); // Regiоn
        assert_diagnostic_range(code, &diagnostics[10], 31, 13, 19); // Lаbell
    }

    #[test]
    fn test_excluded_words_not_reported() {
        let code = r#"
ComОбъект = 1;
HTTPСоединение = 2;
ЧтениеXML = 3;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Default exclusion list should work");
    }

    #[test]
    fn test_short_identifiers_skipped() {
        let code = r#"
Перем А;
Перем a;
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Single-character identifiers should be skipped");
    }

    #[test]
    fn test_pure_cyrillic_not_flagged() {
        let code = r#"
Перем ПеременнаяРусская;
Процедура ПроцедураРусская()
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Pure Cyrillic should not be flagged");
    }

    #[test]
    fn test_pure_latin_not_flagged() {
        let code = r#"
Перем VariableEnglish;
Процедура ProcedureEnglish()
КонецПроцедуры
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Pure Latin should not be flagged");
    }

    #[test]
    fn test_trailing_pattern_allowed() {
        let code = r#"
Процедура ВИмениEnglish()
КонецПроцедуры

Функция InNameРусский()
КонецФункции
"#;
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0, "Trailing pattern should be allowed by default");
    }
}
