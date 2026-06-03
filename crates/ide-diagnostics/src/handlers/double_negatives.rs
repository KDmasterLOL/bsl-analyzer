use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use ide_db::TextRange;
use syntax::{SyntaxKind, SyntaxNode};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 3,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload, MetadataTag::Badpractice],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

pub fn check_node(node: &SyntaxNode, acc: &mut Vec<Diagnostic>, ctx: &DiagnosticsContext) {
    let code = DiagnosticCode::DoubleNegatives;

    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    match node.kind() {
        SyntaxKind::UNARY_EXPR => {
            if let Some(range) = check_double_not_simple(node) {
                acc.push(make_diagnostic(code, range, ctx));
            }
            if let Some(range) = check_not_wrapping_neq_simple(node) {
                acc.push(make_diagnostic(code, range, ctx));
            }
        }
        SyntaxKind::BINARY_EXPR => {
            if let Some(range) = check_not_on_left_neq_simple(node) {
                acc.push(make_diagnostic(code, range, ctx));
            }
        }
        _ => {}
    }
}

fn make_diagnostic(code: DiagnosticCode, range: TextRange, ctx: &DiagnosticsContext) -> Diagnostic {
    Diagnostic {
        code,
        message: "Using double negatives complicates understanding of code".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

fn check_double_not_simple(node: &SyntaxNode) -> Option<TextRange> {
    if !has_not_token(node) {
        return None;
    }

    for descendant in node.descendants().skip(1) {
        if descendant.kind() == SyntaxKind::UNARY_EXPR && has_not_token(&descendant) {
            if contains_logical_operators(node) {
                return None;
            }

            let text = node.text().to_string();
            if text.trim_end().ends_with('=') {
                return None;
            }

            return Some(node.text_range());
        }
    }

    None
}

fn check_not_wrapping_neq_simple(node: &SyntaxNode) -> Option<TextRange> {
    if !has_not_token(node) {
        return None;
    }

    for descendant in node.descendants().skip(1) {
        if descendant.kind() == SyntaxKind::BINARY_EXPR && has_neq_token(&descendant) {
            if contains_logical_operators(node) {
                return None;
            }

            return Some(node.text_range());
        }
    }

    None
}

fn check_not_on_left_neq_simple(node: &SyntaxNode) -> Option<TextRange> {
    if !has_neq_token(node) {
        return None;
    }

    for child in node.children() {
        if child.kind() == SyntaxKind::UNARY_EXPR && has_not_token(&child) {
            if contains_logical_operators(node) {
                return None;
            }

            return Some(node.text_range());
        }
    }

    None
}

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::DoubleNegatives;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let parse = ctx.parse();
    let root = parse.syntax_node();
    let mut diagnostics = Vec::new();

    let mut unary_exprs = Vec::new();
    let mut binary_exprs = Vec::new();
    let mut node_info = std::collections::HashMap::new();

    for node in root.descendants() {
        match node.kind() {
            SyntaxKind::UNARY_EXPR => {
                let has_not = has_not_token(&node);
                node_info.insert(node.text_range().start(), (has_not, false));
                unary_exprs.push(node);
            }
            SyntaxKind::BINARY_EXPR => {
                let has_neq = has_neq_token(&node);
                node_info.insert(node.text_range().start(), (has_neq, false));
                binary_exprs.push(node);
            }
            _ => {}
        }
    }

    for node in &unary_exprs {
        if let Some(range) = check_double_not_optimized(node, &unary_exprs, &node_info) {
            diagnostics.push(Diagnostic {
                code,
                message: "Using double negatives complicates understanding of code".to_string(),
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    for node in &unary_exprs {
        if let Some(range) = check_not_wrapping_neq_optimized(node, &binary_exprs, &node_info) {
            diagnostics.push(Diagnostic {
                code,
                message: "Using double negatives complicates understanding of code".to_string(),
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    for node in &binary_exprs {
        if let Some(range) = check_not_on_left_neq_optimized(node, &unary_exprs, &node_info) {
            diagnostics.push(Diagnostic {
                code,
                message: "Using double negatives complicates understanding of code".to_string(),
                severity: ctx.severity(code),
                range,
                tags: ctx.tags(code),
                fixes: vec![],
            });
        }
    }

    diagnostics
}

fn has_not_token(node: &SyntaxNode) -> bool {
    node.children_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| tok.kind() == SyntaxKind::KW_NOT)
}

fn has_neq_token(node: &SyntaxNode) -> bool {
    node.children_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|tok| tok.kind() == SyntaxKind::NEQ)
}

fn contains_logical_operators(node: &SyntaxNode) -> bool {
    for descendant in node.descendants() {
        if descendant
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .any(|tok| matches!(tok.kind(), SyntaxKind::KW_AND | SyntaxKind::KW_OR))
        {
            return true;
        }
    }
    false
}

fn check_double_not_optimized(
    node: &SyntaxNode,
    unary_exprs: &[SyntaxNode],
    node_info: &std::collections::HashMap<syntax::TextSize, (bool, bool)>,
) -> Option<TextRange> {
    let (has_not, _) = node_info.get(&node.text_range().start())?;
    if !has_not {
        return None;
    }

    let node_range = node.text_range();
    for descendant in unary_exprs {
        if descendant.text_range() == node_range {
            continue;
        }

        if !node_range.contains_range(descendant.text_range()) {
            continue;
        }

        if let Some((desc_has_not, _)) = node_info.get(&descendant.text_range().start()) {
            if *desc_has_not {
                if contains_logical_operators(node) {
                    return None;
                }

                let text = node.text().to_string();
                if text.trim_end().ends_with('=') {
                    return None;
                }

                return Some(node_range);
            }
        }
    }

    None
}

fn check_not_wrapping_neq_optimized(
    node: &SyntaxNode,
    binary_exprs: &[SyntaxNode],
    node_info: &std::collections::HashMap<syntax::TextSize, (bool, bool)>,
) -> Option<TextRange> {
    let (has_not, _) = node_info.get(&node.text_range().start())?;
    if !has_not {
        return None;
    }

    let node_range = node.text_range();
    for descendant in binary_exprs {
        if !node_range.contains_range(descendant.text_range()) {
            continue;
        }

        if let Some((desc_has_neq, _)) = node_info.get(&descendant.text_range().start()) {
            if *desc_has_neq {
                if contains_logical_operators(node) {
                    return None;
                }

                return Some(node_range);
            }
        }
    }

    None
}

fn check_not_on_left_neq_optimized(
    node: &SyntaxNode,
    _unary_exprs: &[SyntaxNode],
    node_info: &std::collections::HashMap<syntax::TextSize, (bool, bool)>,
) -> Option<TextRange> {
    let (has_neq, _) = node_info.get(&node.text_range().start())?;
    if !has_neq {
        return None;
    }

    let node_range = node.text_range();
    for child in node.children() {
        if child.kind() != SyntaxKind::UNARY_EXPR {
            continue;
        }

        if let Some((child_has_not, _)) = node_info.get(&child.text_range().start()) {
            if *child_has_not {
                if contains_logical_operators(node) {
                    return None;
                }

                return Some(node_range);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::test_utils::check_ast_diagnostic;
    #[test]
    fn test_no_double_negative() {
        let code = "А = Не Значение;";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_double_not_russian() {
        let code = "Б = Не (Не Значение);";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_not_neq_russian() {
        let code = "А = Не Отказ <> Ложь;";
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_not_wrapping_neq() {
        let code = "А = Не (Отказ <> Ложь);";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_not_equal_not_detected() {
        let code = "А = Не Отказ = Ложь;";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_with_logical_operators_inside() {
        let code = "А = Не (А <> Неопределено и Б = 5);";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_double_not_with_and_inside() {
        let code = "Б = Не (Не Значение И ДругоеЗначение);";
        let diagnostics = check_ast_diagnostic(code, check);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_comprehensive() {
        let code = r#"// Выражение в условии
Если Не ТаблицаЗначений.Найти(ИскомоеЗначение, "Колонка") <> Неопределено Тогда
    // Сделать действие
КонецЕсли;

А = Не Отказ <> Ложь;
А = Не (Отказ <> Ложь);
А = Не НекотороеЗначение() <> Неопределено;
А = Не Неопределено <> НекотороеЗначение();
А = Не (А <> Неопределено); // срабатывает
А = Не А <> Неопределено И Б = 5; // срабатывает
А = Не (А <> Неопределено и Б = 5); // не срабатывает
А = Не (А <> Неопределено или Б = 5); // не срабатывает
А = Не (Б = 5 и А <> Неопределено); // не срабатывает

Пока Не Таблица.Данные <> Неопределено Цикл
КонецЦикла;

Б = Не (Не А = 1 или Б <> Неопределено); // не срабатывает на "Не А = 1"
Б = Не (А <> 1 или Не Б <> Неопределено); // срабатывает на "Не Б <> Неопределено"
Б = Не (А <> 1 или Не Б = Неопределено); // не срабатывает на "Не Б <> Неопределено" т.к. сравнения вида Не Х = Неопределено популярны

Если Не Т.Найти(Значение) = Неопределено Тогда
    // не срабатывает, т.к. популярный код
КонецЕсли;

// Отрицание с проверкой на неравенство нелитералу

А = Не (Отказ <> НеЛитерал); // срабатывает
А = Не СложнаяФункция() <> НеЛитерал; // срабатывает

Б = Не (А = 1 или Б <> НеЛитерал); // не срабатывает

// Прямое двойное отрицание

Б = Не (Не Значение);
Б = Не (Не Значение И ДругоеЗначение); // не срабатывает

// NoSuchElementException
Запись = РегистрыСведений.ЗаданияКПересчетуСтатуса.СоздатьМенеджерЗаписи();
Запись.Записать(Истина);

// C ошибкой разбора
// Вынесено в отдельную процедуру в блоке subAfterCodeBlock, т.к. иначе парсер ломается целиком и expression tree builder
// ничего не строит
Процедура Тест()

    Если Истина Тогда

        // C ошибкой разбора
        Если Тогда

        КонецЕсли;

        // С ошибкой разбора
        Пока А Цикл
            Если
            #Если Сервер Тогда
            F
            #Иначе
            G
            #КонецЕсли
            Тогда
            КонецЕсли;
        КонецЦикла;

    КонецЕсли;

КонецПроцедуры"#;
        let diagnostics = check_ast_diagnostic(code, check);

        assert_eq!(diagnostics.len(), 12, "Ожидается ровно 12 диагностик");
    }
}
