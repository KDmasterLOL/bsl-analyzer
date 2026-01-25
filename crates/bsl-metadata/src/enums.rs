//! Enums for BSL metadata
//!
//! Ported from <https://github.com/1c-syntax/mdclasses>

use serde::{Deserialize, Serialize};

/// Return value reuse mode for common modules
///
/// Java equivalent: `com.github._1c_syntax.bsl.mdo.support.ReturnValueReuse`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ReturnValueReuse {
    /// Do not reuse return values (default)
    #[serde(rename = "DontUse")]
    DontUse,

    /// Reuse return values during a single request
    #[serde(rename = "DuringRequest")]
    DuringRequest,

    /// Reuse return values throughout a session
    #[serde(rename = "DuringSession")]
    DuringSession,

    /// Unknown/unrecognized option
    #[serde(other)]
    #[default]
    Unknown,
}

impl ReturnValueReuse {
    /// Parse from Russian or English name
    pub fn from_name(name: &str) -> Self {
        let name_lower = name.to_lowercase();
        match name_lower.as_str() {
            "dontuse" | "неиспользовать" => Self::DontUse,
            "duringrequest" | "навремязапроса" => Self::DuringRequest,
            "duringsession" | "навремясеанса" => Self::DuringSession,
            _ => Self::Unknown,
        }
    }
}

/// Module type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ModuleType {
    /// Application module
    #[serde(rename = "ApplicationModule")]
    ApplicationModule,
    /// Common module
    #[serde(rename = "CommonModule")]
    CommonModule,
    /// Session module
    #[serde(rename = "SessionModule")]
    SessionModule,
    /// External connection module
    #[serde(rename = "ExternalConnectionModule")]
    ExternalConnectionModule,
    /// Managed application module
    #[serde(rename = "ManagedApplicationModule")]
    ManagedApplicationModule,
    /// Ordinary application module
    #[serde(rename = "OrdinaryApplicationModule")]
    OrdinaryApplicationModule,
    /// Manager module
    #[serde(rename = "ManagerModule")]
    ManagerModule,
    /// Object module
    #[serde(rename = "ObjectModule")]
    ObjectModule,
    /// Record set module
    #[serde(rename = "RecordSetModule")]
    RecordSetModule,
    /// Value manager module
    #[serde(rename = "ValueManagerModule")]
    ValueManagerModule,
    /// Form module
    #[serde(rename = "FormModule")]
    FormModule,
    /// Command module
    #[serde(rename = "CommandModule")]
    CommandModule,
    /// Unknown module type
    #[serde(other)]
    #[default]
    Unknown,
}

/// Object belonging (own or borrowed from configuration)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ObjectBelonging {
    /// Own object
    #[serde(rename = "Own")]
    #[default]
    Own,
    /// Adopted object from another configuration
    #[serde(rename = "Adopted")]
    Adopted,
    /// Unknown belonging
    #[serde(other)]
    Unknown,
}

/// Support variant for configuration compatibility
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum SupportVariant {
    /// Not editable
    #[serde(rename = "NotEditable")]
    NotEditable,
    /// Editable
    #[serde(rename = "Editable")]
    Editable,
    /// Not supported
    #[serde(rename = "NotSupported")]
    NotSupported,
    /// Unknown support variant
    #[serde(other)]
    #[default]
    Unknown,
}

/// Form type (managed vs ordinary form)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum FormType {
    /// Managed form (управляемая форма)
    #[serde(rename = "Managed")]
    #[default]
    Managed,
    /// Ordinary form (обычная форма)
    #[serde(rename = "Ordinary")]
    Ordinary,
    /// Unknown form type
    #[serde(other)]
    Unknown,
}

impl FormType {
    /// Parse from Russian or English name
    pub fn from_name(name: &str) -> Self {
        let name_lower = name.to_lowercase();
        match name_lower.as_str() {
            "managed" | "управляемая" => Self::Managed,
            "ordinary" | "обычная" => Self::Ordinary,
            _ => Self::Unknown,
        }
    }
}

/// Code series mode for Catalogs, ChartOfCharacteristicTypes, ChartOfAccounts
///
/// Determines how code uniqueness is enforced across the object hierarchy.
///
/// Java equivalent: `com.github._1c_syntax.bsl.mdo.support.CodeSeries`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum CodeSeries {
    /// Codes are unique across the entire catalog/chart
    /// (WholeCatalog, WholeCharacteristicKind, WholeChartOfAccounts)
    #[serde(
        rename = "WholeCatalog",
        alias = "WholeCharacteristicKind",
        alias = "WholeChartOfAccounts"
    )]
    #[default]
    WholeCatalog,

    /// Codes are unique within subordination (parent hierarchy)
    #[serde(rename = "WithinSubordination")]
    WithinSubordination,

    /// Codes are unique within owner
    #[serde(rename = "WithinOwnerSubordination", alias = "WithinOwner")]
    WithinOwnerSubordination,

    /// Unknown code series
    #[serde(other)]
    Unknown,
}

impl CodeSeries {
    /// Check if code series allows FindByCode to return unique results
    ///
    /// Returns `true` only for `WholeCatalog` (and equivalents like
    /// `WholeCharacteristicKind`, `WholeChartOfAccounts`), which means
    /// codes are unique across the entire object.
    pub fn is_whole(&self) -> bool {
        matches!(self, Self::WholeCatalog)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_return_value_reuse_from_name() {
        assert_eq!(ReturnValueReuse::from_name("DontUse"), ReturnValueReuse::DontUse);
        assert_eq!(ReturnValueReuse::from_name("неиспользовать"), ReturnValueReuse::DontUse);
        assert_eq!(ReturnValueReuse::from_name("DuringRequest"), ReturnValueReuse::DuringRequest);
        assert_eq!(ReturnValueReuse::from_name("навремязапроса"), ReturnValueReuse::DuringRequest);
        assert_eq!(ReturnValueReuse::from_name("unknown"), ReturnValueReuse::Unknown);
    }

    #[test]
    fn test_default_values() {
        assert_eq!(ReturnValueReuse::default(), ReturnValueReuse::Unknown);
        assert_eq!(ModuleType::default(), ModuleType::Unknown);
        assert_eq!(ObjectBelonging::default(), ObjectBelonging::Own);
        assert_eq!(SupportVariant::default(), SupportVariant::Unknown);
        assert_eq!(FormType::default(), FormType::Managed);
    }

    #[test]
    fn test_form_type_from_name() {
        assert_eq!(FormType::from_name("Managed"), FormType::Managed);
        assert_eq!(FormType::from_name("управляемая"), FormType::Managed);
        assert_eq!(FormType::from_name("Ordinary"), FormType::Ordinary);
        assert_eq!(FormType::from_name("обычная"), FormType::Ordinary);
        assert_eq!(FormType::from_name("unknown"), FormType::Unknown);
    }
}
