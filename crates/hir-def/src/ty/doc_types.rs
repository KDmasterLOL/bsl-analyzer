use std::borrow::Cow;

use crate::path::QualifiedName;
use crate::type_ref::{BuiltinTypeRef, TypeRef};
use crate::Name;
use stdx::case::CaseExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodTypeHints {
    pub params: Vec<(Name, TypeRef)>,

    pub ret: TypeRef,
}

impl Default for MethodTypeHints {
    fn default() -> Self {
        Self { params: Vec::new(), ret: TypeRef::Unknown }
    }
}

pub fn parse_method_doc_types(doc_comment: &str) -> Option<MethodTypeHints> {
    let _span = tracing::trace_span!("parse_method_doc_types").entered();

    let mut hints = MethodTypeHints::default();
    let mut in_params_section = false;
    let mut in_return_section = false;
    let mut last_param_idx: Option<usize> = None;
    let mut seen_return_bullet = false;

    for line in doc_comment.lines() {
        let line = line.trim();
        let line = line.strip_prefix("//").unwrap_or(line).trim();
        let line_lower = line.fold_lower();

        if is_params_header(&line_lower) {
            in_params_section = true;
            in_return_section = false;
            last_param_idx = None;
            continue;
        }

        if is_return_header(&line_lower) {
            in_params_section = false;
            in_return_section = true;
            last_param_idx = None;
            seen_return_bullet = false;
            continue;
        }

        // Tabs commonly align the `-` separator (`Имя\t\t- Тип`). Every reader below recognises
        // only a space-flanked dash, and the structured doc parser normalises the same way, so a
        // tab here otherwise makes one parser see a type where the other sees prose. Section
        // headers stay deliberately outside this: the structured parser matches them on the raw
        // line, and normalising here would make a tabbed header visible to one parser only.
        let normalized: Cow<'_, str> = if line.contains('\t') {
            Cow::Owned(line.replace('\t', " "))
        } else {
            Cow::Borrowed(line)
        };
        let line = normalized.trim();

        if in_params_section {
            if is_continuation_line(line) {
                if let Some(idx) = last_param_idx {
                    if let Some(addition) = parse_continuation_line(line) {
                        fold_into_union(&mut hints.params[idx].1, addition);
                    }
                }
                continue;
            }

            if let Some((name, type_ref)) = parse_param_line(line) {
                hints.params.push((name, type_ref));
                last_param_idx = Some(hints.params.len() - 1);
                continue;
            }
        }

        if in_return_section {
            // A documented return is often a list of bullet alternatives, with the structure
            // body attached to one of them. Closing the section on the first recognised line
            // would keep only that alternative and type every caller by it alone, so bullets
            // accumulate into a union exactly as they do for a parameter.
            if is_continuation_line(line) {
                // A bullet this parser cannot name is still a documented alternative. Folding it
                // in as `Unknown` would let the kernel drop it (`T | Unknown == T`) and narrow the
                // return to the alternatives that happened to parse, flagging every value of the
                // unnamed one; `Any` keeps the union permissive, exactly as `См.` does.
                let addition = match parse_continuation_line(line) {
                    Some(TypeRef::Unknown) | None => TypeRef::Any,
                    Some(parsed) => parsed,
                };
                if seen_return_bullet {
                    fold_into_union(&mut hints.ret, addition);
                } else {
                    hints.ret = addition;
                    seen_return_bullet = true;
                }
                continue;
            }

            if !seen_return_bullet {
                if let Some(type_ref) = parse_return_line(line) {
                    hints.ret = type_ref;
                    in_return_section = false;
                }
            } else {
                // The bullet list ended: field lines (`* Поле - Тип`) and prose below it
                // describe the alternatives, they do not add further ones.
                in_return_section = false;
            }
        }
    }

    if hints.params.is_empty() && hints.ret == TypeRef::Unknown {
        tracing::trace!("no type hints found in doc comment");
        return None;
    }

    tracing::trace!("parsed {} parameter types, return type: {:?}", hints.params.len(), hints.ret);

    Some(hints)
}

fn is_params_header(line_lower: &str) -> bool {
    line_lower.starts_with("параметры:")
        || line_lower.starts_with("parameters:")
        || line_lower == "параметры"
        || line_lower == "parameters"
}

fn is_return_header(line_lower: &str) -> bool {
    line_lower.starts_with("возвращаемое значение:")
        || line_lower.starts_with("return value:")
        || line_lower.starts_with("returns:")
        || line_lower == "возвращаемое значение"
        || line_lower == "return value"
        || line_lower == "returns"
}

fn parse_param_line(line: &str) -> Option<(Name, TypeRef)> {
    if line.is_empty() {
        return None;
    }

    let parts: Vec<&str> = line.split(" - ").collect();
    if parts.len() < 2 {
        return None;
    }

    let param_name = parts[0].trim();
    let type_name = parts[1].trim();

    if param_name.is_empty() || type_name.is_empty() {
        return None;
    }

    Some((Name::new(param_name), parse_type_name(type_name)))
}

fn is_continuation_line(line: &str) -> bool {
    line.strip_prefix('-').is_some_and(|rest| rest.starts_with(' '))
}

fn parse_continuation_line(line: &str) -> Option<TypeRef> {
    let after_dash = line.strip_prefix("- ")?.trim();
    if after_dash.is_empty() {
        return None;
    }

    let type_name = match after_dash.split_once(" - ") {
        Some((ty, _desc)) => ty.trim(),
        None => after_dash,
    };
    if type_name.is_empty() {
        return None;
    }

    Some(parse_type_name(type_name))
}

fn fold_into_union(existing: &mut TypeRef, addition: TypeRef) {
    let prev = std::mem::replace(existing, TypeRef::Unknown);
    *existing = match prev {
        TypeRef::Union(mut parts) => {
            parts.push(addition);
            TypeRef::Union(parts)
        }
        other => TypeRef::Union(vec![other, addition]),
    };
}

fn parse_return_line(line: &str) -> Option<TypeRef> {
    if line.is_empty() {
        return None;
    }

    // A return type is often written as a bullet (`- Структура`). The structured doc parser
    // strips that dash, so this one must too, or the same slot is a typed structure for one
    // reader and prose for the other. Unlike parameters, the return section takes the first
    // recognised line and stops, so there are no union continuations to confuse with a bullet.
    let line = line.strip_prefix('-').map_or(line, str::trim);

    let type_name = if let Some(dash_pos) = line.find(" - ") { &line[..dash_pos] } else { line };

    let type_name = type_name.trim();
    if type_name.is_empty() {
        return None;
    }

    Some(parse_type_name(type_name))
}

fn parse_type_name(name: &str) -> TypeRef {
    let trimmed = name.trim().trim_end_matches(':').trim_end();
    let trimmed = trimmed.strip_suffix('-').unwrap_or(trimmed).trim_end();
    if trimmed.is_empty() {
        return TypeRef::Unknown;
    }

    // `См. Метод` / `See Method` is the 1C convention for "the type is the structure
    // documented at that reference" — always a real type we cannot resolve cheaply.
    // Lower to `Any` (the top type) rather than a phantom `См.Метод` nominal: a phantom
    // resolves to `Unknown`, which the kernel drops from a union (`Неопределено | Unknown
    // == Неопределено`), narrowing the param to the `Неопределено` default and flagging
    // every real argument. `Any` dominates the union (`Неопределено | Any == Any`) so the
    // param stays permissive.
    if is_see_reference(trimmed) {
        return TypeRef::Any;
    }

    if trimmed.contains(',') {
        let members: Vec<TypeRef> = trimmed
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(parse_type_name)
            .collect();
        if members.iter().all(|m| *m == TypeRef::Unknown) {
            return TypeRef::Unknown;
        }
        return match members.len() {
            0 => TypeRef::Unknown,
            1 => members.into_iter().next().unwrap(),
            _ => TypeRef::Union(members),
        };
    }

    if let Some(tref) = parse_collection_of(trimmed) {
        return tref;
    }

    if let Some(tref) = TypeRef::from_bare_name(trimmed) {
        return tref;
    }

    match trimmed.fold_lower().as_str() {
        "произвольный" | "any" | "arbitrary" => return TypeRef::Any,
        _ => {}
    }

    // Anything past this point becomes a nominal type that downstream lowering
    // trusts verbatim, so only identifier-shaped names may pass: doc comments
    // routinely put free prose, colons and parenthetical remarks in the type
    // position, and a prose-named type later fails every comparison against the
    // genuinely inferred type.
    if trimmed.contains('.') {
        let segments: Vec<Name> = trimmed
            .split('.')
            .map(|seg| Name::new(seg.trim()))
            .filter(|n| !n.as_str().is_empty())
            .collect();
        if segments.len() >= 2 && segments.iter().all(|n| is_identifier_like(n.as_str())) {
            return TypeRef::Name(QualifiedName::from_segments(segments));
        }
        return TypeRef::Unknown;
    }

    if is_identifier_like(trimmed) {
        TypeRef::Name(QualifiedName::from_segments([Name::new(trimmed)]))
    } else {
        TypeRef::Unknown
    }
}

fn is_see_reference(s: &str) -> bool {
    let s = s.trim_start();
    let head_end = s.find([' ', '.']).unwrap_or(s.len());
    matches!(s[..head_end].fold_lower().as_str(), "см" | "смотри" | "see")
}

fn is_identifier_like(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_alphabetic() || first == '_') && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// Splits `<коллекция> из <Тип>` into its head and tail. The two words are matched independently:
/// documentation mixes the languages freely (`Structure из …`, `Массив of …`), and a form one
/// parser recognises while the other does not silently costs the slot its documented fields.
fn split_collection(name: &str) -> Option<(&str, &str)> {
    let lower = name.fold_lower();
    [" из ", " of "].iter().find_map(|marker| {
        lower.find(marker).map(|pos| (name[..pos].trim(), name[pos + marker.len()..].trim()))
    })
}

fn parse_collection_of(name: &str) -> Option<TypeRef> {
    let (head, element) = split_collection(name)?;
    match head.fold_lower().as_str() {
        "массив" | "array" | "фиксированныймассив" | "fixedarray" => {
            Some(TypeRef::Array(Some(Box::new(parse_type_name(element)))))
        }
        "соответствие" | "map" => Some(TypeRef::Map(None)),
        // The value type of `Структура из КлючИЗначение` has nowhere to go, but the structure
        // itself must still be recognised.
        "структура" | "structure" => Some(TypeRef::Builtin(BuiltinTypeRef::Structure)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_ref::BuiltinTypeRef;

    fn builtin(b: BuiltinTypeRef) -> TypeRef {
        TypeRef::Builtin(b)
    }

    #[test]
    fn test_parse_russian_doc() {
        let doc = r#"
// Выполняет сложение двух чисел
//
// Параметры:
//   Левый - Число - первое слагаемое
//   Правый - Число - второе слагаемое
// Возвращаемое значение:
//   Число - сумма двух чисел
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 2);
        assert_eq!(hints.params[0].0.as_str(), "Левый");
        assert_eq!(hints.params[0].1, builtin(BuiltinTypeRef::Number));
        assert_eq!(hints.params[1].0.as_str(), "Правый");
        assert_eq!(hints.params[1].1, builtin(BuiltinTypeRef::Number));
        assert_eq!(hints.ret, builtin(BuiltinTypeRef::Number));
    }

    #[test]
    fn test_parse_english_doc() {
        let doc = r#"
// Checks if a string is empty
//
// Parameters:
//   Text - String - the string to check
// Returns:
//   Boolean - true if empty
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 1);
        assert_eq!(hints.params[0].0.as_str(), "Text");
        assert_eq!(hints.params[0].1, builtin(BuiltinTypeRef::String));
        assert_eq!(hints.ret, builtin(BuiltinTypeRef::Boolean));
    }

    #[test]
    fn test_parse_mixed_types() {
        let doc = r#"
// Параметры:
//   Строка1 - Строка - текст
//   Число1 - Число - количество
//   Флаг - Булево - признак
//   Дата1 - Дата - дата события
// Возвращаемое значение:
//   Массив - результат
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 4);
        assert_eq!(hints.params[0].1, builtin(BuiltinTypeRef::String));
        assert_eq!(hints.params[1].1, builtin(BuiltinTypeRef::Number));
        assert_eq!(hints.params[2].1, builtin(BuiltinTypeRef::Boolean));
        assert_eq!(hints.params[3].1, builtin(BuiltinTypeRef::Date));
        assert_eq!(hints.ret, TypeRef::Array(None));
    }

    #[test]
    fn test_parse_no_params() {
        let doc = r#"
// Получает текущую дату
//
// Возвращаемое значение:
//   Дата - текущая дата
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 0);
        assert_eq!(hints.ret, builtin(BuiltinTypeRef::Date));
    }

    #[test]
    fn test_parse_no_return() {
        let doc = r#"
// Выводит сообщение
//
// Параметры:
//   Текст - Строка - текст сообщения
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 1);
        assert_eq!(hints.ret, TypeRef::Unknown);
    }

    #[test]
    fn test_parse_no_hints() {
        let doc = r#"
// Просто комментарий без типов
// Еще одна строка
"#;

        let hints = parse_method_doc_types(doc);
        assert!(hints.is_none());
    }

    #[test]
    fn test_parse_qualified_metadata_type_keeps_segments() {
        let doc = r#"
// Параметры:
//   Объект - СправочникСсылка.Номенклатура - объект
// Возвращаемое значение:
//   Произвольный - результат
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 1);
        match &hints.params[0].1 {
            TypeRef::Name(qname) => {
                assert_eq!(qname.len(), 2);
                assert_eq!(qname.first().as_str(), "СправочникСсылка");
                assert_eq!(qname.last().as_str(), "Номенклатура");
            }
            other => panic!("expected TypeRef::Name, got {other:?}"),
        }
        assert_eq!(hints.ret, TypeRef::Any);
    }

    #[test]
    fn jsdoc_union_return_type_parses() {
        let doc = r#"
// Возвращаемое значение:
//   Число, Строка - результат сравнения
"#;
        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(
            hints.ret,
            TypeRef::Union(vec![builtin(BuiltinTypeRef::Number), builtin(BuiltinTypeRef::String),])
        );
    }

    #[test]
    fn jsdoc_union_param_type_parses() {
        let doc = r#"
// Параметры:
//   Значение - Число, Дата, Строка - проверяемое значение
"#;
        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 1);
        match &hints.params[0].1 {
            TypeRef::Union(members) => {
                assert_eq!(members.len(), 3);
                assert!(members.contains(&builtin(BuiltinTypeRef::Number)));
                assert!(members.contains(&builtin(BuiltinTypeRef::Date)));
                assert!(members.contains(&builtin(BuiltinTypeRef::String)));
            }
            other => panic!("expected TypeRef::Union, got {other:?}"),
        }
    }

    #[test]
    fn jsdoc_union_duplicate_types_preserve_syntactic_shape() {
        let doc = r#"
// Возвращаемое значение:
//   Число, Число - дубль
"#;
        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(
            hints.ret,
            TypeRef::Union(vec![builtin(BuiltinTypeRef::Number), builtin(BuiltinTypeRef::Number)])
        );
    }

    #[test]
    fn jsdoc_union_collapses_singleton_and_handles_empty_commas() {
        let doc = r#"
// Возвращаемое значение:
//   Число,, - трейлинг
"#;
        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.ret, builtin(BuiltinTypeRef::Number));
    }

    #[test]
    fn jsdoc_union_with_qualified_metadata_ref_preserved() {
        let doc = r#"
// Возвращаемое значение:
//   СправочникСсылка.Номенклатура, Строка - результат
"#;
        let hints = parse_method_doc_types(doc).unwrap();
        match hints.ret {
            TypeRef::Union(ref members) => {
                assert_eq!(members.len(), 2);
                assert!(matches!(members[0], TypeRef::Name(_) | TypeRef::Builtin(_)));
                assert!(matches!(members[1], TypeRef::Name(_) | TypeRef::Builtin(_)));
            }
            other => panic!("expected TypeRef::Union, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_case_insensitive() {
        let doc = r#"
// Параметры:
//   Param1 - СТРОКА - текст
//   param2 - число - число
// ВОЗВРАЩАЕМОЕ ЗНАЧЕНИЕ:
//   булево - результат
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 2);
        assert_eq!(hints.params[0].1, builtin(BuiltinTypeRef::String));
        assert_eq!(hints.params[1].1, builtin(BuiltinTypeRef::Number));
        assert_eq!(hints.ret, builtin(BuiltinTypeRef::Boolean));
    }

    #[test]
    fn parse_type_name_recognises_structure_of_t() {
        // The value type has nowhere to go in the kernel, but the structure must survive: the
        // structured doc parser reads this same slot as a structure, and a slot the two parsers
        // type differently loses its documented fields.
        assert_eq!(
            parse_type_name("Структура из КлючИЗначение"),
            builtin(BuiltinTypeRef::Structure)
        );
        assert_eq!(parse_type_name("Structure of KeyAndValue"), builtin(BuiltinTypeRef::Structure));
        assert_eq!(parse_type_name("Структура"), builtin(BuiltinTypeRef::Structure));
    }

    #[test]
    fn parse_type_name_recognises_array_of_t_russian() {
        let parsed = parse_type_name("Массив из Строка");
        assert_eq!(
            parsed,
            TypeRef::Array(Some(Box::new(TypeRef::Builtin(BuiltinTypeRef::String))))
        );
    }

    #[test]
    fn parse_type_name_recognises_array_of_t_english() {
        let parsed = parse_type_name("Array of Number");
        assert_eq!(
            parsed,
            TypeRef::Array(Some(Box::new(TypeRef::Builtin(BuiltinTypeRef::Number))))
        );
    }

    #[test]
    fn parse_type_name_array_of_t_is_case_insensitive() {
        let lower = parse_type_name("массив из число");
        let mixed = parse_type_name("МаСсИв ИЗ Дата");
        let english = parse_type_name("ARRAY OF Boolean");
        assert_eq!(lower, TypeRef::Array(Some(Box::new(TypeRef::Builtin(BuiltinTypeRef::Number)))));
        assert_eq!(mixed, TypeRef::Array(Some(Box::new(TypeRef::Builtin(BuiltinTypeRef::Date)))));
        assert_eq!(
            english,
            TypeRef::Array(Some(Box::new(TypeRef::Builtin(BuiltinTypeRef::Boolean))))
        );
    }

    #[test]
    fn parse_type_name_fixed_array_folds_into_array() {
        let parsed = parse_type_name("ФиксированныйМассив из Строка");
        assert_eq!(
            parsed,
            TypeRef::Array(Some(Box::new(TypeRef::Builtin(BuiltinTypeRef::String))))
        );
        let english = parse_type_name("FixedArray of Number");
        assert_eq!(
            english,
            TypeRef::Array(Some(Box::new(TypeRef::Builtin(BuiltinTypeRef::Number))))
        );
    }

    #[test]
    fn parse_type_name_map_of_x_collapses_to_map_none() {
        let parsed = parse_type_name("Соответствие из КлючИЗначение");
        assert_eq!(parsed, TypeRef::Map(None));
        let english = parse_type_name("Map of KeyValue");
        assert_eq!(english, TypeRef::Map(None));
    }

    #[test]
    fn parse_type_name_array_of_any_ref() {
        let parsed = parse_type_name("Массив из ЛюбаяСсылка");
        assert_eq!(parsed, TypeRef::Array(Some(Box::new(TypeRef::AnyRef))));
    }

    #[test]
    fn parse_type_name_array_of_t_keeps_unresolved_tail() {
        let parsed = parse_type_name("Массив из ПроизвольныйТип");
        match parsed {
            TypeRef::Array(Some(inner)) => match *inner {
                TypeRef::Name(qname) => {
                    assert_eq!(qname.len(), 1);
                    assert_eq!(qname.first().as_str(), "ПроизвольныйТип");
                }
                other => panic!("expected inner TypeRef::Name, got {other:?}"),
            },
            other => panic!("expected TypeRef::Array(Some(_)), got {other:?}"),
        }
    }

    #[test]
    fn parse_method_doc_types_folds_multiline_union() {
        let doc = r#"
// Параметры:
//  Ссылки - Массив из ЛюбаяСсылка
//         - ФиксированныйМассив из ЛюбаяСсылка - ссылки на объекты.
//           Если массив пуст, то результатом будет пустое соответствие.
//  Реквизиты - Массив из Строка
//            - ФиксированныйМассив из Строка - имена реквизитов в формате требований к свойствам структуры.
//            - Строка - имена реквизитов через запятую
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 2, "Ссылки + Реквизиты");

        match &hints.params[0].1 {
            TypeRef::Union(parts) => {
                assert_eq!(parts.len(), 2, "two ref-array alternatives, no description leakage");
                assert!(matches!(parts[0], TypeRef::Array(Some(_))));
                assert!(matches!(parts[1], TypeRef::Array(Some(_))));
            }
            other => panic!("Ссылки: expected TypeRef::Union, got {other:?}"),
        }

        match &hints.params[1].1 {
            TypeRef::Union(parts) => {
                assert_eq!(parts.len(), 3);
                assert_eq!(
                    parts[0],
                    TypeRef::Array(Some(Box::new(TypeRef::Builtin(BuiltinTypeRef::String))))
                );
                assert_eq!(
                    parts[1],
                    TypeRef::Array(Some(Box::new(TypeRef::Builtin(BuiltinTypeRef::String))))
                );
                assert_eq!(parts[2], TypeRef::Builtin(BuiltinTypeRef::String));
            }
            other => panic!("Реквизиты: expected TypeRef::Union, got {other:?}"),
        }
    }

    #[test]
    fn parse_method_doc_types_unnamed_return_bullet_stays_permissive() {
        // The second alternative is prose: it carries no ` - ` separator and names no type.
        // Folding it in as `Unknown` would let the kernel canonicalise the union down to
        // `Неопределено`, and every guarded use of the real value would be flagged.
        let doc = "\n// Возвращаемое значение:\n                   //   - Неопределено - если не пройдены проверки.\n                   //   - Значение любого типа.\n";

        let hints = parse_method_doc_types(doc).unwrap();
        match hints.ret {
            TypeRef::Union(parts) => {
                assert_eq!(parts.len(), 2);
                assert_eq!(parts[0], TypeRef::Builtin(BuiltinTypeRef::Undefined));
                assert_eq!(parts[1], TypeRef::Any, "the unnamed alternative must dominate");
            }
            other => panic!("expected a permissive union, got {other:?}"),
        }
    }

    #[test]
    fn parse_method_doc_types_names_a_structure_of_a_value_type() {
        // `Структура Из КлючИЗначение.` names a type, unlike the prose above: it must come out as
        // the structure it is, so the documented fields have an untyped structure to fill.
        let doc = "\n// Возвращаемое значение:\n                   //   - Неопределено - если не пройдены проверки.\n                   //   - Структура Из КлючИЗначение.\n";

        let hints = parse_method_doc_types(doc).unwrap();
        match hints.ret {
            TypeRef::Union(parts) => {
                assert_eq!(parts.len(), 2);
                assert_eq!(parts[0], TypeRef::Builtin(BuiltinTypeRef::Undefined));
                assert_eq!(parts[1], TypeRef::Builtin(BuiltinTypeRef::Structure));
            }
            other => panic!("expected a union, got {other:?}"),
        }
    }

    #[test]
    fn parse_method_doc_types_return_bullets_fold_into_union() {
        // Both alternatives are written as bullets, and the structure body belongs to the
        // second one. Taking the first bullet and closing the section types the call as an
        // array of catalog refs, so every column read on the real value-table row becomes an
        // unresolved field.
        let doc = "\n// Возвращаемое значение:\n                   //   - Массив Из СправочникСсылка.Склады - список ссылок.\n                   //   - ТаблицаЗначений - таблица с данными по складам:\n                   //       * Склад - СправочникСсылка.Склады - склад.\n";

        let hints = parse_method_doc_types(doc).unwrap();
        match hints.ret {
            TypeRef::Union(parts) => {
                assert_eq!(parts.len(), 2, "both documented alternatives must survive");
                assert!(matches!(parts[0], TypeRef::Array(Some(_))));
                assert_eq!(parts[1], TypeRef::Builtin(BuiltinTypeRef::ValueTable));
            }
            other => panic!("expected a union of both alternatives, got {other:?}"),
        }
    }

    #[test]
    fn parse_method_doc_types_single_return_bullet_is_not_wrapped_in_union() {
        let doc = "\n// Возвращаемое значение:\n//   - Структура - параметры соединения.\n";

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.ret, TypeRef::Builtin(BuiltinTypeRef::Structure));
    }

    #[test]
    fn parse_method_doc_types_tab_inside_section_header_is_not_a_header() {
        // Section headers are matched on the raw line by the structured doc parser
        // (`docs::returns_section_header`). Normalising tabs here too would make a tabbed
        // header visible to this parser alone, which is the very divergence the tab handling
        // below exists to remove.
        let doc = "\n// Возвращаемое\tзначение:\n//   Структура - данные.\n";

        assert_eq!(parse_method_doc_types(doc), None);
    }

    #[test]
    fn parse_method_doc_types_return_line_keeps_leading_dash_type() {
        let doc = "\n// Возвращаемое значение:\n//   - Структура - параметры соединения.\n";

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.ret, TypeRef::Builtin(BuiltinTypeRef::Structure));
    }

    #[test]
    fn parse_method_doc_types_return_line_separated_by_tab() {
        let doc = "\n// Возвращаемое значение:\n//   Структура\t- параметры соединения.\n";

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.ret, TypeRef::Builtin(BuiltinTypeRef::Structure));
    }

    #[test]
    fn parse_method_doc_types_param_line_separated_by_tab() {
        let doc = "\n// Параметры:\n//  Данные\t\t- Строка - выгружаемые данные.\n";

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 1, "tab-aligned separator must still yield a param hint");
        assert_eq!(hints.params[0].0.as_str(), "Данные");
        assert_eq!(hints.params[0].1, TypeRef::Builtin(BuiltinTypeRef::String));
    }

    #[test]
    fn parse_method_doc_types_ignores_star_bullets_in_params() {
        let doc = r#"
// Параметры:
//  ВложеннаяСтруктура - Структура:
//     * Поле1 - Строка - описание
//     * Поле2 - Число - описание
//  ВтороеПоле - Булево - флаг
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        let nested = hints.params.iter().find(|(n, _)| n.as_str() == "ВложеннаяСтруктура");
        assert!(nested.is_some(), "ВложеннаяСтруктура must be present");
        assert_eq!(nested.unwrap().1, builtin(BuiltinTypeRef::Structure));
    }

    #[test]
    fn bsp_trailing_dash_keeps_first_union_arm_as_array() {
        let doc = r#"
// Параметры:
//  КлючевыеРеквизитыТЧ - Массив -
//                      - Строка - имена реквизитов
//                      - Структура - Ключ это наименование
"#;
        let hints = parse_method_doc_types(doc).unwrap();
        let (_, ty) = &hints.params[0];
        match ty {
            TypeRef::Union(parts) => {
                assert_eq!(parts.len(), 3, "got {:#?}", parts);
                assert_eq!(
                    parts[0],
                    TypeRef::Array(None),
                    "first arm must be Array, got {:?}",
                    parts[0]
                );
                assert_eq!(parts[1], TypeRef::Builtin(BuiltinTypeRef::String));
                assert_eq!(parts[2], TypeRef::Builtin(BuiltinTypeRef::Structure));
            }
            other => panic!("expected Union, got {:?}", other),
        }
    }

    #[test]
    fn parse_type_name_strips_structured_form_colon_marker() {
        assert_eq!(parse_type_name("Структура:"), TypeRef::Builtin(BuiltinTypeRef::Structure));
        assert_eq!(parse_type_name("Соответствие:"), TypeRef::Map(None));
        assert_eq!(parse_type_name("Массив:"), TypeRef::Array(None));
        assert_eq!(parse_type_name("Структура :"), TypeRef::Builtin(BuiltinTypeRef::Structure));
    }

    #[test]
    fn parse_method_doc_types_orphan_continuation_is_dropped() {
        let doc = r#"
// Параметры:
//   - СтройноеЧисло - сирота
//   Param1 - Строка - текст
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 1);
        assert_eq!(hints.params[0].0.as_str(), "Param1");
        assert_eq!(hints.params[0].1, builtin(BuiltinTypeRef::String));
    }

    #[test]
    fn parse_method_doc_types_continuation_with_only_description_is_skipped() {
        let doc = r#"
// Параметры:
//   Реквизиты - Массив из Строка
//            Описание длинное и многословное.
//            - Строка - запасной вариант
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 1);
        match &hints.params[0].1 {
            TypeRef::Union(parts) => {
                assert_eq!(parts.len(), 2);
                assert_eq!(
                    parts[0],
                    TypeRef::Array(Some(Box::new(TypeRef::Builtin(BuiltinTypeRef::String))))
                );
                assert_eq!(parts[1], TypeRef::Builtin(BuiltinTypeRef::String));
            }
            other => panic!("expected TypeRef::Union, got {other:?}"),
        }
    }

    #[test]
    fn parse_method_doc_types_preserves_explicit_any_continuation() {
        let doc = r#"
// Параметры:
//   Значение - Строка
//            - Произвольный - любой другой тип
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 1);
        match &hints.params[0].1 {
            TypeRef::Union(parts) => {
                assert_eq!(parts.len(), 2);
                assert_eq!(parts[0], TypeRef::Builtin(BuiltinTypeRef::String));
                assert_eq!(parts[1], TypeRef::Any);
            }
            other => panic!("expected TypeRef::Union, got {other:?}"),
        }
    }

    #[test]
    fn parse_type_name_rejects_prose_and_parenthetical_tails() {
        assert_eq!(
            parse_type_name("СправочникСсылка.Справочник1 (Необязательный)"),
            TypeRef::Unknown,
            "a parenthetical tail must not leak into a nominal type name"
        );
        assert_eq!(
            parse_type_name("Ссылка на документ видов: ДокументСсылка.Документ1"),
            TypeRef::Unknown,
            "free prose before a dotted name must not become a qualified type"
        );
        assert_eq!(
            parse_type_name("имя редактируемого регистра"),
            TypeRef::Unknown,
            "multi-word prose must not become a nominal type"
        );
        assert_eq!(
            parse_type_name(
                "Ссылка на документ видов: ДокументСсылка.Документ1, \
                 СправочникСсылка.Справочник1 (Необязательный)"
            ),
            TypeRef::Unknown,
            "a comma list whose every member is invalid must collapse to Unknown"
        );
    }

    #[test]
    fn parse_type_name_comma_after_a_collection_splits_before_the_collection() {
        // `Массив из A, B` is written for BOTH readings in real configurations: sometimes one
        // array whose element is either type, sometimes an array OR an unrelated alternative.
        // The split-first reading is kept deliberately, because the alternative one buries a
        // dominating `Произвольный`/`Any` inside the element, where it stops keeping the slot
        // permissive: `Массив из Строка, Произвольный` would become a plain array and flag every
        // map-style use of the value. Measured on a real configuration: claiming the whole list
        // for the collection added 14 mismatches and fixed none.
        let parsed = parse_type_name("Массив из Строка, Произвольный");
        match parsed {
            TypeRef::Union(members) => {
                assert_eq!(members.len(), 2);
                assert!(matches!(members[0], TypeRef::Array(Some(_))));
                assert_eq!(members[1], TypeRef::Any, "the dominating arm must stay at top level");
            }
            other => panic!("expected a union, got {other:?}"),
        }
    }

    #[test]
    fn parse_type_name_keeps_valid_members_of_mixed_comma_list() {
        let parsed = parse_type_name("СправочникСсылка.Номенклатура, совсем не тип");
        match parsed {
            TypeRef::Union(members) => {
                assert_eq!(members.len(), 2);
                assert!(matches!(members[0], TypeRef::Name(_)));
                assert_eq!(
                    members[1],
                    TypeRef::Unknown,
                    "the invalid member must lower to Unknown (canonicalized away at the \
                     kernel level) instead of becoming a phantom that can never match"
                );
            }
            other => panic!("expected TypeRef::Union, got {other:?}"),
        }
    }

    #[test]
    fn parse_type_name_see_reference_becomes_any() {
        assert_eq!(parse_type_name("См. НовыеПараметрыФормы"), TypeRef::Any);
        assert_eq!(parse_type_name("см. ОбщегоНазначения.ОткрытьФорму.Параметры"), TypeRef::Any);
        assert_eq!(parse_type_name("Смотри НовыеПараметрыФормы"), TypeRef::Any);
        assert_eq!(parse_type_name("See SomeStructureConstructor"), TypeRef::Any);
    }

    #[test]
    fn parse_type_name_see_marker_does_not_swallow_real_types() {
        // A real type whose name merely starts with "См" must stay nominal.
        assert!(matches!(parse_type_name("СметнаяСтрока"), TypeRef::Name(_)));
        assert!(matches!(parse_type_name("Смещение"), TypeRef::Name(_)));
    }

    #[test]
    fn param_undefined_plus_see_reference_stays_permissive() {
        let doc = r#"
// Параметры:
//   ПараметрыФормы - Неопределено
//                  - См. НовыеПараметрыФормы
"#;
        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.params.len(), 1);
        // `Неопределено | Any` canonicalises to a permissive type at the kernel level;
        // the TypeRef union must carry the `Any` arm that drives that domination.
        match &hints.params[0].1 {
            TypeRef::Union(parts) => {
                assert_eq!(parts.len(), 2);
                assert_eq!(parts[0], TypeRef::Builtin(BuiltinTypeRef::Undefined));
                assert_eq!(parts[1], TypeRef::Any);
            }
            other => panic!("expected TypeRef::Union, got {other:?}"),
        }
    }

    #[test]
    fn parse_type_name_keeps_identifier_shaped_names() {
        assert!(matches!(parse_type_name("ПроизвольныйТип"), TypeRef::Name(_)));
        assert!(matches!(parse_type_name("СправочникСсылка.Номенклатура"), TypeRef::Name(_)));
    }

    #[test]
    fn split_collection_reads_cyrillic_and_mixed_case() {
        // Byte offsets and case folding both have to survive Cyrillic: the marker is found in the
        // folded text and then cut out of the original.
        assert_eq!(split_collection("Массив из Строка"), Some(("Массив", "Строка")));
        assert_eq!(split_collection("маССив ИЗ Число"), Some(("маССив", "Число")));
        assert_eq!(
            split_collection("Structure из КлючИЗначение"),
            Some(("Structure", "КлючИЗначение"))
        );
        assert_eq!(split_collection("Массив"), None);
    }

    #[test]
    fn parse_type_name_reads_the_marker_in_either_language() {
        assert_eq!(
            parse_type_name("Structure из КлючИЗначение"),
            builtin(BuiltinTypeRef::Structure)
        );
        assert_eq!(
            parse_type_name("Массив of Строка"),
            TypeRef::Array(Some(Box::new(builtin(BuiltinTypeRef::String))))
        );
        // A head that names no collection stays what it was: prose in the type position.
        assert_eq!(parse_type_name("Данные из справочника"), TypeRef::Unknown);
    }
}
