use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use line_index::{LineIndex, TextSize};
use std::collections::HashSet;
use syntax::{ast::AstNode, SyntaxKind, SyntaxNode};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Consistent,
};

const DEFAULT_MAX_LINE_LENGTH: i64 = 120;
const DEFAULT_CHECK_METHOD_DESCRIPTION: bool = true;
const DEFAULT_EXCLUDE_TRAILING_COMMENTS: bool = false;

#[derive(Debug, Clone)]
struct Config {
    max_line_length: usize,
    check_method_description: bool,
    exclude_trailing_comments: bool,
}

impl Config {
    fn from_context(ctx: &DiagnosticsContext) -> Self {
        let code = DiagnosticCode::LineLength;

        let max_line_length =
            ctx.config_int(code, "maxLineLength", DEFAULT_MAX_LINE_LENGTH) as usize;

        let check_method_description =
            ctx.config_bool(code, "checkMethodDescription", DEFAULT_CHECK_METHOD_DESCRIPTION);

        let exclude_trailing_comments =
            ctx.config_bool(code, "excludeTrailingComments", DEFAULT_EXCLUDE_TRAILING_COMMENTS);

        tracing::debug!(
            max_line_length = max_line_length,
            check_method_description = check_method_description,
            exclude_trailing_comments = exclude_trailing_comments,
            "LineLength config loaded"
        );

        Self { max_line_length, check_method_description, exclude_trailing_comments }
    }
}

#[derive(Debug, Clone, Default)]
struct LineInfo {
    max_code_char_pos: usize,
    max_char_pos: usize,
    has_code: bool,
    is_multiline_string: bool,
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("LineLength::check").entered();
    let code = DiagnosticCode::LineLength;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);

    let parse = ctx.parse();
    let root = parse.syntax_node();

    let file_text = ctx.file_text();
    let file_text = file_text.as_ref();

    let line_index = ctx.line_index();
    let num_lines = line_index.len_lines();

    let mut line_infos: Vec<LineInfo> = vec![LineInfo::default(); num_lines as usize];

    mark_multiline_string_lines(&root, &line_index, &mut line_infos);

    process_code_tokens(&root, file_text, &line_index, &mut line_infos);

    let method_desc_lines = if !config.check_method_description {
        find_method_description_lines(&root, &line_index)
    } else {
        HashSet::new()
    };

    process_comments(&root, file_text, &line_index, &mut line_infos, &method_desc_lines, &config);

    let diagnostics = generate_diagnostics(
        &line_infos,
        &line_index,
        file_text,
        config.max_line_length,
        code,
        ctx,
    );

    tracing::debug!(count = diagnostics.len(), "LineLength diagnostics found");

    diagnostics
}

fn mark_multiline_string_lines(
    root: &SyntaxNode,
    line_index: &LineIndex,
    line_infos: &mut [LineInfo],
) {
    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            if matches!(token.kind(), SyntaxKind::STRING_PART | SyntaxKind::STRING_TAIL) {
                let range = token.text_range();
                let start_line = line_index.line_col(range.start()).line;
                let end_line = line_index.line_col(range.end()).line;

                for line in start_line..=end_line {
                    if let Some(info) = line_infos.get_mut(line as usize) {
                        info.is_multiline_string = true;
                    }
                }
            }
        }
    }
}

fn process_code_tokens(
    root: &SyntaxNode,
    file_text: &str,
    line_index: &LineIndex,
    line_infos: &mut [LineInfo],
) {
    let mut prev_token_kind: Option<SyntaxKind> = None;

    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            let kind = token.kind();

            if kind == SyntaxKind::WHITESPACE || kind == SyntaxKind::NEWLINE {
                continue;
            }

            if kind == SyntaxKind::COMMENT {
                continue;
            }

            if matches!(kind, SyntaxKind::STRING_PART | SyntaxKind::STRING_TAIL) {
                prev_token_kind = Some(kind);
                continue;
            }

            if kind == SyntaxKind::SEMICOLON {
                if let Some(prev) = prev_token_kind {
                    if matches!(prev, SyntaxKind::STRING_PART | SyntaxKind::STRING_TAIL) {
                        prev_token_kind = Some(kind);
                        continue;
                    }
                }
            }

            let range = token.text_range();
            let end_pos = line_index.line_col(range.end());
            let line = end_pos.line as usize;

            if let Some(info) = line_infos.get_mut(line) {
                let line_start = line_index.line_start(end_pos.line);
                let byte_col = u32::from(range.end()) - u32::from(line_start);
                let line_text_start: usize = line_start.into();
                let line_text_end = (line_text_start + byte_col as usize).min(file_text.len());

                let line_text_end = file_text.floor_char_boundary(line_text_end);

                let char_col = file_text[line_text_start..line_text_end].chars().count();

                info.max_code_char_pos = info.max_code_char_pos.max(char_col);
                info.max_char_pos = info.max_char_pos.max(char_col);
                info.has_code = true;
            }

            prev_token_kind = Some(kind);
        }
    }
}

fn find_method_description_lines(root: &SyntaxNode, line_index: &LineIndex) -> HashSet<u32> {
    use syntax::ast::{FunctionDef, ProcedureDef};

    let mut method_desc_lines = HashSet::new();

    let mut comments: Vec<(u32, TextRange)> = Vec::new();
    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            if token.kind() == SyntaxKind::COMMENT {
                let range = token.text_range();
                let line = line_index.line_col(range.start()).line;
                comments.push((line, range));
            }
        }
    }

    comments.sort_by_key(|(line, _)| *line);

    for node in root.descendants() {
        let method_start = ProcedureDef::cast(node.clone())
            .map(|proc| proc.syntax().text_range().start())
            .or_else(|| {
                FunctionDef::cast(node.clone()).map(|func| func.syntax().text_range().start())
            });

        if let Some(method_start_pos) = method_start {
            let method_line = line_index.line_col(method_start_pos).line;

            let mut desc_lines = Vec::new();
            for &(comment_line, _) in comments.iter().rev() {
                if comment_line >= method_line {
                    continue;
                }
                if desc_lines.is_empty() || desc_lines.last() == Some(&(comment_line + 1)) {
                    desc_lines.push(comment_line);
                } else {
                    break;
                }
            }

            for line in desc_lines {
                method_desc_lines.insert(line);
            }
        }
    }

    method_desc_lines
}

fn process_comments(
    root: &SyntaxNode,
    file_text: &str,
    line_index: &LineIndex,
    line_infos: &mut [LineInfo],
    method_desc_lines: &HashSet<u32>,
    config: &Config,
) {
    for element in root.descendants_with_tokens() {
        if let Some(token) = element.into_token() {
            if token.kind() != SyntaxKind::COMMENT {
                continue;
            }

            let range = token.text_range();
            let start_pos = line_index.line_col(range.start());
            let line = start_pos.line as usize;

            if !config.check_method_description && method_desc_lines.contains(&(line as u32)) {
                continue;
            }

            if config.exclude_trailing_comments {
                if let Some(info) = line_infos.get(line) {
                    if info.has_code {
                        continue;
                    }
                }
            }

            let end_pos = line_index.line_col(range.end());
            let end_line = end_pos.line as usize;

            if let Some(info) = line_infos.get_mut(end_line) {
                let line_start = line_index.line_start(end_pos.line);
                let byte_col = u32::from(range.end()) - u32::from(line_start);
                let line_text_start: usize = line_start.into();
                let line_text_end = (line_text_start + byte_col as usize).min(file_text.len());

                let line_text_end = file_text.floor_char_boundary(line_text_end);

                let char_col = file_text[line_text_start..line_text_end].chars().count();

                info.max_char_pos = info.max_char_pos.max(char_col);
            }
        }
    }
}

fn generate_diagnostics(
    line_infos: &[LineInfo],
    line_index: &LineIndex,
    file_text: &str,
    max_line_length: usize,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for (line_num, info) in line_infos.iter().enumerate() {
        if info.is_multiline_string {
            continue;
        }

        if info.max_char_pos > max_line_length {
            let line = line_num as u32;
            let line_start = line_index.line_start(line);

            let line_text_start: usize = line_start.into();
            let line_range = line_index.line_range(line);
            let line_text_end: usize =
                line_range.map(|r| r.end().into()).unwrap_or(file_text.len());
            let line_text = &file_text[line_text_start..line_text_end.min(file_text.len())];

            let mut byte_offset = 0usize;
            for (i, ch) in line_text.chars().enumerate() {
                if i >= info.max_char_pos {
                    break;
                }
                byte_offset += ch.len_utf8();
            }

            let range = TextRange::new(line_start, line_start + TextSize::from(byte_offset as u32));

            diagnostics.push(Diagnostic {
                code,
                message: format!(
                    "Длина строки {} превышает максимальную {}",
                    info.max_char_pos, max_line_length
                ),
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::{check_ast_diagnostic, check_ast_diagnostic_with_config, format_diags};
    use crate::{DiagnosticCode, DiagnosticsConfig};
    use expect_test::expect;

    const FIXTURE: &str = r#"А = 0;

А = "фффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффф";
А = "ффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффф";
А = "фффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффф";
А = "ффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффф";

// Просто коммент
// Длинный ОченьДлинный ОченьОченьДлинный Коооооооооооооооооооооомммммммммеееееееееееееееееееенннннтт Просто такооооййй Коммент

    А = Код + Код + Код; // Просто коммент
    А = Код + Код + Коооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооод; // Длииииииииинныййййй коммент
    А = Код + Код + Кооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооодддддддддддддддддддод; // коммент

Запрос.Текст =
    "ВЫБРАТЬ
    | Данные.Поле1,
    | Данные.ОоооооооооооооооооооооооооооооооочеееееееееннннннннннннннннннннььььььььДлинннннннннннннннннннннннннннноооооееееПоле2,
    | Данные.Поле3
    |ИЗ
    | Источник КАК Данные
    |ГДЕ
    | Данные.ПолеУсловия = &ООООООООООООООООООООООООООООООЧЧЧЧЧЧЧЧЧЧЧЧЧЧЧЧЕЕЕЕЕЕЕЕЕЕЕЕЕЕЕЕЕЕНННННННННННЬЬЬЬЬЬЬЬЬДДДДДДДДЛЛЛЛЛЛЛ"
    ;

    Запрос.Текст =
        "ВЫБРАТЬ
        | Данные.Поле1,
        | Данные.ОоооооооооооооооооооооооооооооооочеееееееееннннннннннннннннннннььььььььДлинннннннннннннннннннннннннннноооооееееПоле2,
        | Данные.Поле3
        |ИЗ
        | Источник КАК Данные
        |ГДЕ
        | Данные.ПолеУсловия = &ООООООООООООООООООООООООООООООЧЧЧЧЧЧЧЧЧЧЧЧЧЧЧЧЕЕЕЕЕЕЕЕЕЕЕЕЕЕЕЕЕЕНННННННННННЬЬЬЬЬЬЬЬЬДДДДДДДДЛЛЛЛЛЛЛ";


// Длинный ОченьДлинный ОченьОченьДлинный Коооооооооооооооооооооомммммммммеееееееееееееееееееенннннтт Просто такооооййй Коммент
А = 0;

ТекстСообщения = СтроковыеФункцииКлиентСервер.ПодставитьПараметрыВСтроку(
НСтр("ru = 'Процедуре ЗаполнитьРеквизитСпособОтображенияПодсказки не удалось обработать некоторые вопросы шаблона анкеты (пропущены): %1'"),
ПроблемныхОбъектов);

ТекстСообщения = СтроковыеФункцииКлиентСервер.ПодставитьПараметрыВСтроку(
    Длинныыыыыыыыыыййййй.Метттттттттттттттооооооооооооооодддддддддддд(СПааааааааааааааааррррррррааааааам, Мееееетттттттррррррррраааааамммииии),
	ПроблемныхОбъектов);

Длинныыыыыыыыыыййййй.Метттттттттттттттооооооооооооооодддддддддддд(СПааааааааааааааааррррррррааааааам, Мееееетттттттррррррррраааааамммииии);

Длинныыыыыыыыыыййййй.Метттттттттттттттооооооооооооооодддддддддддд(СПааааааааааааааааррррррррааааааам, Мееееетттттттррррррррраааааамммииии)
;

ТекстСообщения = НСтр("ru = 'Процедуре ЗаполнитьРеквизитСпособОтображенияПодсказки не удалось обработать некоторые вопросы шаблона анкеты (пропущены): %1'", ПроблемныхОбъектов);

// Описание
// Парамеры:
//  Параметр1 - ТипПараметра - Описание ооооооооооооооооооооооооооооооооооооооооооооооооооооооооооооочччччччччччччччччччччччччччччччччччччччень длиннннннннннннное
Процедура Тест(Параметр1)
КонецПроцедуры

// Описание                      длинное                                  очень                                           очень            вввввв
// Парамеры:
//  Параметр1 - ТипПараметра - Описание
Процедура Тест2(Параметр1)
КонецПроцедуры
"#;

    #[test]
    fn test_simple_long_line() {
        let code = r#"А = "фффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффффф";"#;
        let diagnostics = check_ast_diagnostic(code, check);

        expect![[r#"
            LineLength @ 1:1..1:122
              message: Длина строки 121 превышает максимальную 120
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_utf8_characters() {
        let code = "А = \"фф\";  // Short line";
        let diagnostics = check_ast_diagnostic(code, check);

        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_comprehensive() {
        let code = FIXTURE;
        let diagnostics = check_ast_diagnostic(code, check);

        expect![[r#"
            LineLength @ 5:1..5:122
              message: Длина строки 121 превышает максимальную 120
              severity: Information
            LineLength @ 6:1..6:123
              message: Длина строки 122 превышает максимальную 120
              severity: Information
            LineLength @ 9:1..9:128
              message: Длина строки 127 превышает максимальную 120
              severity: Information
            LineLength @ 12:1..12:137
              message: Длина строки 136 превышает максимальную 120
              severity: Information
            LineLength @ 13:1..13:136
              message: Длина строки 135 превышает максимальную 120
              severity: Information
            LineLength @ 37:1..37:128
              message: Длина строки 127 превышает максимальную 120
              severity: Information
            LineLength @ 41:1..41:141
              message: Длина строки 140 превышает максимальную 120
              severity: Information
            LineLength @ 45:1..45:144
              message: Длина строки 143 превышает максимальную 120
              severity: Information
            LineLength @ 48:1..48:140
              message: Длина строки 139 превышает максимальную 120
              severity: Information
            LineLength @ 50:1..50:139
              message: Длина строки 138 превышает максимальную 120
              severity: Information
            LineLength @ 53:1..53:178
              message: Длина строки 177 превышает максимальную 120
              severity: Information
            LineLength @ 57:1..57:163
              message: Длина строки 162 превышает максимальную 120
              severity: Information
            LineLength @ 61:1..61:146
              message: Длина строки 145 превышает максимальную 120
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_configured_max_length() {
        let code = FIXTURE;
        let mut config = DiagnosticsConfig::default();
        config
            .parameters
            .insert(DiagnosticCode::LineLength, serde_json::json!({"maxLineLength": 119}));

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        expect![[r#"
            LineLength @ 4:1..4:121
              message: Длина строки 120 превышает максимальную 119
              severity: Information
            LineLength @ 5:1..5:122
              message: Длина строки 121 превышает максимальную 119
              severity: Information
            LineLength @ 6:1..6:123
              message: Длина строки 122 превышает максимальную 119
              severity: Information
            LineLength @ 9:1..9:128
              message: Длина строки 127 превышает максимальную 119
              severity: Information
            LineLength @ 12:1..12:137
              message: Длина строки 136 превышает максимальную 119
              severity: Information
            LineLength @ 13:1..13:136
              message: Длина строки 135 превышает максимальную 119
              severity: Information
            LineLength @ 37:1..37:128
              message: Длина строки 127 превышает максимальную 119
              severity: Information
            LineLength @ 41:1..41:141
              message: Длина строки 140 превышает максимальную 119
              severity: Information
            LineLength @ 45:1..45:144
              message: Длина строки 143 превышает максимальную 119
              severity: Information
            LineLength @ 48:1..48:140
              message: Длина строки 139 превышает максимальную 119
              severity: Information
            LineLength @ 50:1..50:139
              message: Длина строки 138 превышает максимальную 119
              severity: Information
            LineLength @ 53:1..53:178
              message: Длина строки 177 превышает максимальную 119
              severity: Information
            LineLength @ 57:1..57:163
              message: Длина строки 162 превышает максимальную 119
              severity: Information
            LineLength @ 61:1..61:146
              message: Длина строки 145 превышает максимальную 119
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_exclude_method_description() {
        let code = FIXTURE;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::LineLength,
            serde_json::json!({"checkMethodDescription": false}),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        expect![[r#"
            LineLength @ 5:1..5:122
              message: Длина строки 121 превышает максимальную 120
              severity: Information
            LineLength @ 6:1..6:123
              message: Длина строки 122 превышает максимальную 120
              severity: Information
            LineLength @ 9:1..9:128
              message: Длина строки 127 превышает максимальную 120
              severity: Information
            LineLength @ 12:1..12:137
              message: Длина строки 136 превышает максимальную 120
              severity: Information
            LineLength @ 13:1..13:136
              message: Длина строки 135 превышает максимальную 120
              severity: Information
            LineLength @ 37:1..37:128
              message: Длина строки 127 превышает максимальную 120
              severity: Information
            LineLength @ 41:1..41:141
              message: Длина строки 140 превышает максимальную 120
              severity: Information
            LineLength @ 45:1..45:144
              message: Длина строки 143 превышает максимальную 120
              severity: Information
            LineLength @ 48:1..48:140
              message: Длина строки 139 превышает максимальную 120
              severity: Information
            LineLength @ 50:1..50:139
              message: Длина строки 138 превышает максимальную 120
              severity: Information
            LineLength @ 53:1..53:178
              message: Длина строки 177 превышает максимальную 120
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_exclude_trailing_comments() {
        let code = FIXTURE;
        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::LineLength,
            serde_json::json!({"excludeTrailingComments": true}),
        );

        let diagnostics = check_ast_diagnostic_with_config(code, config, check);

        expect![[r#"
            LineLength @ 5:1..5:122
              message: Длина строки 121 превышает максимальную 120
              severity: Information
            LineLength @ 6:1..6:123
              message: Длина строки 122 превышает максимальную 120
              severity: Information
            LineLength @ 9:1..9:128
              message: Длина строки 127 превышает максимальную 120
              severity: Information
            LineLength @ 13:1..13:125
              message: Длина строки 124 превышает максимальную 120
              severity: Information
            LineLength @ 37:1..37:128
              message: Длина строки 127 превышает максимальную 120
              severity: Information
            LineLength @ 41:1..41:141
              message: Длина строки 140 превышает максимальную 120
              severity: Information
            LineLength @ 45:1..45:144
              message: Длина строки 143 превышает максимальную 120
              severity: Information
            LineLength @ 48:1..48:140
              message: Длина строки 139 превышает максимальную 120
              severity: Information
            LineLength @ 50:1..50:139
              message: Длина строки 138 превышает максимальную 120
              severity: Information
            LineLength @ 53:1..53:178
              message: Длина строки 177 превышает максимальную 120
              severity: Information
            LineLength @ 57:1..57:163
              message: Длина строки 162 превышает максимальную 120
              severity: Information
            LineLength @ 61:1..61:146
              message: Длина строки 145 превышает максимальную 120
              severity: Information"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }
}
