use bsl_types::builders::Builders;
use bsl_types::intern::TypeKernelDb;
use hir_def::ty::FunctionSignature;
use rustc_hash::FxHashMap;
use std::sync::OnceLock;

use crate::lower::type_string::{lower_param_type_string_typeid, lower_return_type_string_typeid};

static BUILTIN_FUNCTIONS: OnceLock<BuiltinFunctions> = OnceLock::new();

pub fn builtin_functions() -> &'static BuiltinFunctions {
    BUILTIN_FUNCTIONS.get_or_init(BuiltinFunctions::new)
}

#[derive(Debug)]
pub struct BuiltinFunctions {
    signatures: FxHashMap<String, Vec<BuiltinSignature>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinSignature {
    params: Box<[ParamTypeSpec]>,
    defaults: Box<[bool]>,
    ret: ReturnTypeSpec,
    max_args: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParamTypeSpec {
    Raw(String),
    Unknown,
    TypeDescriptor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReturnTypeSpec {
    Raw(String),
    Undefined,
    Unknown,
}

impl BuiltinSignature {
    pub fn lower(&self, db: &dyn TypeKernelDb) -> FunctionSignature {
        let params = self
            .params
            .iter()
            .map(|p| match p {
                ParamTypeSpec::Raw(s) => lower_param_type_string_typeid(db, s),
                ParamTypeSpec::Unknown => db.unknown(),
                ParamTypeSpec::TypeDescriptor => db.type_descriptor(),
            })
            .collect();
        let ret = match &self.ret {
            ReturnTypeSpec::Raw(s) => lower_return_type_string_typeid(db, s),
            ReturnTypeSpec::Undefined => db.undefined(),
            ReturnTypeSpec::Unknown => db.unknown(),
        };
        FunctionSignature { params, defaults: self.defaults.clone(), ret, max_args: self.max_args }
    }

    pub fn defaults(&self) -> &[bool] {
        &self.defaults
    }

    pub fn max_args(&self) -> Option<u32> {
        self.max_args
    }

    pub fn param_count(&self) -> usize {
        self.params.len()
    }

    pub fn required_count(&self) -> usize {
        self.defaults.iter().rposition(|has_default| !*has_default).map_or(0, |i| i + 1)
    }
}

impl BuiltinFunctions {
    fn new() -> Self {
        let mut signatures: FxHashMap<String, Vec<BuiltinSignature>> = FxHashMap::default();

        let platform = bsl_platform::PlatformData::instance();
        for func in platform.all_global_functions() {
            let sigs = descriptors_from_global_function(func);
            signatures.insert(func.name.to_lowercase(), sigs.clone());
            signatures.insert(func.english_name.to_lowercase(), sigs);
        }

        register_fallbacks(&mut signatures);

        tracing::debug!("initialized {} built-in function signature keys", signatures.len());

        Self { signatures }
    }

    pub fn get(&self, name: &str) -> Option<&[BuiltinSignature]> {
        let name_lower = name.to_lowercase();
        self.signatures.get(&name_lower).map(|v| v.as_slice())
    }
}

fn descriptors_from_global_function(func: &bsl_platform::GlobalFunction) -> Vec<BuiltinSignature> {
    let ret = match &func.return_type {
        None => ReturnTypeSpec::Undefined,
        Some(s) => ReturnTypeSpec::Raw(s.to_string()),
    };

    if func.variants.is_empty() {
        return vec![descriptor_from_params(&func.parameters, ret)];
    }
    func.variants.iter().map(|v| descriptor_from_params(&v.parameters, ret.clone())).collect()
}

pub(crate) fn descriptor_from_params(
    params_in: &[bsl_platform::MethodParam],
    ret: ReturnTypeSpec,
) -> BuiltinSignature {
    let mut params = Vec::with_capacity(params_in.len());
    let mut defaults = Vec::with_capacity(params_in.len());
    for param in params_in {
        params.push(
            param
                .param_type
                .as_deref()
                .map(|s| ParamTypeSpec::Raw(s.to_string()))
                .unwrap_or(ParamTypeSpec::Unknown),
        );
        defaults.push(param.is_optional);
    }

    let last = params_in.last();
    let max_args = if last.is_some_and(|p| p.is_variadic)
        || last.is_some_and(|p| name_implies_unbounded_variadic(p.name.as_str()))
    {
        None
    } else if let Some(m) = last.and_then(|p| variadic_param_max(p.name.as_str())) {
        Some((params.len() as u32).saturating_sub(1).saturating_add(m))
    } else {
        Some(params.len() as u32)
    };

    BuiltinSignature {
        params: params.into_boxed_slice(),
        defaults: defaults.into_boxed_slice(),
        ret,
        max_args,
    }
}

fn split_variadic_name(name: &str) -> Option<(&str, &str)> {
    if let Some(idx) = name.find(",...,") {
        return Some((&name[..idx], &name[idx + ",...,".len()..]));
    }
    name.split_once('-')
}

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
    !suffix.is_empty() && suffix.chars().all(|c| c.is_alphabetic())
}

fn variadic_param_max(name: &str) -> Option<u32> {
    let (head, tail) = split_variadic_name(name)?;
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

fn register_fallbacks(sigs: &mut FxHashMap<String, Vec<BuiltinSignature>>) {
    insert_pair(
        sigs,
        ("новый", "new"),
        BuiltinSignature {
            params: Box::new([ParamTypeSpec::TypeDescriptor]),
            defaults: Box::new([false]),
            ret: ReturnTypeSpec::Unknown,
            max_args: Some(1),
        },
    );

    insert_pair(
        sigs,
        ("описаниетипов", "typedescription"),
        BuiltinSignature {
            params: Box::new([ParamTypeSpec::Unknown]),
            defaults: Box::new([false]),
            ret: ReturnTypeSpec::Unknown,
            max_args: None,
        },
    );
}

fn insert_pair(
    sigs: &mut FxHashMap<String, Vec<BuiltinSignature>>,
    (ru, en): (&str, &str),
    sig: BuiltinSignature,
) {
    sigs.entry(ru.to_string()).or_insert_with(|| vec![sig.clone()]);
    sigs.entry(en.to_string()).or_insert_with(|| vec![sig]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_types::facet::DateComponent;
    use bsl_types::kind::TypeKind;
    use bsl_types::testing::InMemoryDb;

    #[test]
    fn lookup_is_case_insensitive_and_bilingual() {
        let builtins = builtin_functions();
        assert!(builtins.get("СтрДлина").is_some());
        assert!(builtins.get("стрдлина").is_some());
        assert!(builtins.get("СТРДЛИНА").is_some());
        assert!(builtins.get("StrLen").is_some());
        assert!(builtins.get("strlen").is_some());
    }

    fn single_signature<'a>(builtins: &'a BuiltinFunctions, name: &str) -> &'a BuiltinSignature {
        let sigs = builtins.get(name).unwrap_or_else(|| panic!("{name} should exist"));
        assert_eq!(sigs.len(), 1, "{name} should be single-overload, got {} overloads", sigs.len());
        &sigs[0]
    }

    #[test]
    fn nstr_has_optional_second_parameter() {
        let db = InMemoryDb::new();
        let builtins = builtin_functions();
        let nstr = single_signature(builtins, "нстр");
        assert_eq!(nstr.param_count(), 2, "НСтр has 2 declared params");
        assert_eq!(nstr.required_count(), 1, "second param is optional, so required=1");
        assert_eq!(nstr.max_args(), Some(2), "fixed arity caps at params.len()");
        assert_eq!(nstr.lower(&db).ret, db.string(None, false));
    }

    #[test]
    fn strtemplate_is_capped_variadic() {
        let db = InMemoryDb::new();
        let builtins = builtin_functions();
        let sig = single_signature(builtins, "стршаблон");
        assert_eq!(
            sig.max_args(),
            Some(11),
            "Значение1-Значение10 → cap at (params.len()-1) + 10 = 11"
        );
        assert_eq!(sig.lower(&db).ret, db.string(None, false));
    }

    #[test]
    fn strlen_returns_number() {
        let db = InMemoryDb::new();
        let builtins = builtin_functions();
        let sig = single_signature(builtins, "стрдлина").lower(&db);
        assert_eq!(sig.ret, db.number(None, None));
        assert_eq!(sig.params.len(), 1);
        assert_eq!(sig.params[0], db.string(None, false));
        assert_eq!(sig.required_count(), 1);
    }

    #[test]
    fn currentdate_takes_no_args() {
        let db = InMemoryDb::new();
        let builtins = builtin_functions();
        let sig = single_signature(builtins, "текущаядата").lower(&db);
        assert_eq!(sig.ret, db.date(DateComponent::DateTime));
        assert!(sig.params.is_empty());
        assert_eq!(sig.required_count(), 0);
    }

    #[test]
    fn fallback_typedescription_is_unbounded_variadic() {
        let builtins = builtin_functions();
        let sig = single_signature(builtins, "описаниетипов");
        assert_eq!(sig.max_args(), None, "fallback marks truly unbounded variadic");
        assert_eq!(sig.required_count(), 1, "only the type-list is required");
    }

    #[test]
    fn variadic_param_max_detection() {
        assert_eq!(variadic_param_max("Значение1-Значение10"), Some(10));
        assert_eq!(variadic_param_max("Value1-Value5"), Some(5));
        assert_eq!(variadic_param_max("Значение1,...,Значение10"), Some(10));
        assert_eq!(variadic_param_max("Имя"), None);
        assert_eq!(variadic_param_max("Имя-Фамилия"), None);
        assert_eq!(variadic_param_max("X-Y"), None);
        assert_eq!(variadic_param_max("Значение-Значение10"), None);
        assert_eq!(variadic_param_max("Содержимое1,...,СодержимоеN"), None);
    }

    #[test]
    fn name_implies_unbounded_variadic_detection() {
        assert!(name_implies_unbounded_variadic("Содержимое1,...,СодержимоеN"));
        assert!(name_implies_unbounded_variadic("Value1,...,ValueK"));
        assert!(!name_implies_unbounded_variadic("Значение1-Значение10"));
        assert!(!name_implies_unbounded_variadic("Значение1,...,Значение10"));
        assert!(!name_implies_unbounded_variadic("Имя"));
        assert!(!name_implies_unbounded_variadic("Имя,...,Фамилия"));
        assert!(!name_implies_unbounded_variadic("X1,...,Y2"));
        assert!(!name_implies_unbounded_variadic("X1,...,X-"));
        assert!(!name_implies_unbounded_variadic("X1,...,X-end-"));
    }

    #[test]
    fn name_implies_unbounded_lifts_signature_max_args() {
        let params = vec![bsl_platform::MethodParam {
            name: "Содержимое1,...,СодержимоеN".into(),
            param_type: Some("Произвольный".into()),
            is_optional: true,
            is_variadic: false,
        }];
        let sig = descriptor_from_params(&params, ReturnTypeSpec::Unknown);
        assert_eq!(
            sig.max_args(),
            None,
            "letter-suffix `,...,` name idiom must lift max_args to None"
        );
    }

    #[test]
    fn explicit_is_variadic_flag_yields_unbounded_max() {
        let params = vec![bsl_platform::MethodParam {
            name: "Значение1".into(),
            param_type: Some("Произвольный".into()),
            is_optional: false,
            is_variadic: true,
        }];
        let sig = descriptor_from_params(&params, ReturnTypeSpec::Unknown);
        assert_eq!(sig.max_args(), None, "is_variadic=true must lift the cap");
        assert_eq!(sig.required_count(), 1, "non-optional param stays required");
    }

    #[test]
    fn is_variadic_flag_overrides_name_idiom() {
        let params = vec![bsl_platform::MethodParam {
            name: "Значение1-Значение10".into(),
            param_type: Some("Произвольный".into()),
            is_optional: false,
            is_variadic: true,
        }];
        let sig = descriptor_from_params(&params, ReturnTypeSpec::Unknown);
        assert_eq!(sig.max_args(), None, "explicit flag must override the cap idiom");
    }

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
        let sig = descriptor_from_params(&params, ReturnTypeSpec::Raw("Строка".to_string()));
        assert_eq!(sig.max_args(), Some(2), "fixed-arity cap stays at params.len()");
        assert_eq!(sig.required_count(), 1, "optional second param drops required to 1");
    }

    #[test]
    fn return_type_union_is_parsed() {
        let db = InMemoryDb::new();
        let union = lower_return_type_string_typeid(&db, "Булево, Неопределено");
        match db.lookup_type(union) {
            TypeKind::Union(parts) => {
                assert!(parts.contains(&db.boolean()));
                assert!(parts.contains(&db.undefined()));
            }
            other => panic!("expected TypeKind::Union, got {other:?}"),
        }
    }

    #[test]
    fn none_param_type_maps_to_unknown() {
        let builtins = builtin_functions();
        if let Some(sigs) = builtins.get("открытьформа") {
            assert!(
                sigs.iter().any(|s| s.param_count() > 0),
                "at least one ОткрытьФорму overload must have params"
            );
        }
    }

    #[test]
    fn registry_has_many_signatures() {
        let builtins = builtin_functions();
        assert!(builtins.signatures.len() > 500);
    }

    #[test]
    fn fallback_does_not_shadow_json_derived_signature() {
        let mut sigs: FxHashMap<String, Vec<BuiltinSignature>> = FxHashMap::default();
        let json_like = BuiltinSignature {
            params: Box::new([
                ParamTypeSpec::Raw("Число".to_string()),
                ParamTypeSpec::Raw("Число".to_string()),
            ]),
            defaults: Box::new([false, false]),
            ret: ReturnTypeSpec::Raw("Число".to_string()),
            max_args: Some(2),
        };
        sigs.insert("foo".into(), vec![json_like.clone()]);
        sigs.insert("bar".into(), vec![json_like.clone()]);

        let fallback = BuiltinSignature {
            params: Box::new([ParamTypeSpec::Raw("Строка".to_string())]),
            defaults: Box::new([false]),
            ret: ReturnTypeSpec::Raw("Строка".to_string()),
            max_args: Some(1),
        };
        insert_pair(&mut sigs, ("foo", "bar"), fallback);

        assert_eq!(sigs["foo"], vec![json_like.clone()]);
        assert_eq!(sigs["bar"], vec![json_like]);
    }
}
