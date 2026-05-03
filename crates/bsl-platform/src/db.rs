//! Platform data database with efficient indexing.
//!
//! This module provides both:
//! - Salsa tracked queries for IDE integration with caching
//! - A singleton for standalone usage

use crate::types::{
    ConstructorDocs, GlobalFunction, PlatformConstructor, PlatformMethod, PlatformProperty,
    PlatformType, PropertyDocs,
};
use once_cell::sync::OnceCell;
use rustc_hash::FxHashMap;
use smol_str::SmolStr;
use std::sync::Arc;

static PLATFORM_DATA_SINGLETON: OnceCell<PlatformDataInner> = OnceCell::new();

/// Sentinel `type_name` used in `properties` records that originate from
/// `objects/Global context/properties/` in HBK. Top-level identifiers like
/// `ОбработкаОшибок`, `Метаданные`, `Справочники` are emitted with this
/// owner so they can be filtered out into the dedicated global-scope index
/// without colliding with regular type members.
pub const GLOBAL_CONTEXT_OWNER: &str = "Global context";

/// Internal platform data implementation.
///
/// This struct is not public API. Access it through:
/// - Salsa tracked queries (`platform_type_query`, `platform_method_query`, `global_function_query`)
/// - `PlatformDataInner::instance()` for standalone usage
pub struct PlatformDataInner {
    /// Converted platform types (from raw const data)
    types: Vec<PlatformType>,
    /// Types indexed by lowercase name (both Russian and English)
    types_by_name: FxHashMap<SmolStr, usize>,
    /// Converted platform methods (from raw const data)
    methods: Vec<PlatformMethod>,
    /// Methods indexed by (type_name, method_name)
    methods_by_name: FxHashMap<(SmolStr, SmolStr), usize>,
    /// Converted global functions (from raw const data)
    global_functions: Vec<GlobalFunction>,
    /// Global functions indexed by lowercase name (both Russian and English)
    global_functions_by_name: FxHashMap<SmolStr, usize>,
    /// Method documentation indexed by method ID
    method_docs_by_id: FxHashMap<u32, usize>,
    /// Global function documentation indexed by function ID
    global_function_docs_by_id: FxHashMap<u32, usize>,
    /// Converted platform constructors (from raw const data).
    constructors: Vec<PlatformConstructor>,
    /// Constructors indexed by lowercase type name (both Russian and English).
    /// Each key maps to the list of constructor overloads for that type.
    constructors_by_type: FxHashMap<SmolStr, Vec<usize>>,
    /// Constructor documentation indexed by constructor ID.
    constructor_docs_by_id: FxHashMap<u32, usize>,
    /// Converted platform properties (from raw const data).
    properties: Vec<PlatformProperty>,
    /// Properties indexed by (type_name, property_name). Bilingual — same
    /// indexing shape as `methods_by_name`, so callers can pass either the
    /// Russian or English type name and either the Russian or English
    /// property name.
    properties_by_name: FxHashMap<(SmolStr, SmolStr), usize>,
    /// Property documentation indexed by property ID.
    property_docs_by_id: FxHashMap<u32, usize>,
    /// Global-scope identifiers from `objects/Global context/properties/`
    /// (e.g. `ОбработкаОшибок` → `МенеджерОбработкиОшибок`). Indexed by
    /// lowercased name, both Russian and English. Values are indices into
    /// `properties` so the declared type and documentation are reached
    /// through the same record as regular type members.
    global_properties_by_name: FxHashMap<SmolStr, usize>,
}

impl PlatformDataInner {
    /// Get the global platform data instance.
    pub fn instance() -> &'static Self {
        PLATFORM_DATA_SINGLETON.get_or_init(Self::new)
    }

    /// Initialize platform data and build indices.
    fn new() -> Self {
        // Convert raw types to SmolStr-based types
        let types: Vec<PlatformType> =
            crate::generated::PLATFORM_TYPES.iter().map(PlatformType::from).collect();

        let mut types_by_name = FxHashMap::default();

        // Index platform types
        for (idx, ty) in types.iter().enumerate() {
            let ru_key: SmolStr = ty.name.to_lowercase().into();
            let en_key: SmolStr = ty.english_name.to_lowercase().into();
            types_by_name.insert(ru_key, idx);
            types_by_name.insert(en_key, idx);
        }

        // Convert raw methods to SmolStr-based methods
        let methods: Vec<PlatformMethod> =
            crate::generated::PLATFORM_METHODS.iter().map(PlatformMethod::from).collect();

        let mut methods_by_name = FxHashMap::default();

        // Build English→Russian type name mapping for bilingual method lookup.
        let mut type_en_to_ru: FxHashMap<SmolStr, SmolStr> = FxHashMap::default();
        for ty in &types {
            let en_key: SmolStr = ty.english_name.to_lowercase().into();
            let ru_key: SmolStr = ty.name.to_lowercase().into();
            type_en_to_ru.insert(en_key, ru_key);
        }

        // Index platform methods by (type_name, method_name).
        // method.type_name is English (e.g. "Array"), so we also index by
        // the Russian type name (e.g. "Массив") for bilingual lookup.
        for (idx, method) in methods.iter().enumerate() {
            let en_type_key: SmolStr = method.type_name.to_lowercase().into();
            let ru_method_key: SmolStr = method.name.to_lowercase().into();
            let en_method_key: SmolStr = method.english_name.to_lowercase().into();

            // (english_type, russian_method) and (english_type, english_method)
            methods_by_name.insert((en_type_key.clone(), ru_method_key.clone()), idx);
            methods_by_name.insert((en_type_key.clone(), en_method_key.clone()), idx);

            // (russian_type, russian_method) and (russian_type, english_method)
            if let Some(ru_type_key) = type_en_to_ru.get(&en_type_key) {
                methods_by_name.insert((ru_type_key.clone(), ru_method_key), idx);
                methods_by_name.insert((ru_type_key.clone(), en_method_key), idx);
            }
        }

        // Convert raw global functions to SmolStr-based functions
        let global_functions: Vec<GlobalFunction> =
            crate::generated::PLATFORM_GLOBAL_FUNCTIONS.iter().map(GlobalFunction::from).collect();

        let mut global_functions_by_name = FxHashMap::default();

        // Index global functions by name (both Russian and English)
        for (idx, function) in global_functions.iter().enumerate() {
            let ru_key: SmolStr = function.name.to_lowercase().into();
            let en_key: SmolStr = function.english_name.to_lowercase().into();

            // Index by Russian name
            global_functions_by_name.insert(ru_key, idx);
            // Index by English name
            global_functions_by_name.insert(en_key, idx);
        }

        // Index method documentation by ID
        let mut method_docs_by_id = FxHashMap::default();
        for (idx, docs) in crate::generated::METHOD_DOCS.iter().enumerate() {
            method_docs_by_id.insert(docs.method_id, idx);
        }

        // Index global function documentation by ID
        let mut global_function_docs_by_id = FxHashMap::default();
        for (idx, docs) in crate::generated::GLOBAL_FUNCTION_DOCS.iter().enumerate() {
            global_function_docs_by_id.insert(docs.method_id, idx);
        }

        // Convert raw constructors and build per-type index. Bilingual keying
        // mirrors `methods_by_name`: same `type_en_to_ru` mapping, same
        // lowercase normalization, so callers can look constructors up by
        // either Russian or English type name without special-casing.
        let mut constructors: Vec<PlatformConstructor> =
            crate::generated::PLATFORM_CONSTRUCTORS.iter().map(PlatformConstructor::from).collect();

        // HBK-data correction: a handful of constructors are variadic per
        // `Описание:` (free text) but encode a fixed `Синтаксис:` line, so
        // PR2's `>,...,<`-in-syntax detector misses them. Apply the overlay
        // before the per-type index is built so every consumer (completion
        // / hover / inference) sees the corrected `is_variadic` flag.
        apply_docs_only_variadic_overlay(&mut constructors);

        let mut constructors_by_type: FxHashMap<SmolStr, Vec<usize>> = FxHashMap::default();
        for (idx, ctor) in constructors.iter().enumerate() {
            let en_key: SmolStr = ctor.type_name.to_lowercase().into();
            constructors_by_type.entry(en_key.clone()).or_default().push(idx);
            if let Some(ru_key) = type_en_to_ru.get(&en_key) {
                constructors_by_type.entry(ru_key.clone()).or_default().push(idx);
            }
        }

        // Index constructor documentation by constructor ID.
        let mut constructor_docs_by_id = FxHashMap::default();
        for (idx, docs) in crate::generated::CONSTRUCTOR_DOCS.iter().enumerate() {
            constructor_docs_by_id.insert(docs.constructor_id, idx);
        }

        // Convert raw properties and build the bilingual lookup index. Key
        // shape mirrors `methods_by_name`: `(lowercase_type, lowercase_prop)`
        // with four combinations (en/ru × en/ru) per entry so callers can
        // supply either language.
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

        // Index property documentation by property ID.
        let mut property_docs_by_id = FxHashMap::default();
        for (idx, docs) in crate::generated::PROPERTY_DOCS.iter().enumerate() {
            property_docs_by_id.insert(docs.property_id, idx);
        }

        // Build the global-scope index from properties whose owner is the
        // sentinel `Global context` type. Both Russian and English names are
        // mapped to the same property index so callers can look up either.
        // Unlike regular properties, we do NOT key by `(owner, name)` — the
        // global namespace is flat and the owner is implicit.
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

    /// Get platform type by name (case-insensitive, bilingual).
    ///
    /// Accepts either the Russian or English name. Exposed publicly so
    /// `symbol-info` adapters can translate an English primary key
    /// (`PlatformMethod::type_name`, `PlatformConstructor::type_name`)
    /// back to the Russian display name when building a qualifier. Cheap —
    /// single hash lookup via `types_by_name`.
    pub fn get_type(&self, name: &str) -> Option<&PlatformType> {
        let key: SmolStr = name.to_lowercase().into();
        let idx = *self.types_by_name.get(&key)?;
        self.types.get(idx)
    }

    /// Get all platform types.
    pub fn all_types(&self) -> &[PlatformType] {
        &self.types
    }

    /// Get platform method by type and method name (case-insensitive, bilingual).
    pub fn get_method(&self, type_name: &str, method_name: &str) -> Option<&PlatformMethod> {
        let type_key: SmolStr = type_name.to_lowercase().into();
        let method_key: SmolStr = method_name.to_lowercase().into();
        let idx = *self.methods_by_name.get(&(type_key, method_key))?;
        self.methods.get(idx)
    }

    /// Get all platform methods.
    pub fn all_methods(&self) -> &[PlatformMethod] {
        &self.methods
    }

    /// Get all methods for a specific type (case-insensitive, bilingual).
    ///
    /// Accepts both Russian and English type names (e.g. "Массив" or "Array").
    pub fn get_type_methods(&self, type_name: &str) -> Vec<&PlatformMethod> {
        // Resolve to the English type name used in method.type_name.
        let type_key: SmolStr = type_name.to_lowercase().into();
        let en_type_key = if let Some(idx) = self.types_by_name.get(&type_key) {
            self.types[*idx].english_name.to_lowercase().into()
        } else {
            type_key
        };
        self.methods.iter().filter(|m| m.type_name.to_lowercase() == en_type_key.as_str()).collect()
    }

    /// Get all methods for a manager type prefix (case-insensitive).
    ///
    /// Manager type names in platform data have the form `"CatalogManager.<Catalog name>"`.
    /// This method matches all methods whose type_name starts with `"{prefix}."`.
    ///
    /// Example: `get_manager_methods("CatalogManager")` returns НайтиПоКоду, СоздатьЭлемент, etc.
    pub fn get_manager_methods(&self, manager_prefix: &str) -> Vec<&PlatformMethod> {
        let prefix = format!("{}.", manager_prefix.to_lowercase());
        self.methods.iter().filter(|m| m.type_name.to_lowercase().starts_with(&prefix)).collect()
    }

    /// Get global function by name (case-insensitive, bilingual).
    pub fn get_global_function(&self, name: &str) -> Option<&GlobalFunction> {
        let key: SmolStr = name.to_lowercase().into();
        let idx = *self.global_functions_by_name.get(&key)?;
        self.global_functions.get(idx)
    }

    /// Get all global functions.
    pub fn all_global_functions(&self) -> &[GlobalFunction] {
        &self.global_functions
    }

    /// Get method documentation by method ID.
    pub fn get_method_docs(&self, method_id: u32) -> Option<crate::types::MethodDocs> {
        let idx = *self.method_docs_by_id.get(&method_id)?;
        let raw_docs = crate::generated::METHOD_DOCS.get(idx)?;
        Some(crate::types::MethodDocs::from(raw_docs))
    }

    /// Get constructors for a platform type (case-insensitive, bilingual).
    ///
    /// Returns all overloads (`Новый Массив()` vs `Новый Массив(КоличествоЭлементов)`
    /// vs `Новый Массив(ФиксированныйМассив)`) in source order from the HBK.
    /// Empty vector when the type has no constructors or the name is unknown.
    pub fn get_constructors(&self, type_name: &str) -> Vec<&PlatformConstructor> {
        let key: SmolStr = type_name.to_lowercase().into();
        match self.constructors_by_type.get(&key) {
            Some(indices) => indices.iter().filter_map(|&i| self.constructors.get(i)).collect(),
            None => Vec::new(),
        }
    }

    /// Get all platform constructors.
    pub fn all_constructors(&self) -> &[PlatformConstructor] {
        &self.constructors
    }

    /// Get constructor documentation by constructor ID.
    pub fn get_constructor_docs(&self, constructor_id: u32) -> Option<ConstructorDocs> {
        let idx = *self.constructor_docs_by_id.get(&constructor_id)?;
        let raw = crate::generated::CONSTRUCTOR_DOCS.get(idx)?;
        Some(ConstructorDocs::from(raw))
    }

    /// Get global function documentation by function ID.
    pub fn get_global_function_docs(&self, function_id: u32) -> Option<crate::types::MethodDocs> {
        let idx = *self.global_function_docs_by_id.get(&function_id)?;
        let raw_docs = crate::generated::GLOBAL_FUNCTION_DOCS.get(idx)?;
        Some(crate::types::MethodDocs::from(raw_docs))
    }

    /// Get platform property by type and property name (case-insensitive, bilingual).
    ///
    /// Accepts both Russian and English names on either side. Returns `None`
    /// when the type has no declared properties, when the property name is
    /// unknown, or when the type name itself is not in the platform catalogue.
    pub fn get_property(&self, type_name: &str, prop_name: &str) -> Option<&PlatformProperty> {
        let type_key: SmolStr = type_name.to_lowercase().into();
        let prop_key: SmolStr = prop_name.to_lowercase().into();
        let idx = *self.properties_by_name.get(&(type_key, prop_key))?;
        self.properties.get(idx)
    }

    /// Get all properties for a specific type (case-insensitive, bilingual).
    ///
    /// Mirrors [`Self::get_type_methods`]: walks `properties` and filters by
    /// the English type key so the caller can pass either the Russian or
    /// English display name.
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

    /// Get all properties indexed under a composite manager-style prefix
    /// (case-insensitive).
    ///
    /// Companion to [`Self::get_manager_methods`]. Composite type pages
    /// (`InformationRegisterRecordSet.<Information register name>`,
    /// `CatalogObject.<Catalog name>`, …) carry their property rows with
    /// the full composite `type_name`; per-MDO callers don't know the
    /// concrete `<Имя>` placeholder text so they walk by `"{prefix}."`
    /// the same way method lookup does.
    ///
    /// Example: `get_manager_properties("InformationRegisterRecordSet")`
    /// returns `Отбор`, `Записывать`, `ЗаписьИсторииДанных`, …
    pub fn get_manager_properties(&self, manager_prefix: &str) -> Vec<&PlatformProperty> {
        let prefix = format!("{}.", manager_prefix.to_lowercase());
        self.properties.iter().filter(|p| p.type_name.to_lowercase().starts_with(&prefix)).collect()
    }

    /// Get all platform properties.
    pub fn all_properties(&self) -> &[PlatformProperty] {
        &self.properties
    }

    /// Resolve a top-level identifier to its global-scope property record.
    ///
    /// Returns `Some` for built-in platform globals like `ОбработкаОшибок`,
    /// `Метаданные`, `Справочники`. Bilingual and case-insensitive. Used by
    /// the IDE-facing context wrapper and by `hir-ty` to type-tag a bare
    /// `Path` expression that names a platform global.
    pub fn get_global_property(&self, name: &str) -> Option<&PlatformProperty> {
        let key: SmolStr = name.to_lowercase().into();
        let idx = *self.global_properties_by_name.get(&key)?;
        self.properties.get(idx)
    }

    /// Iterate every global-scope property record. Used for top-level
    /// completion (`Обработ|`) where the candidate set must include all
    /// global identifiers alongside global functions and user CommonModules.
    pub fn all_global_properties(&self) -> Vec<&PlatformProperty> {
        let mut seen: Vec<usize> = self.global_properties_by_name.values().copied().collect();
        seen.sort_unstable();
        seen.dedup();
        seen.into_iter().filter_map(|i| self.properties.get(i)).collect()
    }

    /// Resolve `(global_name, member_name)` to a method on the declared type
    /// of the global identifier. This is the centralised lookup that
    /// suppresses the false-positive `MissingCommonModuleMethod` for
    /// `ОбработкаОшибок.КраткоеПредставлениеОшибки(...)`-shaped calls.
    ///
    /// Returns `None` when the global is unknown, when its declared
    /// `property_types` is empty, or when no method with that name exists on
    /// the declared type. We pick the FIRST declared type — global properties
    /// in HBK are scalar (single-element `property_types`), so this is
    /// unambiguous in practice. Union-typed globals would need a different
    /// resolution strategy and currently do not occur.
    pub fn resolve_global_member(
        &self,
        global_name: &str,
        member_name: &str,
    ) -> Option<&PlatformMethod> {
        let prop = self.get_global_property(global_name)?;
        let declared_type = prop.property_types.first()?;
        self.get_method(declared_type.as_str(), member_name)
    }

    /// Get property documentation by property id.
    pub fn get_property_docs(&self, property_id: u32) -> Option<PropertyDocs> {
        let idx = *self.property_docs_by_id.get(&property_id)?;
        let raw = crate::generated::PROPERTY_DOCS.get(idx)?;
        Some(PropertyDocs::from(raw))
    }

    /// Returns keyword documentation by name (case-insensitive).
    ///
    /// Examples: "ВызватьИсключение", "Для", "Если"
    pub fn get_keyword_docs(&self, keyword: &str) -> Option<crate::types::KeywordDocs> {
        get_keyword_docs_static(keyword)
    }
}

/// HBK-data correction: lift `is_variadic = true` on the trailing
/// parameter of constructors that are variadic per `Описание:` (free
/// text) but encode a fixed `Синтаксис:` line in HBK. Applied during
/// [`PlatformDataInner::new`] before the per-type index is built so all
/// consumers (completion / hover / inference) see the corrected flag
/// uniformly.
///
/// **This is a data correction, not a runtime-inference shortcut.** If a
/// future regen of `platform_data.json` makes the JSON correct (or HBK
/// starts encoding these properly), the matching entry becomes a
/// no-op — the overlay only touches param flags it does not already
/// see set, and missing entries (renamed types/variants) silently skip.
///
/// Inventory (audit 2026-04-29 against the 8.3.27.1989 HBK dump):
/// - `Structure.По ключам и значениям` — `parameters: [Ключи, Значения]`
///   both optional. Description: «Если в первом параметре заданы ключи
///   элементов структуры, то в следующих параметрах могут быть указаны
///   значения этих элементов в том порядке, в котором они расположены
///   в строке в первом параметре.»
/// - `FixedStructure.По ключам и значениям` — `[Ключ, Значения]`
///   `Ключ` required + `Значения` optional. Description mirrors
///   Structure (additional values are unbounded).
/// - `DynamicListRowKey.На основе путей и значений полей` —
///   `[ПутиКлючевыхПолей, Значения]` both required. Description: «в
///   следующих параметрах — значения этих ключевых полей в том
///   порядке, в котором они расположены в строке в первом параметре».
fn apply_docs_only_variadic_overlay(constructors: &mut [PlatformConstructor]) {
    /// `(type_name, variant_name)` pairs of constructors whose trailing
    /// parameter must be lifted to `is_variadic = true`. Both fields are
    /// matched verbatim against the values in `PlatformConstructor`
    /// (English `type_name`, Russian `variant_name`).
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

/// Static keyword documentation (hardcoded for proof-of-concept).
///
/// TODO: Generate from shlang_ru.hbk HTML files in build.rs
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

// Backward compatibility: type alias for non-Salsa usage
pub type PlatformData = PlatformDataInner;

// ============================================================================
// Salsa Interned Inputs
// ============================================================================

/// Interned type name for platform lookups.
///
/// This allows Salsa to intern and deduplicate type name strings.
#[salsa::interned(debug)]
pub struct TypeNameInput {
    pub name: String,
}

/// Interned method lookup key (type_name + method_name).
#[salsa::interned(debug)]
pub struct MethodLookupInput {
    pub type_name: String,
    pub method_name: String,
}

// ============================================================================
// Salsa Queries
// ============================================================================

/// Lookup platform type by name (case-insensitive, bilingual).
///
/// This Salsa query provides cached access to platform types.
/// The underlying data never changes (loaded once at startup), but Salsa caching
/// provides efficient repeated lookups.
///
/// # Example
/// ```ignore
/// let input = TypeNameInput::new(db, "Строка".to_string());
/// let ty = platform_type_query(db, input);
/// assert_eq!(ty.unwrap().english_name.as_str(), "String");
/// ```
#[salsa::tracked(lru = 256)]
pub fn platform_type_query<'db>(
    db: &'db dyn salsa::Database,
    input: TypeNameInput<'db>,
) -> Option<PlatformType> {
    let name = input.name(db);
    let data = PlatformDataInner::instance();
    data.get_type(&name).cloned()
}

/// Lookup platform method by type and method name (case-insensitive, bilingual).
///
/// This Salsa query provides cached access to platform methods.
///
/// # Example
/// ```ignore
/// let input = MethodLookupInput::new(db, "Строка".to_string(), "ВРег".to_string());
/// let method = platform_method_query(db, input);
/// assert_eq!(method.unwrap().english_name.as_str(), "Upper");
/// ```
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

/// Get all methods for a specific type (case-insensitive, bilingual).
///
/// Returns Arc<Vec<>> for efficient sharing across queries.
///
/// # Example
/// ```ignore
/// let input = TypeNameInput::new(db, "Строка".to_string());
/// let methods = type_methods_query(db, input);
/// for method in methods.iter() {
///     println!("{} / {}", method.name, method.english_name);
/// }
/// ```
#[salsa::tracked(lru = 128)]
pub fn type_methods_query<'db>(
    db: &'db dyn salsa::Database,
    input: TypeNameInput<'db>,
) -> Arc<Vec<PlatformMethod>> {
    let type_name = input.name(db);
    let data = PlatformDataInner::instance();
    Arc::new(data.get_type_methods(&type_name).into_iter().cloned().collect())
}

/// Get all manager methods for a manager-type prefix (e.g.
/// `"CatalogManager"`, `"DocumentManager"`).
///
/// Mirrors [`type_methods_query`] for manager-collective receivers:
/// completion on `Справочники.Валюты.` needs `НайтиПоКоду`,
/// `СоздатьЭлемент`, etc., which live under
/// `CatalogManager.<Name>` in the platform index. Wrapping the
/// [`PlatformDataInner::get_manager_methods`] lookup in a Salsa query
/// keeps `ide::completion::mdo_completion` off of
/// `PlatformData::instance()` and matches the facade-boundary
/// Invariant #3 in M3.
///
/// # Example
/// ```ignore
/// let input = TypeNameInput::new(db, "CatalogManager".to_string());
/// let methods = manager_methods_query(db, input);
/// for method in methods.iter() {
///     // НайтиПоКоду, СоздатьЭлемент, ...
/// }
/// ```
#[salsa::tracked(lru = 128)]
pub fn manager_methods_query<'db>(
    db: &'db dyn salsa::Database,
    input: TypeNameInput<'db>,
) -> Arc<Vec<PlatformMethod>> {
    let prefix = input.name(db);
    let data = PlatformDataInner::instance();
    Arc::new(data.get_manager_methods(&prefix).into_iter().cloned().collect())
}

/// Lookup global function by name (case-insensitive, bilingual).
///
/// This Salsa query provides cached access to global platform functions.
///
/// # Example
/// ```ignore
/// let input = TypeNameInput::new(db, "НачатьТранзакцию".to_string());
/// let function = global_function_query(db, input);
/// assert_eq!(function.unwrap().english_name.as_str(), "BeginTransaction");
/// ```
#[salsa::tracked(lru = 256)]
pub fn global_function_query<'db>(
    db: &'db dyn salsa::Database,
    input: TypeNameInput<'db>,
) -> Option<GlobalFunction> {
    let name = input.name(db);
    let data = PlatformDataInner::instance();
    data.get_global_function(&name).cloned()
}

/// Lookup platform constructors for a type (case-insensitive, bilingual).
///
/// Returns `Arc<Vec<_>>` so callers can share ownership across queries —
/// mirrors the shape of [`type_methods_query`] / [`manager_methods_query`].
/// Empty vector for types with no constructors (e.g. `Строка`, `Число`)
/// or for unknown type names.
///
/// # Example
/// ```ignore
/// let input = TypeNameInput::new(db, "Массив".to_string());
/// let ctors = platform_constructors_query(db, input);
/// assert!(!ctors.is_empty(), "Массив has multiple constructor overloads");
/// ```
#[salsa::tracked(lru = 128)]
pub fn platform_constructors_query<'db>(
    db: &'db dyn salsa::Database,
    input: TypeNameInput<'db>,
) -> Arc<Vec<PlatformConstructor>> {
    let type_name = input.name(db);
    let data = PlatformDataInner::instance();
    Arc::new(data.get_constructors(&type_name).into_iter().cloned().collect())
}

/// Lookup a platform property by type + property name (case-insensitive, bilingual).
///
/// Thin Salsa wrapper around [`PlatformDataInner::get_property`]. Uses the
/// existing [`MethodLookupInput`] interned input: property lookup shares the
/// `(type_name, member_name)` key shape with method lookup, and reusing the
/// same interned input avoids paying for a second Salsa interner over the
/// same string pairs.
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

/// Get all properties for a specific platform type (case-insensitive, bilingual).
///
/// Mirrors [`type_methods_query`] — returns `Arc<Vec<_>>` so completion
/// providers can share ownership across queries, and uses the same
/// [`TypeNameInput`] interned key so both queries cache against the same
/// input.
#[salsa::tracked(lru = 128)]
pub fn type_properties_query<'db>(
    db: &'db dyn salsa::Database,
    input: TypeNameInput<'db>,
) -> Arc<Vec<PlatformProperty>> {
    let type_name = input.name(db);
    let data = PlatformDataInner::instance();
    Arc::new(data.get_type_properties(&type_name).into_iter().cloned().collect())
}

/// Lookup a global-scope identifier by name (case-insensitive, bilingual).
///
/// Salsa wrapper around [`PlatformDataInner::get_global_property`]. Reuses
/// [`TypeNameInput`] — the interned string keyspace is the same as for type
/// lookups, so no second interner is needed.
#[salsa::tracked(lru = 256)]
pub fn global_property_query<'db>(
    db: &'db dyn salsa::Database,
    input: TypeNameInput<'db>,
) -> Option<PlatformProperty> {
    let name = input.name(db);
    let data = PlatformDataInner::instance();
    data.get_global_property(&name).cloned()
}

/// Resolve `(global_name, member_name)` to a method on the global's declared
/// type. Salsa wrapper around [`PlatformDataInner::resolve_global_member`].
/// Reuses [`MethodLookupInput`] for the same `(name, member)` interner used
/// by regular method/property lookups.
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

        // Same instance
        assert!(std::ptr::eq(data1, data2));
    }

    #[test]
    fn test_get_type_case_insensitive() {
        let data = PlatformDataInner::instance();

        // When platform is not installed, no types available
        if data.all_types().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        let ty = data.all_types().first().unwrap();
        let name = ty.name.as_str();

        // Should find by different cases
        assert!(data.get_type(name).is_some());
        assert!(data.get_type(&name.to_lowercase()).is_some());
        assert!(data.get_type(&name.to_uppercase()).is_some());
    }

    #[test]
    fn test_get_method_bilingual() {
        let data = PlatformDataInner::instance();

        if data.all_methods().is_empty() {
            println!("Skipping test: no platform methods available");
            return;
        }

        // Test bilingual method lookup
        let method = data.all_methods().first().unwrap();
        let type_name = method.type_name.as_str();
        let ru_name = method.name.as_str();
        let en_name = method.english_name.as_str();

        // Should find by Russian name
        let found = data.get_method(type_name, ru_name);
        assert!(found.is_some(), "Should find method by Russian name");
        assert_eq!(found.unwrap().id, method.id);

        // Should find by English name
        let found = data.get_method(type_name, en_name);
        assert!(found.is_some(), "Should find method by English name");
        assert_eq!(found.unwrap().id, method.id);

        // Should be case-insensitive
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

        // Get first type that has methods
        let ty = data.all_types().first().unwrap();
        let methods = data.get_type_methods(&ty.english_name);

        if !methods.is_empty() {
            println!("Type {} has {} methods", ty.english_name, methods.len());
            // All methods should belong to this type
            for method in &methods {
                assert_eq!(method.type_name.to_lowercase(), ty.english_name.to_lowercase());
            }
        }
    }

    // Salsa query tests - these test the Salsa integration
    #[salsa::db]
    #[derive(Clone, Default)]
    struct TestDatabase {
        storage: salsa::Storage<Self>,
    }

    impl salsa::Database for TestDatabase {}

    #[test]
    fn test_platform_type_query() {
        let db = TestDatabase::default();

        // Skip if no platform data available
        let data = PlatformDataInner::instance();
        if data.all_types().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        // Case-insensitive lookup using interned inputs
        let input1 = TypeNameInput::new(&db, "Строка".to_string());
        let input2 = TypeNameInput::new(&db, "СТРОКА".to_string());
        let input3 = TypeNameInput::new(&db, "String".to_string());

        let ty1 = platform_type_query(&db, input1);
        let ty2 = platform_type_query(&db, input2);
        let ty3 = platform_type_query(&db, input3);

        // All lookups should return the same type (or all None)
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

        // Skip if no platform data available
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

        // Skip if no platform data available
        let data = PlatformDataInner::instance();
        if data.all_types().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        let input = TypeNameInput::new(&db, "Строка".to_string());
        let methods = type_methods_query(&db, input);

        if !methods.is_empty() {
            // All methods should belong to String type
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

        // Test НачатьТранзакцию
        let func = data.get_global_function("НачатьТранзакцию");
        assert!(func.is_some(), "Should find НачатьТранзакцию");

        let func = func.unwrap();
        assert_eq!(func.name.as_str(), "НачатьТранзакцию");
        assert_eq!(func.english_name.as_str(), "BeginTransaction");
        println!("Found function: {} / {}", func.name, func.english_name);

        // Test with English name
        let func_en = data.get_global_function("BeginTransaction");
        assert!(func_en.is_some(), "Should find by English name");
        assert_eq!(func_en.unwrap().id, func.id);

        // Test case insensitivity
        let func_upper = data.get_global_function("НАЧАТЬТРАНЗАКЦИЮ");
        assert!(func_upper.is_some(), "Should be case-insensitive");
        assert_eq!(func_upper.unwrap().id, func.id);
    }

    /// Pin the exact set of platform entries whose last parameter is
    /// the unbounded-variadic tail. Sources:
    ///
    /// - **PR2** detector — `<X1>,...,<XN>` substring (`>,...,<` after
    ///   HTML decoding) in the HBK `Синтаксис:` chapter sets the JSON
    ///   field directly during regen.
    /// - **PR3** [`apply_docs_only_variadic_overlay`] — corrects three
    ///   constructors that are variadic per HBK `Описание:` but encode
    ///   a fixed `Синтаксис:` line; the overlay flips the trailing
    ///   param's flag at `PlatformDataInner::new` time.
    ///
    /// Combined inventory (audit 2026-04-29 against the 8.3.27.1989
    /// HBK dump):
    ///
    /// - **Globals** (3): `Мин`, `Макс`, `ПродолжитьВызов`.
    /// - **Constructors** (6): `Array.По количеству элементов`,
    ///   `COMSafeArray.Из массива 1`, `COMSafeArray.По типу элемента 1`
    ///   (PR2 — from syntax) and `Structure.По ключам и значениям`,
    ///   `FixedStructure.По ключам и значениям`,
    ///   `DynamicListRowKey.На основе путей и значений полей`
    ///   (PR3 — from overlay).
    ///
    /// `FormattedString.На основании строк` is intentionally NOT in
    /// this set: its variadic shape is encoded inside ONE rubric
    /// `MethodParam.name` (`Содержимое1,...,СодержимоеN`), so the JSON
    /// flag stays `false` and the adapter (`hir-ty/builtin.rs`) lifts
    /// `max_args = None` via [`name_implies_unbounded_variadic`].
    ///
    /// Any drift — extra entries, missing entries, or the flag landing
    /// on a non-last parameter — fails this test loudly.
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
            // The flag belongs only to the LAST parameter of the
            // signature. Walk all params and check both the
            // expected-on-last and not-on-others invariants.
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
            // No multi-variant global currently uses the unbounded
            // shape — variants must all stay at false.
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

        // Constructors — pinned by (type_name, variant_name). The first
        // three come from PR2's syntax-line detector; the last three are
        // PR3's `apply_docs_only_variadic_overlay` (variadic per HBK
        // description, fixed per HBK syntax).
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

        // Массив has multiple constructor overloads; both Russian and English
        // lookups must agree on overload count and ids.
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

        // `Query.Текст` is the canonical sanity check: writable scalar
        // returning `Строка`. Verify bilingual lookup on both sides —
        // (en type, ru prop), (ru type, en prop), (en type, en prop).
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

        // `Запрос.Параметры` is the user's headline scenario — read-only
        // Структура. Flag + scalar type must round-trip through the query.
        let got = platform_property_query(
            &db,
            MethodLookupInput::new(&db, "Запрос".to_string(), "Параметры".to_string()),
        );
        let prop = got.expect("Query.Parameters property must exist");
        assert!(prop.is_readonly, "Параметры is read-only");
        assert_eq!(prop.property_types, vec![smol_str::SmolStr::new("Структура")]);

        // `Запрос.МенеджерВременныхТаблиц` is the union property sanity check.
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

        // ОбработкаОшибок is the headline case (bsl-analyzer issue): platform
        // global of type МенеджерОбработкиОшибок introduced in 8.3.17.
        let ru = data.get_global_property("ОбработкаОшибок").expect("ru name must resolve");
        let en = data.get_global_property("ErrorProcessing").expect("en name must resolve");
        assert_eq!(ru.id, en.id);
        assert_eq!(ru.property_types, vec![smol_str::SmolStr::new("МенеджерОбработкиОшибок")]);
        // Case-insensitivity contract.
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

        // ОбработкаОшибок.КраткоеПредставлениеОшибки → method on
        // МенеджерОбработкиОшибок returning Строка.
        let m = data
            .resolve_global_member("ОбработкаОшибок", "КраткоеПредставлениеОшибки")
            .expect("ru.ru must resolve");
        assert_eq!(m.name.as_str(), "КраткоеПредставлениеОшибки");
        assert_eq!(m.english_name.as_str(), "BriefErrorDescription");

        // English on either side must also resolve.
        let m_en = data
            .resolve_global_member("ErrorProcessing", "BriefErrorDescription")
            .expect("en.en must resolve");
        assert_eq!(m_en.id, m.id);

        // Negative: unknown member.
        assert!(data.resolve_global_member("ОбработкаОшибок", "ЗаведомоНеТакогоМетода").is_none());
        // Negative: unknown global.
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
        // Pick a constructor that actually has a documentation record and
        // check that get_constructor_docs returns the same id.
        let ctor_with_docs =
            data.all_constructors().iter().find(|c| data.get_constructor_docs(c.id).is_some());
        if let Some(ctor) = ctor_with_docs {
            let docs = data.get_constructor_docs(ctor.id).unwrap();
            assert_eq!(docs.constructor_id, ctor.id);
        }
    }
}
