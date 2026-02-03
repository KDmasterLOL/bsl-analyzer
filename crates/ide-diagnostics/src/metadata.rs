//! Diagnostic metadata matching bsl-language-server @DiagnosticMetadata.
//!
//! Provides compile-time const metadata definitions with zero-cost abstraction.
//! Runtime config can override metadata through JSON configuration.

use crate::Severity;
use serde::{Deserialize, Serialize};

// ============================================================================
// SonarQube Clean Code Taxonomy
// ============================================================================

/// SonarQube Clean Code Attribute (12 values across 4 categories).
///
/// Used for Generic Issue Import format to categorize issues beyond simple severity.
/// See: <https://docs.sonarqube.org/latest/project-administration/clean-code/>
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CleanCodeAttribute {
    // Consistent category - code follows standards and conventions
    /// Code is properly formatted
    Formatted,
    /// Code follows naming conventions
    Conventional,
    /// Identifiers are meaningful and clear
    Identifiable,

    // Intentional category - code clearly expresses intent
    /// Code intent is clear and understandable
    Clear,
    /// Implementation is complete (no TODOs, missing cases)
    Complete,
    /// Code is efficient and performant
    Efficient,
    /// Code logic is correct and sound
    Logical,

    // Adaptable category - code is easy to change
    /// No code duplication
    Distinct,
    /// Single responsibility principle
    Focused,
    /// Proper modularization
    Modular,
    /// Code is testable
    Tested,

    // Responsible category - code respects guidelines and users
    /// Respects licenses and legal requirements
    Lawful,
    /// Respects users (i18n, accessibility)
    Respectful,
    /// Secure and trustworthy code
    Trustworthy,
}

impl CleanCodeAttribute {
    /// Returns the category name for this attribute.
    pub const fn category(&self) -> &'static str {
        match self {
            Self::Formatted | Self::Conventional | Self::Identifiable => "CONSISTENT",
            Self::Clear | Self::Complete | Self::Efficient | Self::Logical => "INTENTIONAL",
            Self::Distinct | Self::Focused | Self::Modular | Self::Tested => "ADAPTABLE",
            Self::Lawful | Self::Respectful | Self::Trustworthy => "RESPONSIBLE",
        }
    }
}

/// SonarQube Software Quality dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SoftwareQuality {
    /// Code maintainability
    Maintainability,
    /// Code reliability (bugs)
    Reliability,
    /// Code security (vulnerabilities)
    Security,
}

/// Impact severity level for SonarQube.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ImpactSeverity {
    Low,
    Medium,
    High,
}

/// Impact on a software quality dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Impact {
    #[serde(rename = "softwareQuality")]
    pub software_quality: SoftwareQuality,
    pub severity: ImpactSeverity,
}

impl Impact {
    /// Create a new impact.
    pub const fn new(software_quality: SoftwareQuality, severity: ImpactSeverity) -> Self {
        Self { software_quality, severity }
    }
}

// ============================================================================
// Clean Code Attribute Derivation
// ============================================================================

/// Derive clean code attribute from metadata tag and diagnostic type.
///
/// This provides sensible defaults based on existing categorization.
/// Individual diagnostics can override if needed.
pub const fn derive_clean_code_attribute(
    tag: MetadataTag,
    dtype: DiagnosticType,
) -> CleanCodeAttribute {
    match (tag, dtype) {
        // Security issues -> Trustworthy
        (_, DiagnosticType::Vulnerability | DiagnosticType::SecurityHotspot) => {
            CleanCodeAttribute::Trustworthy
        }

        // Standard compliance -> Conventional
        (MetadataTag::Standard, _) => CleanCodeAttribute::Conventional,

        // Deprecated -> Conventional (outdated patterns)
        (MetadataTag::Deprecated, _) => CleanCodeAttribute::Conventional,

        // Design issues -> Focused (single responsibility)
        (MetadataTag::Design, _) => CleanCodeAttribute::Focused,

        // Performance -> Efficient
        (MetadataTag::Performance, _) => CleanCodeAttribute::Efficient,

        // Localization -> Respectful
        (MetadataTag::Localize, _) => CleanCodeAttribute::Respectful,

        // Unused code -> Distinct (no dead code)
        (MetadataTag::Unused, _) => CleanCodeAttribute::Distinct,

        // Errors in code -> Logical
        (MetadataTag::Error, DiagnosticType::Error) => CleanCodeAttribute::Logical,

        // Bad practice -> depends on type
        (MetadataTag::Badpractice, DiagnosticType::Error) => CleanCodeAttribute::Logical,
        (MetadataTag::Badpractice, _) => CleanCodeAttribute::Conventional,

        // Suspicious code -> Logical
        (MetadataTag::Suspicious, _) => CleanCodeAttribute::Logical,

        // Unpredictable -> Logical
        (MetadataTag::Unpredictable, _) => CleanCodeAttribute::Logical,

        // Brain overload -> Clear
        (MetadataTag::Brainoverload, _) => CleanCodeAttribute::Clear,

        // Clumsy code -> Clear
        (MetadataTag::Clumsy, _) => CleanCodeAttribute::Clear,

        // SQL issues -> depends on type
        (MetadataTag::Sql, DiagnosticType::Error) => CleanCodeAttribute::Logical,
        (MetadataTag::Sql, _) => CleanCodeAttribute::Efficient,

        // OS-specific -> Modular
        (MetadataTag::Lockinos, _) => CleanCodeAttribute::Modular,

        // Default for other errors
        (MetadataTag::Error, _) => CleanCodeAttribute::Complete,
    }
}

/// Derive primary impact from diagnostic type and severity.
pub const fn derive_primary_impact(
    dtype: DiagnosticType,
    severity: DiagnosticSeverityLevel,
) -> Impact {
    let software_quality = match dtype {
        DiagnosticType::Vulnerability | DiagnosticType::SecurityHotspot => {
            SoftwareQuality::Security
        }
        DiagnosticType::Error => SoftwareQuality::Reliability,
        DiagnosticType::CodeSmell => SoftwareQuality::Maintainability,
    };

    let impact_severity = match severity {
        DiagnosticSeverityLevel::Blocker | DiagnosticSeverityLevel::Critical => {
            ImpactSeverity::High
        }
        DiagnosticSeverityLevel::Major => ImpactSeverity::Medium,
        DiagnosticSeverityLevel::Minor | DiagnosticSeverityLevel::Info => ImpactSeverity::Low,
    };

    Impact::new(software_quality, impact_severity)
}

/// Diagnostic type (matches Java DiagnosticType).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticType {
    /// Ошибка в коде
    Error,
    /// Code smell
    CodeSmell,
    /// Уязвимость безопасности
    Vulnerability,
    /// Точка для проверки безопасности
    SecurityHotspot,
}

/// Diagnostic severity level (matches Java DiagnosticSeverity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticSeverityLevel {
    /// Информация
    Info,
    /// Незначительная проблема
    Minor,
    /// Значимая проблема
    Major,
    /// Критическая проблема
    Critical,
    /// Блокирующая проблема
    Blocker,
}

/// Diagnostic tag (matches Java DiagnosticTag).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataTag {
    /// Нарушение стандартов 1С
    Standard,
    /// Не работает на другой ОС
    Lockinos,
    /// Проблема с запросом
    Sql,
    /// Проблема производительности
    Performance,
    /// Непонятный код
    Brainoverload,
    /// Плохая практика
    Badpractice,
    /// Излишние действия
    Clumsy,
    /// Ошибка в проектировании
    Design,
    /// Подозрительный код
    Suspicious,
    /// Непредсказуемо работающий
    Unpredictable,
    /// Устаревшая функциональность
    Deprecated,
    /// Неиспользуемый код
    Unused,
    /// Ошибочная конструкция
    Error,
    /// Проблемы локализации
    Localize,
}

/// Diagnostic scope (matches Java DiagnosticScope).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticScope {
    /// BSL и OneScript
    All,
    /// Только OneScript
    Os,
    /// Только BSL
    Bsl,
}

/// Diagnostic compatibility mode (matches Java DiagnosticCompatibilityMode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticCompatibilityMode {
    Undefined,
    CompatibilityMode8_3_1,
    CompatibilityMode8_3_3,
    CompatibilityMode8_3_6,
    CompatibilityMode8_3_10,
    CompatibilityMode8_3_12,
    CompatibilityMode8_3_14,
    CompatibilityMode8_3_17,
    Compatibility8320,
}

/// Compile-time diagnostic metadata (zero-cost).
///
/// This struct mirrors Java's @DiagnosticMetadata annotation.
/// All fields are const-friendly for compile-time definitions.
#[derive(Debug, Clone, Copy)]
pub struct DiagnosticMetadata {
    pub diagnostic_type: DiagnosticType,
    pub severity: DiagnosticSeverityLevel,
    pub scope: DiagnosticScope,
    /// Пустой массив = все модули
    pub modules: &'static [bsl_metadata::ModuleType],
    pub minutes_to_fix: u32,
    pub activated_by_default: bool,
    pub compatibility_mode: DiagnosticCompatibilityMode,
    pub tags: &'static [MetadataTag],
    pub can_locate_on_project: bool,
    pub extra_min_for_complexity: f64,
    /// "" = auto-calculate from diagnostic_type + severity
    pub lsp_severity_override: &'static str,
    /// SonarQube Clean Code attribute
    pub clean_code_attribute: CleanCodeAttribute,
    /// SonarQube impacts on software quality dimensions
    pub impacts: &'static [Impact],
}

impl DiagnosticMetadata {
    /// Calculate severity (matches Java logic).
    ///
    /// Java DiagnosticSeverityMapper::getDiagnosticSeverity logic:
    /// - CODE_SMELL + INFO → Hint
    /// - CODE_SMELL + MINOR → Information
    /// - CODE_SMELL + MAJOR/CRITICAL/BLOCKER → Warning
    /// - SECURITY_HOTSPOT → Warning
    /// - ERROR/VULNERABILITY + severity level → map directly
    pub const fn calculate_severity(&self) -> Severity {
        match self.diagnostic_type {
            DiagnosticType::CodeSmell => match self.severity {
                DiagnosticSeverityLevel::Info => Severity::Hint,
                DiagnosticSeverityLevel::Minor => Severity::Information,
                DiagnosticSeverityLevel::Major
                | DiagnosticSeverityLevel::Critical
                | DiagnosticSeverityLevel::Blocker => Severity::Warning,
            },
            DiagnosticType::SecurityHotspot => Severity::Warning,
            DiagnosticType::Error | DiagnosticType::Vulnerability => match self.severity {
                DiagnosticSeverityLevel::Blocker => Severity::Blocker,
                DiagnosticSeverityLevel::Critical => Severity::Critical,
                DiagnosticSeverityLevel::Major => Severity::Major,
                DiagnosticSeverityLevel::Minor | DiagnosticSeverityLevel::Info => Severity::Error,
            },
        }
    }
}

/// Default impact for CodeSmell + Major severity
const DEFAULT_IMPACT: Impact =
    Impact::new(SoftwareQuality::Maintainability, ImpactSeverity::Medium);

impl Default for DiagnosticMetadata {
    /// Default metadata (enabled by default, no specific tags/modules).
    fn default() -> Self {
        Self {
            diagnostic_type: DiagnosticType::CodeSmell,
            severity: DiagnosticSeverityLevel::Major,
            scope: DiagnosticScope::All,
            modules: &[],
            minutes_to_fix: 5,
            activated_by_default: true,
            compatibility_mode: DiagnosticCompatibilityMode::Undefined,
            tags: &[],
            can_locate_on_project: false,
            extra_min_for_complexity: 0.0,
            lsp_severity_override: "",
            clean_code_attribute: CleanCodeAttribute::Clear,
            impacts: &[DEFAULT_IMPACT],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_mapping_code_smell() {
        let meta = DiagnosticMetadata {
            diagnostic_type: DiagnosticType::CodeSmell,
            severity: DiagnosticSeverityLevel::Info,
            ..Default::default()
        };
        assert_eq!(meta.calculate_severity(), Severity::Hint);

        let meta = DiagnosticMetadata {
            diagnostic_type: DiagnosticType::CodeSmell,
            severity: DiagnosticSeverityLevel::Minor,
            ..Default::default()
        };
        assert_eq!(meta.calculate_severity(), Severity::Information);

        let meta = DiagnosticMetadata {
            diagnostic_type: DiagnosticType::CodeSmell,
            severity: DiagnosticSeverityLevel::Major,
            ..Default::default()
        };
        assert_eq!(meta.calculate_severity(), Severity::Warning);
    }

    #[test]
    fn test_severity_mapping_error() {
        // Error type maps severity level directly
        let meta_blocker = DiagnosticMetadata {
            diagnostic_type: DiagnosticType::Error,
            severity: DiagnosticSeverityLevel::Blocker,
            ..Default::default()
        };
        assert_eq!(meta_blocker.calculate_severity(), Severity::Blocker);

        let meta_critical = DiagnosticMetadata {
            diagnostic_type: DiagnosticType::Error,
            severity: DiagnosticSeverityLevel::Critical,
            ..Default::default()
        };
        assert_eq!(meta_critical.calculate_severity(), Severity::Critical);

        let meta_major = DiagnosticMetadata {
            diagnostic_type: DiagnosticType::Error,
            severity: DiagnosticSeverityLevel::Major,
            ..Default::default()
        };
        assert_eq!(meta_major.calculate_severity(), Severity::Major);

        let meta_minor = DiagnosticMetadata {
            diagnostic_type: DiagnosticType::Error,
            severity: DiagnosticSeverityLevel::Minor,
            ..Default::default()
        };
        assert_eq!(meta_minor.calculate_severity(), Severity::Error);
    }

    #[test]
    fn test_severity_mapping_vulnerability() {
        // Vulnerability type maps severity level directly
        let meta_blocker = DiagnosticMetadata {
            diagnostic_type: DiagnosticType::Vulnerability,
            severity: DiagnosticSeverityLevel::Blocker,
            ..Default::default()
        };
        assert_eq!(meta_blocker.calculate_severity(), Severity::Blocker);

        let meta_critical = DiagnosticMetadata {
            diagnostic_type: DiagnosticType::Vulnerability,
            severity: DiagnosticSeverityLevel::Critical,
            ..Default::default()
        };
        assert_eq!(meta_critical.calculate_severity(), Severity::Critical);

        let meta_major = DiagnosticMetadata {
            diagnostic_type: DiagnosticType::Vulnerability,
            severity: DiagnosticSeverityLevel::Major,
            ..Default::default()
        };
        assert_eq!(meta_major.calculate_severity(), Severity::Major);

        let meta_minor = DiagnosticMetadata {
            diagnostic_type: DiagnosticType::Vulnerability,
            severity: DiagnosticSeverityLevel::Minor,
            ..Default::default()
        };
        assert_eq!(meta_minor.calculate_severity(), Severity::Error);
    }

    #[test]
    fn test_clean_code_attribute_category() {
        assert_eq!(CleanCodeAttribute::Formatted.category(), "CONSISTENT");
        assert_eq!(CleanCodeAttribute::Conventional.category(), "CONSISTENT");
        assert_eq!(CleanCodeAttribute::Identifiable.category(), "CONSISTENT");

        assert_eq!(CleanCodeAttribute::Clear.category(), "INTENTIONAL");
        assert_eq!(CleanCodeAttribute::Complete.category(), "INTENTIONAL");
        assert_eq!(CleanCodeAttribute::Efficient.category(), "INTENTIONAL");
        assert_eq!(CleanCodeAttribute::Logical.category(), "INTENTIONAL");

        assert_eq!(CleanCodeAttribute::Distinct.category(), "ADAPTABLE");
        assert_eq!(CleanCodeAttribute::Focused.category(), "ADAPTABLE");
        assert_eq!(CleanCodeAttribute::Modular.category(), "ADAPTABLE");
        assert_eq!(CleanCodeAttribute::Tested.category(), "ADAPTABLE");

        assert_eq!(CleanCodeAttribute::Lawful.category(), "RESPONSIBLE");
        assert_eq!(CleanCodeAttribute::Respectful.category(), "RESPONSIBLE");
        assert_eq!(CleanCodeAttribute::Trustworthy.category(), "RESPONSIBLE");
    }

    #[test]
    fn test_derive_clean_code_attribute() {
        // Security issues -> Trustworthy
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Badpractice, DiagnosticType::Vulnerability),
            CleanCodeAttribute::Trustworthy
        );
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Error, DiagnosticType::SecurityHotspot),
            CleanCodeAttribute::Trustworthy
        );

        // Standard -> Conventional
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Standard, DiagnosticType::CodeSmell),
            CleanCodeAttribute::Conventional
        );

        // Design -> Focused
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Design, DiagnosticType::CodeSmell),
            CleanCodeAttribute::Focused
        );

        // Performance -> Efficient
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Performance, DiagnosticType::CodeSmell),
            CleanCodeAttribute::Efficient
        );

        // Localize -> Respectful
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Localize, DiagnosticType::CodeSmell),
            CleanCodeAttribute::Respectful
        );

        // Unused -> Distinct
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Unused, DiagnosticType::CodeSmell),
            CleanCodeAttribute::Distinct
        );

        // Error + Error type -> Logical
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Error, DiagnosticType::Error),
            CleanCodeAttribute::Logical
        );

        // Error + CodeSmell -> Complete
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Error, DiagnosticType::CodeSmell),
            CleanCodeAttribute::Complete
        );
    }

    #[test]
    fn test_derive_primary_impact() {
        // Security types -> Security quality
        let impact =
            derive_primary_impact(DiagnosticType::Vulnerability, DiagnosticSeverityLevel::Critical);
        assert_eq!(impact.software_quality, SoftwareQuality::Security);
        assert_eq!(impact.severity, ImpactSeverity::High);

        let impact =
            derive_primary_impact(DiagnosticType::SecurityHotspot, DiagnosticSeverityLevel::Major);
        assert_eq!(impact.software_quality, SoftwareQuality::Security);
        assert_eq!(impact.severity, ImpactSeverity::Medium);

        // Error type -> Reliability quality
        let impact = derive_primary_impact(DiagnosticType::Error, DiagnosticSeverityLevel::Major);
        assert_eq!(impact.software_quality, SoftwareQuality::Reliability);
        assert_eq!(impact.severity, ImpactSeverity::Medium);

        // CodeSmell -> Maintainability quality
        let impact =
            derive_primary_impact(DiagnosticType::CodeSmell, DiagnosticSeverityLevel::Minor);
        assert_eq!(impact.software_quality, SoftwareQuality::Maintainability);
        assert_eq!(impact.severity, ImpactSeverity::Low);

        // Blocker severity -> High impact
        let impact =
            derive_primary_impact(DiagnosticType::CodeSmell, DiagnosticSeverityLevel::Blocker);
        assert_eq!(impact.severity, ImpactSeverity::High);

        // Info severity -> Low impact
        let impact =
            derive_primary_impact(DiagnosticType::CodeSmell, DiagnosticSeverityLevel::Info);
        assert_eq!(impact.severity, ImpactSeverity::Low);
    }

    #[test]
    fn test_clean_code_attribute_serialization() {
        assert_eq!(serde_json::to_string(&CleanCodeAttribute::Formatted).unwrap(), "\"FORMATTED\"");
        assert_eq!(
            serde_json::to_string(&CleanCodeAttribute::Trustworthy).unwrap(),
            "\"TRUSTWORTHY\""
        );
    }

    #[test]
    fn test_software_quality_serialization() {
        assert_eq!(
            serde_json::to_string(&SoftwareQuality::Maintainability).unwrap(),
            "\"MAINTAINABILITY\""
        );
        assert_eq!(
            serde_json::to_string(&SoftwareQuality::Reliability).unwrap(),
            "\"RELIABILITY\""
        );
        assert_eq!(serde_json::to_string(&SoftwareQuality::Security).unwrap(), "\"SECURITY\"");
    }

    #[test]
    fn test_impact_serialization() {
        let impact = Impact::new(SoftwareQuality::Maintainability, ImpactSeverity::Medium);
        let json = serde_json::to_string(&impact).unwrap();
        assert_eq!(json, r#"{"softwareQuality":"MAINTAINABILITY","severity":"MEDIUM"}"#);
    }
}
