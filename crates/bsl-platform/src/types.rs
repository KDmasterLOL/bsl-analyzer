//! Platform data structures.

use smol_str::SmolStr;

/// Platform type (Строка, Число, Массив, etc.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformType {
    /// Russian name (e.g., "Строка")
    pub name: SmolStr,
    /// English name (e.g., "String")
    pub english_name: SmolStr,
}

/// Raw platform type for const initialization (internal use only)
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct RawPlatformType {
    pub name: &'static str,
    pub english_name: &'static str,
}

impl From<&RawPlatformType> for PlatformType {
    fn from(raw: &RawPlatformType) -> Self {
        Self { name: raw.name.into(), english_name: raw.english_name.into() }
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
}

/// Raw platform method for const initialization (internal use only)
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct RawPlatformMethod {
    pub id: u32,
    pub type_name: &'static str,
    pub name: &'static str,
    pub english_name: &'static str,
}

impl From<&RawPlatformMethod> for PlatformMethod {
    fn from(raw: &RawPlatformMethod) -> Self {
        Self {
            id: raw.id,
            type_name: raw.type_name.into(),
            name: raw.name.into(),
            english_name: raw.english_name.into(),
        }
    }
}

/// Method parameter
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodParam {
    /// Parameter name
    pub name: SmolStr,
    /// Parameter type
    pub type_name: SmolStr,
    /// Whether parameter is optional
    pub optional: bool,
}

/// Full method documentation (available only with platform_docs feature)
#[cfg(feature = "platform_docs")]
#[derive(Debug, Clone)]
pub struct MethodDocs {
    /// Method ID
    pub method_id: u32,
    /// Syntax description
    pub syntax: String,
    /// Parameters description
    pub params_desc: String,
    /// Return value description
    pub returns_desc: String,
    /// Detailed description
    pub description: String,
    /// Example code
    pub example: Option<String>,
    /// Availability (client, server, etc.)
    pub availability: String,
    /// Version info
    pub version_info: String,
}
