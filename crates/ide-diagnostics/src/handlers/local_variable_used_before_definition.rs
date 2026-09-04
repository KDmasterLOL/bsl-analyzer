use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::Name;
use ide_db::TextRange;
use std::collections::{HashMap, HashSet};
use stdx::case::CaseExt;
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 2,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

#[derive(Debug, Clone)]
struct NamedPosition {
    name: String,
    start: u32,
    range: TextRange,
}

/// This is a file-syntax fact, not a type-inference fact. Extension directives
/// such as #Вставка can keep valid source text outside the detached method/HIR
/// statement walk, so the rule must inspect the raw file tree that Designer
/// compiles. The verdict stays conservative: only a future For/For Each
/// iterator with the same name can trigger it.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::LocalVariableUsedBeforeDefinition;
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let root = ctx.parse().syntax_node();
    let mut diagnostics = Vec::new();
    for method in root
        .descendants()
        .filter(|node| matches!(node.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF))
    {
        scan_method(&method, ctx, &mut diagnostics);
    }
    diagnostics
}

fn scan_method(method: &SyntaxNode, ctx: &DiagnosticsContext, acc: &mut Vec<Diagnostic>) {
    let mut declared = HashSet::new();
    for param in method.descendants().filter(|node| node.kind() == SyntaxKind::PARAM) {
        if let Some(param) = first_ident(param) {
            declared.insert(param.name);
        }
    }
    for var_def in method.descendants().filter(|node| node.kind() == SyntaxKind::VAR_DEF) {
        declared.extend(
            var_def
                .children_with_tokens()
                .filter_map(|element| element.into_token())
                .filter(|token| token.kind() == SyntaxKind::IDENT)
                .map(|token| token.text().fold_lower()),
        );
    }

    // Keep the source token stream, including flat #Вставка bodies. Code under
    // #Удаление is not part of the effective method. Conditional preprocessor
    // branches are deliberately skipped until this checker models branch
    // environments; reporting less is preferable to a cross-branch false error.
    let tokens = method
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| !inside_excluded_preprocessor(token))
        .collect::<Vec<_>>();

    let mut loop_positions: HashMap<String, Vec<u32>> = HashMap::new();
    let mut iterator_ranges = HashSet::new();
    for index in 0..tokens.len() {
        if tokens[index].kind() != SyntaxKind::KW_FOR {
            continue;
        }
        let Some(mut iterator_index) = next_code_index(&tokens, index) else {
            continue;
        };
        if tokens[iterator_index].kind() == SyntaxKind::KW_EACH {
            let Some(next) = next_code_index(&tokens, iterator_index) else {
                continue;
            };
            iterator_index = next;
        }
        let iterator = &tokens[iterator_index];
        if iterator.kind() != SyntaxKind::IDENT {
            continue;
        }
        let range = iterator.text_range();
        loop_positions.entry(iterator.text().fold_lower()).or_default().push(range.start().into());
        iterator_ranges.insert(range);
    }
    if loop_positions.is_empty() {
        return;
    }
    for positions in loop_positions.values_mut() {
        positions.sort_unstable();
    }

    let mut direct_assignment_ranges = HashSet::new();
    let mut assignment_positions: HashMap<String, Vec<u32>> = HashMap::new();

    // Parsed statements provide exact ordinary-code assignment targets.
    for assignment in method
        .descendants()
        .filter(|node| node.kind() == SyntaxKind::ASSIGN_STMT)
        .filter_map(bare_assignment_target)
    {
        direct_assignment_ranges.insert(assignment.range);
        assignment_positions.entry(assignment.name).or_default().push(assignment.start);
    }

    // #Вставка stores its body as flat tokens rather than ASSIGN_STMT nodes.
    // Recognise only unambiguous bare assignments that begin a statement line;
    // field/index writes are receiver uses and must not define the receiver.
    for insert in method.descendants().filter(|node| node.kind() == SyntaxKind::PRE_INSERT_DIR) {
        collect_flat_insert_assignments(
            &insert,
            &mut direct_assignment_ranges,
            &mut assignment_positions,
        );
    }
    for positions in assignment_positions.values_mut() {
        positions.sort_unstable();
    }

    for (index, token) in tokens.iter().enumerate() {
        if token.kind() != SyntaxKind::IDENT {
            continue;
        }
        let range = token.text_range();
        if iterator_ranges.contains(&range) || direct_assignment_ranges.contains(&range) {
            continue;
        }

        let raw_name = token.text().to_string();
        let name = raw_name.fold_lower();
        let Some(definitions) = loop_positions.get(&name) else {
            continue;
        };

        if declared.contains(&name)
            || ctx.interface_variable_named(&Name::new(&raw_name)).is_some()
            || identifier_is_non_value(&tokens, index)
        {
            continue;
        }

        let use_start: u32 = range.start().into();
        if definitions.iter().any(|definition| *definition <= use_start) {
            continue;
        }
        if assignment_positions
            .get(&name)
            .is_some_and(|positions| positions.iter().any(|assignment| *assignment < use_start))
        {
            continue;
        }

        let message = match ctx.locale() {
            base_db::Locale::Ru => {
                format!("Локальная переменная '{}' используется до определения", raw_name)
            }
            base_db::Locale::En => {
                format!("Local variable '{}' is used before definition", raw_name)
            }
        };
        acc.push(Diagnostic {
            code: DiagnosticCode::LocalVariableUsedBeforeDefinition,
            message,
            severity: ctx.severity(DiagnosticCode::LocalVariableUsedBeforeDefinition),
            range,
            tags: ctx.tags(DiagnosticCode::LocalVariableUsedBeforeDefinition),
            fixes: Vec::new(),
        });
    }
}

fn first_ident(node: SyntaxNode) -> Option<NamedPosition> {
    let token = node
        .children_with_tokens()
        .filter_map(|element| element.into_token())
        .find(|token| token.kind() == SyntaxKind::IDENT)?;
    let range = token.text_range();
    Some(NamedPosition { name: token.text().fold_lower(), start: range.start().into(), range })
}

fn bare_assignment_target(node: SyntaxNode) -> Option<NamedPosition> {
    let target = expression_core(node.children().next()?);
    if target.kind() != SyntaxKind::IDENT {
        return None;
    }
    let range = target.text_range();
    Some(NamedPosition {
        name: target.text().to_string().fold_lower(),
        start: range.start().into(),
        range,
    })
}

fn expression_core(mut node: SyntaxNode) -> SyntaxNode {
    while node.kind() == SyntaxKind::EXPR {
        let Some(child) = node.children().next() else {
            break;
        };
        node = child;
    }
    node
}

fn inside_excluded_preprocessor(token: &SyntaxToken) -> bool {
    token.parent().is_some_and(|parent| {
        parent.ancestors().any(|ancestor| {
            matches!(ancestor.kind(), SyntaxKind::PRE_DELETE_DIR | SyntaxKind::PRE_IF_DIR)
        })
    })
}

fn is_layout(kind: SyntaxKind) -> bool {
    matches!(kind, SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE | SyntaxKind::COMMENT)
}

fn next_code_index(tokens: &[SyntaxToken], index: usize) -> Option<usize> {
    ((index + 1)..tokens.len()).find(|candidate| !is_layout(tokens[*candidate].kind()))
}

fn prev_code_index(tokens: &[SyntaxToken], index: usize) -> Option<usize> {
    (0..index).rev().find(|candidate| !is_layout(tokens[*candidate].kind()))
}

fn identifier_is_non_value(tokens: &[SyntaxToken], index: usize) -> bool {
    let previous = prev_code_index(tokens, index).map(|i| tokens[i].kind());
    let next = next_code_index(tokens, index).map(|i| tokens[i].kind());

    // Property names, call names, constructor type names, goto/label names.
    previous == Some(SyntaxKind::DOT)
        || previous == Some(SyntaxKind::KW_NEW)
        || previous == Some(SyntaxKind::KW_GOTO)
        || previous == Some(SyntaxKind::TILDE)
        || next == Some(SyntaxKind::L_PAREN)
}

fn collect_flat_insert_assignments(
    insert: &SyntaxNode,
    ranges: &mut HashSet<TextRange>,
    positions: &mut HashMap<String, Vec<u32>>,
) {
    let tokens = insert
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .collect::<Vec<_>>();
    let mut statement_start = true;

    for index in 0..tokens.len() {
        let token = &tokens[index];
        match token.kind() {
            SyntaxKind::WHITESPACE | SyntaxKind::COMMENT => continue,
            SyntaxKind::NEWLINE | SyntaxKind::SEMICOLON => {
                statement_start = true;
                continue;
            }
            SyntaxKind::PRE_INSERT | SyntaxKind::PRE_END_INSERT => continue,
            _ => {}
        }

        if statement_start && token.kind() == SyntaxKind::IDENT {
            let next_same_line = ((index + 1)..tokens.len()).find(|candidate| {
                !matches!(tokens[*candidate].kind(), SyntaxKind::WHITESPACE | SyntaxKind::COMMENT)
            });
            if let Some(next) = next_same_line {
                if tokens[next].kind() == SyntaxKind::EQ {
                    let range = token.text_range();
                    ranges.insert(range);
                    positions
                        .entry(token.text().fold_lower())
                        .or_default()
                        .push(range.start().into());
                }
            }
        }
        statement_start = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagnostics(source: &str) -> Vec<crate::Diagnostic> {
        crate::test_utils::check_with_cfe_config(
            source,
            test_fixture::CfeFixtureBuilder::new("").build(),
            crate::DiagnosticsConfig::default(),
        )
    }

    fn matching(source: &str) -> Vec<crate::Diagnostic> {
        diagnostics(source)
            .into_iter()
            .filter(|diag| diag.code == DiagnosticCode::LocalVariableUsedBeforeDefinition)
            .collect()
    }

    #[test]
    fn reports_extension_insertion_before_loop() {
        let source = r#"
&ИзменениеИКонтроль("ОбработатьТаблицу")
Процедура Расш1_ОбработатьТаблицу(ИсходныеДанные, Таблица)
    Таблица = ПолучитьТаблицу();
    #Вставка
    Строка.Признак = Ложь;
    Строка.ТребуетсяДополнение = Ложь;
    #КонецВставки
    Для Каждого Строка Из Таблица Цикл
        Строка.Признак = Ложь;
    КонецЦикла;
КонецПроцедуры
"#;
        let matching = matching(source);
        assert_eq!(matching.len(), 2, "both inserted pre-loop reads must block: {matching:?}");
        assert!(matching.iter().all(|diag| diag.message.contains("Строка")));
    }

    #[test]
    fn loop_iterator_is_valid_inside_and_after_definition() {
        let source = r#"
Процедура Тест(Коллекция)
    Для Каждого Элемент Из Коллекция Цикл
        Результат = Элемент;
    КонецЦикла;
    После = Элемент;
КонецПроцедуры
"#;
        assert!(matching(source).is_empty());
    }

    #[test]
    fn numeric_for_counter_used_before_loop_is_rejected() {
        let source = r#"
Процедура Тест(Граница)
    До = Счетчик + 1;
    Для Счетчик = 1 По Граница Цикл
        Внутри = Счетчик;
    КонецЦикла;
КонецПроцедуры
"#;
        let matching = matching(source);
        assert_eq!(matching.len(), 1, "numeric For must use the same scope rule: {matching:?}");
    }

    #[test]
    fn prior_assignment_or_explicit_var_prevents_false_positive() {
        for source in [
            r#"
Процедура Тест(Коллекция)
    Элемент = Неопределено;
    До = Элемент;
    Для Каждого Элемент Из Коллекция Цикл
    КонецЦикла;
КонецПроцедуры
"#,
            r#"
Процедура Тест(Коллекция)
    Перем Элемент;
    До = Элемент;
    Для Каждого Элемент Из Коллекция Цикл
    КонецЦикла;
КонецПроцедуры
"#,
        ] {
            assert!(matching(source).is_empty(), "existing owner must be accepted");
        }
    }

    #[test]
    fn assignment_after_read_does_not_retroactively_define_variable() {
        let source = r#"
Процедура Тест(Коллекция)
    До = Элемент;
    Элемент = Неопределено;
    Для Каждого Элемент Из Коллекция Цикл
    КонецЦикла;
КонецПроцедуры
"#;
        assert_eq!(matching(source).len(), 1);
    }

    #[test]
    fn module_variable_or_parameter_prevents_false_positive() {
        for source in [
            r#"
Перем Элемент;
Процедура Тест(Коллекция)
    До = Элемент;
    Для Каждого Элемент Из Коллекция Цикл
    КонецЦикла;
КонецПроцедуры
"#,
            r#"
Процедура Тест(Элемент, Коллекция)
    До = Элемент;
    Для Каждого Элемент Из Коллекция Цикл
    КонецЦикла;
КонецПроцедуры
"#,
        ] {
            assert!(matching(source).is_empty(), "pre-existing owner must win");
        }
    }

    #[test]
    fn property_and_method_names_are_not_loop_variable_reads() {
        let source = r#"
Процедура Тест(Объект, Коллекция)
    Объект.Элемент = 1;
    Элемент();
    Для Каждого Элемент Из Коллекция Цикл
    КонецЦикла;
КонецПроцедуры
"#;
        assert!(matching(source).is_empty());
    }
}
