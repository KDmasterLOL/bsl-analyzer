use crate::define_metadata;
use crate::metadata::*;
use crate::{BodyContext, Diagnostic, DiagnosticCode};
use hir::LocalRange;
use ide_db::TextRange;
use syntax::{SyntaxKind, SyntaxNode};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Intentional,
};

const MESSAGE: &str = "Инструкция препроцессора разорвана переводом строки";

/// Проверка живёт здесь, а не в грамматике, потому что разрыв не мешает
/// разобрать конструкцию: платформа её отвергает, но синтаксически она
/// однозначна. Грамматика о переводе строки внутри инструкции не ветвится
/// вовсе, и норма на это счёт объявлена в
/// `docs/architecture/adr/ADR-02-line-sensitivity.md`.
pub fn check_body(ctx: &BodyContext, acc: &mut Vec<Diagnostic<LocalRange>>) {
    let _span = tracing::debug_span!("MultilinePreprocessorInstruction::check").entered();

    let code = DiagnosticCode::MultilinePreprocessorInstruction;

    if ctx.is_disabled_with_metadata(code) {
        return;
    }

    let line_index = ctx.line_index();
    let mut diagnostics = Vec::new();

    for node in ctx.nodes() {
        let header = match node.kind() {
            SyntaxKind::PRE_IF_DIR | SyntaxKind::PRE_ELSIF_CLAUSE => instruction_header(&node),
            SyntaxKind::PRE_REGION_DIR => region_header(&node),
            _ => continue,
        };

        let Some(header) = header else {
            continue;
        };

        if line_index.line_col(header.start()).line == line_index.line_col(header.end()).line {
            continue;
        }

        diagnostics.push(Diagnostic {
            code,
            message: MESSAGE.to_string(),
            severity: ctx.severity(code),
            range: LocalRange::of_detached_node(header),
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    tracing::debug!(
        count = diagnostics.len(),
        "MultilinePreprocessorInstruction diagnostics found"
    );

    acc.extend(diagnostics);
}

/// Диапазон самой инструкции — от её слова до закрывающего `Тогда`.
///
/// Тело `#Если` лежит в том же узле, и без этой границы любой перевод строки
/// внутри тела читался бы как разрыв инструкции.
///
/// Инструкция без `Тогда` пропускается: она уже сломана, и об этом сообщает
/// разбор. Второе сообщение о том же месте пользы не несёт.
///
/// Разрыв опознаётся по номерам строк концов этого диапазона, а не обходом
/// токенов внутри него: тело инструкции лежит в том же узле, и обход поддерева
/// у каждого заголовка стоил бы квадрата на вложенных директивах.
fn instruction_header(node: &SyntaxNode) -> Option<TextRange> {
    let start = node.first_token()?.text_range().start();
    let then = node
        .children_with_tokens()
        .filter_map(|child| child.into_token())
        .find(|token| token.kind() == SyntaxKind::KW_THEN)?;

    Some(TextRange::new(start, then.text_range().end()))
}

/// Диапазон `#Область` вместе с именем, унесённым на следующую строку.
///
/// Имя за переводом строки областью не берётся — иначе директива утащила бы
/// первое слово следующего оператора, — поэтому такое имя остаётся отдельным
/// словом, и разбор сообщает о нём как о неверном операторе. Сообщение верное,
/// но о следствии; причину называет эта проверка.
///
/// Форма узнаётся по трём различающим признакам, у каждого свой вход: имя у
/// директивы отсутствует (иначе унесённого имени нет), следом стоит ровно одно
/// значимое слово (иначе это оператор), и точки с запятой за ним нет (иначе
/// писали вызов без скобок, а не имя). Про область, у которой имени нет вовсе,
/// здесь не утверждается ничего.
fn region_header(node: &SyntaxNode) -> Option<TextRange> {
    let opener = node
        .children_with_tokens()
        .filter_map(|child| child.into_token())
        .find(|token| token.kind() == SyntaxKind::PRE_REGION)?;

    let named = node
        .children_with_tokens()
        .filter_map(|child| child.into_token())
        .any(|token| token.kind() == SyntaxKind::IDENT);
    if named {
        return None;
    }

    let orphan = node.next_sibling()?;
    let mut significant = orphan
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| !token.kind().is_trivia());

    let name = significant.next()?;
    if name.kind() != SyntaxKind::IDENT || significant.next().is_some() {
        return None;
    }

    // Точка с запятой стоит вне узла, поэтому одинокое слово и вызов без
    // скобок доходят сюда одинаковыми. Их различает она сама: у имени области
    // её не бывает, а поставивший её писал оператор.
    //
    // Смотреть надо ровно на следующий элемент, а не на следующий ТОКЕН: узлы
    // пропускать нельзя. Присваивание и вызов тоже выносят свою точку с
    // запятой наружу, и обход, перескакивающий их узлы, принимал бы её за
    // свою — унесённое имя, за которым идёт обычный оператор, тогда глохнет.
    let terminated = orphan
        .siblings_with_tokens(syntax::Direction::Next)
        .skip(1)
        .find(|element| !element.as_token().is_some_and(|token| token.kind().is_trivia()))
        .is_some_and(|element| element.kind() == SyntaxKind::SEMICOLON);
    if terminated {
        return None;
    }

    Some(TextRange::new(opener.text_range().start(), name.text_range().end()))
}

#[cfg(test)]
mod tests {
    use super::check_body;
    use crate::test_utils::{check_body_diagnostic, format_diags};
    use expect_test::expect;

    /// Контроль: то же самое в одну строку молчит. Без него утверждения ниже
    /// зелены и у проверки, которая ругается на любую инструкцию.
    #[test]
    fn an_instruction_on_one_line_is_silent() {
        let code = "#Если Сервер И Клиент Тогда\n#ИначеЕсли Клиент Тогда\n#КонецЕсли\n";
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn a_condition_carried_to_the_next_line_is_reported() {
        let code = "#Если\nСервер Тогда\n#КонецЕсли\n";
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#"
            MultilinePreprocessorInstruction @ 1:1..2:13
              message: Инструкция препроцессора разорвана переводом строки
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn an_operand_carried_to_the_next_line_is_reported() {
        let code = "#Если Сервер\nИ Клиент Тогда\n#КонецЕсли\n";
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#"
            MultilinePreprocessorInstruction @ 1:1..2:15
              message: Инструкция препроцессора разорвана переводом строки
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn a_then_carried_to_the_next_line_is_reported() {
        let code = "#Если Сервер\nТогда\n#КонецЕсли\n";
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#"
            MultilinePreprocessorInstruction @ 1:1..2:6
              message: Инструкция препроцессора разорвана переводом строки
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn an_elsif_carried_to_the_next_line_is_reported() {
        let code = "#Если Сервер Тогда\n#ИначеЕсли\nКлиент Тогда\n#КонецЕсли\n";
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#"
            MultilinePreprocessorInstruction @ 2:1..3:13
              message: Инструкция препроцессора разорвана переводом строки
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    /// Тело инструкции переводами строки полно по построению, и граница
    /// заголовка — единственное, что отделяет их от разрыва самой инструкции.
    #[test]
    fn line_breaks_in_the_body_are_not_a_break_of_the_instruction() {
        let code = "#Если Сервер Тогда\n\tА = 1;\n\tБ = 2;\n#КонецЕсли\n";
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn a_region_name_carried_to_the_next_line_is_reported() {
        let code = "#Область\nСлужебные\n#КонецОбласти\n";
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#"
            MultilinePreprocessorInstruction @ 1:1..2:10
              message: Инструкция препроцессора разорвана переводом строки
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn a_region_name_on_its_own_line_is_silent() {
        let code = "#Область Служебные\n#КонецОбласти\n";
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    /// Обычный код после безымянной области именем не является, и принять его
    /// за унесённое имя проверка не должна.
    #[test]
    fn a_statement_after_a_nameless_region_is_not_a_carried_name() {
        let code = "#Область\nА = 1;\n#КонецОбласти\n";
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    /// Точка с запятой оператора, идущего ЗА унесённым именем, к имени
    /// отношения не имеет: у присваивания и вызова она лежит вне узла, и обход,
    /// пропускающий узлы, принимал её за свою.
    #[test]
    fn a_semicolon_of_the_next_statement_does_not_belong_to_the_carried_name() {
        let code = "#Область\nСлужебные\nА = 1;\n#КонецОбласти\n";
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#"
            MultilinePreprocessorInstruction @ 1:1..2:10
              message: Инструкция препроцессора разорвана переводом строки
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    #[test]
    fn the_same_holds_inside_a_procedure() {
        let code = "Процедура П()\n#Область\nСлужебные\nМетод();\n#КонецОбласти\nКонецПроцедуры\n";
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#"
            MultilinePreprocessorInstruction @ 2:1..3:10
              message: Инструкция препроцессора разорвана переводом строки
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    /// Самый коварный представитель разряда: вызов без скобок за унесённым
    /// именем даёт узел той же формы, что и само имя, и его точку с запятой
    /// легко принять за принадлежащую имени. Отличается он тем, что стоит
    /// ПОСЛЕ имени, а не на его месте — сравни с
    /// `a_terminated_word_after_a_nameless_region_is_not_a_carried_name`.
    #[test]
    fn a_parenless_call_after_the_carried_name_does_not_silence_it() {
        let code = "#Область\nСлужебные\nМетод;\n#КонецОбласти\n";
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#"
            MultilinePreprocessorInstruction @ 1:1..2:10
              message: Инструкция препроцессора разорвана переводом строки
              severity: Major"#]]
        .assert_eq(&format_diags(code, &diagnostics));
    }

    /// Оператор без завершающей точки с запятой: её проверка тут не спасает,
    /// и различает вход только счёт слов.
    #[test]
    fn an_unterminated_statement_after_a_nameless_region_is_not_a_carried_name() {
        let code = "#Область\nА = 1\n#КонецОбласти\n";
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    /// Вызов без скобок даёт ту же форму, что унесённое имя: точка с запятой
    /// лежит вне узла ошибки. Различает их она сама.
    #[test]
    fn a_terminated_word_after_a_nameless_region_is_not_a_carried_name() {
        let code = "#Область\nМетод;\n#КонецОбласти\n";
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    /// У области с именем унесённого имени быть не может, что бы ни стояло
    /// следующей строкой.
    #[test]
    fn a_stray_word_after_a_named_region_is_not_a_carried_name() {
        let code = "#Область Имя\nСлужебные\n#КонецОбласти\n";
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }

    /// Про область, у которой имени нет вовсе, проверка не утверждает ничего:
    /// наблюдения о том, отвергает ли её платформа, у нас нет.
    #[test]
    fn a_region_with_no_name_at_all_is_silent() {
        let code = "#Область\n#КонецОбласти\n";
        let diagnostics = check_body_diagnostic(code, check_body);
        expect![[r#""#]].assert_eq(&format_diags(code, &diagnostics));
    }
}
