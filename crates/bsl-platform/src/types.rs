//! Platform data structures.

use smol_str::SmolStr;

/// Platform type (Строка, Число, Массив, etc.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformType {
    /// Russian name (e.g., "Строка")
    pub name: SmolStr,
    /// English name (e.g., "String")
    pub english_name: SmolStr,
    /// Minimum version (e.g., "8.0")
    pub min_version: Option<SmolStr>,
    /// Context availability
    pub context: Option<ContextAvailability>,
    /// Element types from the HBK `Элементы коллекции:` chapter, in source
    /// order. Empty when the type is not iterable per the platform syntax
    /// help (the vast majority of types). Single-element for collections
    /// like `Массив` (`["Произвольный"]`) or `ТаблицаЗначений`
    /// (`["СтрокаТаблицыЗначений"]`); multi-element for pages like
    /// `ПоляКолонкиСхемыЗапроса`
    /// (`["ВыражениеСхемыЗапроса", "ВложеннаяТаблицаСхемыЗапроса",
    /// "Неопределено"]`). Strings are stored verbatim as scraped from the
    /// HBK, including the `<Имя …>` placeholder used for parametric
    /// element types like `РегистрСведенийЗапись.<Имя регистра>`.
    /// Placeholder substitution and `Ty` resolution happen in `hir-ty`.
    pub iter_element_types: Vec<SmolStr>,
}

/// Raw platform type for const initialization (internal use only)
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct RawPlatformType {
    pub name: &'static str,
    pub english_name: &'static str,
    pub min_version: Option<&'static str>,
    pub context: Option<RawContextAvailability>,
    pub iter_element_types: &'static [&'static str],
}

impl From<&RawPlatformType> for PlatformType {
    fn from(raw: &RawPlatformType) -> Self {
        Self {
            name: raw.name.into(),
            english_name: raw.english_name.into(),
            min_version: raw.min_version.map(SmolStr::from),
            context: raw.context.as_ref().map(ContextAvailability::from),
            iter_element_types: raw.iter_element_types.iter().map(|s| SmolStr::new(*s)).collect(),
        }
    }
}

/// Platform method
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformMethod {
    /// Method ID (unique identifier)
    pub id: u32,
    /// Type this method belongs to
    pub type_name: SmolStr,
    /// Russian method name (e.g., "ВРег")
    pub name: SmolStr,
    /// English method name (e.g., "Upper")
    pub english_name: SmolStr,
    /// Return type (e.g., "Строка")
    pub return_type: Option<SmolStr>,
    /// Method parameters — flattened union across all syntax variants
    /// (legacy field used by hover / completion). Multi-overload methods
    /// also populate [`Self::variants`]; arity / type checks must consult
    /// `variants`.
    pub parameters: Vec<MethodParam>,
    /// Per-variant parameter lists for methods whose HBK page declares
    /// several `Вариант синтаксиса:` sections (e.g.
    /// `ЧтениеXML.ПолучитьАтрибут`, `ТаблицаЗначений.Скопировать`,
    /// `COMSafeArray.GetValue`). Empty for the vast majority of
    /// single-overload methods — callers should treat empty as
    /// "use `parameters` as the only signature".
    pub variants: Vec<MethodVariant>,
    /// Minimum version (e.g., "8.0")
    pub min_version: Option<SmolStr>,
    /// Context availability
    pub context: Option<ContextAvailability>,
}

/// One syntax variant of a multi-overload platform method. Structurally
/// identical to [`GlobalFunctionVariant`] but kept separate so that
/// changes to one signature kind don't accidentally widen the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodVariant {
    /// Variant name from the `Вариант синтаксиса:` chapter (e.g.
    /// `"По номеру"`). `None` for anonymous variants — not emitted today.
    pub variant_name: Option<SmolStr>,
    /// Parameters declared inside this variant's section.
    pub parameters: Vec<MethodParam>,
}

/// Raw platform method for const initialization (internal use only)
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct RawPlatformMethod {
    pub id: u32,
    pub type_name: &'static str,
    pub name: &'static str,
    pub english_name: &'static str,
    pub return_type: Option<&'static str>,
    pub parameters: &'static [RawMethodParam],
    pub variants: &'static [RawMethodVariant],
    pub min_version: Option<&'static str>,
    pub context: Option<RawContextAvailability>,
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct RawMethodVariant {
    pub variant_name: Option<&'static str>,
    pub parameters: &'static [RawMethodParam],
}

impl From<&RawPlatformMethod> for PlatformMethod {
    fn from(raw: &RawPlatformMethod) -> Self {
        Self {
            id: raw.id,
            type_name: raw.type_name.into(),
            name: raw.name.into(),
            english_name: raw.english_name.into(),
            return_type: raw.return_type.map(SmolStr::from),
            parameters: raw.parameters.iter().map(MethodParam::from).collect(),
            variants: raw.variants.iter().map(MethodVariant::from).collect(),
            min_version: raw.min_version.map(SmolStr::from),
            context: raw.context.as_ref().map(ContextAvailability::from),
        }
    }
}

impl From<&RawMethodVariant> for MethodVariant {
    fn from(raw: &RawMethodVariant) -> Self {
        Self {
            variant_name: raw.variant_name.map(SmolStr::from),
            parameters: raw.parameters.iter().map(MethodParam::from).collect(),
        }
    }
}

/// Global function
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalFunction {
    /// Function ID (unique identifier)
    pub id: u32,
    /// Russian function name (e.g., "НачатьТранзакцию")
    pub name: SmolStr,
    /// English function name (e.g., "BeginTransaction")
    pub english_name: SmolStr,
    /// Return type (e.g., "Строка")
    pub return_type: Option<SmolStr>,
    /// Function parameters — flattened union across all syntax variants for
    /// hover / completion. Multi-overload functions also populate
    /// [`Self::variants`]; arity / type checks must consult `variants`.
    pub parameters: Vec<MethodParam>,
    /// Per-variant parameter lists for functions whose HBK page declares
    /// several `Вариант синтаксиса:` sections (e.g.
    /// `ПодключитьВнешнююКомпоненту`, `Дата`, `ОткрытьФорму`).
    /// Empty for the vast majority of single-overload functions — callers
    /// should treat empty as "use `parameters` as the only signature".
    pub variants: Vec<GlobalFunctionVariant>,
    /// Minimum version (e.g., "8.0")
    pub min_version: Option<SmolStr>,
    /// Context availability
    pub context: Option<ContextAvailability>,
}

/// One syntax variant of a multi-overload global function. Mirrors the
/// `<p class="V8SH_chapter">Вариант синтаксиса: …</p>` partition of the
/// platform HBK page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalFunctionVariant {
    /// Human-readable variant name (the suffix after `Вариант синтаксиса:`,
    /// e.g. "По идентификатору"). `None` for an anonymous variant — never
    /// emitted by the parser today, but allowed in the schema so that
    /// future single-page overloads without a chapter label can fit.
    pub variant_name: Option<SmolStr>,
    /// Parameters declared inside this variant's section.
    pub parameters: Vec<MethodParam>,
}

/// Raw global function for const initialization (internal use only)
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct RawGlobalFunction {
    pub id: u32,
    pub name: &'static str,
    pub english_name: &'static str,
    pub return_type: Option<&'static str>,
    pub parameters: &'static [RawMethodParam],
    pub variants: &'static [RawGlobalFunctionVariant],
    pub min_version: Option<&'static str>,
    pub context: Option<RawContextAvailability>,
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct RawGlobalFunctionVariant {
    pub variant_name: Option<&'static str>,
    pub parameters: &'static [RawMethodParam],
}

impl From<&RawGlobalFunction> for GlobalFunction {
    fn from(raw: &RawGlobalFunction) -> Self {
        Self {
            id: raw.id,
            name: raw.name.into(),
            english_name: raw.english_name.into(),
            return_type: raw.return_type.map(SmolStr::from),
            parameters: raw.parameters.iter().map(MethodParam::from).collect(),
            variants: raw.variants.iter().map(GlobalFunctionVariant::from).collect(),
            min_version: raw.min_version.map(SmolStr::from),
            context: raw.context.as_ref().map(ContextAvailability::from),
        }
    }
}

impl From<&RawGlobalFunctionVariant> for GlobalFunctionVariant {
    fn from(raw: &RawGlobalFunctionVariant) -> Self {
        Self {
            variant_name: raw.variant_name.map(SmolStr::from),
            parameters: raw.parameters.iter().map(MethodParam::from).collect(),
        }
    }
}

/// Platform constructor overload (`Новый X(...)`).
///
/// A single BSL platform type can expose several constructor forms (`Массив`,
/// `Структура`, `СписокЗначений` — all have multiple variants). Each variant is
/// a distinct `PlatformConstructor`. `id` mirrors the source HBK's
/// `ctor{N}.html` filename so the numeric identity stays stable across
/// regenerations of the same help book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformConstructor {
    /// Stable numeric id (from `ctor{N}.html`).
    pub id: u32,
    /// English name of the enclosing platform type (e.g. "Array"). Matches
    /// `PlatformMethod::type_name` so both tables key on the same string.
    pub type_name: SmolStr,
    /// Human-readable variant label (e.g. "По количеству элементов"). `None`
    /// only for malformed HBK pages; normal pages always have it.
    pub variant_name: Option<SmolStr>,
    /// Declared parameters of this overload.
    pub parameters: Vec<MethodParam>,
    pub min_version: Option<SmolStr>,
    pub context: Option<ContextAvailability>,
}

/// Raw platform constructor for const initialization (internal use only).
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct RawPlatformConstructor {
    pub id: u32,
    pub type_name: &'static str,
    pub variant_name: Option<&'static str>,
    pub parameters: &'static [RawMethodParam],
    pub min_version: Option<&'static str>,
    pub context: Option<RawContextAvailability>,
}

impl From<&RawPlatformConstructor> for PlatformConstructor {
    fn from(raw: &RawPlatformConstructor) -> Self {
        Self {
            id: raw.id,
            type_name: raw.type_name.into(),
            variant_name: raw.variant_name.map(SmolStr::from),
            parameters: raw.parameters.iter().map(MethodParam::from).collect(),
            min_version: raw.min_version.map(SmolStr::from),
            context: raw.context.as_ref().map(ContextAvailability::from),
        }
    }
}

/// Property of a platform type (e.g. `Запрос.Параметры`, `Запрос.Текст`).
///
/// Unlike [`PlatformMethod`], property pages in HBK encode the value type as a
/// list of references inside the `Описание:` chapter. A property may declare
/// several types simultaneously (`МенеджерВременныхТаблиц, Неопределено`), so
/// the runtime keeps them as a `Vec<SmolStr>` and lets the `hir-ty` adapter
/// decide whether to collapse to a scalar `Ty` or a union. The `is_readonly`
/// flag is derived from the `Использование:` chapter (`"Только чтение"` vs
/// `"Чтение и запись"`) and feeds the `ReadOnlyPropertyAssignment` diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformProperty {
    /// Stable numeric id (monotonic across the extraction run).
    pub id: u32,
    /// English name of the enclosing platform type (e.g. "Query"). Same shape
    /// and case as [`PlatformMethod::type_name`] so both indices key on the
    /// same string.
    pub type_name: SmolStr,
    /// Russian property name (e.g. "Параметры").
    pub name: SmolStr,
    /// English property name (e.g. "Parameters").
    pub english_name: SmolStr,
    /// Declared value types in source order. Single-element for scalars,
    /// multi-element for union declarations. Empty when the HBK page omits
    /// the `Тип:` marker (free-prose description).
    pub property_types: Vec<SmolStr>,
    /// `true` when the `Использование:` chapter reads "Только чтение";
    /// `false` for read-write properties.
    pub is_readonly: bool,
    pub min_version: Option<SmolStr>,
    pub context: Option<ContextAvailability>,
}

/// Raw platform property for const initialization (internal use only).
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct RawPlatformProperty {
    pub id: u32,
    pub type_name: &'static str,
    pub name: &'static str,
    pub english_name: &'static str,
    pub property_types: &'static [&'static str],
    pub is_readonly: bool,
    pub min_version: Option<&'static str>,
    pub context: Option<RawContextAvailability>,
}

impl From<&RawPlatformProperty> for PlatformProperty {
    fn from(raw: &RawPlatformProperty) -> Self {
        Self {
            id: raw.id,
            type_name: raw.type_name.into(),
            name: raw.name.into(),
            english_name: raw.english_name.into(),
            property_types: raw.property_types.iter().map(|s| SmolStr::new(*s)).collect(),
            is_readonly: raw.is_readonly,
            min_version: raw.min_version.map(SmolStr::from),
            context: raw.context.as_ref().map(ContextAvailability::from),
        }
    }
}

/// Method parameter
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodParam {
    /// Parameter name
    pub name: SmolStr,
    /// Parameter type (e.g., "Число", "Произвольный")
    pub param_type: Option<SmolStr>,
    /// Whether parameter is optional
    pub is_optional: bool,
    /// Whether this parameter is the unbounded-variadic tail of the
    /// signature (`<X1>,...,<XN>` shape in HBK syntax). Only ever true
    /// for the last parameter — there are no required-after-ellipsis
    /// shapes in the BSL platform corpus. Defaults to `false` when the
    /// JSON omits the field.
    pub is_variadic: bool,
}

/// Raw method parameter for const initialization (internal use only)
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct RawMethodParam {
    pub name: &'static str,
    pub param_type: Option<&'static str>,
    pub is_optional: bool,
    pub is_variadic: bool,
}

impl From<&RawMethodParam> for MethodParam {
    fn from(raw: &RawMethodParam) -> Self {
        Self {
            name: raw.name.into(),
            param_type: raw.param_type.map(SmolStr::from),
            is_optional: raw.is_optional,
            is_variadic: raw.is_variadic,
        }
    }
}

/// Context availability information
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextAvailability {
    /// Available on thick client
    pub thick_client: bool,
    /// Available on thin client
    pub thin_client: bool,
    /// Available on web client
    pub web_client: bool,
    /// Available on server
    pub server: bool,
    /// Available on mobile client
    pub mobile_client: bool,
    /// Available on external connection
    pub external_connection: bool,
}

/// Raw context availability for const initialization (internal use only)
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct RawContextAvailability {
    pub thick_client: bool,
    pub thin_client: bool,
    pub web_client: bool,
    pub server: bool,
    pub mobile_client: bool,
    pub external_connection: bool,
}

impl From<&RawContextAvailability> for ContextAvailability {
    fn from(raw: &RawContextAvailability) -> Self {
        Self {
            thick_client: raw.thick_client,
            thin_client: raw.thin_client,
            web_client: raw.web_client,
            server: raw.server,
            mobile_client: raw.mobile_client,
            external_connection: raw.external_connection,
        }
    }
}

/// Full method documentation
#[derive(Debug, Clone)]
pub struct MethodDocs {
    /// Method ID
    pub method_id: u32,
    /// Syntax description
    pub syntax: String,
    /// Detailed description
    pub description: String,
    /// Parameter descriptions
    pub params: Vec<ParamDocs>,
    /// Code examples
    pub examples: Vec<CodeExample>,
    /// Notes
    pub notes: Option<String>,
    /// See also links
    pub see_also: Vec<String>,
}

/// Parameter documentation
#[derive(Debug, Clone)]
pub struct ParamDocs {
    /// Parameter name
    pub name: SmolStr,
    /// Full description
    pub description: String,
    /// Default value (e.g., "Неопределено")
    pub default_value: Option<String>,
}

/// Code example
#[derive(Debug, Clone)]
pub struct CodeExample {
    /// Code text
    pub code: String,
    /// Optional description
    pub description: Option<String>,
}

/// Raw method documentation for const initialization
#[derive(Debug, Clone)]
pub struct RawMethodDocs {
    pub method_id: u32,
    pub syntax: &'static str,
    pub description: &'static str,
    pub params: &'static [RawParamDocs],
    pub examples: &'static [RawCodeExample],
    pub notes: Option<&'static str>,
    pub see_also: &'static [&'static str],
}

/// Raw parameter documentation for const initialization
#[derive(Debug, Clone)]
pub struct RawParamDocs {
    pub name: &'static str,
    pub description: &'static str,
    pub default_value: Option<&'static str>,
}

/// Raw code example for const initialization
#[derive(Debug, Clone)]
pub struct RawCodeExample {
    pub code: &'static str,
    pub description: Option<&'static str>,
}

impl From<&RawMethodDocs> for MethodDocs {
    fn from(raw: &RawMethodDocs) -> Self {
        Self {
            method_id: raw.method_id,
            syntax: raw.syntax.to_string(),
            description: raw.description.to_string(),
            params: raw.params.iter().map(ParamDocs::from).collect(),
            examples: raw.examples.iter().map(CodeExample::from).collect(),
            notes: raw.notes.map(|n| n.to_string()),
            see_also: raw.see_also.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl From<&RawParamDocs> for ParamDocs {
    fn from(raw: &RawParamDocs) -> Self {
        Self {
            name: SmolStr::new(raw.name),
            description: raw.description.to_string(),
            default_value: raw.default_value.map(String::from),
        }
    }
}

impl From<&RawCodeExample> for CodeExample {
    fn from(raw: &RawCodeExample) -> Self {
        Self { code: raw.code.to_string(), description: raw.description.map(|d| d.to_string()) }
    }
}

/// Full constructor documentation (by constructor id).
///
/// Mirrors [`MethodDocs`] but carries `constructor_id` instead of `method_id`
/// so the lookup index in `PlatformDataInner` cannot be accidentally confused
/// with the method index.
#[derive(Debug, Clone)]
pub struct ConstructorDocs {
    pub constructor_id: u32,
    pub syntax: String,
    pub description: String,
    pub params: Vec<ParamDocs>,
    pub examples: Vec<CodeExample>,
    pub notes: Option<String>,
    pub see_also: Vec<String>,
}

/// Raw constructor documentation for const initialization.
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct RawConstructorDocs {
    pub constructor_id: u32,
    pub syntax: &'static str,
    pub description: &'static str,
    pub params: &'static [RawParamDocs],
    pub examples: &'static [RawCodeExample],
    pub notes: Option<&'static str>,
    pub see_also: &'static [&'static str],
}

impl From<&RawConstructorDocs> for ConstructorDocs {
    fn from(raw: &RawConstructorDocs) -> Self {
        Self {
            constructor_id: raw.constructor_id,
            syntax: raw.syntax.to_string(),
            description: raw.description.to_string(),
            params: raw.params.iter().map(ParamDocs::from).collect(),
            examples: raw.examples.iter().map(CodeExample::from).collect(),
            notes: raw.notes.map(|n| n.to_string()),
            see_also: raw.see_also.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// Full property documentation (by property id).
///
/// Mirrors [`MethodDocs`] / [`ConstructorDocs`] but stripped down to the parts
/// a property page actually ships: a free-prose description, an optional
/// `Примечание:` block, and a `См. также:` list. Structured data like
/// `Тип:` already lives on [`PlatformProperty::property_types`], so it's
/// deliberately not duplicated here.
#[derive(Debug, Clone)]
pub struct PropertyDocs {
    pub property_id: u32,
    pub description: String,
    pub notes: Option<String>,
    pub see_also: Vec<String>,
}

/// Raw property documentation for const initialization.
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct RawPropertyDocs {
    pub property_id: u32,
    pub description: &'static str,
    pub notes: Option<&'static str>,
    pub see_also: &'static [&'static str],
}

impl From<&RawPropertyDocs> for PropertyDocs {
    fn from(raw: &RawPropertyDocs) -> Self {
        Self {
            property_id: raw.property_id,
            description: raw.description.to_string(),
            notes: raw.notes.map(|n| n.to_string()),
            see_also: raw.see_also.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// Keyword documentation (for BSL language constructs like Если, Для, etc.)
#[derive(Debug, Clone)]
pub struct KeywordDocs {
    /// Russian keyword name (e.g., "Для")
    pub keyword_ru: SmolStr,
    /// English keyword name (e.g., "For")
    pub keyword_en: SmolStr,
    /// Syntax description
    pub syntax: String,
    /// Detailed description
    pub description: String,
    /// Parameter descriptions
    pub params: Vec<ParamDocs>,
    /// Minimum version (e.g., "8.0")
    pub min_version: Option<String>,
}

/// Raw keyword documentation for const initialization
#[derive(Debug, Clone)]
pub struct RawKeywordDocs {
    pub keyword_ru: &'static str,
    pub keyword_en: &'static str,
    pub syntax: &'static str,
    pub description: &'static str,
    pub params: &'static [RawParamDocs],
    pub min_version: Option<&'static str>,
}

impl From<&RawKeywordDocs> for KeywordDocs {
    fn from(raw: &RawKeywordDocs) -> Self {
        Self {
            keyword_ru: SmolStr::new(raw.keyword_ru),
            keyword_en: SmolStr::new(raw.keyword_en),
            syntax: raw.syntax.to_string(),
            description: raw.description.to_string(),
            params: raw.params.iter().map(ParamDocs::from).collect(),
            min_version: raw.min_version.map(|v| v.to_string()),
        }
    }
}
