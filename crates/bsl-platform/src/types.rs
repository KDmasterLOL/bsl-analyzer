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
}

/// Raw platform type for const initialization (internal use only)
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct RawPlatformType {
    pub name: &'static str,
    pub english_name: &'static str,
    pub min_version: Option<&'static str>,
    pub context: Option<RawContextAvailability>,
}

impl From<&RawPlatformType> for PlatformType {
    fn from(raw: &RawPlatformType) -> Self {
        Self {
            name: raw.name.into(),
            english_name: raw.english_name.into(),
            min_version: raw.min_version.map(SmolStr::from),
            context: raw.context.as_ref().map(ContextAvailability::from),
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
    /// Method parameters
    pub parameters: Vec<MethodParam>,
    /// Minimum version (e.g., "8.0")
    pub min_version: Option<SmolStr>,
    /// Context availability
    pub context: Option<ContextAvailability>,
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
    pub min_version: Option<&'static str>,
    pub context: Option<RawContextAvailability>,
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
            min_version: raw.min_version.map(SmolStr::from),
            context: raw.context.as_ref().map(ContextAvailability::from),
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
    /// Function parameters
    pub parameters: Vec<MethodParam>,
    /// Minimum version (e.g., "8.0")
    pub min_version: Option<SmolStr>,
    /// Context availability
    pub context: Option<ContextAvailability>,
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
    pub min_version: Option<&'static str>,
    pub context: Option<RawContextAvailability>,
}

impl From<&RawGlobalFunction> for GlobalFunction {
    fn from(raw: &RawGlobalFunction) -> Self {
        Self {
            id: raw.id,
            name: raw.name.into(),
            english_name: raw.english_name.into(),
            return_type: raw.return_type.map(SmolStr::from),
            parameters: raw.parameters.iter().map(MethodParam::from).collect(),
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
}

/// Raw method parameter for const initialization (internal use only)
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct RawMethodParam {
    pub name: &'static str,
    pub param_type: Option<&'static str>,
    pub is_optional: bool,
}

impl From<&RawMethodParam> for MethodParam {
    fn from(raw: &RawMethodParam) -> Self {
        Self {
            name: raw.name.into(),
            param_type: raw.param_type.map(SmolStr::from),
            is_optional: raw.is_optional,
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
        Self { name: SmolStr::new(raw.name), description: raw.description.to_string() }
    }
}

impl From<&RawCodeExample> for CodeExample {
    fn from(raw: &RawCodeExample) -> Self {
        Self { code: raw.code.to_string(), description: raw.description.map(|d| d.to_string()) }
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
