//! Code templates (snippets) and curated keyword completions for the unqualified
//! ("code console") position.
//!
//! Mirrors RDT1C's `Конструкция` entries: control-flow / structural skeletons
//! that expand to a full block. Templates carry LSP snippet syntax (`${1:..}`
//! tab stops, `$0` final cursor) and are ranked in the same stream as ordinary
//! identifiers, keyed by how the typed text hits their *trigger* word.

use super::fuzzy::{MatchResult, PrefixMatcher};
use super::{CompletionItem, CompletionItemKind};

/// Where a template/keyword is offered. Gating is purely syntactic (no `db`):
/// inside a method body vs. at module level (between methods).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Gate {
    /// Statement position inside a method body (loops, conditionals, …).
    Statement,
    /// Module level, between methods (procedure/function/region skeletons).
    ModuleLevel,
    /// Offered in either context.
    Any,
}

impl Gate {
    fn allows(self, in_method: bool) -> bool {
        match self {
            Gate::Statement => in_method,
            Gate::ModuleLevel => !in_method,
            Gate::Any => true,
        }
    }
}

struct Template {
    /// Russian trigger word matched against the typed prefix.
    trigger_ru: &'static str,
    /// English trigger word.
    trigger_en: &'static str,
    /// Display label (Russian).
    label_ru: &'static str,
    /// Display label (English).
    label_en: &'static str,
    /// LSP snippet body, Russian keywords.
    snippet_ru: &'static str,
    /// LSP snippet body, English keywords.
    snippet_en: &'static str,
    gate: Gate,
}

struct Keyword {
    ru: &'static str,
    en: &'static str,
    gate: Gate,
}

/// The template registry. Russian and English variants are emitted as separate
/// items; the one whose trigger the user is actually typing wins on tier.
const TEMPLATES: &[Template] = &[
    Template {
        trigger_ru: "Если",
        trigger_en: "If",
        label_ru: "Если … Тогда … КонецЕсли",
        label_en: "If … Then … EndIf",
        snippet_ru: "Если ${1:Условие} Тогда\n\t$0\nКонецЕсли;",
        snippet_en: "If ${1:Condition} Then\n\t$0\nEndIf;",
        gate: Gate::Statement,
    },
    Template {
        trigger_ru: "Если",
        trigger_en: "If",
        label_ru: "Если ЗначениеЗаполнено(…) Тогда",
        label_en: "If ValueIsFilled(…) Then",
        snippet_ru: "Если ЗначениеЗаполнено(${1:Значение}) Тогда\n\t$0\nКонецЕсли;",
        snippet_en: "If ValueIsFilled(${1:Value}) Then\n\t$0\nEndIf;",
        gate: Gate::Statement,
    },
    Template {
        trigger_ru: "Если",
        trigger_en: "If",
        label_ru: "Если … = Неопределено Тогда",
        label_en: "If … = Undefined Then",
        snippet_ru: "Если ${1:Значение} = Неопределено Тогда\n\t$0\nКонецЕсли;",
        snippet_en: "If ${1:Value} = Undefined Then\n\t$0\nEndIf;",
        gate: Gate::Statement,
    },
    Template {
        trigger_ru: "ИначеЕсли",
        trigger_en: "ElsIf",
        label_ru: "ИначеЕсли … Тогда",
        label_en: "ElsIf … Then",
        snippet_ru: "ИначеЕсли ${1:Условие} Тогда\n\t$0",
        snippet_en: "ElsIf ${1:Condition} Then\n\t$0",
        gate: Gate::Statement,
    },
    Template {
        trigger_ru: "ДляКаждого",
        trigger_en: "ForEach",
        label_ru: "Для Каждого … Из … Цикл",
        label_en: "For Each … In … Do",
        snippet_ru: "Для Каждого ${1:Элемент} Из ${2:Коллекция} Цикл\n\t$0\nКонецЦикла;",
        snippet_en: "For Each ${1:Item} In ${2:Collection} Do\n\t$0\nEndDo;",
        gate: Gate::Statement,
    },
    Template {
        trigger_ru: "Для",
        trigger_en: "For",
        label_ru: "Для … = … По … Цикл",
        label_en: "For … = … To … Do",
        snippet_ru: "Для ${1:Сч} = ${2:1} По ${3:Граница} Цикл\n\t$0\nКонецЦикла;",
        snippet_en: "For ${1:Index} = ${2:1} To ${3:Bound} Do\n\t$0\nEndDo;",
        gate: Gate::Statement,
    },
    Template {
        trigger_ru: "Пока",
        trigger_en: "While",
        label_ru: "Пока … Цикл",
        label_en: "While … Do",
        snippet_ru: "Пока ${1:Условие} Цикл\n\t$0\nКонецЦикла;",
        snippet_en: "While ${1:Condition} Do\n\t$0\nEndDo;",
        gate: Gate::Statement,
    },
    Template {
        trigger_ru: "Попытка",
        trigger_en: "Try",
        label_ru: "Попытка … Исключение … КонецПопытки",
        label_en: "Try … Except … EndTry",
        snippet_ru: "Попытка\n\t$0\nИсключение\n\t${1:ВызватьИсключение;}\nКонецПопытки;",
        snippet_en: "Try\n\t$0\nExcept\n\t${1:Raise;}\nEndTry;",
        gate: Gate::Statement,
    },
    Template {
        trigger_ru: "НачатьТранзакцию",
        trigger_en: "BeginTransaction",
        label_ru: "НачатьТранзакцию … Попытка … КонецПопытки",
        label_en: "BeginTransaction … Try … EndTry",
        snippet_ru: "НачатьТранзакцию();\nПопытка\n\t$0\n\tЗафиксироватьТранзакцию();\nИсключение\n\tОтменитьТранзакцию();\n\tВызватьИсключение;\nКонецПопытки;",
        snippet_en: "BeginTransaction();\nTry\n\t$0\n\tCommitTransaction();\nExcept\n\tRollbackTransaction();\n\tRaise;\nEndTry;",
        gate: Gate::Statement,
    },
    Template {
        trigger_ru: "Запрос",
        trigger_en: "Query",
        label_ru: "Запрос = Новый Запрос … Выполнить()",
        label_en: "Query = New Query … Execute()",
        snippet_ru: "${1:Запрос} = Новый Запрос;\n${1:Запрос}.Текст =\n\t\"$0\";\n${2:РезультатЗапроса} = ${1:Запрос}.Выполнить();",
        snippet_en: "${1:Query} = New Query;\n${1:Query}.Text =\n\t\"$0\";\n${2:QueryResult} = ${1:Query}.Execute();",
        gate: Gate::Statement,
    },
    Template {
        trigger_ru: "Процедура",
        trigger_en: "Procedure",
        label_ru: "Процедура … КонецПроцедуры",
        label_en: "Procedure … EndProcedure",
        snippet_ru: "Процедура ${1:ИмяПроцедуры}(${2:Параметры})\n\t$0\nКонецПроцедуры",
        snippet_en: "Procedure ${1:Name}(${2:Params})\n\t$0\nEndProcedure",
        gate: Gate::ModuleLevel,
    },
    Template {
        trigger_ru: "Функция",
        trigger_en: "Function",
        label_ru: "Функция … КонецФункции",
        label_en: "Function … EndFunction",
        snippet_ru: "Функция ${1:ИмяФункции}(${2:Параметры})\n\t$0\n\tВозврат ${3:Результат};\nКонецФункции",
        snippet_en: "Function ${1:Name}(${2:Params})\n\t$0\n\tReturn ${3:Result};\nEndFunction",
        gate: Gate::ModuleLevel,
    },
    Template {
        trigger_ru: "Область",
        trigger_en: "Region",
        label_ru: "#Область … #КонецОбласти",
        label_en: "#Region … #EndRegion",
        snippet_ru: "#Область ${1:ИмяОбласти}\n\n$0\n\n#КонецОбласти",
        snippet_en: "#Region ${1:Name}\n\n$0\n\n#EndRegion",
        gate: Gate::ModuleLevel,
    },
    Template {
        trigger_ru: "Если",
        trigger_en: "If",
        label_ru: "#Если … #КонецЕсли",
        label_en: "#If … #EndIf",
        snippet_ru: "#Если ${1:Сервер} Тогда\n\t$0\n#КонецЕсли",
        snippet_en: "#If ${1:Server} Then\n\t$0\n#EndIf",
        gate: Gate::Any,
    },
];

/// Curated standalone keywords worth suggesting (RDT1C also seeds the word list
/// with keywords). Control-flow block keywords are covered by [`TEMPLATES`].
const KEYWORDS: &[Keyword] = &[
    Keyword { ru: "Возврат", en: "Return", gate: Gate::Statement },
    Keyword { ru: "Прервать", en: "Break", gate: Gate::Statement },
    Keyword { ru: "Продолжить", en: "Continue", gate: Gate::Statement },
    Keyword { ru: "ВызватьИсключение", en: "Raise", gate: Gate::Statement },
    Keyword { ru: "Новый", en: "New", gate: Gate::Any },
    Keyword { ru: "Истина", en: "True", gate: Gate::Any },
    Keyword { ru: "Ложь", en: "False", gate: Gate::Any },
    Keyword { ru: "Неопределено", en: "Undefined", gate: Gate::Any },
    Keyword { ru: "Не", en: "Not", gate: Gate::Any },
];

/// A completion item paired with the quality of how the typed text hit its
/// trigger, so the caller can fold it into the shared sort key.
pub(super) struct Scored {
    pub item: CompletionItem,
    pub result: MatchResult,
}

/// Build template and keyword completions for the given typed prefix and
/// context. `in_method` is true when the cursor sits inside a procedure/function
/// body. `indent`, when `Some((base_indent, indent_unit))`, bakes the cursor's
/// indentation into snippet continuation lines so they land correctly even in
/// clients that don't re-indent multi-line completion snippets (see
/// [`reindent_snippet`]); when `None` the raw relative-tab snippet is emitted for
/// clients that re-indent completions themselves.
pub(super) fn complete_templates(
    matcher: &mut PrefixMatcher,
    in_method: bool,
    indent: Option<(&str, &str)>,
) -> Vec<Scored> {
    let mut out = Vec::new();

    for tmpl in TEMPLATES {
        if !tmpl.gate.allows(in_method) {
            continue;
        }
        // Russian variant.
        if let Some(result) = matcher.score(tmpl.trigger_ru) {
            out.push(Scored {
                item: template_item(
                    tmpl.label_ru,
                    tmpl.snippet_ru,
                    tmpl.trigger_ru,
                    tmpl.trigger_en,
                    indent,
                ),
                result,
            });
        }
        // English variant, only when the English trigger is what matched (avoids
        // a duplicate English row for every Russian-typed prefix).
        if matcher.is_empty() {
            continue;
        }
        if let Some(result) = matcher.score(tmpl.trigger_en) {
            out.push(Scored {
                item: template_item(
                    tmpl.label_en,
                    tmpl.snippet_en,
                    tmpl.trigger_ru,
                    tmpl.trigger_en,
                    indent,
                ),
                result,
            });
        }
    }

    for kw in KEYWORDS {
        if !kw.gate.allows(in_method) {
            continue;
        }
        if let Some(result) = matcher.score(kw.ru) {
            out.push(Scored { item: keyword_item(kw.ru, kw.en), result });
        }
        if matcher.is_empty() {
            continue;
        }
        if let Some(result) = matcher.score(kw.en) {
            out.push(Scored { item: keyword_item(kw.en, kw.ru), result });
        }
    }

    out
}

/// Re-render a snippet body so every continuation line carries the cursor line's
/// indentation. Lines are authored with leading tabs standing for nesting levels;
/// each level becomes `base_indent` (the column the snippet starts at) plus one
/// `indent_unit` per level. The first line is left untouched — the cursor already
/// sits at its indent in the document. Blank lines stay blank to avoid trailing
/// whitespace. This makes the snippet self-indenting regardless of whether the
/// LSP client re-indents multi-line completion snippets (Zed does not).
fn reindent_snippet(snippet: &str, base_indent: &str, indent_unit: &str) -> String {
    let mut out = String::with_capacity(snippet.len());
    for (i, line) in snippet.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if i == 0 {
            out.push_str(line);
            continue;
        }
        let levels = line.bytes().take_while(|b| *b == b'\t').count();
        let rest = &line[levels..];
        if rest.is_empty() {
            continue;
        }
        out.push_str(base_indent);
        for _ in 0..levels {
            out.push_str(indent_unit);
        }
        out.push_str(rest);
    }
    out
}

fn template_item(
    label: &str,
    snippet: &str,
    trigger_ru: &str,
    trigger_en: &str,
    indent: Option<(&str, &str)>,
) -> CompletionItem {
    let insert_text = match indent {
        Some((base_indent, indent_unit)) => reindent_snippet(snippet, base_indent, indent_unit),
        None => snippet.to_string(),
    };
    CompletionItem {
        label: label.to_string(),
        detail: Some("Шаблон кода".to_string()),
        kind: CompletionItemKind::Snippet,
        insert_text,
        documentation: None,
        sort_text: None,
        // Let the client filter by either trigger word regardless of which
        // language variant this row renders.
        filter_text: Some(format!("{trigger_ru} {trigger_en}")),
        source: None,
    }
}

fn keyword_item(text: &str, alias: &str) -> CompletionItem {
    CompletionItem {
        label: text.to_string(),
        detail: Some("Ключевое слово".to_string()),
        kind: CompletionItemKind::Keyword,
        insert_text: text.to_string(),
        documentation: None,
        sort_text: None,
        filter_text: Some(format!("{text} {alias}")),
        source: None,
    }
}

#[cfg(test)]
mod tests {
    use super::reindent_snippet;

    #[test]
    fn first_line_is_left_untouched() {
        // The cursor already sits at its indentation; only continuation lines get
        // the base indent prepended.
        let got = reindent_snippet("Если ${1:Условие} Тогда\n\t$0\nКонецЕсли;", "\t\t", "\t");
        assert_eq!(got, "Если ${1:Условие} Тогда\n\t\t\t$0\n\t\tКонецЕсли;");
    }

    #[test]
    fn multi_level_transaction_block_indents_each_line() {
        let snippet =
            "НачатьТранзакцию();\nПопытка\n\t$0\n\tЗафиксироватьТранзакцию();\nИсключение\n\tОтменитьТранзакцию();\n\tВызватьИсключение;\nКонецПопытки;";
        let got = reindent_snippet(snippet, "    ", "    ");
        assert_eq!(
            got,
            "НачатьТранзакцию();\n    Попытка\n        $0\n        ЗафиксироватьТранзакцию();\n    Исключение\n        ОтменитьТранзакцию();\n        ВызватьИсключение;\n    КонецПопытки;"
        );
    }

    #[test]
    fn blank_lines_stay_blank_without_trailing_whitespace() {
        let got = reindent_snippet("#Область ${1:Имя}\n\n$0\n\n#КонецОбласти", "\t", "\t");
        assert_eq!(got, "#Область ${1:Имя}\n\n\t$0\n\n\t#КонецОбласти");
    }

    #[test]
    fn empty_base_indent_keeps_relative_nesting() {
        let got = reindent_snippet("Пока ${1:Условие} Цикл\n\t$0\nКонецЦикла;", "", "\t");
        assert_eq!(got, "Пока ${1:Условие} Цикл\n\t$0\nКонецЦикла;");
    }

    #[test]
    fn placeholder_default_after_indent_is_preserved() {
        // Indentation tabs precede the placeholder; the `${1:..}` payload is part
        // of `rest` and must survive untouched. `Исключение`/`КонецПопытки` are
        // level-0 continuation lines: they take the base indent so they align
        // with the first line sitting at the cursor column.
        let got = reindent_snippet(
            "Попытка\n\t$0\nИсключение\n\t${1:ВызватьИсключение;}\nКонецПопытки;",
            "\t",
            "  ",
        );
        assert_eq!(
            got,
            "Попытка\n\t  $0\n\tИсключение\n\t  ${1:ВызватьИсключение;}\n\tКонецПопытки;"
        );
    }
}
