//! Reports module-level variable declarations that have no description comment.

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{ModItem, VariableId};
use syntax::{SyntaxKind, TextRange};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Minor,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Checks top-level module variables for a trailing or header description comment.
///
/// Track 2 §5.2: consumes the structured `VariableDocs` from the
/// SymbolTree-cached `ctx.variable_docs(var_id)` and applies a
/// hyperlink-first / presence / emptiness emission sequence. Hyperlink
/// docs are treated as delegated (parity with `MissingParameterDescription`
/// and `MissingReturnedValueDescription`); whitespace-only doc strings
/// no longer pass as a description.
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::MissingVariablesDescription;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    let tree = ctx.item_tree();
    let parse = ctx.parse();
    let root = parse.syntax_node();
    let module_id = hir::ModuleId::new(ctx.file_id);

    for (idx, item) in tree.top_level_items().iter().enumerate() {
        let ModItem::Variable(var_idx) = item else {
            continue;
        };

        let var = tree.variable(*var_idx);
        let variable_id = VariableId { module: module_id, local_id: idx as u32 };

        let docs = ctx.variable_docs(variable_id);

        // Hyperlink docs are intentionally delegated (`См. Метод()`),
        // never treated as missing.
        if docs.as_ref().is_some_and(|d| d.is_hyperlink()) {
            continue;
        }

        // Both "no docs at all" and "docs with empty/whitespace purpose"
        // collapse to the same diagnostic — no description.
        let has_meaningful_purpose =
            docs.as_ref().and_then(|d| d.purpose.as_deref()).is_some_and(|p| !p.trim().is_empty());

        if has_meaningful_purpose {
            continue;
        }

        // Find the VAR_DEF node only when we actually need to emit, to
        // compute the diagnostic range. The handler's data is otherwise
        // SymbolTree-cached.
        let Some(var_node) = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::VAR_DEF && n.text_range() == var.source_range)
        else {
            continue;
        };

        let diag_range =
            compute_variable_diagnostic_range(&var_node, var.name_range, var.is_export);
        diagnostics.push(create_diagnostic(diag_range, code, ctx));
    }

    diagnostics
}

fn compute_variable_diagnostic_range(
    node: &syntax::SyntaxNode,
    name_range: TextRange,
    is_export: bool,
) -> TextRange {
    if !is_export {
        return name_range;
    }

    for child in node.children_with_tokens() {
        if let Some(token) = child.as_token() {
            if token.kind() == SyntaxKind::KW_EXPORT {
                return TextRange::new(name_range.start(), token.text_range().end());
            }
        }
    }

    name_range
}

fn create_diagnostic(
    range: TextRange,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    Diagnostic {
        code,
        message: "Все объявления переменных должны иметь описание".to_string(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_diagnostics_snapshot_for;
    use crate::DiagnosticCode;
    use expect_test::expect;
    const FIXTURE: &str = r#"
Перем ПеременнаяБезОписания;

Перем ЭкспортнаяПеременнаяБезОписания Экспорт;

Перем ПеременнаяСОписанием; // описание

Перем ЭкспортнаяПеременнаяСОписанием Экспорт; // описание

// описание
Перем ПеременнаяСОписаниемВыше;

// описание
Перем ЭкспортнаяПеременнаяСОписаниемВыше Экспорт;

 // неточное описание

Перем ПеременнаяСНевернымОписаниемВыше;

 // неточное описание

Перем ЭкспортнаяПеременнаяСНевернымОписаниемВыше Экспорт;

&Идентификатор
&ГенерируемоеЗначение
&Колонка(Тип = "Целое")
Перем ПеременнаяСАннотациямиOS Экспорт; // Внутренний идентификатор объекта

// Описание какое-то в шапке
&Идентификатор
&ГенерируемоеЗначение
&Колонка(Тип = "НеЦелое")
Перем ПеременнаяСАннотациямиOS Экспорт; // Висячий комментарий

&Идентификатор
&ГенерируемоеЗначение
&Колонка(Тип = "Вместилище")
Перем ПеременнаяСАннотациямиOSБезОписания Экспорт;

&Идентификатор
&ГенерируемоеЗначение
Перем ПеременнаяСАннотациямиСОписаниемВСтроке Экспорт; // это описание

// это описание переменной в шапке
&Идентификатор
&ГенерируемоеЗначение
Перем ПеременнаяСАннотациямиСОписаниемВШапке Экспорт;

&Идентификатор
// это описание переменной между аннотациями, но тоже подойдет
&ГенерируемоеЗначение
Перем ПеременнаяСАннотациямиСОписаниемВШапке Экспорт;

&Идентификатор
&ГенерируемоеЗначение
// это описание переменной под аннотациями
Перем ПеременнаяСАннотациямиСОписаниемВШапке Экспорт;

// это описание переменной в шапке и будет использовано именно оно
&Идентификатор
// это описание переменной между аннотациями использовано не будет
&ГенерируемоеЗначение
// это описание переменной под аннотациями и так как есть в шапке, будем игнорировать
Перем ПеременнаяСАннотациямиСОписаниемВШапке;
"#;

    #[test]
    fn test_java_fixture_compatibility() {
        check_diagnostics_snapshot_for(
            FIXTURE,
            DiagnosticCode::MissingVariablesDescription,
            expect![[r#"
                MissingVariablesDescription @ 2:7..2:28
                  message: Все объявления переменных должны иметь описание
                  severity: Information
                MissingVariablesDescription @ 4:7..4:46
                  message: Все объявления переменных должны иметь описание
                  severity: Information
                MissingVariablesDescription @ 18:7..18:39
                  message: Все объявления переменных должны иметь описание
                  severity: Information
                MissingVariablesDescription @ 22:7..22:57
                  message: Все объявления переменных должны иметь описание
                  severity: Information
                MissingVariablesDescription @ 38:7..38:50
                  message: Все объявления переменных должны иметь описание
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_trailing_comment() {
        let code = "Перем Переменная; // описание";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingVariablesDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_no_description() {
        let code = "Перем Переменная;";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingVariablesDescription,
            expect![[r#"
                MissingVariablesDescription @ 1:7..1:17
                  message: Все объявления переменных должны иметь описание
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_leading_comment_direct() {
        let code = "// описание\nПерем Переменная;";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingVariablesDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_leading_comment_with_empty_line() {
        let code = "// описание\n\nПерем Переменная;";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingVariablesDescription,
            expect![[r#"
                MissingVariablesDescription @ 3:7..3:17
                  message: Все объявления переменных должны иметь описание
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_annotated_variable_with_trailing_comment() {
        let code = "&НаКлиенте\nПерем Переменная; // описание";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingVariablesDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_annotated_variable_with_header_comment() {
        let code = "// описание\n&НаКлиенте\nПерем Переменная;";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingVariablesDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_annotated_variable_with_comment_between() {
        let code = "&НаКлиенте\n// описание\n&НаСервере\nПерем Переменная;";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingVariablesDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_annotated_variable_with_comment_below() {
        let code = "&НаКлиенте\n// описание\nПерем Переменная;";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingVariablesDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_annotated_variable_no_description() {
        let code = "&НаКлиенте\n&НаСервере\nПерем Переменная;";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingVariablesDescription,
            expect![[r#"
                MissingVariablesDescription @ 3:7..3:17
                  message: Все объявления переменных должны иметь описание
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_local_variable_not_checked() {
        let code = "Процедура Тест()\n    Перем Локальная;\nКонецПроцедуры";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingVariablesDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_multiple_variables() {
        let code = "Перем А;\nПерем Б; // описание\n// описание\nПерем В;";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingVariablesDescription,
            expect![[r#"
                MissingVariablesDescription @ 1:7..1:8
                  message: Все объявления переменных должны иметь описание
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_whitespace_only_comment_emits() {
        // Track 2 §5.2 audit gap: an isolated `//` with no text was
        // accepted by the legacy binary helper. The structured parser
        // returns `purpose: None`, so the handler now emits.
        let code = "//\nПерем Переменная;";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingVariablesDescription,
            expect![[r#"
                MissingVariablesDescription @ 2:7..2:17
                  message: Все объявления переменных должны иметь описание
                  severity: Information"#]],
        );
    }

    #[test]
    fn test_hyperlink_only_doc_is_accepted() {
        // `См.` delegated docs are intentionally not flagged — parity
        // with `MissingParameterDescription` and
        // `MissingReturnedValueDescription` hyperlink handling.
        let code = "// См. ОбщегоНазначения.СомеVariable\nПерем Переменная;";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingVariablesDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_empty_marker_then_hyperlink_is_accepted() {
        // Codex round-B Q5 NIT: an isolated `//` ahead of the hyperlink
        // is filtered by the extractor, so the parser still sees just
        // the hyperlink and the handler skips. Documents the boundary
        // between whitespace-only filtering and hyperlink delegation.
        let code = "//\n// См. ОбщегоНазначения.Имя\nПерем Переменная;";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingVariablesDescription,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_duplicate_name_variables_each_get_own_diagnostic() {
        // Codex round-B Q1 NIT: SymbolTree now stores every variable
        // declaration in the arena (legacy code skipped duplicate-name
        // entries entirely). Each duplicate carries its own
        // `VariableId`, so `ctx.variable_docs(id)` resolves correctly
        // per declaration and every undocumented duplicate emits.
        let code = "Перем Дубликат;\nПерем Дубликат;";
        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::MissingVariablesDescription,
            expect![[r#"
                MissingVariablesDescription @ 1:7..1:15
                  message: Все объявления переменных должны иметь описание
                  severity: Information
                MissingVariablesDescription @ 2:7..2:15
                  message: Все объявления переменных должны иметь описание
                  severity: Information"#]],
        );
    }
}
