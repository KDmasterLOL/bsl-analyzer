use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use stdx::case::CaseExt;
use syntax::ast::{AstNode, PreRegionDir};
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
            .map(|s| s.trim().fold_lower())
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

#[derive(Debug, Clone)]
struct IdentifierInfo {
    text: String,
    range: TextRange,
}

#[inline]
fn is_cyrillic(c: char) -> bool {
    matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')
}

#[inline]
fn is_latin(c: char) -> bool {
    c.is_ascii_alphabetic()
}

#[inline]
fn is_cyrillic_upper(c: char) -> bool {
    matches!(c, 'А'..='Я' | 'Ё')
}

#[inline]
fn is_latin_upper(c: char) -> bool {
    c.is_ascii_uppercase()
}

#[inline]
fn quick_mixed_check(text: &str) -> Option<bool> {
    let bytes = text.as_bytes();

    let has_ascii_letter = bytes.iter().any(|&b| b.is_ascii_alphabetic());
    if !has_ascii_letter {
        return Some(false);
    }

    let has_high_byte = bytes.iter().any(|&b| b >= 0x80);
    if !has_high_byte {
        return Some(false);
    }

    None
}

fn has_mixed_scripts(text: &str) -> bool {
    if let Some(result) = quick_mixed_check(text) {
        return result;
    }

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

fn is_excluded(text: &str, exclude_words: &[String]) -> bool {
    let text_lower = text.fold_lower();
    exclude_words.contains(&text_lower)
}

fn matches_trailing_pattern(text: &str) -> bool {
    if text.len() < 4 {
        return false;
    }

    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 4 {
        return false;
    }

    let first = chars[0];

    if is_latin_upper(first) {
        let mut latin_end = 0;
        for (i, &c) in chars.iter().enumerate() {
            if is_latin(c) || c.is_ascii_digit() || c == '_' {
                latin_end = i + 1;
            } else {
                break;
            }
        }

        if latin_end < 2 {
            return false;
        }

        let remaining = &chars[latin_end..];
        if remaining.len() < 2 {
            return false;
        }

        if !is_cyrillic_upper(remaining[0]) {
            return false;
        }

        return remaining.iter().all(|&c| is_cyrillic(c) || c.is_ascii_digit() || c == '_');
    }

    if is_cyrillic_upper(first) {
        let mut cyrillic_end = 0;
        for (i, &c) in chars.iter().enumerate() {
            if is_cyrillic(c) || c.is_ascii_digit() || c == '_' {
                cyrillic_end = i + 1;
            } else {
                break;
            }
        }

        if cyrillic_end < 2 {
            return false;
        }

        let remaining = &chars[cyrillic_end..];
        if remaining.len() < 2 {
            return false;
        }

        if !is_latin_upper(remaining[0]) {
            return false;
        }

        return remaining.iter().all(|&c| is_latin(c) || c.is_ascii_digit() || c == '_');
    }

    false
}

fn should_report(id: &IdentifierInfo, config: &Config) -> bool {
    if !config.allow_trailing_parts {
        return true;
    }

    !matches_trailing_pattern(&id.text)
}

fn extract_region_name(node: &syntax::SyntaxNode) -> Option<IdentifierInfo> {
    let dir = PreRegionDir::cast(node.clone())?;
    if !dir.is_start() {
        return None;
    }

    node.children_with_tokens()
        .filter_map(|element| element.into_token())
        .find(|token| token.kind() == SyntaxKind::IDENT)
        .map(|token| IdentifierInfo { text: token.text().to_string(), range: token.text_range() })
}

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
#[inline]
fn is_mixed_candidate(text: &str) -> bool {
    text.len() >= 2 && has_mixed_scripts(text)
}

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

            SyntaxKind::VAR_DEF => {
                for element in node.descendants_with_tokens() {
                    if let Some(token) = element.as_token() {
                        if token.kind() == SyntaxKind::IDENT {
                            process_ident_token(token, config, &mut diagnostics, code, ctx);
                        }
                    }
                }
            }

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

    collect_and_check(ctx, &config, code)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_comprehensive() {
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
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LatinAndCyrillicSymbolInWord,
            expect![[r#"
                LatinAndCyrillicSymbolInWord @ 1:7..1:11
                  message: Identifier 'Namе' contains mixed Latin and Cyrillic characters
                  severity: Information
                LatinAndCyrillicSymbolInWord @ 6:21..6:24
                  message: Identifier 'сcс' contains mixed Latin and Cyrillic characters
                  severity: Information
                LatinAndCyrillicSymbolInWord @ 13:11..13:13
                  message: Identifier 'Аg' contains mixed Latin and Cyrillic characters
                  severity: Information
                LatinAndCyrillicSymbolInWord @ 14:11..14:16
                  message: Identifier '_Аg09' contains mixed Latin and Cyrillic characters
                  severity: Information
                LatinAndCyrillicSymbolInWord @ 17:5..17:24
                  message: Identifier '_3C_omRRRО_5__бъект' contains mixed Latin and Cyrillic characters
                  severity: Information
                LatinAndCyrillicSymbolInWord @ 20:2..20:11
                  message: Identifier 'Аnotation' contains mixed Latin and Cyrillic characters
                  severity: Information
                LatinAndCyrillicSymbolInWord @ 24:12..24:20
                  message: Identifier 'Парaметр' contains mixed Latin and Cyrillic characters
                  severity: Information
                LatinAndCyrillicSymbolInWord @ 28:10..28:16
                  message: Identifier 'Regiоn' contains mixed Latin and Cyrillic characters
                  severity: Information
                LatinAndCyrillicSymbolInWord @ 31:18..31:27
                  message: Identifier 'ПараметрY' contains mixed Latin and Cyrillic characters
                  severity: Information
                LatinAndCyrillicSymbolInWord @ 31:34..31:40
                  message: Identifier 'ParamЫ' contains mixed Latin and Cyrillic characters
                  severity: Information
                LatinAndCyrillicSymbolInWord @ 32:14..32:20
                  message: Identifier 'Lаbell' contains mixed Latin and Cyrillic characters
                  severity: Information
                LatinAndCyrillicSymbolInWord @ 35:11..35:20
                  message: Identifier 'Tутошибка' contains mixed Latin and Cyrillic characters
                  severity: Information
                LatinAndCyrillicSymbolInWord @ 36:11..36:22
                  message: Identifier 'ПеременнаяA' contains mixed Latin and Cyrillic characters
                  severity: Information
                LatinAndCyrillicSymbolInWord @ 37:11..37:23
                  message: Identifier 'ПеременнаяAМ' contains mixed Latin and Cyrillic characters
                  severity: Information
                LatinAndCyrillicSymbolInWord @ 38:11..38:19
                  message: Identifier 'XПириенс' contains mixed Latin and Cyrillic characters
                  severity: Information"#]],
        );
    }

    /// Region directives are case-insensitive and admit a blank after `#`, so the
    /// name must be found under every spelling the lexer accepts.
    #[test]
    fn region_name_checked_under_off_canon_directive() {
        check_diagnostics_snapshot_for(
            "#ОБЛАСТЬ Regiоn\n#КОНЕЦОБЛАСТИ\n",
            DiagnosticCode::LatinAndCyrillicSymbolInWord,
            expect![[r#"
                LatinAndCyrillicSymbolInWord @ 1:10..1:16
                  message: Identifier 'Regiоn' contains mixed Latin and Cyrillic characters
                  severity: Information"#]],
        );

        check_diagnostics_snapshot_for(
            "# Область Regiоn\n# КонецОбласти\n",
            DiagnosticCode::LatinAndCyrillicSymbolInWord,
            expect![[r#"
                LatinAndCyrillicSymbolInWord @ 1:11..1:17
                  message: Identifier 'Regiоn' contains mixed Latin and Cyrillic characters
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_excluded_words_not_reported() {
        let code = r#"
ComОбъект = 1;
HTTPСоединение = 2;
ЧтениеXML = 3;
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LatinAndCyrillicSymbolInWord,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_short_identifiers_skipped() {
        let code = r#"
Перем А;
Перем a;
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LatinAndCyrillicSymbolInWord,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_pure_cyrillic_not_flagged() {
        let code = r#"
Перем ПеременнаяРусская;
Процедура ПроцедураРусская()
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LatinAndCyrillicSymbolInWord,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_pure_latin_not_flagged() {
        let code = r#"
Перем VariableEnglish;
Процедура ProcedureEnglish()
КонецПроцедуры
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LatinAndCyrillicSymbolInWord,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_trailing_pattern_allowed() {
        let code = r#"
Процедура ВИмениEnglish()
КонецПроцедуры

Функция InNameРусский()
КонецФункции
"#;
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::LatinAndCyrillicSymbolInWord,
            expect![[r#""#]],
        );
    }
}
