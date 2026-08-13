use smol_str::SmolStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformType {
    pub name: SmolStr,
    pub english_name: SmolStr,
    pub min_version: Option<SmolStr>,
    pub context: Option<ContextAvailability>,
    pub iter_element_types: Vec<SmolStr>,
    /// XDTO type name declared by the type's help page, when present. Some
    /// configuration XML serializes attribute types by this name rather than
    /// the class name (e.g. `ГрафическаяСхема` ↔ `FlowchartContextType`).
    pub xdto_name: Option<SmolStr>,
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct RawPlatformType {
    pub name: &'static str,
    pub english_name: &'static str,
    pub min_version: Option<&'static str>,
    pub context: Option<RawContextAvailability>,
    pub iter_element_types: &'static [&'static str],
    pub xdto_name: Option<&'static str>,
}

impl From<&RawPlatformType> for PlatformType {
    fn from(raw: &RawPlatformType) -> Self {
        Self {
            name: raw.name.into(),
            english_name: raw.english_name.into(),
            min_version: raw.min_version.map(SmolStr::from),
            context: raw.context.as_ref().map(ContextAvailability::from),
            iter_element_types: raw.iter_element_types.iter().map(|s| SmolStr::new(*s)).collect(),
            xdto_name: raw.xdto_name.map(SmolStr::from),
        }
    }
}

impl PlatformType {
    /// Heap bytes owned by this type, memoised by `bsl-platform`'s
    /// `platform_type_query` for Salsa's `heap_size` hook: its name/version/XDTO
    /// `SmolStr`s (spilled ones only) plus the element-type vec. `context` is
    /// `Copy` and owns no heap. New heap-owning fields must be added here too.
    pub fn estimated_heap_size(&self) -> usize {
        stdx::heap::smol_str_bytes(self.name.len())
            + stdx::heap::smol_str_bytes(self.english_name.len())
            + self.min_version.as_ref().map_or(0, |s| stdx::heap::smol_str_bytes(s.len()))
            + self.xdto_name.as_ref().map_or(0, |s| stdx::heap::smol_str_bytes(s.len()))
            + stdx::heap::vec_bytes::<SmolStr>(self.iter_element_types.len())
            + self
                .iter_element_types
                .iter()
                .map(|s| stdx::heap::smol_str_bytes(s.len()))
                .sum::<usize>()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformMethod {
    pub id: u32,
    pub type_name: SmolStr,
    pub name: SmolStr,
    pub english_name: SmolStr,
    pub return_type: Option<SmolStr>,
    pub parameters: Vec<MethodParam>,
    pub variants: Vec<MethodVariant>,
    pub min_version: Option<SmolStr>,
    pub context: Option<ContextAvailability>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodVariant {
    pub variant_name: Option<SmolStr>,
    pub parameters: Vec<MethodParam>,
}

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

impl PlatformMethod {
    /// Heap bytes owned by this method, memoised by `bsl-platform`'s
    /// `platform_method_query`/`type_methods_query`/`manager_methods_query`/
    /// `prefixed_method_query`/`global_member_method_query` for Salsa's
    /// `heap_size` hook: its name/version `SmolStr`s plus the parameter and
    /// overload-variant vecs. `id`/`context` are `Copy` and own no heap. New
    /// heap-owning fields must be added here too.
    pub fn estimated_heap_size(&self) -> usize {
        stdx::heap::smol_str_bytes(self.type_name.len())
            + stdx::heap::smol_str_bytes(self.name.len())
            + stdx::heap::smol_str_bytes(self.english_name.len())
            + self.return_type.as_ref().map_or(0, |s| stdx::heap::smol_str_bytes(s.len()))
            + stdx::heap::vec_bytes::<MethodParam>(self.parameters.len())
            + self.parameters.iter().map(MethodParam::estimated_heap_size).sum::<usize>()
            + stdx::heap::vec_bytes::<MethodVariant>(self.variants.len())
            + self.variants.iter().map(MethodVariant::estimated_heap_size).sum::<usize>()
            + self.min_version.as_ref().map_or(0, |s| stdx::heap::smol_str_bytes(s.len()))
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

impl MethodVariant {
    /// Heap bytes owned by this overload variant: its name plus the parameter
    /// vec and each parameter's own owned payload.
    pub fn estimated_heap_size(&self) -> usize {
        self.variant_name.as_ref().map_or(0, |s| stdx::heap::smol_str_bytes(s.len()))
            + stdx::heap::vec_bytes::<MethodParam>(self.parameters.len())
            + self.parameters.iter().map(MethodParam::estimated_heap_size).sum::<usize>()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalFunction {
    pub id: u32,
    pub name: SmolStr,
    pub english_name: SmolStr,
    pub return_type: Option<SmolStr>,
    pub parameters: Vec<MethodParam>,
    pub variants: Vec<GlobalFunctionVariant>,
    pub min_version: Option<SmolStr>,
    pub context: Option<ContextAvailability>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalFunctionVariant {
    pub variant_name: Option<SmolStr>,
    pub parameters: Vec<MethodParam>,
}

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

impl GlobalFunction {
    /// Heap bytes owned by this global function, memoised by `bsl-platform`'s
    /// `global_function_query` for Salsa's `heap_size` hook: its name/version
    /// `SmolStr`s plus the parameter and overload-variant vecs. New
    /// heap-owning fields must be added here too.
    pub fn estimated_heap_size(&self) -> usize {
        stdx::heap::smol_str_bytes(self.name.len())
            + stdx::heap::smol_str_bytes(self.english_name.len())
            + self.return_type.as_ref().map_or(0, |s| stdx::heap::smol_str_bytes(s.len()))
            + stdx::heap::vec_bytes::<MethodParam>(self.parameters.len())
            + self.parameters.iter().map(MethodParam::estimated_heap_size).sum::<usize>()
            + stdx::heap::vec_bytes::<GlobalFunctionVariant>(self.variants.len())
            + self.variants.iter().map(GlobalFunctionVariant::estimated_heap_size).sum::<usize>()
            + self.min_version.as_ref().map_or(0, |s| stdx::heap::smol_str_bytes(s.len()))
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

impl GlobalFunctionVariant {
    /// Heap bytes owned by this overload variant: its name plus the parameter
    /// vec and each parameter's own owned payload.
    pub fn estimated_heap_size(&self) -> usize {
        self.variant_name.as_ref().map_or(0, |s| stdx::heap::smol_str_bytes(s.len()))
            + stdx::heap::vec_bytes::<MethodParam>(self.parameters.len())
            + self.parameters.iter().map(MethodParam::estimated_heap_size).sum::<usize>()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformConstructor {
    pub id: u32,
    pub type_name: SmolStr,
    pub variant_name: Option<SmolStr>,
    pub parameters: Vec<MethodParam>,
    pub min_version: Option<SmolStr>,
    pub context: Option<ContextAvailability>,
}

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

impl PlatformConstructor {
    /// Heap bytes owned by this constructor overload, memoised by
    /// `bsl-platform`'s `platform_constructors_query` for Salsa's `heap_size`
    /// hook: its name/version `SmolStr`s plus the parameter vec. New
    /// heap-owning fields must be added here too.
    pub fn estimated_heap_size(&self) -> usize {
        stdx::heap::smol_str_bytes(self.type_name.len())
            + self.variant_name.as_ref().map_or(0, |s| stdx::heap::smol_str_bytes(s.len()))
            + stdx::heap::vec_bytes::<MethodParam>(self.parameters.len())
            + self.parameters.iter().map(MethodParam::estimated_heap_size).sum::<usize>()
            + self.min_version.as_ref().map_or(0, |s| stdx::heap::smol_str_bytes(s.len()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformProperty {
    pub id: u32,
    pub type_name: SmolStr,
    pub name: SmolStr,
    pub english_name: SmolStr,
    pub property_types: Vec<SmolStr>,
    pub is_readonly: bool,
    pub min_version: Option<SmolStr>,
    pub context: Option<ContextAvailability>,
}

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

impl PlatformProperty {
    /// Heap bytes owned by this property, memoised by `bsl-platform`'s
    /// `platform_property_query`/`type_properties_query`/`global_property_query`
    /// for Salsa's `heap_size` hook: its name/version `SmolStr`s plus the
    /// property-type vec. `id`/`is_readonly`/`context` are `Copy` and own no
    /// heap. New heap-owning fields must be added here too.
    pub fn estimated_heap_size(&self) -> usize {
        stdx::heap::smol_str_bytes(self.type_name.len())
            + stdx::heap::smol_str_bytes(self.name.len())
            + stdx::heap::smol_str_bytes(self.english_name.len())
            + stdx::heap::vec_bytes::<SmolStr>(self.property_types.len())
            + self.property_types.iter().map(|s| stdx::heap::smol_str_bytes(s.len())).sum::<usize>()
            + self.min_version.as_ref().map_or(0, |s| stdx::heap::smol_str_bytes(s.len()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodParam {
    pub name: SmolStr,
    pub param_type: Option<SmolStr>,
    pub is_optional: bool,
    pub is_variadic: bool,
}

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

impl MethodParam {
    /// Heap bytes owned by this parameter: its name/type `SmolStr`s (spilled
    /// ones only). `is_optional`/`is_variadic` are `Copy` and own no heap.
    pub fn estimated_heap_size(&self) -> usize {
        stdx::heap::smol_str_bytes(self.name.len())
            + self.param_type.as_ref().map_or(0, |s| stdx::heap::smol_str_bytes(s.len()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextAvailability {
    pub thick_client: bool,
    pub thin_client: bool,
    pub web_client: bool,
    pub server: bool,
    pub mobile_client: bool,
    pub external_connection: bool,
}

impl ContextAvailability {
    /// Availability of an entry the syntax helper leaves unmarked. The helper writes a
    /// "Доступность" list only where the platform restricts an entry, so a missing list means
    /// "everywhere" — the opposite of an entry marked available in no context at all.
    pub const UNRESTRICTED: Self = Self {
        thick_client: true,
        thin_client: true,
        web_client: true,
        server: true,
        mobile_client: true,
        external_connection: true,
    };

    /// Availability of an entry whose markup may be absent, with [`Self::UNRESTRICTED`]
    /// standing in for the missing one. Every consumer must read a missing markup the same
    /// way, so the rule lives here rather than in each of them.
    pub fn effective(context: Option<&Self>) -> Self {
        context.copied().unwrap_or(Self::UNRESTRICTED)
    }
}

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

#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawPlatformGlobalKind {
    Function,
    Property,
    SystemEnum,
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct RawPlatformGlobalSymbol {
    pub canonical_ru: &'static str,
    pub canonical_en: &'static str,
    pub kind: RawPlatformGlobalKind,
    /// Bit layout is attested in `data/global_catalog.json`.
    pub environment_mask: u8,
    pub writable: bool,
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct RawPlatformGlobalCatalogMetadata {
    pub schema_version: u32,
    pub platform_version: &'static str,
    pub edt_version: &'static str,
    pub global_context_sha256: &'static str,
    pub system_enums_sha256: &'static str,
    pub complete_global_context: bool,
    pub complete_system_enums: bool,
}

#[derive(Debug, Clone)]
pub struct MethodDocs {
    pub method_id: u32,
    pub syntax: String,
    pub description: String,
    pub params: Vec<ParamDocs>,
    pub examples: Vec<CodeExample>,
    pub notes: Option<String>,
    pub see_also: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ParamDocs {
    pub name: SmolStr,
    pub description: String,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CodeExample {
    pub code: String,
    pub description: Option<String>,
}

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

#[derive(Debug, Clone)]
pub struct RawParamDocs {
    pub name: &'static str,
    pub description: &'static str,
    pub default_value: Option<&'static str>,
}

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

#[derive(Debug, Clone)]
pub struct PropertyDocs {
    pub property_id: u32,
    pub description: String,
    pub notes: Option<String>,
    pub see_also: Vec<String>,
}

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

#[derive(Debug, Clone)]
pub struct KeywordDocs {
    pub keyword_ru: SmolStr,
    pub keyword_en: SmolStr,
    pub syntax: String,
    pub description: String,
    pub params: Vec<ParamDocs>,
    pub min_version: Option<String>,
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_method_heap_counts_params_and_spilled_strings() {
        let long_type_name = "ПользовательскийТипСДлиннымИменем";
        let method = PlatformMethod {
            id: 1,
            type_name: SmolStr::new(long_type_name),
            name: SmolStr::new("Метод"),
            english_name: SmolStr::new("Method"),
            return_type: Some(SmolStr::new(long_type_name)),
            parameters: vec![
                MethodParam {
                    name: SmolStr::new("ПараметрСДлиннымИменемБезИнлайна"),
                    param_type: Some(SmolStr::new(long_type_name)),
                    is_optional: false,
                    is_variadic: false,
                },
                MethodParam {
                    name: SmolStr::new("Второй"),
                    param_type: None,
                    is_optional: true,
                    is_variadic: false,
                },
            ],
            variants: vec![],
            min_version: None,
            context: None,
        };

        let long_bytes = long_type_name.len();
        let bytes = method.estimated_heap_size();
        // At least the two spilled-`SmolStr` occurrences that appear verbatim
        // (`type_name`/`return_type`); well under a kilobyte for two params.
        assert!(bytes > long_bytes * 2);
        assert!(bytes < 1024);
    }
}
