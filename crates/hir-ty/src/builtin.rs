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
/// signature list.
///
/// **Multi-overload model.** Each name maps to a `Vec<FunctionSignature>`. The
/// vast majority of platform functions have a single-element vector, but a
/// handful (`ПодключитьВнешнююКомпоненту`, `Дата`, `ОткрытьФорму`, …) declare
/// several `<p class="V8SH_chapter">Вариант синтаксиса:</p>` sections in
/// the HBK source — those become one [`FunctionSignature`] per overload. A
/// call is accepted as soon as ANY overload's arity / type-check accepts it
/// (see consumers in `infer.rs`).
#[derive(Debug)]
pub struct BuiltinFunctions {
    /// Overload sets indexed by lowercase function name.
    signatures: FxHashMap<String, Vec<FunctionSignature>>,
}

impl BuiltinFunctions {
    /// Create and populate the built-in functions registry.
    fn new() -> Self {
        let mut signatures: FxHashMap<String, Vec<FunctionSignature>> = FxHashMap::default();

        // 1. Adapt every platform global function from the JSON-backed
        //    `bsl-platform` registry into a list of typed
        //    `FunctionSignature`s — one entry for single-overload
        //    functions, one per variant for multi-overload pages.
        let platform = bsl_platform::PlatformData::instance();
        for func in platform.all_global_functions() {
            let sigs = signatures_from_global_function(func);
            signatures.insert(func.name.to_lowercase(), sigs.clone());
            signatures.insert(func.english_name.to_lowercase(), sigs);
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

    /// Get the overload set for a function by name (case-insensitive).
    ///
    /// Returns the full overload list — callers that only need the first
    /// signature can use `.first()`. For multi-overload functions an arity /
    /// type check accepts the call when ANY overload accepts it.
    pub fn get(&self, name: &str) -> Option<&[FunctionSignature]> {
        let name_lower = name.to_lowercase();
        self.signatures.get(&name_lower).map(|v| v.as_slice())
    }
}

/// Convert a [`bsl_platform::GlobalFunction`] into one or more typed
/// [`FunctionSignature`]s — one per declared syntax variant.
///
/// Mapping rules (per signature):
/// - Each parameter's `param_type` is run through [`map_type_string`]; `None`
///   or unrecognised tokens collapse to `Ty::Unknown` (deliberately permissive
///   — `MismatchedArgCount` only checks arity, not assignability).
/// - `defaults[i]` mirrors `parameters[i].is_optional`.
/// - `max_args` is derived in this precedence (see [`signature_from_params`]):
///   1. **Explicit flag** — `last.is_variadic == true` lifts to `None`
///      (truly unbounded, e.g. `Мин`/`Макс`).
///   2. **Name idiom** — last param named `<word>N-<word>M` (e.g.
///      `Значение1-Значение10` for `СтрШаблон`) caps at
///      `(params.len() - 1) + M`.
///   3. **Fixed arity** — `Some(params.len())` otherwise.
/// - `return_type` may be a comma-separated union (`"Булево, Неопределено"`);
///   each piece is mapped individually and recombined via `Ty::union`. The
///   same return type is shared across all overloads — the platform JSON
///   does not carry per-variant return types today.
///
/// **Multi-overload functions.** When `func.variants` is non-empty (e.g.
/// `ПодключитьВнешнююКомпоненту` with its `По идентификатору` and
/// `По имени и местоположению` forms), every variant produces its own
/// signature. Callers in `infer.rs` accept a call if ANY of these
/// signatures accepts it. When `func.variants` is empty, a single
/// signature is built from the legacy flat `func.parameters` list — the
/// pre-overload behaviour.
fn signatures_from_global_function(func: &bsl_platform::GlobalFunction) -> Vec<FunctionSignature> {
    let ret = match &func.return_type {
        None => Ty::Undefined,
        Some(s) => map_return_type(s.as_str()),
    };

    if func.variants.is_empty() {
        return vec![signature_from_params(&func.parameters, ret)];
    }
    func.variants.iter().map(|v| signature_from_params(&v.parameters, ret.clone())).collect()
}

/// Build a single [`FunctionSignature`] from a flat parameter list and a
/// pre-mapped return type. Shared between single-overload and per-variant
/// paths.
///
/// Variadic-encoding precedence on the **last** parameter:
/// 1. **Explicit flag** — `last.is_variadic == true` → `max_args = None`
///    (truly unbounded tail, e.g. `Мин`/`Макс`/`ПродолжитьВызов`,
///    `Array.По количеству элементов`, two `COMSafeArray` ctors).
/// 2. **Name implies unbounded** — name shape `<word>N,...,<word>M` where
///    `M` is a literal letter (not a digit run) → `max_args = None`.
///    Used by `FormattedString.На основании строк` whose param name in
///    HBK is literally `Содержимое1,...,СодержимоеN`.
/// 3. **Name idiom (capped)** — `<word>N-<word>M` (dash) or
///    `<word>N,...,<word>M` (ellipsis) where `M` is a digit run →
///    `max_args = (params.len() - 1) + M`. The dash form covers
///    `СтрШаблон`'s `Значение1-Значение10`; the ellipsis form is reserved
///    for symmetry — no current corpus entry uses it.
/// 4. **Fixed arity** — otherwise `max_args = Some(params.len())`.
pub(crate) fn signature_from_params(
    params_in: &[bsl_platform::MethodParam],
    ret: Ty,
) -> FunctionSignature {
    let mut params = Vec::with_capacity(params_in.len());
    let mut defaults = Vec::with_capacity(params_in.len());
    for param in params_in {
        params.push(map_type_string(param.param_type.as_deref()));
        defaults.push(param.is_optional);
    }

    let last = params_in.last();
    let max_args = if last.is_some_and(|p| p.is_variadic)
        || last.is_some_and(|p| name_implies_unbounded_variadic(p.name.as_str()))
    {
        None
    } else if let Some(m) = last.and_then(|p| variadic_param_max(p.name.as_str())) {
        // The trailing variadic slot is one declared param that absorbs
        // `M` trailing args; the cap is `(params.len() - 1) + M`. Subtract
        // one to avoid double-counting the slot the param already
        // represents.
        Some((params.len() as u32).saturating_sub(1).saturating_add(m))
    } else {
        Some(params.len() as u32)
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

/// Split a capped-variadic name idiom into `(head, tail)` around either
/// the dash or the `,...,` separator. Returns `None` when neither
/// separator is present. Used by both [`variadic_param_max`] (capped,
/// digit suffix) and [`name_implies_unbounded_variadic`] (unbounded,
/// letter suffix).
fn split_variadic_name(name: &str) -> Option<(&str, &str)> {
    if let Some(idx) = name.find(",...,") {
        return Some((&name[..idx], &name[idx + ",...,".len()..]));
    }
    name.split_once('-')
}

/// `true` when the last param's name encodes an unbounded variadic with
/// a letter suffix instead of a digit cap, e.g. `Содержимое1,...,
/// СодержимоеN`. This is the in-name twin of [`bsl_platform::MethodParam::is_variadic`]
/// and is the only way `FormattedString.На основании строк` declares its
/// variadic shape today (HBK encodes the entire ellipsis inside ONE
/// rubric name; the page-level syntax `<Содержимое1,...,СодержимоеN>` is
/// a single bracket group, so PR2's `>,...,<` detector skipped it).
fn name_implies_unbounded_variadic(name: &str) -> bool {
    let Some((head, tail)) = split_variadic_name(name) else {
        return false;
    };
    let Some(digits_start) = head
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map(|(idx, _)| idx)
    else {
        return false;
    };
    let (head_word, head_digits) = head.split_at(digits_start);
    if head_word.is_empty() || head_digits.is_empty() {
        return false;
    }
    let tail = tail.trim_start();
    if !tail.starts_with(head_word) {
        return false;
    }
    let suffix = &tail[head_word.len()..];
    // Suffix must be a non-empty all-alphabetic token (e.g. `N`, `K`,
    // `енд`). Stricter than "no digits, ≥1 letter" — also rejects
    // mixed-punctuation tails like `-end` or `-` that the looser rule
    // would have admitted (Codex review 2026-04-29). Capped variants
    // (digit suffix) stay on [`variadic_param_max`] where they belong.
    !suffix.is_empty() && suffix.chars().all(|c| c.is_alphabetic())
}

/// Recognise the platform-help idiom for **capped** variadic last
/// parameters, `<имя>N-<имя>M` or `<имя>N,...,<имя>M` where `M` is a
/// digit run, returning the upper bound `M` (e.g. `10` for
/// `Значение1-Значение10`). Letter-suffix variants (`<имя>N`) are
/// covered by [`name_implies_unbounded_variadic`] instead.
///
/// All slicing here uses byte indices returned by `char_indices` — `rfind`
/// returns the start of a char and adding 1 to that offset would split a
/// multibyte UTF-8 sequence (e.g. Cyrillic `е` occupies bytes 14..16 in
/// `Значение1`).
fn variadic_param_max(name: &str) -> Option<u32> {
    let (head, tail) = split_variadic_name(name)?;
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
fn register_fallbacks(sigs: &mut FxHashMap<String, Vec<FunctionSignature>>) {
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
/// This is the fallback layer's contract: the JSON-derived overload set is
/// authoritative. If the platform extractor starts shipping a previously
/// missing name, our hand-rolled stub stays out of the way. The fallback
/// is single-overload by design — when a name matters enough to be
/// hand-curated, we have a specific shape in mind.
fn insert_pair(
    sigs: &mut FxHashMap<String, Vec<FunctionSignature>>,
    (ru, en): (&str, &str),
    sig: FunctionSignature,
) {
    sigs.entry(ru.to_string()).or_insert_with(|| vec![sig.clone()]);
    sigs.entry(en.to_string()).or_insert_with(|| vec![sig]);
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

    /// Helper: assert the lookup returns a single-element overload set and
    /// hand back the lone signature for further assertions. The vast majority
    /// of platform functions go through this path.
    fn single_signature<'a>(builtins: &'a BuiltinFunctions, name: &str) -> &'a FunctionSignature {
        let sigs = builtins.get(name).unwrap_or_else(|| panic!("{name} should exist"));
        assert_eq!(sigs.len(), 1, "{name} should be single-overload, got {} overloads", sigs.len());
        &sigs[0]
    }

    #[test]
    fn nstr_has_optional_second_parameter() {
        // The bug that drove the Slice 1 work — НСтр in `platform_data.json`
        // declares `КодЯзыка` with `is_optional=true`. Calling
        // `НСтр("ru = '...'", "ru")` must satisfy arity (required=1, total=2).
        let builtins = builtin_functions();
        let nstr = single_signature(builtins, "нстр");
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
        let sig = single_signature(builtins, "стршаблон");
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
        let sig = single_signature(builtins, "стрдлина");
        assert_eq!(*sig.ret, Ty::Number);
        assert_eq!(sig.params.len(), 1);
        assert_eq!(sig.params[0], Ty::String);
        assert_eq!(sig.required_count(), 1);
    }

    #[test]
    fn currentdate_takes_no_args() {
        let builtins = builtin_functions();
        let sig = single_signature(builtins, "текущаядата");
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
        let sig = single_signature(builtins, "описаниетипов");
        assert_eq!(sig.max_args, None, "fallback marks truly unbounded variadic");
        assert_eq!(sig.required_count(), 1, "only the type-list is required");
    }

    #[test]
    fn variadic_param_max_detection() {
        // Real platform idiom: returns the digit suffix as the cap.
        assert_eq!(variadic_param_max("Значение1-Значение10"), Some(10));
        assert_eq!(variadic_param_max("Value1-Value5"), Some(5));
        // Comma-ellipsis separator with digit suffix — capped variant
        // (no current corpus entry uses this shape, retained for symmetry
        // with the dash form).
        assert_eq!(variadic_param_max("Значение1,...,Значение10"), Some(10));
        // Negatives — None means "not capped via this idiom".
        assert_eq!(variadic_param_max("Имя"), None);
        assert_eq!(variadic_param_max("Имя-Фамилия"), None);
        assert_eq!(variadic_param_max("X-Y"), None);
        assert_eq!(variadic_param_max("Значение-Значение10"), None);
        // Letter-suffix shape belongs to `name_implies_unbounded_variadic`,
        // NOT here. Returning `None` here is correct.
        assert_eq!(variadic_param_max("Содержимое1,...,СодержимоеN"), None);
    }

    #[test]
    fn name_implies_unbounded_variadic_detection() {
        // `FormattedString.На основании строк` — only current
        // corpus consumer of this shape (param name in HBK literally
        // contains `,...,` separator and a letter suffix `N`).
        assert!(name_implies_unbounded_variadic("Содержимое1,...,СодержимоеN"));
        // Latin-letter equivalent — locks the "no digits in suffix" rule.
        assert!(name_implies_unbounded_variadic("Value1,...,ValueK"));
        // Capped variants must NOT trigger the unbounded detector.
        assert!(!name_implies_unbounded_variadic("Значение1-Значение10"));
        assert!(!name_implies_unbounded_variadic("Значение1,...,Значение10"));
        // No separator → not a variadic-name idiom at all.
        assert!(!name_implies_unbounded_variadic("Имя"));
        // Separator present but head has no digit suffix → not a
        // numbered family, can't be variadic.
        assert!(!name_implies_unbounded_variadic("Имя,...,Фамилия"));
        // Tail does not start with head_word — different families.
        assert!(!name_implies_unbounded_variadic("X1,...,Y2"));
        // Punctuation-only suffix must NOT flag — defensive against
        // exotic HBK encodings where the trailing token is `-` or
        // similar (Codex review 2026-04-29).
        assert!(!name_implies_unbounded_variadic("X1,...,X-"));
        assert!(!name_implies_unbounded_variadic("X1,...,X-end-"));
    }

    /// `signature_from_params` lifts `max_args = None` for the
    /// `<word>N,...,<word>N` letter-suffix shape, even when the
    /// `MethodParam.is_variadic` JSON flag is `false`. Locks the PR3
    /// Step 1 fix for `FormattedString.На основании строк`.
    #[test]
    fn name_implies_unbounded_lifts_signature_max_args() {
        let params = vec![bsl_platform::MethodParam {
            name: "Содержимое1,...,СодержимоеN".into(),
            param_type: Some("Произвольный".into()),
            is_optional: true,
            is_variadic: false,
        }];
        let sig = signature_from_params(&params, Ty::Unknown);
        assert_eq!(
            sig.max_args, None,
            "letter-suffix `,...,` name idiom must lift max_args to None"
        );
    }

    /// `is_variadic` on the last param lifts `max_args` to `None` even
    /// when the name idiom does not match. Models the `Мин`/`Макс`
    /// shape: one required param with name `Значение1`, no `-ЗначениеN`
    /// in the name — variadicity comes from the explicit flag.
    #[test]
    fn explicit_is_variadic_flag_yields_unbounded_max() {
        let params = vec![bsl_platform::MethodParam {
            name: "Значение1".into(),
            param_type: Some("Произвольный".into()),
            is_optional: false,
            is_variadic: true,
        }];
        let sig = signature_from_params(&params, Ty::Unknown);
        assert_eq!(sig.max_args, None, "is_variadic=true must lift the cap");
        assert_eq!(sig.required_count(), 1, "non-optional param stays required");
    }

    /// Flag wins over the name idiom: even if the param name matched the
    /// `Значение1-Значение10` shape, an explicit `is_variadic = true`
    /// must still yield `None` (truly unbounded > documented cap).
    #[test]
    fn is_variadic_flag_overrides_name_idiom() {
        let params = vec![bsl_platform::MethodParam {
            name: "Значение1-Значение10".into(),
            param_type: Some("Произвольный".into()),
            is_optional: false,
            is_variadic: true,
        }];
        let sig = signature_from_params(&params, Ty::Unknown);
        assert_eq!(sig.max_args, None, "explicit flag must override the cap idiom");
    }

    /// Regression guard for `НСтр`-shaped fixed-arity signatures: with
    /// `is_variadic = false` and no name idiom on the last param, the
    /// cap stays at `params.len()`. Protects PR1's "no behaviour change
    /// when JSON does not yet set the flag" promise.
    #[test]
    fn no_flag_no_idiom_preserves_fixed_arity() {
        let params = vec![
            bsl_platform::MethodParam {
                name: "Шаблон".into(),
                param_type: Some("Строка".into()),
                is_optional: false,
                is_variadic: false,
            },
            bsl_platform::MethodParam {
                name: "КодЯзыка".into(),
                param_type: Some("Строка".into()),
                is_optional: true,
                is_variadic: false,
            },
        ];
        let sig = signature_from_params(&params, Ty::String);
        assert_eq!(sig.max_args, Some(2), "fixed-arity cap stays at params.len()");
        assert_eq!(sig.required_count(), 1, "optional second param drops required to 1");
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
        // Ty::Unknown rather than panicking. ОткрытьФорму is also a
        // multi-overload page, so the lookup may return several
        // signatures — at least one must have parameters.
        let builtins = builtin_functions();
        if let Some(sigs) = builtins.get("открытьформа") {
            assert!(
                sigs.iter().any(|s| !s.params.is_empty()),
                "at least one ОткрытьФорму overload must have params"
            );
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
        // overload set under both lowercase keys, then ask `insert_pair`
        // to register a different signature for the same pair.
        let mut sigs: FxHashMap<String, Vec<FunctionSignature>> = FxHashMap::default();
        let json_like = FunctionSignature::function(vec![Ty::Number, Ty::Number], Ty::Number);
        sigs.insert("foo".into(), vec![json_like.clone()]);
        sigs.insert("bar".into(), vec![json_like.clone()]);

        let fallback = FunctionSignature::function(vec![Ty::String], Ty::String);
        insert_pair(&mut sigs, ("foo", "bar"), fallback);

        // Both keys keep the JSON-like overload set, not the fallback.
        assert_eq!(sigs["foo"], vec![json_like.clone()]);
        assert_eq!(sigs["bar"], vec![json_like]);
    }
}
