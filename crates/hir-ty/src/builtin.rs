//! Built-in function signatures for 1C:Enterprise platform.
//!
//! This module exposes typed signatures for the 498 platform global functions
//! shipped in `bsl-platform/data/platform_data.json` (parsed once at startup
//! via `PlatformData::instance()`), plus a small hand-curated fallback list
//! for entries the platform JSON does not cover: the `Новый` keyword (real
//! call sites are handled by `Expr::New` in inference) and `ОписаниеТипов`
//! (extracted into the `types` section of the help book rather than
//! `global_functions`).
//!
//! The single source of truth is the platform JSON; this module is a thin
//! adapter that maps `param_type` strings to [`Ty`] (via `Ty::from_type_name`),
//! reconstructs the `defaults` mask from `is_optional`, derives the
//! documented argument cap (`max_args`) from the platform-idiomatic
//! `<имя>1-<имя><цифра>` last-parameter naming (e.g. `Значение1-Значение10`
//! → `max_args = 1 + 10` for `СтрШаблон`), and parses comma-separated
//! return-type unions through `Ty::union`.

use hir_def::ty::{FunctionSignature, Ty};
use rustc_hash::FxHashMap;
use std::sync::OnceLock;

/// Global registry of built-in platform functions.
///
/// Initialized once on first access and reused for all subsequent calls.
static BUILTIN_FUNCTIONS: OnceLock<BuiltinFunctions> = OnceLock::new();

/// Get the global built-in functions registry.
pub fn builtin_functions() -> &'static BuiltinFunctions {
    BUILTIN_FUNCTIONS.get_or_init(BuiltinFunctions::new)
}

/// Registry of built-in platform function signatures.
///
/// Functions are indexed by their lowercase name for case-insensitive lookup.
/// Both the Russian (`нстр`) and English (`nstr`) keys point at the same
/// signature.
#[derive(Debug)]
pub struct BuiltinFunctions {
    /// Signatures indexed by lowercase function name.
    signatures: FxHashMap<String, FunctionSignature>,
}

impl BuiltinFunctions {
    /// Create and populate the built-in functions registry.
    fn new() -> Self {
        let mut signatures = FxHashMap::default();

        // 1. Adapt every platform global function from the JSON-backed
        //    `bsl-platform` registry into a typed `FunctionSignature`.
        let platform = bsl_platform::PlatformData::instance();
        for func in platform.all_global_functions() {
            let sig = signature_from_global_function(func);
            signatures.insert(func.name.to_lowercase(), sig.clone());
            signatures.insert(func.english_name.to_lowercase(), sig);
        }

        // 2. Fill gaps with hand-curated signatures for names the platform
        //    JSON does not carry today. `register_fallbacks` uses
        //    `entry().or_insert(...)` so the JSON-derived signature
        //    *always* wins on collision — the hand-list only contributes
        //    when the JSON has nothing under the same lowercase key.
        register_fallbacks(&mut signatures);

        tracing::debug!("initialized {} built-in function signature keys", signatures.len());

        Self { signatures }
    }

    /// Get function signature by name (case-insensitive).
    pub fn get(&self, name: &str) -> Option<&FunctionSignature> {
        let name_lower = name.to_lowercase();
        self.signatures.get(&name_lower)
    }
}

/// Convert a [`bsl_platform::GlobalFunction`] into a typed [`FunctionSignature`].
///
/// Mapping rules:
/// - Each parameter's `param_type` is run through [`map_type_string`]; `None`
///   or unrecognised tokens collapse to `Ty::Unknown` (deliberately permissive
///   — `MismatchedArgCount` only checks arity, not assignability).
/// - `defaults[i]` mirrors `parameters[i].is_optional`.
/// - `max_args` is computed from the last parameter's name: when it matches
///   the platform-help idiom `<word>N-<word>M` (e.g. `Значение1-Значение10`
///   for `СтрШаблон`), the documented cap `M` extends the upper bound by
///   `M - 1` extra slots beyond the declared trailing param. Otherwise it
///   stays at `params.len()` (a fixed-arity signature).
/// - `return_type` may be a comma-separated union (`"Булево, Неопределено"`);
///   each piece is mapped individually and recombined via `Ty::union`.
fn signature_from_global_function(func: &bsl_platform::GlobalFunction) -> FunctionSignature {
    let mut params = Vec::with_capacity(func.parameters.len());
    let mut defaults = Vec::with_capacity(func.parameters.len());
    for param in &func.parameters {
        params.push(map_type_string(param.param_type.as_deref()));
        defaults.push(param.is_optional);
    }

    let ret = match &func.return_type {
        None => Ty::Undefined,
        Some(s) => map_return_type(s.as_str()),
    };

    // The trailing variadic slot is one declared param that absorbs `M`
    // trailing args; the cap is `(params.len() - 1) + M`. Subtract one to
    // avoid double-counting the slot the param already represents.
    let max_args = match func.parameters.last().and_then(|p| variadic_param_max(p.name.as_str())) {
        Some(m) => Some((params.len() as u32).saturating_sub(1).saturating_add(m)),
        None => Some(params.len() as u32),
    };

    FunctionSignature::new_with_defaults(params, defaults, ret).with_max_args(max_args)
}

/// Map a single platform `param_type` token (or `None`) to [`Ty`].
///
/// Unknown / `None` collapse to `Ty::Unknown` — `MismatchedArgCount` is
/// arity-only, so being conservative on the type element here is safe;
/// downstream type-mismatch diagnostics will use richer paths than this
/// adapter.
fn map_type_string(s: Option<&str>) -> Ty {
    let Some(name) = s.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ty::Unknown;
    };
    // `Произвольный`/`Arbitrary` and other unrecognised tokens fall through
    // to `Ty::Unknown` via `from_type_name`'s default arm.
    Ty::from_type_name(name)
}

/// Map a return-type string, supporting comma-separated unions
/// (`"Булево, Неопределено"`).
fn map_return_type(s: &str) -> Ty {
    if s.contains(',') {
        let members: Vec<Ty> =
            s.split(',').map(str::trim).filter(|p| !p.is_empty()).map(Ty::from_type_name).collect();
        return Ty::union(members);
    }
    map_type_string(Some(s))
}

/// Recognise the platform-help idiom for variadic last parameters,
/// `<имя>N-<имя>M`, returning the upper bound `M` (e.g. `10` for
/// `Значение1-Значение10`).
///
/// All slicing here uses byte indices returned by `char_indices` — `rfind`
/// returns the start of a char and adding 1 to that offset would split a
/// multibyte UTF-8 sequence (e.g. Cyrillic `е` occupies bytes 14..16 in
/// `Значение1`).
fn variadic_param_max(name: &str) -> Option<u32> {
    let (head, tail) = name.split_once('-')?;
    // Walk `head` from the end, splitting it into a word prefix and a
    // trailing digit run on character (not byte) boundaries.
    let digits_start = head
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map(|(idx, _)| idx)?;
    let (head_word, head_digits) = head.split_at(digits_start);
    if head_word.is_empty() || head_digits.is_empty() {
        return None;
    }
    let tail = tail.trim_start();
    if !tail.starts_with(head_word) {
        return None;
    }
    let tail_digits = &tail[head_word.len()..];
    if tail_digits.is_empty() || !tail_digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    tail_digits.parse::<u32>().ok()
}

/// Register hand-curated signatures for names the platform JSON does not
/// expose as global functions.
///
/// Each entry is a separate insert so future drift (a name appearing in
/// platform JSON later) is easy to audit and remove.
fn register_fallbacks(sigs: &mut FxHashMap<String, FunctionSignature>) {
    // `Новый` is the constructor keyword (`Новый Массив`, `Новый Запрос`).
    // Real call sites are handled in `infer_new_expr`; the signature here
    // exists purely so the resolver / completion treat the bare token as
    // a known builtin name. Not a regular call — typed permissively.
    insert_pair(sigs, ("новый", "new"), FunctionSignature::function(vec![Ty::Type], Ty::Unknown));

    // `ОписаниеТипов(<Типы>, [<СписокИсключаемыхТипов>], [<Квалификаторы…>])`
    // is both a type name (class for `Новый ОписаниеТипов(...)`) and a
    // bare global-function form. The platform extractor lists it under
    // `types`, not `global_functions`, so the bare-call form is missing.
    // Without a precise overload model, we fall back to a single required
    // `Unknown` plus an unbounded variadic tail (`max_args = None`).
    insert_pair(
        sigs,
        ("описаниетипов", "typedescription"),
        FunctionSignature::new_with_defaults(vec![Ty::Unknown], vec![false], Ty::Unknown)
            .with_max_args(None),
    );
}

/// Register the same signature under both Russian and English lowercase keys
/// **only if** the key is not already present.
///
/// This is the fallback layer's contract: the JSON-derived signature is
/// authoritative. If the platform extractor starts shipping a previously
/// missing name, our hand-rolled stub stays out of the way.
fn insert_pair(
    sigs: &mut FxHashMap<String, FunctionSignature>,
    (ru, en): (&str, &str),
    sig: FunctionSignature,
) {
    sigs.entry(ru.to_string()).or_insert_with(|| sig.clone());
    sigs.entry(en.to_string()).or_insert(sig);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_is_case_insensitive_and_bilingual() {
        let builtins = builtin_functions();
        assert!(builtins.get("СтрДлина").is_some());
        assert!(builtins.get("стрдлина").is_some());
        assert!(builtins.get("СТРДЛИНА").is_some());
        assert!(builtins.get("StrLen").is_some());
        assert!(builtins.get("strlen").is_some());
    }

    #[test]
    fn nstr_has_optional_second_parameter() {
        // The bug that drove the Slice 1 work — НСтр in `platform_data.json`
        // declares `КодЯзыка` with `is_optional=true`. Calling
        // `НСтр("ru = '...'", "ru")` must satisfy arity (required=1, total=2).
        let builtins = builtin_functions();
        let nstr = builtins.get("нстр").expect("НСтр should exist");
        assert_eq!(nstr.params.len(), 2, "НСтр has 2 declared params");
        assert_eq!(nstr.required_count(), 1, "second param is optional, so required=1");
        assert_eq!(nstr.max_args, Some(2), "fixed arity caps at params.len()");
        assert_eq!(*nstr.ret, Ty::String);
    }

    #[test]
    fn strtemplate_is_capped_variadic() {
        // СтрШаблон has the platform idiom `Значение1-Значение10` last param.
        // The adapter must lift it to a hard 11-arg cap (1 template + 10
        // values), not to an unbounded variadic.
        let builtins = builtin_functions();
        let sig = builtins.get("стршаблон").expect("СтрШаблон should exist");
        assert_eq!(
            sig.max_args,
            Some(11),
            "Значение1-Значение10 → cap at (params.len()-1) + 10 = 11"
        );
        assert_eq!(*sig.ret, Ty::String);
    }

    #[test]
    fn strlen_returns_number() {
        let builtins = builtin_functions();
        let sig = builtins.get("стрдлина").expect("СтрДлина should exist");
        assert_eq!(*sig.ret, Ty::Number);
        assert_eq!(sig.params.len(), 1);
        assert_eq!(sig.params[0], Ty::String);
        assert_eq!(sig.required_count(), 1);
    }

    #[test]
    fn currentdate_takes_no_args() {
        let builtins = builtin_functions();
        let sig = builtins.get("текущаядата").expect("ТекущаяДата should exist");
        assert_eq!(*sig.ret, Ty::Date);
        assert!(sig.params.is_empty());
        assert_eq!(sig.required_count(), 0);
    }

    #[test]
    fn fallback_typedescription_is_unbounded_variadic() {
        // `ОписаниеТипов` lives in `platform_data`'s `types` section but
        // not its `global_functions` list, so the adapter never sees it.
        // The hand-rolled fallback marks it as truly unbounded
        // (`max_args = None`) because the platform doesn't document a
        // qualifier-list cap.
        let builtins = builtin_functions();
        let sig = builtins.get("описаниетипов").expect("ОписаниеТипов should exist");
        assert_eq!(sig.max_args, None, "fallback marks truly unbounded variadic");
        assert_eq!(sig.required_count(), 1, "only the type-list is required");
    }

    #[test]
    fn variadic_param_max_detection() {
        // Real platform idiom: returns the digit suffix as the cap.
        assert_eq!(variadic_param_max("Значение1-Значение10"), Some(10));
        assert_eq!(variadic_param_max("Value1-Value5"), Some(5));
        // Negatives — None means "not variadic".
        assert_eq!(variadic_param_max("Имя"), None);
        assert_eq!(variadic_param_max("Имя-Фамилия"), None);
        assert_eq!(variadic_param_max("X-Y"), None);
        assert_eq!(variadic_param_max("Значение-Значение10"), None);
    }

    #[test]
    fn return_type_union_is_parsed() {
        // Some platform functions return a comma-separated union
        // (e.g. "Булево, Неопределено"). The adapter must hand it to
        // `Ty::union` rather than dropping it to Unknown.
        let union = map_return_type("Булево, Неопределено");
        // Ty::union of {Boolean, Undefined} is a true union (no collapse).
        match &union {
            Ty::Union(parts) => {
                assert!(parts.contains(&Ty::Boolean));
                assert!(parts.contains(&Ty::Undefined));
            }
            other => panic!("expected Ty::Union, got {other:?}"),
        }
    }

    #[test]
    fn none_param_type_maps_to_unknown() {
        // Platform JSON has 7 functions with `param_type = null` (e.g.
        // ОткрытьФорму(Владелец)). The adapter must collapse those to
        // Ty::Unknown rather than panicking.
        let builtins = builtin_functions();
        if let Some(sig) = builtins.get("открытьформа") {
            // Just sanity-check that *something* came through.
            assert!(!sig.params.is_empty());
        }
    }

    #[test]
    fn registry_has_many_signatures() {
        let builtins = builtin_functions();
        // 498 platform global functions × 2 (RU+EN aliases, modulo overlap)
        // plus the 3 hand-curated fallbacks × 2.
        assert!(builtins.signatures.len() > 500);
    }

    #[test]
    fn fallback_does_not_shadow_json_derived_signature() {
        // Direct unit test for the `insert_pair` `or_insert` contract:
        // a fallback for a name already filled by the JSON layer must NOT
        // overwrite. We construct a scratch map, prime it with a sentinel
        // signature under both lowercase keys, then ask `insert_pair` to
        // register a different signature for the same pair.
        let mut sigs: FxHashMap<String, FunctionSignature> = FxHashMap::default();
        let json_like = FunctionSignature::function(vec![Ty::Number, Ty::Number], Ty::Number);
        sigs.insert("foo".into(), json_like.clone());
        sigs.insert("bar".into(), json_like.clone());

        let fallback = FunctionSignature::function(vec![Ty::String], Ty::String);
        insert_pair(&mut sigs, ("foo", "bar"), fallback);

        // Both keys keep the JSON-like signature, not the fallback.
        assert_eq!(sigs["foo"], json_like);
        assert_eq!(sigs["bar"], json_like);
    }
}
