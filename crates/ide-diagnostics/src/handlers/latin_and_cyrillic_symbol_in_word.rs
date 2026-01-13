//! LatinAndCyrillicSymbolInWord diagnostic
//!
//! Detects mixed Latin and Cyrillic characters in identifiers.
//!
//! **Source:** bsl-language-server/LatinAndCyrillicSymbolInWordDiagnostic.java
//!
//! ## Why?
//!
//! Mixed Latin/Cyrillic characters in identifiers create serious readability issues:
//! - Characters like 'e' (Latin) and 'е' (Cyrillic) look identical but are different
//! - Makes code searching and refactoring unreliable
//! - Increases cognitive load when reading code
//! - Often appears when copying code from different sources
//!
//! ## What gets checked?
//!
//! The diagnostic analyzes 8 types of identifiers:
//! 1. Function and procedure names
//! 2. Variable declarations
//! 3. Parameters
//! 4. Annotation names
//! 5. Annotation parameter names
//! 6. Region names
//! 7. Goto labels
//! 8. Assignment left-hand side
//!
//! ## Configuration
//!
//! ### `excludeWords` (String)
//! Comma-separated list of words to exclude from checking.
//! Default: `"ЧтениеXML, ЧтениеJSON, ЗаписьXML, ЗаписьJSON, ComОбъект, ФабрикаXDTO, ОбъектXDTO, СоединениеFTP, HTTPСоединение, HTTPЗапрос, HTTPСервисОтвет, SMSСообщение, WSПрокси"`
//!
//! ### `allowTrailingPartsInAnotherLanguage` (Boolean)
//! When `true` (default), allows identifiers that start with one language and end with another,
//! like `HTTPСоединение` or `ВИмениEnglish` (minimum 2 characters per language, total length ≥ 4).
//! When `false`, all mixed-script identifiers are flagged.

use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use ide_db::TextRange;
use regex::{Regex, RegexBuilder};
use syntax::SyntaxKind;

const DEFAULT_EXCLUDE_WORDS: &str = "ЧтениеXML, ЧтениеJSON, ЗаписьXML, ЗаписьJSON, ComОбъект, \
    ФабрикаXDTO, ОбъектXDTO, СоединениеFTP, HTTPСоединение, HTTPЗапрос, HTTPСервисОтвет, \
    SMSСообщение, WSПрокси";

const DEFAULT_ALLOW_TRAILING_PARTS: bool = true;

/// Configuration for the diagnostic
#[derive(Debug, Clone)]
struct Config {
    exclude_words: String,
    allow_trailing_parts: bool,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let exclude_words = ctx
            .config
            .get_string(DiagnosticCode::LatinAndCyrillicSymbolInWord, "excludeWords")
            .map(|s| s.to_string())
            .unwrap_or_else(|| DEFAULT_EXCLUDE_WORDS.to_string());

        let allow_trailing_parts = ctx
            .config
            .get_bool(
                DiagnosticCode::LatinAndCyrillicSymbolInWord,
                "allowTrailingPartsInAnotherLanguage",
            )
            .unwrap_or(DEFAULT_ALLOW_TRAILING_PARTS);

        Self { exclude_words, allow_trailing_parts }
    }
}

/// Information about an identifier found in the code
#[derive(Debug, Clone)]
struct IdentifierInfo {
    text: String,
    range: TextRange,
}

/// Creates a case-insensitive regex pattern for exclusion list
/// Matches Java: Pattern.quote() + case-insensitive matching
fn create_exclude_pattern(words: &str) -> Regex {
    let parts: Vec<String> = words.split(',').map(|s| regex::escape(s.trim())).collect();

    let pattern = format!("^({})", parts.join("|"));

    RegexBuilder::new(&pattern)
        .case_insensitive(true)
        .build()
        .expect("Failed to compile exclusion pattern")
}

/// Checks if text contains mixed Latin and Cyrillic scripts
fn has_mixed_scripts(text: &str, cyrillic: &Regex, latin: &Regex) -> bool {
    cyrillic.is_match(text) && latin.is_match(text)
}

/// Determines if identifier should be reported based on configuration
fn should_report(id: &IdentifierInfo, config: &Config, trailing: &Regex) -> bool {
    if !config.allow_trailing_parts {
        return true; // Strict mode: report all mixed scripts
    }

    // Check trailing pattern exception (requires length ≥ 4)
    if id.text.len() >= 4 && trailing.is_match(&id.text) {
        return false; // Matches allowed pattern, don't report
    }

    true // Report
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
    // Split by whitespace and take first word, or split by '//' and take first part
    let identifier = if let Some(comment_pos) = trimmed.find("//") {
        trimmed[..comment_pos].trim()
    } else {
        // No comment, but might have trailing whitespace
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

/// Collects annotation parameter names from annotation node
fn collect_annotation_params(annotation_node: &syntax::SyntaxNode) -> Vec<IdentifierInfo> {
    let mut params = Vec::new();

    for child in annotation_node.descendants() {
        if child.kind() == SyntaxKind::ANNOTATION_PARAM {
            // Extract parameter name (first IDENT before '=')
            for element in child.children_with_tokens() {
                if let Some(token) = element.as_token() {
                    if token.kind() == SyntaxKind::IDENT {
                        params.push(IdentifierInfo {
                            text: token.text().to_string(),
                            range: token.text_range(),
                        });
                        break; // Only first IDENT is the param name
                    }
                }
            }
        }
    }

    params
}

/// Collects all identifiers from the syntax tree
fn collect_identifiers(ctx: &DiagnosticsContext) -> Vec<IdentifierInfo> {
    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut identifiers = Vec::new();

    for node in root.descendants() {
        match node.kind() {
            // 1. Function names
            SyntaxKind::FUNCTION_DEF => {
                for element in node.children_with_tokens() {
                    if let Some(token) = element.as_token() {
                        if token.kind() == SyntaxKind::IDENT {
                            identifiers.push(IdentifierInfo {
                                text: token.text().to_string(),
                                range: token.text_range(),
                            });
                            break;
                        }
                    }
                }
            }

            // 2. Procedure names
            SyntaxKind::PROCEDURE_DEF => {
                for element in node.children_with_tokens() {
                    if let Some(token) = element.as_token() {
                        if token.kind() == SyntaxKind::IDENT {
                            identifiers.push(IdentifierInfo {
                                text: token.text().to_string(),
                                range: token.text_range(),
                            });
                            break;
                        }
                    }
                }
            }

            // 3. Variable declarations (can have multiple names)
            SyntaxKind::VAR_DEF => {
                for element in node.descendants_with_tokens() {
                    if let Some(token) = element.as_token() {
                        if token.kind() == SyntaxKind::IDENT {
                            identifiers.push(IdentifierInfo {
                                text: token.text().to_string(),
                                range: token.text_range(),
                            });
                        }
                    }
                }
            }

            // 4. Parameters
            SyntaxKind::PARAM => {
                for element in node.children_with_tokens() {
                    if let Some(token) = element.as_token() {
                        if token.kind() == SyntaxKind::IDENT {
                            identifiers.push(IdentifierInfo {
                                text: token.text().to_string(),
                                range: token.text_range(),
                            });
                            break;
                        }
                    }
                }
            }

            // 5. Annotations (both name and params)
            SyntaxKind::ANNOTATION | SyntaxKind::COMPILER_DIRECTIVE => {
                // Check for ANN_CUSTOM tokens (custom annotations like &Аnotation)
                for element in node.children_with_tokens() {
                    if let Some(token) = element.as_token() {
                        if token.kind() == SyntaxKind::ANN_CUSTOM {
                            // ANN_CUSTOM token includes '&' prefix, extract the identifier
                            let text = token.text();
                            if let Some(name) = text.strip_prefix('&') {
                                identifiers.push(IdentifierInfo {
                                    text: name.to_string(),
                                    // Adjust range to exclude '&' (1 byte)
                                    range: TextRange::new(
                                        (u32::from(token.text_range().start()) + 1).into(),
                                        token.text_range().end(),
                                    ),
                                });
                            }
                        } else if token.kind() == SyntaxKind::IDENT {
                            // Also handle IDENT tokens in annotations
                            identifiers.push(IdentifierInfo {
                                text: token.text().to_string(),
                                range: token.text_range(),
                            });
                        }
                    }
                }

                // Annotation parameters
                identifiers.extend(collect_annotation_params(&node));
            }

            // 6. Region names
            SyntaxKind::PRE_REGION_DIR => {
                if let Some(info) = extract_region_name(&node) {
                    identifiers.push(info);
                }
            }

            // 7. Goto labels
            SyntaxKind::GOTO_STMT => {
                if let Some(info) = extract_goto_label(&node) {
                    identifiers.push(info);
                }
            }

            // 8. Assignment left-hand side
            SyntaxKind::ASSIGN_STMT => {
                if let Some(info) = extract_assign_lvalue(&node) {
                    identifiers.push(info);
                }
            }

            _ => {}
        }
    }

    identifiers
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::LatinAndCyrillicSymbolInWord) {
        return Vec::new();
    }

    // 1. Parse configuration
    let config = Config::from_context(ctx);

    // 2. Compile patterns once
    let exclude_pattern = create_exclude_pattern(&config.exclude_words);

    let cyrillic = RegexBuilder::new(r"[а-яё]")
        .case_insensitive(true)
        .build()
        .expect("Failed to compile Cyrillic pattern");

    let latin = RegexBuilder::new(r"[a-z]")
        .case_insensitive(true)
        .build()
        .expect("Failed to compile Latin pattern");

    // Trailing pattern (case-sensitive, requires uppercase start)
    let trailing = Regex::new(
        r"^([A-Z][A-Za-z]+[A-Za-z1-9_]*[А-ЯЁ][А-Яа-яЁё]+[А-Яа-я1-9Ёё_]*|[А-ЯЁ][А-Яа-яЁё]+[А-Яа-я1-9Ёё_]*[A-Z][A-Za-z]+[A-Za-z1-9_]*)$",
    )
    .expect("Failed to compile trailing pattern");

    // 3. Collect all identifiers
    let identifiers = collect_identifiers(ctx);

    // 4. Filter and create diagnostics
    identifiers
        .into_iter()
        .filter(|id| id.text.len() >= 2) // Minimum length
        .filter(|id| !exclude_pattern.is_match(&id.text)) // Not in exclusion list
        .filter(|id| has_mixed_scripts(&id.text, &cyrillic, &latin)) // Mixed scripts
        .filter(|id| should_report(id, &config, &trailing)) // Check trailing pattern
        .map(|id| Diagnostic {
            code: DiagnosticCode::LatinAndCyrillicSymbolInWord,
            message: format!(
                "Identifier '{}' contains mixed Latin and Cyrillic characters",
                id.text
            ),
            severity: Severity::Warning,
            range: id.range,
            tags: vec![],
            fixes: vec![],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{assert_diagnostic_range, check_ast_diagnostic};

    #[test]
    fn test_comprehensive() {
        let code = include_str!("../../test_data/LatinAndCyrillicSymbolInWordDiagnostic.bsl");
        let diagnostics = check_ast_diagnostic(code, check);

        // Java expects 15 diagnostics
        assert_eq!(diagnostics.len(), 15, "Expected 15 diagnostics");

        // Verify all positions match Java test
        // Java test order (from LatinAndCyrillicSymbolInWordDiagnosticTest.java):
        // methods, variables, annotations, other

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
        assert_diagnostic_range(code, &diagnostics[12], 35, 10, 21); // ПеременнаяA (21 not 20!)
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
        // Test that words in default exclusion list are not flagged
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
        // Test that identifiers < 2 chars are not checked
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
        // Test that identifiers matching trailing pattern are allowed by default
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
