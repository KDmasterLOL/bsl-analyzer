use crate::types::{
    ConstructorDocs, GlobalFunction, PlatformConstructor, PlatformMethod, PlatformProperty,
    PlatformType, PropertyDocs,
};
use once_cell::sync::OnceCell;
use rustc_hash::FxHashMap;
use smol_str::SmolStr;
use std::sync::Arc;

static PLATFORM_DATA_SINGLETON: OnceCell<PlatformDataInner> = OnceCell::new();

pub const GLOBAL_CONTEXT_OWNER: &str = "Global context";

pub struct PlatformDataInner {
    types: Vec<PlatformType>,
    types_by_name: FxHashMap<SmolStr, usize>,
    methods: Vec<PlatformMethod>,
    methods_by_name: FxHashMap<(SmolStr, SmolStr), usize>,
    global_functions: Vec<GlobalFunction>,
    global_functions_by_name: FxHashMap<SmolStr, usize>,
    method_docs_by_id: FxHashMap<u32, usize>,
    global_function_docs_by_id: FxHashMap<u32, usize>,
    constructors: Vec<PlatformConstructor>,
    constructors_by_type: FxHashMap<SmolStr, Vec<usize>>,
    constructor_docs_by_id: FxHashMap<u32, usize>,
    properties: Vec<PlatformProperty>,
    properties_by_name: FxHashMap<(SmolStr, SmolStr), usize>,
    property_docs_by_id: FxHashMap<u32, usize>,
    global_properties_by_name: FxHashMap<SmolStr, usize>,
}

impl PlatformDataInner {
    pub fn instance() -> &'static Self {
        PLATFORM_DATA_SINGLETON.get_or_init(Self::new)
    }

    fn new() -> Self {
        let types: Vec<PlatformType> =
            crate::generated::PLATFORM_TYPES.iter().map(PlatformType::from).collect();

        let mut types_by_name = FxHashMap::default();

        // Several types can declare the same XDTO name (e.g. `Field`, `Parameter`
        // across data-composition types); such an alias is ambiguous and must not
        // be indexed, or it would resolve to an arbitrary one of them.
        let mut xdto_counts: FxHashMap<SmolStr, u32> = FxHashMap::default();
        for ty in &types {
            if let Some(xdto) = &ty.xdto_name {
                *xdto_counts.entry(xdto.to_lowercase().into()).or_insert(0) += 1;
            }
        }

        for (idx, ty) in types.iter().enumerate() {
            let ru_key: SmolStr = ty.name.to_lowercase().into();
            let en_key: SmolStr = ty.english_name.to_lowercase().into();
            types_by_name.insert(ru_key, idx);
            types_by_name.insert(en_key, idx);
            // The XDTO name is an additional, non-overriding alias: index it only
            // when unambiguous, and never let it shadow a type's own key.
            if let Some(xdto) = &ty.xdto_name {
                let key: SmolStr = xdto.to_lowercase().into();
                if xdto_counts.get(&key) == Some(&1) {
                    types_by_name.entry(key).or_insert(idx);
                }
            }
        }

        let methods: Vec<PlatformMethod> =
            crate::generated::PLATFORM_METHODS.iter().map(PlatformMethod::from).collect();

        let mut methods_by_name = FxHashMap::default();

        let mut type_en_to_ru: FxHashMap<SmolStr, SmolStr> = FxHashMap::default();
        for ty in &types {
            let en_key: SmolStr = ty.english_name.to_lowercase().into();
            let ru_key: SmolStr = ty.name.to_lowercase().into();
            type_en_to_ru.insert(en_key, ru_key);
        }

        for (idx, method) in methods.iter().enumerate() {
            let en_type_key: SmolStr = method.type_name.to_lowercase().into();
            let ru_method_key: SmolStr = method.name.to_lowercase().into();
            let en_method_key: SmolStr = method.english_name.to_lowercase().into();

            methods_by_name.insert((en_type_key.clone(), ru_method_key.clone()), idx);
            methods_by_name.insert((en_type_key.clone(), en_method_key.clone()), idx);

            if let Some(ru_type_key) = type_en_to_ru.get(&en_type_key) {
                methods_by_name.insert((ru_type_key.clone(), ru_method_key), idx);
                methods_by_name.insert((ru_type_key.clone(), en_method_key), idx);
            }
        }

        let global_functions: Vec<GlobalFunction> =
            crate::generated::PLATFORM_GLOBAL_FUNCTIONS.iter().map(GlobalFunction::from).collect();

        let mut global_functions_by_name = FxHashMap::default();

        for (idx, function) in global_functions.iter().enumerate() {
            let ru_key: SmolStr = function.name.to_lowercase().into();
            let en_key: SmolStr = function.english_name.to_lowercase().into();

            global_functions_by_name.insert(ru_key, idx);
            global_functions_by_name.insert(en_key, idx);
        }

        let mut method_docs_by_id = FxHashMap::default();
        for (idx, docs) in crate::generated::METHOD_DOCS.iter().enumerate() {
            method_docs_by_id.insert(docs.method_id, idx);
        }

        let mut global_function_docs_by_id = FxHashMap::default();
        for (idx, docs) in crate::generated::GLOBAL_FUNCTION_DOCS.iter().enumerate() {
            global_function_docs_by_id.insert(docs.method_id, idx);
        }

        let mut constructors: Vec<PlatformConstructor> =
            crate::generated::PLATFORM_CONSTRUCTORS.iter().map(PlatformConstructor::from).collect();

        apply_docs_only_variadic_overlay(&mut constructors);

        let mut constructors_by_type: FxHashMap<SmolStr, Vec<usize>> = FxHashMap::default();
        for (idx, ctor) in constructors.iter().enumerate() {
            let en_key: SmolStr = ctor.type_name.to_lowercase().into();
            constructors_by_type.entry(en_key.clone()).or_default().push(idx);
            if let Some(ru_key) = type_en_to_ru.get(&en_key) {
                constructors_by_type.entry(ru_key.clone()).or_default().push(idx);
            }
        }

        let mut constructor_docs_by_id = FxHashMap::default();
        for (idx, docs) in crate::generated::CONSTRUCTOR_DOCS.iter().enumerate() {
            constructor_docs_by_id.insert(docs.constructor_id, idx);
        }

        let properties: Vec<PlatformProperty> =
            crate::generated::PLATFORM_PROPERTIES.iter().map(PlatformProperty::from).collect();

        let mut properties_by_name = FxHashMap::default();
        for (idx, prop) in properties.iter().enumerate() {
            let en_type_key: SmolStr = prop.type_name.to_lowercase().into();
            let ru_prop_key: SmolStr = prop.name.to_lowercase().into();
            let en_prop_key: SmolStr = prop.english_name.to_lowercase().into();

            properties_by_name.insert((en_type_key.clone(), ru_prop_key.clone()), idx);
            properties_by_name.insert((en_type_key.clone(), en_prop_key.clone()), idx);

            if let Some(ru_type_key) = type_en_to_ru.get(&en_type_key) {
                properties_by_name.insert((ru_type_key.clone(), ru_prop_key), idx);
                properties_by_name.insert((ru_type_key.clone(), en_prop_key), idx);
            }
        }

        let mut property_docs_by_id = FxHashMap::default();
        for (idx, docs) in crate::generated::PROPERTY_DOCS.iter().enumerate() {
            property_docs_by_id.insert(docs.property_id, idx);
        }

        let mut global_properties_by_name = FxHashMap::default();
        for (idx, prop) in properties.iter().enumerate() {
            if prop.type_name.as_str() != GLOBAL_CONTEXT_OWNER {
                continue;
            }
            let ru_key: SmolStr = prop.name.to_lowercase().into();
            let en_key: SmolStr = prop.english_name.to_lowercase().into();
            global_properties_by_name.insert(ru_key, idx);
            global_properties_by_name.insert(en_key, idx);
        }

        Self {
            types,
            types_by_name,
            methods,
            methods_by_name,
            global_functions,
            global_functions_by_name,
            method_docs_by_id,
            global_function_docs_by_id,
            constructors,
            constructors_by_type,
            constructor_docs_by_id,
            properties,
            properties_by_name,
            property_docs_by_id,
            global_properties_by_name,
        }
    }

    pub fn get_type(&self, name: &str) -> Option<&PlatformType> {
        let key: SmolStr = name.to_lowercase().into();
        let idx = *self.types_by_name.get(&key)?;
        self.types.get(idx)
    }

    pub fn all_types(&self) -> &[PlatformType] {
        &self.types
    }

    pub fn get_method(&self, type_name: &str, method_name: &str) -> Option<&PlatformMethod> {
        let type_key: SmolStr = type_name.to_lowercase().into();
        let method_key: SmolStr = method_name.to_lowercase().into();
        let idx = *self.methods_by_name.get(&(type_key, method_key))?;
        self.methods.get(idx)
    }

    pub fn all_methods(&self) -> &[PlatformMethod] {
        &self.methods
    }

    pub fn get_type_methods(&self, type_name: &str) -> Vec<&PlatformMethod> {
        let type_key: SmolStr = type_name.to_lowercase().into();
        let en_type_key = if let Some(idx) = self.types_by_name.get(&type_key) {
            self.types[*idx].english_name.to_lowercase().into()
        } else {
            type_key
        };
        self.methods.iter().filter(|m| m.type_name.to_lowercase() == en_type_key.as_str()).collect()
    }

    pub fn get_manager_methods(&self, manager_prefix: &str) -> Vec<&PlatformMethod> {
        let prefix = format!("{}.", manager_prefix.to_lowercase());
        self.methods.iter().filter(|m| m.type_name.to_lowercase().starts_with(&prefix)).collect()
    }

    pub fn get_global_function(&self, name: &str) -> Option<&GlobalFunction> {
        let key: SmolStr = name.to_lowercase().into();
        let idx = *self.global_functions_by_name.get(&key)?;
        self.global_functions.get(idx)
    }

    pub fn all_global_functions(&self) -> &[GlobalFunction] {
        &self.global_functions
    }

    pub fn get_method_docs(&self, method_id: u32) -> Option<crate::types::MethodDocs> {
        let idx = *self.method_docs_by_id.get(&method_id)?;
        let raw_docs = crate::generated::METHOD_DOCS.get(idx)?;
        Some(crate::types::MethodDocs::from(raw_docs))
    }

    pub fn get_constructors(&self, type_name: &str) -> Vec<&PlatformConstructor> {
        let key: SmolStr = type_name.to_lowercase().into();
        match self.constructors_by_type.get(&key) {
            Some(indices) => indices.iter().filter_map(|&i| self.constructors.get(i)).collect(),
            None => Vec::new(),
        }
    }

    pub fn all_constructors(&self) -> &[PlatformConstructor] {
        &self.constructors
    }

    pub fn get_constructor_docs(&self, constructor_id: u32) -> Option<ConstructorDocs> {
        let idx = *self.constructor_docs_by_id.get(&constructor_id)?;
        let raw = crate::generated::CONSTRUCTOR_DOCS.get(idx)?;
        Some(ConstructorDocs::from(raw))
    }

    pub fn get_global_function_docs(&self, function_id: u32) -> Option<crate::types::MethodDocs> {
        let idx = *self.global_function_docs_by_id.get(&function_id)?;
        let raw_docs = crate::generated::GLOBAL_FUNCTION_DOCS.get(idx)?;
        Some(crate::types::MethodDocs::from(raw_docs))
    }

    pub fn get_property(&self, type_name: &str, prop_name: &str) -> Option<&PlatformProperty> {
        let type_key: SmolStr = type_name.to_lowercase().into();
        let prop_key: SmolStr = prop_name.to_lowercase().into();
        let idx = *self.properties_by_name.get(&(type_key, prop_key))?;
        self.properties.get(idx)
    }

    pub fn get_type_properties(&self, type_name: &str) -> Vec<&PlatformProperty> {
        let type_key: SmolStr = type_name.to_lowercase().into();
        let en_type_key = if let Some(idx) = self.types_by_name.get(&type_key) {
            self.types[*idx].english_name.to_lowercase().into()
        } else {
            type_key
        };
        self.properties
            .iter()
            .filter(|p| p.type_name.to_lowercase() == en_type_key.as_str())
            .collect()
    }

    pub fn get_manager_properties(&self, manager_prefix: &str) -> Vec<&PlatformProperty> {
        let prefix = format!("{}.", manager_prefix.to_lowercase());
        self.properties.iter().filter(|p| p.type_name.to_lowercase().starts_with(&prefix)).collect()
    }

    pub fn all_properties(&self) -> &[PlatformProperty] {
        &self.properties
    }

    pub fn get_global_property(&self, name: &str) -> Option<&PlatformProperty> {
        let key: SmolStr = name.to_lowercase().into();
        let idx = *self.global_properties_by_name.get(&key)?;
        self.properties.get(idx)
    }

    pub fn all_global_properties(&self) -> Vec<&PlatformProperty> {
        let mut seen: Vec<usize> = self.global_properties_by_name.values().copied().collect();
        seen.sort_unstable();
        seen.dedup();
        seen.into_iter().filter_map(|i| self.properties.get(i)).collect()
    }

    pub fn resolve_global_member(
        &self,
        global_name: &str,
        member_name: &str,
    ) -> Option<&PlatformMethod> {
        let prop = self.get_global_property(global_name)?;
        let declared_type = prop.property_types.first()?;
        self.get_method(declared_type.as_str(), member_name)
    }

    pub fn get_property_docs(&self, property_id: u32) -> Option<PropertyDocs> {
        let idx = *self.property_docs_by_id.get(&property_id)?;
        let raw = crate::generated::PROPERTY_DOCS.get(idx)?;
        Some(PropertyDocs::from(raw))
    }

    pub fn get_keyword_docs(&self, keyword: &str) -> Option<crate::types::KeywordDocs> {
        get_keyword_docs_static(keyword)
    }
}

fn apply_docs_only_variadic_overlay(constructors: &mut [PlatformConstructor]) {
    const OVERLAY: &[(&str, &str)] = &[
        ("Structure", "По ключам и значениям"),
        ("FixedStructure", "По ключам и значениям"),
        ("DynamicListRowKey", "На основе путей и значений полей"),
    ];

    for ctor in constructors.iter_mut() {
        let variant = ctor.variant_name.as_deref().unwrap_or("");
        if !OVERLAY.iter().any(|(t, v)| *t == ctor.type_name.as_str() && *v == variant) {
            continue;
        }
        if let Some(last) = ctor.parameters.last_mut() {
            last.is_variadic = true;
        }
    }
}

fn get_keyword_docs_static(keyword: &str) -> Option<crate::types::KeywordDocs> {
    use crate::types::{KeywordDocs, ParamDocs};
    use smol_str::SmolStr;

    let keyword_lower = keyword.to_lowercase();

    match keyword_lower.as_str() {
        "вызватьисключение" | "raise" => Some(KeywordDocs {
            keyword_ru: SmolStr::new("ВызватьИсключение"),
            keyword_en: SmolStr::new("Raise"),
            syntax: "ВызватьИсключение;".to_string(),
            description: "Оператор позволяет вызвать исключение в тех случаях, когда несмотря на отработку исключительной ситуации операторами исключения необходимо прервать выполнение модуля с ошибкой времени выполнения.\n\nОператор допустим только внутри операторных скобок Исключение – КонецПопытки.\n\nВыполнение данного оператора прекращает выполнение последовательности операторов исключения и производит поиск более \"внешнего\" обработчика исключения (при вложенных попытках). Если таковой есть, то управление передается на его первый оператор. Если нет, то исключительная ситуация обрабатывается системно, выдается сообщение о первоначально возникшей ошибке, а выполнение модуля прекращается.".to_string(),
            params: vec![],
            min_version: Some("8.0".to_string()),
        }),
        "для" | "for" => Some(KeywordDocs {
            keyword_ru: SmolStr::new("Для"),
            keyword_en: SmolStr::new("For"),
            syntax: "Для <Имя переменной> = <Выражение 1> По <Выражение 2> Цикл\n    // Операторы\n    [Прервать;]\n    [Продолжить;]\nКонецЦикла;".to_string(),
            description: "Оператор цикла Для предназначен для циклического повторения операторов, находящихся внутри конструкции Цикл – КонецЦикла.\n\nПеред началом выполнения цикла значение <Выражение 1> присваивается переменной <Имя переменной>. Значение <Имя переменной> автоматически увеличивается при каждом проходе цикла. Величина приращения счетчика при каждом выполнении цикла равна 1.\n\nЦикл выполняется, пока значение переменной меньше или равно значению <Выражение 2>. Условие выполнения цикла всегда проверяется в начале, перед выполнением цикла.".to_string(),
            params: vec![
                ParamDocs {
                    name: SmolStr::new("Имя переменной"),
                    description: "Идентификатор переменной (счетчика цикла), значение которой автоматически увеличивается на 1 при каждом повторении цикла.".to_string(),
                    default_value: None,
                },
                ParamDocs {
                    name: SmolStr::new("Выражение 1"),
                    description: "Числовое выражение, которое задает начальное значение, присваиваемое счетчику цикла при первом проходе цикла.".to_string(),
                    default_value: None,
                },
                ParamDocs {
                    name: SmolStr::new("Выражение 2"),
                    description: "Максимальное значение счетчика цикла. Когда переменная становится больше чем <Выражение 2>, выполнение оператора цикла Для прекращается.".to_string(),
                    default_value: None,
                },
            ],
            min_version: Some("8.0".to_string()),
        }),
        "если" | "if" => Some(KeywordDocs {
            keyword_ru: SmolStr::new("Если"),
            keyword_en: SmolStr::new("If"),
            syntax: "Если <Условие> Тогда\n    // Операторы\n[ИначеЕсли <Условие> Тогда]\n    // Операторы\n[Иначе]\n    // Операторы\nКонецЕсли;".to_string(),
            description: "Условный оператор Если предназначен для организации выполнения или невыполнения некоторого набора операторов в зависимости от заданных условий.\n\nУсловие – это выражение булева типа. Если значение выражения равно Истина, то выполняются операторы, следующие за ключевым словом Тогда до ближайшего ключевого слова ИначеЕсли, Иначе или КонецЕсли.".to_string(),
            params: vec![
                ParamDocs {
                    name: SmolStr::new("Условие"),
                    description: "Выражение булева типа. Если значение равно Истина, выполняются операторы в соответствующем блоке.".to_string(),
                    default_value: None,
                },
            ],
            min_version: Some("8.0".to_string()),
        }),
        "попытка" | "try" => Some(KeywordDocs {
            keyword_ru: SmolStr::new("Попытка"),
            keyword_en: SmolStr::new("Try"),
            syntax: "Попытка\n    // Операторы попытки\nИсключение\n    // Операторы исключения\n    [ВызватьИсключение;]\nКонецПопытки;".to_string(),
            description: "Оператор Попытка управляет выполнением программы, основываясь на возникающих при выполнении модуля ошибочных (исключительных) ситуациях, и определяет обработку этих ситуаций.\n\nЕсли при выполнении последовательности операторов попытки произошла ошибка времени выполнения, то выполнение оператора, вызвавшего ошибку, прерывается и управление передается на первый оператор последовательности операторов исключения.\n\nКонструкции Попытка – Исключение – КонецПопытки могут быть вложенными.".to_string(),
            params: vec![],
            min_version: Some("8.0".to_string()),
        }),
        _ => None,
    }
}

pub type PlatformData = PlatformDataInner;

#[salsa::interned(debug)]
pub struct TypeNameInput {
    pub name: String,
}

#[salsa::interned(debug)]
pub struct MethodLookupInput {
    pub type_name: String,
    pub method_name: String,
}

#[salsa::interned(debug)]
pub struct PrefixedMethodLookupInput {
    pub prefix: String,
    pub method_name: String,
}

#[salsa::tracked(lru = 256)]
pub fn platform_type_query<'db>(
    db: &'db dyn salsa::Database,
    input: TypeNameInput<'db>,
) -> Option<PlatformType> {
    let name = input.name(db);
    let data = PlatformDataInner::instance();
    data.get_type(&name).cloned()
}

#[salsa::tracked(lru = 256)]
pub fn platform_method_query<'db>(
    db: &'db dyn salsa::Database,
    input: MethodLookupInput<'db>,
) -> Option<PlatformMethod> {
    let type_name = input.type_name(db);
    let method_name = input.method_name(db);
    let data = PlatformDataInner::instance();
    data.get_method(&type_name, &method_name).cloned()
}

#[salsa::tracked(lru = 128)]
pub fn type_methods_query<'db>(
    db: &'db dyn salsa::Database,
    input: TypeNameInput<'db>,
) -> Arc<Vec<PlatformMethod>> {
    let type_name = input.name(db);
    let data = PlatformDataInner::instance();
    Arc::new(data.get_type_methods(&type_name).into_iter().cloned().collect())
}

#[salsa::tracked(lru = 128)]
pub fn manager_methods_query<'db>(
    db: &'db dyn salsa::Database,
    input: TypeNameInput<'db>,
) -> Arc<Vec<PlatformMethod>> {
    let prefix = input.name(db);
    let data = PlatformDataInner::instance();
    Arc::new(data.get_manager_methods(&prefix).into_iter().cloned().collect())
}

#[salsa::tracked(lru = 256)]
pub fn prefixed_method_query<'db>(
    db: &'db dyn salsa::Database,
    input: PrefixedMethodLookupInput<'db>,
) -> Option<PlatformMethod> {
    let prefix = input.prefix(db);
    let method_name = input.method_name(db);
    find_prefixed_method(&prefix, &method_name)
}

pub fn find_prefixed_method(prefix: &str, method_name: &str) -> Option<PlatformMethod> {
    let method_lower = method_name.to_lowercase();
    let data = PlatformDataInner::instance();
    data.get_manager_methods(prefix)
        .into_iter()
        .find(|m| {
            let docs = data.get_method_docs(m.id);
            let ru_match = docs
                .as_ref()
                .and_then(|d| d.syntax.split('(').next())
                .is_some_and(|ru| ru.to_lowercase() == method_lower);
            if ru_match {
                return true;
            }
            let en_name =
                m.english_name.rsplit_once('.').map(|(_, n)| n).unwrap_or(&m.english_name);
            en_name.to_lowercase() == method_lower
        })
        .cloned()
}

#[salsa::tracked(lru = 256)]
pub fn global_function_query<'db>(
    db: &'db dyn salsa::Database,
    input: TypeNameInput<'db>,
) -> Option<GlobalFunction> {
    let name = input.name(db);
    let data = PlatformDataInner::instance();
    data.get_global_function(&name).cloned()
}

#[salsa::tracked(lru = 128)]
pub fn platform_constructors_query<'db>(
    db: &'db dyn salsa::Database,
    input: TypeNameInput<'db>,
) -> Arc<Vec<PlatformConstructor>> {
    let type_name = input.name(db);
    let data = PlatformDataInner::instance();
    Arc::new(data.get_constructors(&type_name).into_iter().cloned().collect())
}

#[salsa::tracked(lru = 256)]
pub fn platform_property_query<'db>(
    db: &'db dyn salsa::Database,
    input: MethodLookupInput<'db>,
) -> Option<PlatformProperty> {
    let type_name = input.type_name(db);
    let prop_name = input.method_name(db);
    let data = PlatformDataInner::instance();
    data.get_property(&type_name, &prop_name).cloned()
}

#[salsa::tracked(lru = 128)]
pub fn type_properties_query<'db>(
    db: &'db dyn salsa::Database,
    input: TypeNameInput<'db>,
) -> Arc<Vec<PlatformProperty>> {
    let type_name = input.name(db);
    let data = PlatformDataInner::instance();
    Arc::new(data.get_type_properties(&type_name).into_iter().cloned().collect())
}

#[salsa::tracked(lru = 256)]
pub fn global_property_query<'db>(
    db: &'db dyn salsa::Database,
    input: TypeNameInput<'db>,
) -> Option<PlatformProperty> {
    let name = input.name(db);
    let data = PlatformDataInner::instance();
    data.get_global_property(&name).cloned()
}

#[salsa::tracked(lru = 256)]
pub fn global_member_method_query<'db>(
    db: &'db dyn salsa::Database,
    input: MethodLookupInput<'db>,
) -> Option<PlatformMethod> {
    let global_name = input.type_name(db);
    let member_name = input.method_name(db);
    let data = PlatformDataInner::instance();
    data.resolve_global_member(&global_name, &member_name).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_data_singleton() {
        let data1 = PlatformDataInner::instance();
        let data2 = PlatformDataInner::instance();

        assert!(std::ptr::eq(data1, data2));
    }

    #[test]
    fn test_get_type_case_insensitive() {
        let data = PlatformDataInner::instance();

        if data.all_types().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        let ty = data.all_types().first().unwrap();
        let name = ty.name.as_str();

        assert!(data.get_type(name).is_some());
        assert!(data.get_type(&name.to_lowercase()).is_some());
        assert!(data.get_type(&name.to_uppercase()).is_some());
    }

    #[test]
    fn test_get_type_by_unique_xdto_name() {
        let data = PlatformDataInner::instance();
        if data.all_types().is_empty() {
            return;
        }
        // `FlowchartContextType` is the unique XDTO name of `ГрафическаяСхема`.
        let via_xdto = data.get_type("FlowchartContextType");
        let via_class = data.get_type("GraphicalSchema");
        assert!(via_xdto.is_some(), "unique XDTO name must resolve");
        assert_eq!(
            via_xdto.map(|t| t.english_name.as_str()),
            via_class.map(|t| t.english_name.as_str()),
            "XDTO alias must resolve to the same type as the class name"
        );
    }

    #[test]
    fn test_ambiguous_xdto_name_is_not_indexed() {
        let data = PlatformDataInner::instance();
        if data.all_types().is_empty() {
            return;
        }
        // `ParameterValue` is the XDTO name of several data-composition types and
        // is not itself a class name, so it must not resolve to an arbitrary one.
        assert!(data.get_type("ParameterValue").is_none());
    }

    #[test]
    fn test_get_method_bilingual() {
        let data = PlatformDataInner::instance();

        if data.all_methods().is_empty() {
            println!("Skipping test: no platform methods available");
            return;
        }

        let method = data.all_methods().first().unwrap();
        let type_name = method.type_name.as_str();
        let ru_name = method.name.as_str();
        let en_name = method.english_name.as_str();

        let found = data.get_method(type_name, ru_name);
        assert!(found.is_some(), "Should find method by Russian name");
        assert_eq!(found.unwrap().id, method.id);

        let found = data.get_method(type_name, en_name);
        assert!(found.is_some(), "Should find method by English name");
        assert_eq!(found.unwrap().id, method.id);

        let found = data.get_method(&type_name.to_uppercase(), &ru_name.to_uppercase());
        assert!(found.is_some(), "Should be case-insensitive");
    }

    #[test]
    fn test_get_type_methods() {
        let data = PlatformDataInner::instance();

        if data.all_types().is_empty() || data.all_methods().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        let ty = data.all_types().first().unwrap();
        let methods = data.get_type_methods(&ty.english_name);

        if !methods.is_empty() {
            println!("Type {} has {} methods", ty.english_name, methods.len());
            for method in &methods {
                assert_eq!(method.type_name.to_lowercase(), ty.english_name.to_lowercase());
            }
        }
    }

    #[salsa::db]
    #[derive(Clone, Default)]
    struct TestDatabase {
        storage: salsa::Storage<Self>,
    }

    impl salsa::Database for TestDatabase {}

    #[test]
    fn test_platform_type_query() {
        let db = TestDatabase::default();

        let data = PlatformDataInner::instance();
        if data.all_types().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        let input1 = TypeNameInput::new(&db, "Строка".to_string());
        let input2 = TypeNameInput::new(&db, "СТРОКА".to_string());
        let input3 = TypeNameInput::new(&db, "String".to_string());

        let ty1 = platform_type_query(&db, input1);
        let ty2 = platform_type_query(&db, input2);
        let ty3 = platform_type_query(&db, input3);

        assert_eq!(ty1.is_some(), ty2.is_some());
        assert_eq!(ty1.is_some(), ty3.is_some());

        if let (Some(t1), Some(t2), Some(t3)) = (ty1, ty2, ty3) {
            assert_eq!(t1.name, t2.name);
            assert_eq!(t1.name, t3.name);
        }
    }

    #[test]
    fn test_platform_method_query() {
        let db = TestDatabase::default();

        let data = PlatformDataInner::instance();
        if data.all_methods().is_empty() {
            println!("Skipping test: no platform methods available");
            return;
        }

        let input = MethodLookupInput::new(&db, "Строка".to_string(), "ВРег".to_string());
        let method = platform_method_query(&db, input);

        if let Some(method) = method {
            assert_eq!(method.name.as_str(), "ВРег");
            assert_eq!(method.english_name.as_str(), "Upper");
        }
    }

    #[test]
    fn test_type_methods_query() {
        let db = TestDatabase::default();

        let data = PlatformDataInner::instance();
        if data.all_types().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        let input = TypeNameInput::new(&db, "Строка".to_string());
        let methods = type_methods_query(&db, input);

        if !methods.is_empty() {
            for method in methods.iter() {
                assert_eq!(method.type_name.to_lowercase(), "строка");
            }
        }
    }

    #[test]
    fn test_global_functions() {
        let data = PlatformDataInner::instance();

        if data.all_global_functions().is_empty() {
            println!("Skipping test: no global functions available");
            return;
        }

        println!("Total global functions: {}", data.all_global_functions().len());

        let func = data.get_global_function("НачатьТранзакцию");
        assert!(func.is_some(), "Should find НачатьТранзакцию");

        let func = func.unwrap();
        assert_eq!(func.name.as_str(), "НачатьТранзакцию");
        assert_eq!(func.english_name.as_str(), "BeginTransaction");
        println!("Found function: {} / {}", func.name, func.english_name);

        let func_en = data.get_global_function("BeginTransaction");
        assert!(func_en.is_some(), "Should find by English name");
        assert_eq!(func_en.unwrap().id, func.id);

        let func_upper = data.get_global_function("НАЧАТЬТРАНЗАКЦИЮ");
        assert!(func_upper.is_some(), "Should be case-insensitive");
        assert_eq!(func_upper.unwrap().id, func.id);
    }

    #[test]
    fn test_is_variadic_marks_expected_unbounded_entries_only() {
        let data = PlatformDataInner::instance();
        let funcs = data.all_global_functions();
        if funcs.is_empty() {
            println!("Skipping test: no global functions available");
            return;
        }
        let expected_global_ru: &[&str] = &["Мин", "Макс", "ПродолжитьВызов"];
        let mut seen_global_hits = 0usize;

        for func in funcs {
            let expected = expected_global_ru.contains(&func.name.as_str());
            let last_idx = func.parameters.len().saturating_sub(1);
            for (idx, param) in func.parameters.iter().enumerate() {
                if expected && idx == last_idx {
                    assert!(
                        param.is_variadic,
                        "{} must mark its last param ({}) as variadic",
                        func.name, param.name
                    );
                    seen_global_hits += 1;
                } else {
                    assert!(
                        !param.is_variadic,
                        "{} param {} unexpectedly carries is_variadic=true",
                        func.name, param.name
                    );
                }
            }
            for variant in &func.variants {
                for param in &variant.parameters {
                    assert!(
                        !param.is_variadic,
                        "global variant param must stay non-variadic \
                         (function={}, variant={:?}, param={})",
                        func.name, variant.variant_name, param.name
                    );
                }
            }
        }
        assert_eq!(
            seen_global_hits,
            expected_global_ru.len(),
            "every entry in `expected_global_ru` must be present in the platform corpus \
             — a missing global silently passing the per-function loop is a regression"
        );

        let expected_ctors: &[(&str, &str)] = &[
            ("Array", "По количеству элементов"),
            ("COMSafeArray", "Из массива 1"),
            ("COMSafeArray", "По типу элемента 1"),
            ("Structure", "По ключам и значениям"),
            ("FixedStructure", "По ключам и значениям"),
            ("DynamicListRowKey", "На основе путей и значений полей"),
        ];
        let mut seen_ctor_hits = 0usize;
        for ctor in data.all_constructors() {
            let ctor_key = (ctor.type_name.as_str(), ctor.variant_name.as_deref().unwrap_or(""));
            let expected = expected_ctors.contains(&ctor_key);
            let last_idx = ctor.parameters.len().saturating_sub(1);
            for (idx, param) in ctor.parameters.iter().enumerate() {
                if expected && idx == last_idx {
                    assert!(
                        param.is_variadic,
                        "{}.{} must mark its last param ({}) as variadic",
                        ctor.type_name, ctor_key.1, param.name
                    );
                    seen_ctor_hits += 1;
                } else {
                    assert!(
                        !param.is_variadic,
                        "{}.{:?} param {} unexpectedly carries is_variadic=true",
                        ctor.type_name, ctor.variant_name, param.name
                    );
                }
            }
        }
        assert_eq!(
            seen_ctor_hits,
            expected_ctors.len(),
            "every entry in `expected_ctors` must be present in the platform corpus"
        );
    }

    #[test]
    fn test_platform_constructors_query_bilingual() {
        let db = TestDatabase::default();
        let data = PlatformDataInner::instance();
        if data.all_constructors().is_empty() {
            println!("Skipping test: no constructor data");
            return;
        }

        let ru = platform_constructors_query(&db, TypeNameInput::new(&db, "Массив".to_string()));
        let en = platform_constructors_query(&db, TypeNameInput::new(&db, "Array".to_string()));

        assert!(!ru.is_empty(), "Массив must have at least one constructor");
        assert_eq!(ru.len(), en.len(), "RU and EN lookups must return the same overload set");
        let ru_ids: Vec<u32> = ru.iter().map(|c| c.id).collect();
        let en_ids: Vec<u32> = en.iter().map(|c| c.id).collect();
        assert_eq!(ru_ids, en_ids);
    }

    #[test]
    fn test_platform_constructors_query_unknown() {
        let db = TestDatabase::default();
        let result = platform_constructors_query(
            &db,
            TypeNameInput::new(&db, "ЗаведомоНесуществующийТип".to_string()),
        );
        assert!(result.is_empty());
    }

    #[test]
    fn test_platform_property_query_bilingual() {
        let db = TestDatabase::default();
        let data = PlatformDataInner::instance();
        if data.all_properties().is_empty() {
            println!("Skipping test: no property data");
            return;
        }

        let ru_ru = platform_property_query(
            &db,
            MethodLookupInput::new(&db, "Запрос".to_string(), "Текст".to_string()),
        );
        let en_en = platform_property_query(
            &db,
            MethodLookupInput::new(&db, "Query".to_string(), "Text".to_string()),
        );
        let en_ru = platform_property_query(
            &db,
            MethodLookupInput::new(&db, "Query".to_string(), "Текст".to_string()),
        );
        let ru_en = platform_property_query(
            &db,
            MethodLookupInput::new(&db, "Запрос".to_string(), "Text".to_string()),
        );

        for got in [&ru_ru, &en_en, &en_ru, &ru_en] {
            let prop = got.as_ref().expect("Query.Text property must exist in platform data");
            assert_eq!(prop.name.as_str(), "Текст");
            assert_eq!(prop.english_name.as_str(), "Text");
            assert_eq!(prop.property_types, vec![smol_str::SmolStr::new("Строка")]);
            assert!(!prop.is_readonly, "Текст is read-write");
        }
    }

    #[test]
    fn test_platform_property_query_readonly_union() {
        let db = TestDatabase::default();
        let data = PlatformDataInner::instance();
        if data.all_properties().is_empty() {
            println!("Skipping test: no property data");
            return;
        }

        let got = platform_property_query(
            &db,
            MethodLookupInput::new(&db, "Запрос".to_string(), "Параметры".to_string()),
        );
        let prop = got.expect("Query.Parameters property must exist");
        assert!(prop.is_readonly, "Параметры is read-only");
        assert_eq!(prop.property_types, vec![smol_str::SmolStr::new("Структура")]);

        let got = platform_property_query(
            &db,
            MethodLookupInput::new(
                &db,
                "Запрос".to_string(),
                "МенеджерВременныхТаблиц".to_string(),
            ),
        );
        let prop = got.expect("Query.TempTablesManager property must exist");
        assert!(!prop.is_readonly, "МенеджерВременныхТаблиц is read-write");
        assert_eq!(prop.property_types.len(), 2, "union property_types");
    }

    #[test]
    fn test_type_properties_query_lists_all_members() {
        let db = TestDatabase::default();
        let data = PlatformDataInner::instance();
        if data.all_properties().is_empty() {
            println!("Skipping test: no property data");
            return;
        }

        let props = type_properties_query(&db, TypeNameInput::new(&db, "Запрос".to_string()));
        let names: Vec<&str> = props.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"Текст"));
        assert!(names.contains(&"Параметры"));
        assert!(names.contains(&"МенеджерВременныхТаблиц"));
    }

    #[test]
    fn test_platform_property_query_unknown_returns_none() {
        let db = TestDatabase::default();
        let got = platform_property_query(
            &db,
            MethodLookupInput::new(
                &db,
                "Запрос".to_string(),
                "ЗаведомоНесуществующееСвойство".to_string(),
            ),
        );
        assert!(got.is_none());
    }

    #[test]
    fn test_global_property_lookup_bilingual() {
        let data = PlatformDataInner::instance();
        if data.all_global_properties().is_empty() {
            println!("Skipping test: no global properties available");
            return;
        }

        let ru = data.get_global_property("ОбработкаОшибок").expect("ru name must resolve");
        let en = data.get_global_property("ErrorProcessing").expect("en name must resolve");
        assert_eq!(ru.id, en.id);
        assert_eq!(ru.property_types, vec![smol_str::SmolStr::new("МенеджерОбработкиОшибок")]);
        assert!(data.get_global_property("ОБРАБОТКАОШИБОК").is_some());
        assert!(data.get_global_property("errorprocessing").is_some());
    }

    #[test]
    fn test_global_member_resolution_bilingual() {
        let data = PlatformDataInner::instance();
        if data.all_global_properties().is_empty() {
            println!("Skipping test: no global properties available");
            return;
        }

        let m = data
            .resolve_global_member("ОбработкаОшибок", "КраткоеПредставлениеОшибки")
            .expect("ru.ru must resolve");
        assert_eq!(m.name.as_str(), "КраткоеПредставлениеОшибки");
        assert_eq!(m.english_name.as_str(), "BriefErrorDescription");

        let m_en = data
            .resolve_global_member("ErrorProcessing", "BriefErrorDescription")
            .expect("en.en must resolve");
        assert_eq!(m_en.id, m.id);

        assert!(data.resolve_global_member("ОбработкаОшибок", "ЗаведомоНеТакогоМетода").is_none());
        assert!(data.resolve_global_member("ЗаведомоНеТакогоГлобала", "X").is_none());
    }

    #[test]
    fn test_global_property_query_salsa() {
        let db = TestDatabase::default();
        let data = PlatformDataInner::instance();
        if data.all_global_properties().is_empty() {
            println!("Skipping test: no global properties available");
            return;
        }

        let got =
            global_property_query(&db, TypeNameInput::new(&db, "ОбработкаОшибок".to_string()))
                .expect("global must resolve");
        assert_eq!(got.english_name.as_str(), "ErrorProcessing");

        let got_member = global_member_method_query(
            &db,
            MethodLookupInput::new(
                &db,
                "ОбработкаОшибок".to_string(),
                "КраткоеПредставлениеОшибки".to_string(),
            ),
        )
        .expect("member must resolve");
        assert_eq!(got_member.english_name.as_str(), "BriefErrorDescription");
    }

    #[test]
    fn test_constructor_docs_by_id() {
        let data = PlatformDataInner::instance();
        if data.all_constructors().is_empty() {
            println!("Skipping test: no constructor data");
            return;
        }
        let ctor_with_docs =
            data.all_constructors().iter().find(|c| data.get_constructor_docs(c.id).is_some());
        if let Some(ctor) = ctor_with_docs {
            let docs = data.get_constructor_docs(ctor.id).unwrap();
            assert_eq!(docs.constructor_id, ctor.id);
        }
    }

    #[test]
    fn find_prefixed_method_resolves_information_register_record_set_read() {
        let data = PlatformDataInner::instance();
        if data.get_manager_methods("InformationRegisterRecordSet").is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }
        let m = find_prefixed_method("InformationRegisterRecordSet", "Прочитать")
            .expect("Прочитать must resolve under InformationRegisterRecordSet");
        assert!(
            m.english_name.to_lowercase().ends_with(".read"),
            "english_name must end with `.Read`, got `{}`",
            m.english_name
        );
        assert_ne!(m.id, 0);

        let m_en = find_prefixed_method("InformationRegisterRecordSet", "Read")
            .expect("Read must resolve bilingually");
        assert_eq!(m.id, m_en.id);
    }

    #[test]
    fn prefixed_method_query_caches_through_salsa() {
        let db = TestDatabase::default();
        let data = PlatformDataInner::instance();
        if data.get_manager_methods("InformationRegisterRecordSet").is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        let input1 = PrefixedMethodLookupInput::new(
            &db,
            "InformationRegisterRecordSet".to_string(),
            "Прочитать".to_string(),
        );
        let input2 = PrefixedMethodLookupInput::new(
            &db,
            "InformationRegisterRecordSet".to_string(),
            "Прочитать".to_string(),
        );
        assert_eq!(input1, input2);

        let m1 = prefixed_method_query(&db, input1).expect("must resolve");
        let m2 = prefixed_method_query(&db, input2).expect("must resolve");
        assert_eq!(m1.id, m2.id);
    }
}
