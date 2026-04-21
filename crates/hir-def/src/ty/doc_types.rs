//! JSDoc-style type annotation parser for BSL.
//!
//! Parses type hints from BSL doc comments like:
//! ```bsl
//! // Параметры:
//! //   Param1 - Строка - описание параметра
//! //   Param2 - Число - описание параметра
//! // Возвращаемое значение:
//! //   Булево - описание возвращаемого значения
//! ```
//!
//! Output is a [`TypeRef`] — the *syntactic* layer. Downstream callers
//! (e.g. `hir-ty::method_resolution::materialise_signature`) run it through
//! [`hir_ty::TyLoweringContext::lower_type_ref`] to obtain the semantic
//! [`crate::ty::Ty`]. Keeping the parser at the TypeRef level means JSDoc
//! and XML attributes share a single lowering pipeline, and adding a new
//! primitive or a qualified pattern (`Массив из Число`) only requires one
//! edit in `TyLoweringContext`.

use crate::path::QualifiedName;
use crate::type_ref::TypeRef;
use crate::Name;

/// Type hints extracted from method documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodTypeHints {
    /// Parameter type hints: (parameter_name, syntactic type).
    pub params: Vec<(Name, TypeRef)>,

    /// Return type hint. `TypeRef::Unknown` means "no hint" and is how the
    /// default value indicates a procedure with no return section.
    pub ret: TypeRef,
}

impl Default for MethodTypeHints {
    fn default() -> Self {
        Self { params: Vec::new(), ret: TypeRef::Unknown }
    }
}

/// Parse type hints from method documentation comment.
///
/// Recognizes the following patterns:
/// - Russian: "Параметры:", "Возвращаемое значение:"
/// - English: "Parameters:", "Return value:", "Returns:"
///
/// ## Example
/// ```text
/// // Параметры:
/// //   Строка1 - Строка - первая строка
/// //   Число1 - Число - какое-то число
/// // Возвращаемое значение:
/// //   Булево - результат сравнения
/// ```
pub fn parse_method_doc_types(doc_comment: &str) -> Option<MethodTypeHints> {
    let _span = tracing::trace_span!("parse_method_doc_types").entered();

    let mut hints = MethodTypeHints::default();
    let mut in_params_section = false;
    let mut in_return_section = false;

    for line in doc_comment.lines() {
        // Strip comment markers and whitespace
        let line = line.trim();
        // BSL uses only // comments, not ///
        let line = line.strip_prefix("//").unwrap_or(line).trim();

        // Check for section headers
        let line_lower = line.to_lowercase();

        if is_params_header(&line_lower) {
            in_params_section = true;
            in_return_section = false;
            continue;
        }

        if is_return_header(&line_lower) {
            in_params_section = false;
            in_return_section = true;
            continue;
        }

        // Parse parameter line
        if in_params_section {
            if let Some((name, type_ref)) = parse_param_line(line) {
                hints.params.push((name, type_ref));
            }
        }

        // Parse return type line
        if in_return_section {
            if let Some(type_ref) = parse_return_line(line) {
                hints.ret = type_ref;
                in_return_section = false; // Only first non-empty line
            }
        }
    }

    // Return None if no hints were found.
    // `ret == TypeRef::Unknown` is the "no return section" default; paired
    // with an empty `params` vector it means the doc comment carried no
    // type information at all.
    if hints.params.is_empty() && hints.ret == TypeRef::Unknown {
        tracing::trace!("no type hints found in doc comment");
        return None;
    }

    tracing::trace!("parsed {} parameter types, return type: {:?}", hints.params.len(), hints.ret);

    Some(hints)
}

/// Check if line is a parameters section header.
fn is_params_header(line_lower: &str) -> bool {
    line_lower.starts_with("параметры:")
        || line_lower.starts_with("parameters:")
        || line_lower == "параметры"
        || line_lower == "parameters"
}

/// Check if line is a return value section header.
fn is_return_header(line_lower: &str) -> bool {
    line_lower.starts_with("возвращаемое значение:")
        || line_lower.starts_with("return value:")
        || line_lower.starts_with("returns:")
        || line_lower == "возвращаемое значение"
        || line_lower == "return value"
        || line_lower == "returns"
}

/// Parse a parameter line: "Param1 - Строка - описание"
///
/// Expected format:
/// - Parameter name
/// - " - " separator
/// - Type name
/// - Optional " - " separator and description
fn parse_param_line(line: &str) -> Option<(Name, TypeRef)> {
    // Skip empty lines
    if line.is_empty() {
        return None;
    }

    // Split by " - " separator
    let parts: Vec<&str> = line.split(" - ").collect();
    if parts.len() < 2 {
        return None;
    }

    let param_name = parts[0].trim();
    let type_name = parts[1].trim();

    // Skip empty names or types
    if param_name.is_empty() || type_name.is_empty() {
        return None;
    }

    Some((Name::new(param_name), parse_type_name(type_name)))
}

/// Parse a return type line: "Булево - описание"
///
/// Expected format:
/// - Type name
/// - Optional " - " separator and description
fn parse_return_line(line: &str) -> Option<TypeRef> {
    // Skip empty lines
    if line.is_empty() {
        return None;
    }

    // Split by " - " separator (description is optional)
    let type_name = if let Some(dash_pos) = line.find(" - ") { &line[..dash_pos] } else { line };

    let type_name = type_name.trim();
    if type_name.is_empty() {
        return None;
    }

    Some(parse_type_name(type_name))
}

/// Parse a type-name token into a [`TypeRef`].
///
/// Resolution order:
/// 1. **Union** — a comma-separated list (`"Число, Строка"`) becomes
///    [`TypeRef::Union`] with each member recursively parsed. This lets
///    authors declare `// Возвращаемое значение: Число, Строка` and have
///    downstream lowering hand back a `Ty::Union([Number, String])`. Empty
///    commas (trailing, double) are dropped; a single surviving member
///    collapses back to a bare `TypeRef`.
/// 2. [`TypeRef::from_bare_name`] (primitives + Array/Map collections).
/// 3. `Произвольный` / `Any` / `Arbitrary` → [`TypeRef::Unknown`]
///    (authors write this when they don't want to commit to a type).
/// 4. Dotted form like `СправочникСсылка.Номенклатура` →
///    [`TypeRef::Name`] with all segments preserved. Lowering
///    (`TyLoweringContext::lower_qualified`) then decides whether it
///    becomes `Ty::MetadataRef`, `Ty::Unknown`, or something else.
/// 5. Any other single token → [`TypeRef::Name`] of one segment. Lowering
///    takes this through the manager/platform-object cascade.
fn parse_type_name(name: &str) -> TypeRef {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return TypeRef::Unknown;
    }

    // 1. Union — comma-separated alternatives.
    if trimmed.contains(',') {
        let members: Vec<TypeRef> = trimmed
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(parse_type_name)
            .collect();
        return match members.len() {
            0 => TypeRef::Unknown,
            1 => members.into_iter().next().unwrap(),
            _ => TypeRef::Union(members),
        };
    }

    // 2. Primitive / collection builtins.
    if let Some(tref) = TypeRef::from_bare_name(trimmed) {
        return tref;
    }

    // 3. Explicit "any" fallbacks.
    match trimmed.to_lowercase().as_str() {
        "произвольный" | "any" | "arbitrary" => return TypeRef::Unknown,
        _ => {}
    }

    // 4. Qualified names (`СправочникСсылка.Номенклатура`, `ОпределяемыйТип.Х`).
    if trimmed.contains('.') {
        let segments: Vec<Name> = trimmed
            .split('.')
            .map(|seg| Name::new(seg.trim()))
            .filter(|n| !n.as_str().is_empty())
            .collect();
        if segments.len() >= 2 {
            return TypeRef::Name(QualifiedName::from_segments(segments));
        }
    }

    // 5. Any other token — keep as a single-segment Name so the lowering
    //    cascade can decide (manager collective, platform object, …).
    TypeRef::Name(QualifiedName::from_segments([Name::new(trimmed)]))
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
        // After the TypeRef migration qualified types no longer collapse
        // into `Ty::Unknown` — they survive as `TypeRef::Name([...])` so
        // that `TyLoweringContext::lower_qualified` can decide between
        // `Ty::MetadataRef`, `Ty::Unknown`, or a future variant.
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
        // "Произвольный" remains the opt-out marker — still `Unknown`.
        assert_eq!(hints.ret, TypeRef::Unknown);
    }

    #[test]
    fn jsdoc_union_return_type_parses() {
        // Authors writing a polymorphic return type get a `TypeRef::Union`
        // preserved all the way into signature materialisation. The final
        // canonicalisation (sort / dedup) is the smart constructor's job at
        // lowering time — here we just check the syntactic shape.
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
        // Union works on parameter position too — matches BSL idiom for
        // `ОписаниеТипов`-shaped parameters.
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
    fn jsdoc_union_collapses_singleton_and_handles_empty_commas() {
        // A single survivor collapses back to the bare TypeRef — avoids
        // spurious `Union([x])`. Trailing / double commas are dropped.
        let doc = r#"
// Возвращаемое значение:
//   Число,, - трейлинг
"#;
        let hints = parse_method_doc_types(doc).unwrap();
        assert_eq!(hints.ret, builtin(BuiltinTypeRef::Number));
    }

    #[test]
    fn jsdoc_union_with_qualified_metadata_ref_preserved() {
        // Mixing a qualified ref with a primitive reaches lowering as a
        // `TypeRef::Union` — `TyLoweringContext` will then turn each branch
        // into its own `Ty` (MetadataRef, Builtin).
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
}
