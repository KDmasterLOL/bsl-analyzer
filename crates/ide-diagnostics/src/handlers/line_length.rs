use crate::define_metadata;
use crate::metadata::*;
use crate::slab::{self, Block};
use crate::{AnalysisContext, Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::LocalRange;
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
    fn from_context(ctx: &AnalysisContext) -> Self {
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
    slab::check_file_by_blocks(ctx, check_block)
}

pub fn check_block(ctx: &AnalysisContext, block: &Block) -> Vec<Diagnostic<LocalRange>> {
    let _span = tracing::debug_span!("LineLength::check").entered();
    let code = DiagnosticCode::LineLength;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);
    let num_lines = block.line_index.len_lines();
    let mut line_infos: Vec<LineInfo> = vec![LineInfo::default(); num_lines as usize];

    mark_multiline_string_lines(block, &mut line_infos);
    process_code_tokens(block, &mut line_infos);
    process_comments(block, &mut line_infos, &config);

    let diagnostics = generate_diagnostics(&line_infos, block, config.max_line_length, code, ctx);

    tracing::debug!(count = diagnostics.len(), "LineLength diagnostics found");

    diagnostics
}

fn mark_multiline_string_lines(block: &Block, line_infos: &mut [LineInfo]) {
    for token in block.tokens {
        if matches!(token.kind, SyntaxKind::STRING_PART | SyntaxKind::STRING_TAIL) {
            let start_line = block.line_index.line_col(token.range.start()).line;
            let end_line = block.line_index.line_col(token.range.end()).line;

            for line in start_line..=end_line {
                if let Some(info) = line_infos.get_mut(line as usize) {
                    info.is_multiline_string = true;
                }
            }
        }
    }
}

/// Строка, на которой кончается токен, и его конец в символах от её начала.
fn column_at(block: &Block, offset: TextSize) -> (usize, usize) {
    let pos = block.line_index.line_col(offset);
    let line_start: usize = block.line_index.line_start(pos.line).into();
    let end = usize::from(offset).min(block.text.len());
    let end = block.text.floor_char_boundary(end);
    (pos.line as usize, block.text[line_start..end].chars().count())
}

fn process_code_tokens(block: &Block, line_infos: &mut [LineInfo]) {
    // Сосед слева нужен только `;` первым значимым токеном: после хвоста
    // строкового литерала он длину строки не считает.
    let mut prev_token_kind: Option<SyntaxKind> = block.prev_significant();

    for token in block.tokens {
        let kind = token.kind;

        if kind.is_trivia() {
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

        let (line, char_col) = column_at(block, token.range.end());
        if let Some(info) = line_infos.get_mut(line) {
            info.max_code_char_pos = info.max_code_char_pos.max(char_col);
            info.max_char_pos = info.max_char_pos.max(char_col);
            info.has_code = true;
        }

        prev_token_kind = Some(kind);
    }
}

/// Строки описания методов по файлу: ближайший комментарий над методом на
/// любом расстоянии и смежные с ним выше — независимо от того, в каком узле
/// они лежат. Отсортированы по возрастанию.
pub(crate) fn find_method_description_lines(root: &SyntaxNode, line_index: &LineIndex) -> Vec<u32> {
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

    let mut lines: Vec<u32> = method_desc_lines.into_iter().collect();
    lines.sort_unstable();
    lines
}

fn process_comments(block: &Block, line_infos: &mut [LineInfo], config: &Config) {
    for token in block.tokens {
        if token.kind != SyntaxKind::COMMENT {
            continue;
        }

        let line = block.line_index.line_col(token.range.start()).line;

        if !config.check_method_description && block.description_lines.binary_search(&line).is_ok()
        {
            continue;
        }

        if config.exclude_trailing_comments {
            if let Some(info) = line_infos.get(line as usize) {
                if info.has_code {
                    continue;
                }
            }
        }

        let (end_line, char_col) = column_at(block, token.range.end());
        if let Some(info) = line_infos.get_mut(end_line) {
            info.max_char_pos = info.max_char_pos.max(char_col);
        }
    }
}

fn generate_diagnostics(
    line_infos: &[LineInfo],
    block: &Block,
    max_line_length: usize,
    code: DiagnosticCode,
    ctx: &AnalysisContext,
) -> Vec<Diagnostic<LocalRange>> {
    let mut diagnostics = Vec::new();

    for (line_num, info) in line_infos.iter().enumerate() {
        if info.is_multiline_string {
            continue;
        }

        if info.max_char_pos > max_line_length {
            let line = line_num as u32;
            let line_start = block.line_index.line_start(line);
            let line_text = block.line_text(line).unwrap_or("");

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
                range: LocalRange::of_detached_node(range),
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

    /// BOM не делает первую строку несущей код.
    ///
    /// Замер длины пропускает тривию, и BOM обязан идти с нею: место,
    /// считающее его значимым токеном, метит первую строку как строку с
    /// кодом, а при включённом исключении хвостовых комментариев комментарий
    /// на такой строке из замера выпадает — и находка пропадает целиком.
    ///
    /// Вход подобран так, чтобы разница была ВИДНА: длинный комментарий стоит
    /// первой строкой, потому что только там BOM ему соседствует.
    #[test]
    fn a_byte_order_mark_does_not_make_the_first_line_code() {
        let long_comment = format!("// {}", "о".repeat(150));
        let code = format!("{long_comment}\nПроцедура Тест()\nКонецПроцедуры\n");

        let mut config = DiagnosticsConfig::default();
        config.parameters.insert(
            DiagnosticCode::LineLength,
            serde_json::json!({"excludeTrailingComments": true}),
        );

        let without = check_ast_diagnostic_with_config(&code, config.clone(), check);
        assert_eq!(without.len(), 1, "вход обязан давать находку, иначе сверка пуста");

        let with_bom = check_ast_diagnostic_with_config(&format!("\u{feff}{code}"), config, check);
        assert_eq!(with_bom.len(), without.len(), "BOM отменил находку о длине строки");
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
