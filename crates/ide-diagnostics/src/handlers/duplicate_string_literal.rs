use crate::define_metadata;
use crate::metadata::*;
use crate::{AnalysisContext, BodyContext, Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::LocalRange;
use ide_db::TextRange;
use std::collections::HashMap;
use stdx::case::CaseExt;
use syntax::{SyntaxKind, SyntaxNode};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Файловый вход: только свод по файлу (`analyzeFile=true`). Область по
/// умолчанию — метод, и её считает [`check_body`] по отсоединённому корню:
/// файловая проверка при `analyzeFile=false` молчит, иначе находки
/// удвоились бы.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::DuplicateStringLiteral;
    if ctx.is_disabled_with_metadata(code) || !Config::from_context(ctx).analyze_file {
        return Vec::new();
    }
    check_file(ctx)
}

/// Прежний файловый алгоритм в обоих режимах: эталон для проверки сборки
/// по методам и тело файлового входа при `analyzeFile=true`.
pub(crate) fn check_file(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let _span = tracing::debug_span!("DuplicateStringLiteral::check").entered();
    let code = DiagnosticCode::DuplicateStringLiteral;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let config = Config::from_context(ctx);

    let parse = ctx.parse();
    let root = parse.syntax_node();

    let scopes = find_scopes(&root, config.analyze_file);
    let mut diagnostics = Vec::new();

    for scope in scopes {
        let groups = collect_strings(&scope, &config);
        diagnostics.extend(report_duplicates(groups, &config, code, ctx, |range| range));
    }

    tracing::debug!(count = diagnostics.len(), "DuplicateStringLiteral diagnostics found");
    diagnostics
}

/// Область по умолчанию — метод: литералы тела и их вызовы-исключения лежат
/// в узле метода, позиций проверка не читает, так что отсоединённый корень
/// даёт ровно то, что дал бы обход файла по этому методу. Модульный код в
/// этом режиме не судится, а при `analyzeFile=true` свод делает файл.
pub fn check_body(ctx: &BodyContext, acc: &mut Vec<Diagnostic<LocalRange>>) {
    let code = DiagnosticCode::DuplicateStringLiteral;
    if ctx.is_module_code() || ctx.is_disabled_with_metadata(code) {
        return;
    }
    let config = Config::from_context(ctx);
    if config.analyze_file {
        return;
    }
    let groups = collect_strings(ctx.root(), &config);
    acc.extend(report_duplicates(groups, &config, code, ctx, LocalRange::of_detached_node));
}

#[derive(Debug, Clone)]
struct Config {
    allowed_number_copies: usize,
    analyze_file: bool,
    case_sensitive: bool,
    min_text_length: usize,
    excluded_methods: Vec<String>,
}

impl Config {
    fn from_context(ctx: &AnalysisContext) -> Self {
        let code = DiagnosticCode::DuplicateStringLiteral;

        let mut allowed = ctx.config_int(code, "allowedNumberCopies", 2) as usize;
        if allowed < 1 {
            tracing::warn!("allowedNumberCopies < 1 ({}), resetting to default (2)", allowed);
            allowed = 2;
        }

        let analyze_file = ctx.config_bool(code, "analyzeFile", false);

        let case_sensitive = ctx.config_bool(code, "caseSensitive", false);

        let min_length = ctx.config_int(code, "minTextLength", 5) as usize;
        let min_text_length = min_length.max(5);

        let excluded_methods = ctx
            .config
            .get_string_array(code, "excludedMethods")
            .unwrap_or_else(|| {
                vec![
                    "Тип".to_string(),
                    "Type".to_string(),
                    "ОписаниеТипов".to_string(),
                    "TypeDescription".to_string(),
                ]
            })
            .iter()
            .map(|s| s.fold_lower())
            .collect();

        tracing::debug!(
            allowed_number_copies = allowed,
            analyze_file = analyze_file,
            case_sensitive = case_sensitive,
            min_text_length = min_text_length,
            ?excluded_methods,
            "Config loaded"
        );

        Self {
            allowed_number_copies: allowed,
            analyze_file,
            case_sensitive,
            min_text_length,
            excluded_methods,
        }
    }
}

fn find_scopes(root: &SyntaxNode, analyze_file: bool) -> Vec<SyntaxNode> {
    if analyze_file {
        return vec![root.clone()];
    }

    root.descendants()
        .filter(|n| matches!(n.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF))
        .collect()
}

fn collect_strings(
    scope: &SyntaxNode,
    config: &Config,
) -> HashMap<String, Vec<(String, TextRange)>> {
    let mut groups: HashMap<String, Vec<(String, TextRange)>> = HashMap::new();
    let mut string_count = 0;

    for node in scope.descendants() {
        if node.kind() == SyntaxKind::LITERAL {
            let has_string = node.children_with_tokens().any(|elem| {
                elem.as_token()
                    .map(|t| {
                        matches!(
                            t.kind(),
                            SyntaxKind::STRING
                                | SyntaxKind::STRING_START
                                | SyntaxKind::STRING_TAIL
                                | SyntaxKind::STRING_PART
                        )
                    })
                    .unwrap_or(false)
            });

            if !has_string {
                continue;
            }

            if !config.excluded_methods.is_empty()
                && is_excluded_call_argument(&node, &config.excluded_methods)
            {
                continue;
            }

            string_count += 1;
            let text = node.text().to_string();

            tracing::trace!(
                text = %text,
                len = text.len(),
                min_len = config.min_text_length,
                "Found string literal"
            );

            if text.len() < config.min_text_length {
                tracing::trace!("Filtered by min_text_length");
                continue;
            }

            let key = if config.case_sensitive { text.clone() } else { text.fold_lower() };

            groups.entry(key).or_default().push((text, node.text_range()));
        }
    }

    tracing::debug!(string_count = string_count, groups = groups.len(), "Collected strings");

    groups
}

fn report_duplicates<R: Copy>(
    groups: HashMap<String, Vec<(String, TextRange)>>,
    config: &Config,
    code: DiagnosticCode,
    ctx: &AnalysisContext,
    range: impl Fn(TextRange) -> R,
) -> Vec<Diagnostic<R>> {
    let mut diagnostics = Vec::new();

    for (_, occurrences) in groups {
        if occurrences.len() > config.allowed_number_copies {
            let (first_text, first_range) = &occurrences[0];

            let message = format!(
                "Необходимо избавиться от многократного использования строкового литерала \"{}\"",
                first_text
            );

            diagnostics.push((
                *first_range,
                Diagnostic {
                    code,
                    message,
                    severity: ctx.severity(code),
                    range: range(*first_range),
                    tags: ctx.tags(code),
                    fixes: vec![],
                },
            ));
        }
    }

    // The groups map iterates in hash order, which varies run to run; emit in source
    // order (position of each literal's first occurrence) so the output is stable.
    diagnostics.sort_by_key(|(range, _)| (range.start(), range.end()));

    diagnostics.into_iter().map(|(_, diagnostic)| diagnostic).collect()
}

fn is_excluded_call_argument(literal: &SyntaxNode, excluded: &[String]) -> bool {
    let call = literal
        .ancestors()
        .find(|n| matches!(n.kind(), SyntaxKind::CALL_EXPR | SyntaxKind::NEW_EXPR));
    let Some(call) = call else { return false };

    let callee_name = extract_callee_name(&call);

    match callee_name {
        Some(name) => {
            let lower = name.fold_lower();
            excluded.contains(&lower)
        }
        None => false,
    }
}

fn extract_callee_name(node: &SyntaxNode) -> Option<String> {
    for child in node.children() {
        if child.kind() == SyntaxKind::IDENT {
            return Some(child.text().to_string());
        }
    }
    for elem in node.children_with_tokens() {
        if let Some(token) = elem.as_token() {
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use expect_test::expect;

    #[test]
    fn test_duplicate_in_method() {
        let code = r#"Процедура Метод1()
    Ц = "Строка2";
    Если Ц = "Строка2" Тогда
        Ф = ВРег("Строка2") + НРег("Строка3");
    Иначе
        Ф = НРег("Строка2");
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#"
            DuplicateStringLiteral @ 2:9..2:18
              message: Необходимо избавиться от многократного использования строкового литерала ""Строка2""
              severity: Information"#]].assert_eq(&format_diags(code, &diagnostics));
        assert!(diagnostics[0].message.contains("Строка2"));
    }

    #[test]
    fn test_duplicate_case_insensitive_in_method() {
        let code = r#"Процедура Метод2()
    Ц2 = "Строка22";
    Если Ц2 = "Строка22" Тогда
        Ф2 = Метод7("строка22");
    Иначе
        Ф2 = ("Строка3" + "Строка4" + "СтрОкА22");
    КонецЕсли;
КонецПроцедуры"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#"
            DuplicateStringLiteral @ 2:10..2:20
              message: Необходимо избавиться от многократного использования строкового литерала ""Строка22""
              severity: Information"#]].assert_eq(&format_diags(code, &diagnostics));
        assert!(diagnostics[0].message.contains("Строка22"));
    }

    #[test]
    fn test_case_insensitive() {
        let code = r#"
Процедура Тест()
    А = "Ошибка";
    Б = "ошибка";
    В = "ОШИБКА";
КонецПроцедуры
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#"
            DuplicateStringLiteral @ 3:9..3:17
              message: Необходимо избавиться от многократного использования строкового литерала ""Ошибка""
              severity: Information"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_min_length_filter() {
        let code = r#"
Процедура Тест()
    А = "OK";
    Б = "OK";
    В = "OK";
КонецПроцедуры
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_threshold() {
        let code = r#"
Процедура Тест()
    А = "Текст1";
    Б = "Текст1";
КонецПроцедуры
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_exceeds_threshold() {
        let code = r#"
Процедура Тест()
    А = "Текст1";
    Б = "Текст1";
    В = "Текст1";
КонецПроцедуры
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#"
            DuplicateStringLiteral @ 3:9..3:17
              message: Необходимо избавиться от многократного использования строкового литерала ""Текст1""
              severity: Information"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_excluded_methods_type() {
        let code = r#"
Процедура Тест()
    А = Тип("СправочникСсылка.Товары");
    Б = Тип("СправочникСсылка.Товары");
    В = Тип("СправочникСсылка.Товары");
КонецПроцедуры
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_excluded_methods_type_english() {
        let code = r#"
Процедура Тест()
    А = Type("CatalogRef.Goods");
    Б = Type("CatalogRef.Goods");
    В = Type("CatalogRef.Goods");
КонецПроцедуры
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_excluded_methods_mixed_with_regular() {
        let code = r#"
Процедура Тест()
    А = Тип("СправочникСсылка.Товары");
    Б = Тип("СправочникСсылка.Товары");
    В = "СправочникСсылка.Товары";
    Г = "СправочникСсылка.Товары";
    Д = "СправочникСсылка.Товары";
КонецПроцедуры
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#"
            DuplicateStringLiteral @ 5:9..5:34
              message: Необходимо избавиться от многократного использования строкового литерала ""СправочникСсылка.Товары""
              severity: Information"#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_excluded_type_description_constructor() {
        let code = r#"
Процедура Тест()
    ТаблицаДанных.Колонки.Добавить("ОстаткиПоЯчейкам", Новый ОписаниеТипов("Число", , , Новый КвалификаторыЧисла(10, 3)));
    ТаблицаДанных.Колонки.Добавить("ОстаткиНаСкладе", Новый ОписаниеТипов("Число", , , Новый КвалификаторыЧисла(10, 3)));
    ТаблицаДанных.Колонки.Добавить("Разница", Новый ОписаниеТипов("Число", , , Новый КвалификаторыЧисла(10, 3)));
КонецПроцедуры
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn test_groups_emitted_in_source_order() {
        let code = r#"
Процедура Тест()
    А = "Первый литерал";
    Б = "Второй литерал";
    В = "Первый литерал";
    Г = "Второй литерал";
    Д = "Первый литерал";
    Е = "Второй литерал";
КонецПроцедуры
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics[0].message.contains("Первый литерал"));
        assert!(diagnostics[1].message.contains("Второй литерал"));
    }

    #[test]
    fn test_separate_scopes() {
        let code = r#"
Процедура Метод1()
    А = "Текст1";
    Б = "Текст1";
КонецПроцедуры

Процедура Метод2()
    В = "Текст1";
    Г = "Текст1";
КонецПроцедуры
"#;
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    /// Свод по файлу: одинаковые литералы двух методов считаются вместе и
    /// отмечаются по первому вхождению; в режиме по умолчанию тот же текст
    /// молчит на файловом входе — область там метод.
    #[test]
    fn file_mode_counts_across_methods() {
        let code = "Процедура А()\n\tХ = \"Строка\";\n\tУ = \"Строка\";\nКонецПроцедуры\nПроцедура Б()\n\tЗ = \"Строка\";\nКонецПроцедуры\n";
        let mut config = crate::DiagnosticsConfig::all_enabled();
        config.parameters.insert(
            DiagnosticCode::DuplicateStringLiteral,
            serde_json::json!({"analyzeFile": true}),
        );
        let file = check_ast_diagnostic_with_config(code, config.clone(), check);
        assert_eq!(file.len(), 1, "{file:?}");
        assert_eq!(u32::from(file[0].range.start()), 30, "первое вхождение — в А");
        let bodies = check_body_diagnostic_with_config(code, config, check_body);
        assert!(bodies.is_empty(), "в режиме файла тела молчат");

        let default_file = check_ast_diagnostic(code, check);
        assert!(default_file.is_empty(), "в режиме метода файловый вход молчит");
    }
}
