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

impl From<&RawMethodVariant> for MethodVariant {
    fn from(raw: &RawMethodVariant) -> Self {
        Self {
            variant_name: raw.variant_name.map(SmolStr::from),
            parameters: raw.parameters.iter().map(MethodParam::from).collect(),
        }
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

impl From<&RawGlobalFunctionVariant> for GlobalFunctionVariant {
    fn from(raw: &RawGlobalFunctionVariant) -> Self {
        Self {
            variant_name: raw.variant_name.map(SmolStr::from),
            parameters: raw.parameters.iter().map(MethodParam::from).collect(),
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextAvailability {
    pub thick_client: bool,
    pub thin_client: bool,
    pub web_client: bool,
    pub server: bool,
    pub mobile_client: bool,
    pub external_connection: bool,
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
