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
    // Index of the most recent param accepted in the params section. Set
    // when a `Имя - Тип …` line lands; used to fold БСП-style multi-line
    // alternatives (`- Тип2 - desc`) into that param's TypeRef as a Union.
    let mut last_param_idx: Option<usize> = None;

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
            last_param_idx = None;
            continue;
        }

        if is_return_header(&line_lower) {
            in_params_section = false;
            in_return_section = true;
            last_param_idx = None;
            continue;
        }

        // Parse parameter line
        if in_params_section {
            // Continuation of the previous param's type — БСП writes union
            // alternatives on follow-on lines that start with `- `. Must be
            // checked *before* `parse_param_line` because that helper would
            // misparse such a line by treating the leading `-` as part of a
            // bogus parameter name. `*`-bullets (Structure field
            // descriptions) do not match `is_continuation_line` and fall
            // through; they are then handled (or dropped) by
            // `parse_param_line` exactly as before this fix.
            if is_continuation_line(line) {
                if let Some(idx) = last_param_idx {
                    if let Some(addition) = parse_continuation_line(line) {
                        fold_into_union(&mut hints.params[idx].1, addition);
                    }
                }
                // Orphan continuation (no preceding param to fold into)
                // is silently dropped. Falling through to
                // `parse_param_line` would let the leading `-` slip into a
                // bogus parameter name — a pre-existing latent bug we
                // foreclose here.
                continue;
            }

            if let Some((name, type_ref)) = parse_param_line(line) {
                hints.params.push((name, type_ref));
                last_param_idx = Some(hints.params.len() - 1);
                continue;
            }
            // Description-only continuation lines (no leading `-`, free
            // prose) are silently dropped. We keep `last_param_idx` set so
            // a later `- Тип2` continuation still folds into the right
            // param — matches BSP style where description and union
            // alternatives can interleave inside one param block.
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

/// True if `line` is a multi-line union continuation marker — a line of
/// the form `- <тип>[- описание]`, used by BSP to add an alternative type
/// to the previous parameter.
///
/// Excludes `*`-bullets (Structure field descriptions inside Параметры:),
/// double-`--` prose dashes, and any line that does not start with the
/// `- ` token. The caller must already be in the params section *and*
/// have a previous param to fold into; this helper is purely a syntactic
/// shape check.
fn is_continuation_line(line: &str) -> bool {
    line.strip_prefix('-').is_some_and(|rest| rest.starts_with(' '))
}

/// Parse a continuation line into the type alternative it carries.
///
/// `line` is expected to start with `- ` (caller guards via
/// [`is_continuation_line`]). Strips the leading dash, peels off an
/// optional ` - <описание>` tail, and runs the type fragment through
/// [`parse_type_name`].
///
/// Returns `None` only when the type fragment is empty (caller should
/// drop the line). [`TypeRef::Unknown`] is **kept** — `parse_type_name`
/// only returns it for the explicit `Произвольный` / `Any` / `Arbitrary`
/// gradual-top opt-in (or for syntactically empty input, which we filter
/// here). Dropping it would silently erase an author's deliberate "or
/// anything else" alternative; [`crate::Ty::union`] preserves `Unknown`
/// inside unions on purpose, see its doc comment.
fn parse_continuation_line(line: &str) -> Option<TypeRef> {
    let after_dash = line.strip_prefix("- ")?.trim();
    if after_dash.is_empty() {
        return None;
    }

    // Type fragment ends at the first " - " separator (which introduces
    // the optional description). The fragment itself can contain spaces
    // (e.g. `ФиксированныйМассив из Строка`) — that's fine because the
    // separator is a *spaced* dash, not a bare `-`.
    let type_name = match after_dash.split_once(" - ") {
        Some((ty, _desc)) => ty.trim(),
        None => after_dash,
    };
    if type_name.is_empty() {
        return None;
    }

    Some(parse_type_name(type_name))
}

/// Fold `addition` into `existing` as an additional union alternative.
///
/// Preserves the smart-constructor invariant for unions at the syntactic
/// layer: nesting and dedup happen at lowering time via [`crate::Ty::union`].
/// Here we just append.
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
/// 2. **Parameterised collection** — `Массив из X` / `Array of X`,
///    `ФиксированныйМассив из X` / `FixedArray of X` (folded into
///    `TypeRef::Array(Some(_))` until a dedicated `Ty::FixedArray`
///    variant exists), and `Соответствие из X` / `Map of X` (collapses to
///    `TypeRef::Map(None)` because `Ty::Map` is not parameterised yet).
///    Must run before [`TypeRef::from_bare_name`] — the bare-name table
///    only handles the unqualified head.
/// 3. [`TypeRef::from_bare_name`] (primitives + Array/Map collections).
/// 4. `Произвольный` / `Any` / `Arbitrary` → [`TypeRef::Unknown`]
///    (authors write this when they don't want to commit to a type).
/// 5. Dotted form like `СправочникСсылка.Номенклатура` →
///    [`TypeRef::Name`] with all segments preserved. Lowering
///    (`TyLoweringContext::lower_qualified`) then decides whether it
///    becomes `Ty::MetadataRef`, `Ty::Unknown`, or something else.
/// 6. Any other single token → [`TypeRef::Name`] of one segment. Lowering
///    takes this through the manager/platform-object cascade.
fn parse_type_name(name: &str) -> TypeRef {
    // The trailing `:` is a JSDoc structured-form marker (`Структура:`,
    // `Соответствие:`, `Массив:` introducing a `* Поле - Тип` field list).
    // It is never part of a BSL type name, so strip it before classification —
    // otherwise the bare-name table misses and the token degenerates into
    // `Ty::PlatformObject("Структура:")`, fabricating a `TypeMismatch` against
    // any real `Ty::Structure` argument.
    let trimmed = name.trim().trim_end_matches(':').trim_end();
    // BSP convention writes the first arm of a multi-line union as
    // `Type -` — the trailing separator marks "empty description, more arms
    // below". A bare BSL type never carries a trailing `-`, so stripping
    // it lets the bare-name table classify `Массив -` as `TypeRef::Array`
    // instead of degenerating into `TypeRef::Name([Массив -])`.
    let trimmed = trimmed.strip_suffix('-').unwrap_or(trimmed).trim_end();
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

    // 2. Parameterised collection forms (`Массив из Строка`,
    //    `Соответствие из КлючИЗначение`, …) — БСП-idiom written in
    //    documentation prose. Must run *before* `from_bare_name` because the
    //    bare-name table only recognises the unqualified head.
    if let Some(tref) = parse_collection_of(trimmed) {
        return tref;
    }

    // 3. Primitive / collection builtins.
    if let Some(tref) = TypeRef::from_bare_name(trimmed) {
        return tref;
    }

    // 4. Explicit "any" fallbacks.
    match trimmed.to_lowercase().as_str() {
        "произвольный" | "any" | "arbitrary" => return TypeRef::Unknown,
        _ => {}
    }

    // 5. Qualified names (`СправочникСсылка.Номенклатура`, `ОпределяемыйТип.Х`).
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

    // 6. Any other token — keep as a single-segment Name so the lowering
    //    cascade can decide (manager collective, platform object, …).
    TypeRef::Name(QualifiedName::from_segments([Name::new(trimmed)]))
}

/// Strip `prefix` from `s` case-insensitively, returning a slice of the
/// **original** `s` (preserving the author's casing of the tail).
///
/// # Domain invariant
///
/// Every char in BSL JSDoc collection keywords (`Массив`, `Array`,
/// `ФиксированныйМассив`, `FixedArray`, `Соответствие`, `Map`, `из`, `of`)
/// belongs to ASCII or Russian Cyrillic `а-яА-ЯёЁ`. For these chars,
/// `char::to_lowercase` always emits exactly one codepoint, so the
/// lowercase-form char count equals the original char count and we can
/// recover the tail by skipping `prefix.chars().count()` chars in `s`.
///
/// Outside that domain (e.g. Turkish `İ` → `i + ◌̇`) this trick is unsafe;
/// the helper is therefore deliberately scoped to private use inside the
/// JSDoc parser.
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let s_lower = s.to_lowercase();
    let prefix_lower = prefix.to_lowercase();

    if !s_lower.starts_with(&prefix_lower) {
        return None;
    }

    let prefix_chars = prefix.chars().count();
    let tail_start = s.char_indices().nth(prefix_chars).map(|(idx, _)| idx).unwrap_or(s.len());

    Some(&s[tail_start..])
}

/// Recognise the БСП-idiomatic `<коллекция> из <T>` documentation prose
/// (and its English `<collection> of <T>` form).
///
/// Returns `None` when `name` is not one of the supported collection
/// prefixes — the caller then falls through to the generic name-parsing
/// cascade.
///
/// Supported shapes:
/// - `Массив из <T>` / `Array of <T>` →
///   `TypeRef::Array(Some(parse_type_name(<T>)))`.
/// - `ФиксированныйМассив из <T>` / `FixedArray of <T>` → same as above
///   (folded into `TypeRef::Array` because `Ty` does not yet distinguish
///   `FixedArray`; this is enough to unblock assignability against
///   `Новый Массив` callers without inventing a dead variant). TODO:
///   introduce a dedicated `TypeRef::FixedArray` once `Ty` carries it.
/// - `Соответствие из <X>` / `Map of <X>` → `TypeRef::Map(None)`. The
///   parameter is dropped because `Ty::Map` is not parameterised yet, and
///   the typical БСП usage is `Соответствие из КлючИЗначение` followed by
///   `*`-bullet sub-fields, which the line-based parser cannot consume
///   into key/value types reliably.
fn parse_collection_of(name: &str) -> Option<TypeRef> {
    for prefix in ["Массив из ", "Array of "] {
        if let Some(tail) = strip_prefix_ci(name, prefix) {
            return Some(TypeRef::Array(Some(Box::new(parse_type_name(tail)))));
        }
    }
    for prefix in ["ФиксированныйМассив из ", "FixedArray of "] {
        if let Some(tail) = strip_prefix_ci(name, prefix) {
            return Some(TypeRef::Array(Some(Box::new(parse_type_name(tail)))));
        }
    }
    for prefix in ["Соответствие из ", "Map of "] {
        if strip_prefix_ci(name, prefix).is_some() {
            return Some(TypeRef::Map(None));
        }
    }
    None
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
    fn jsdoc_union_duplicate_types_preserve_syntactic_shape() {
        // The parser stays syntactic — `"Число, Число"` yields
        // `TypeRef::Union([Number, Number])` and lets the `Ty::union`
        // smart constructor at lowering time perform the dedup. This pins
        // that contract: deduplication is NOT the parser's job, so
        // removing the smart constructor wouldn't silently bypass dedup.
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

    #[test]
    fn parse_type_name_recognises_array_of_t_russian() {
        // `Массив из Строка` is the canonical БСП way of writing a string
        // array. Before the fix the whole literal degenerated into a
        // single-segment `TypeRef::Name`, which lowering then turned into
        // `Ty::PlatformObject("Массив из Строка")` — the source of the
        // false-positive `TypeMismatch` against `Новый Массив` callers.
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
        // Case-insensitive prefix match must hold for both Russian and
        // English forms — pins the Cyrillic side of the
        // `strip_prefix_ci` helper's domain invariant.
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
        // `ФиксированныйМассив` has no dedicated `Ty` variant yet — fold
        // it into `TypeRef::Array(Some(_))` so callers passing a plain
        // `Новый Массив` still satisfy the expected type. When the type
        // system gains `Ty::FixedArray` this test will need to flip to
        // assert the dedicated variant; today the assignability gain
        // is the win.
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
        // `Соответствие из КлючИЗначение` is the БСП idiom that introduces
        // a Structure-of-fields description. Until `Ty::Map` carries
        // key/value parameters there's nothing useful to keep from the
        // tail — collapse to `TypeRef::Map(None)` so it lowers to
        // `Ty::Map` and stays compatible with `Новый Соответствие`.
        let parsed = parse_type_name("Соответствие из КлючИЗначение");
        assert_eq!(parsed, TypeRef::Map(None));
        let english = parse_type_name("Map of KeyValue");
        assert_eq!(english, TypeRef::Map(None));
    }

    #[test]
    fn parse_type_name_array_of_t_keeps_unresolved_tail() {
        // `Массив из ЛюбаяСсылка` has a tail that's not a builtin —
        // it falls through to `TypeRef::Name` at lowering time. The
        // important thing here is that the wrapper survives intact and
        // the tail is *not* lowercased (would damage `Name` Eq/Hash and
        // hover rendering).
        let parsed = parse_type_name("Массив из ЛюбаяСсылка");
        match parsed {
            TypeRef::Array(Some(inner)) => match *inner {
                TypeRef::Name(qname) => {
                    assert_eq!(qname.len(), 1);
                    assert_eq!(qname.first().as_str(), "ЛюбаяСсылка");
                }
                other => panic!("expected inner TypeRef::Name, got {other:?}"),
            },
            other => panic!("expected TypeRef::Array(Some(_)), got {other:?}"),
        }
    }

    #[test]
    fn parse_method_doc_types_folds_multiline_union() {
        // The exact shape from the BSP `ЗначенияРеквизитовОбъектовЕслиСуществуют`
        // signature that triggered the bug report. After the fix the
        // continuation lines must collapse into a `TypeRef::Union`
        // covering both array forms and the bare-string fallback — this
        // is what stops the false-positive `TypeMismatch` against a
        // `Новый Массив` argument.
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

        // Ссылки: Массив из ЛюбаяСсылка | ФиксированныйМассив из ЛюбаяСсылка
        // (the description-only "Если массив пуст" line must not show up
        // as an alternative).
        match &hints.params[0].1 {
            TypeRef::Union(parts) => {
                assert_eq!(parts.len(), 2, "two ref-array alternatives, no description leakage");
                assert!(matches!(parts[0], TypeRef::Array(Some(_))));
                assert!(matches!(parts[1], TypeRef::Array(Some(_))));
            }
            other => panic!("Ссылки: expected TypeRef::Union, got {other:?}"),
        }

        // Реквизиты: Массив из Строка | ФиксированныйМассив из Строка | Строка
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
    fn parse_method_doc_types_ignores_star_bullets_in_params() {
        // `*`-bullets describe Structure fields — they must NOT be folded
        // into the parent param's type as union alternatives. Only `- `
        // continuations count.
        let doc = r#"
// Параметры:
//  ВложеннаяСтруктура - Структура:
//     * Поле1 - Строка - описание
//     * Поле2 - Число - описание
//  ВтороеПоле - Булево - флаг
"#;

        let hints = parse_method_doc_types(doc).unwrap();
        // The `*`-bullets currently parse through `parse_param_line`
        // (pre-existing behaviour, orthogonal to this fix) so the param
        // count may include them — what matters here is that
        // `ВложеннаяСтруктура` does NOT become a union folded with
        // `Поле1`/`Поле2`.
        let nested = hints.params.iter().find(|(n, _)| n.as_str() == "ВложеннаяСтруктура");
        assert!(nested.is_some(), "ВложеннаяСтруктура must be present");
        // The structured-form marker `:` is JSDoc syntax, not part of the
        // type name — `Структура:` must classify as the builtin Structure,
        // not degenerate into `TypeRef::Name([Структура:])` (which would
        // lower to `Ty::PlatformObject("Структура:")` and fabricate a
        // `TypeMismatch` against any real `Ty::Structure` argument).
        assert_eq!(nested.unwrap().1, builtin(BuiltinTypeRef::Structure));
    }

    #[test]
    fn bsp_trailing_dash_keeps_first_union_arm_as_array() {
        // BSP convention: first-line union arm written as `Type -` with an
        // empty description after the trailing separator. The continuation
        // lines below carry the rest of the arms.
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
        // The trailing `:` is JSDoc syntax that introduces a `* Поле - Тип`
        // field list — it is never part of a BSL type name. All three
        // structured-form markers must classify to their bare-name builtin.
        assert_eq!(parse_type_name("Структура:"), TypeRef::Builtin(BuiltinTypeRef::Structure));
        assert_eq!(parse_type_name("Соответствие:"), TypeRef::Map(None));
        assert_eq!(parse_type_name("Массив:"), TypeRef::Array(None));
        // Whitespace between the name and the colon must also be tolerated.
        assert_eq!(parse_type_name("Структура :"), TypeRef::Builtin(BuiltinTypeRef::Structure));
    }

    #[test]
    fn parse_method_doc_types_orphan_continuation_is_dropped() {
        // A `- Тип` line that appears before any param line has no anchor
        // to fold into. It must be silently dropped — both the
        // continuation logic (no `last_param_idx`) and `parse_param_line`
        // (no name segment) refuse it. Pinning this so a future
        // refactor doesn't accidentally treat it as a degenerate param.
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
        // A free-prose description line between the param header and a
        // following continuation must not break the continuation chain —
        // we keep `last_param_idx` set across description lines so the
        // later `- Тип2` still folds into the right param.
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
        // `Произвольный` / `Any` is the author's deliberate gradual-top
        // opt-in: "this param accepts the listed types OR anything else".
        // The continuation parser must keep it as `TypeRef::Unknown`
        // inside the union so `Ty::union` (which preserves Unknown by
        // design) propagates the gradual-top semantics to assignability.
        // Dropping it would silently narrow the contract to the explicit
        // alternatives only.
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
                assert_eq!(parts[1], TypeRef::Unknown);
            }
            other => panic!("expected TypeRef::Union, got {other:?}"),
        }
    }

    #[test]
    fn strip_prefix_ci_handles_cyrillic_round_trip() {
        // Pins the helper's domain invariant: a successfully stripped
        // tail is a slice of the *original* input, with the original
        // casing preserved (critical because downstream `Name::new` is
        // case-sensitive and hover renders the raw author casing).
        let tail = strip_prefix_ci("Массив из Строка", "Массив из ").unwrap();
        assert_eq!(tail, "Строка");

        let tail_mixed = strip_prefix_ci("маССив ИЗ Число", "Массив из ").unwrap();
        assert_eq!(tail_mixed, "Число");

        // No match — the prefix differs in non-case ways.
        assert!(strip_prefix_ci("Соответствие из X", "Массив из ").is_none());
    }
}
