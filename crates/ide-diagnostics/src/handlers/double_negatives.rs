use crate::define_metadata;
use crate::metadata::*;
use crate::{BodyContext, Diagnostic, DiagnosticCode};
use hir::LocalRange;
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

pub fn check_node(node: &SyntaxNode, acc: &mut Vec<Diagnostic<LocalRange>>, ctx: &BodyContext) {
    let code = DiagnosticCode::DoubleNegatives;

    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    match node.kind() {
        SyntaxKind::UNARY_EXPR => {
            if let Some(range) = check_double_not_simple(node) {
                acc.push(make_diagnostic(code, LocalRange::of_detached_node(range), ctx));
            }
            if let Some(range) = check_not_wrapping_neq_simple(node) {
                acc.push(make_diagnostic(code, LocalRange::of_detached_node(range), ctx));
            }
        }
        SyntaxKind::BINARY_EXPR => {
            if let Some(range) = check_not_on_left_neq_simple(node) {
                acc.push(make_diagnostic(code, LocalRange::of_detached_node(range), ctx));
            }
        }
        _ => {}
    }
}

fn make_diagnostic(
    code: DiagnosticCode,
    range: LocalRange,
    ctx: &BodyContext,
) -> Diagnostic<LocalRange> {
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

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_for;
    use crate::DiagnosticCode;
    #[test]
    fn test_no_double_negative() {
        let code = "А = Не Значение;";
        let diagnostics = check_diagnostics_for(code, DiagnosticCode::DoubleNegatives);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_double_not_russian() {
        let code = "Б = Не (Не Значение);";
        let diagnostics = check_diagnostics_for(code, DiagnosticCode::DoubleNegatives);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_not_neq_russian() {
        let code = "А = Не Отказ <> Ложь;";
        let diagnostics = check_diagnostics_for(code, DiagnosticCode::DoubleNegatives);

        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_not_wrapping_neq() {
        let code = "А = Не (Отказ <> Ложь);";
        let diagnostics = check_diagnostics_for(code, DiagnosticCode::DoubleNegatives);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_not_equal_not_detected() {
        let code = "А = Не Отказ = Ложь;";
        let diagnostics = check_diagnostics_for(code, DiagnosticCode::DoubleNegatives);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_with_logical_operators_inside() {
        let code = "А = Не (А <> Неопределено и Б = 5);";
        let diagnostics = check_diagnostics_for(code, DiagnosticCode::DoubleNegatives);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_double_not_with_and_inside() {
        let code = "Б = Не (Не Значение И ДругоеЗначение);";
        let diagnostics = check_diagnostics_for(code, DiagnosticCode::DoubleNegatives);
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
        let diagnostics = check_diagnostics_for(code, DiagnosticCode::DoubleNegatives);

        assert_eq!(diagnostics.len(), 12, "Ожидается ровно 12 диагностик");
    }
}
