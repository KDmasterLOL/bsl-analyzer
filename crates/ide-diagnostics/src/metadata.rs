//! Diagnostic metadata.
//!
//! Provides compile-time const metadata definitions with zero-cost abstraction.
//! Runtime config can override metadata through JSON configuration.

use crate::Severity;
use serde::{Deserialize, Serialize};

// ============================================================================
// SonarQube Clean Code Taxonomy
// ============================================================================

/// SonarQube Clean Code Attribute (4 categories mapped to default attributes).
///
/// Used for Generic Issue Import format to categorize issues beyond simple severity.
/// See: <https://docs.sonarqube.org/latest/project-administration/clean-code/>
///
/// Categories are mapped to valid SonarQube attribute values:
/// - Consistent → CONVENTIONAL
/// - Intentional → CLEAR
/// - Adaptable → FOCUSED
/// - Responsible → TRUSTWORTHY
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CleanCodeAttribute {
    /// Code follows standards and conventions (formatting, naming)
    #[serde(rename = "CONVENTIONAL")]
    Consistent,
    /// Code clearly expresses intent (clarity, completeness, logic)
    #[serde(rename = "CLEAR")]
    Intentional,
    /// Code is easy to change (design, modularity, no duplication)
    #[serde(rename = "FOCUSED")]
    Adaptable,
    /// Code respects guidelines and users (security, i18n)
    #[serde(rename = "TRUSTWORTHY")]
    Responsible,
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
        // Security issues -> Responsible
        (_, DiagnosticType::Vulnerability | DiagnosticType::SecurityHotspot) => {
            CleanCodeAttribute::Responsible
        }

        // Localization -> Responsible
        (MetadataTag::Localize, _) => CleanCodeAttribute::Responsible,

        // Standard/conventions -> Consistent
        (MetadataTag::Standard, _) => CleanCodeAttribute::Consistent,
        (MetadataTag::Deprecated, _) => CleanCodeAttribute::Consistent,

        // Design/architecture -> Adaptable
        (MetadataTag::Design, _) => CleanCodeAttribute::Adaptable,
        (MetadataTag::Performance, _) => CleanCodeAttribute::Adaptable,
        (MetadataTag::Unused, _) => CleanCodeAttribute::Adaptable,
        (MetadataTag::Lockinos, _) => CleanCodeAttribute::Adaptable,

        // Logic/clarity issues -> Intentional
        (MetadataTag::Error, _) => CleanCodeAttribute::Intentional,
        (MetadataTag::Suspicious, _) => CleanCodeAttribute::Intentional,
        (MetadataTag::Unpredictable, _) => CleanCodeAttribute::Intentional,
        (MetadataTag::Brainoverload, _) => CleanCodeAttribute::Intentional,
        (MetadataTag::Clumsy, _) => CleanCodeAttribute::Intentional,

        // Bad practice -> depends on type
        (MetadataTag::Badpractice, DiagnosticType::Error) => CleanCodeAttribute::Intentional,
        (MetadataTag::Badpractice, _) => CleanCodeAttribute::Consistent,

        // SQL issues -> depends on type
        (MetadataTag::Sql, DiagnosticType::Error) => CleanCodeAttribute::Intentional,
        (MetadataTag::Sql, _) => CleanCodeAttribute::Adaptable,
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

/// Diagnostic type.
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

/// Diagnostic severity level.
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

/// Diagnostic tag.
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

/// Diagnostic scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticScope {
    /// BSL и OneScript
    All,
    /// Только OneScript
    Os,
    /// Только BSL
    Bsl,
}

/// Diagnostic compatibility mode.
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
/// Compile-time diagnostic metadata definition.
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
    /// Calculate LSP severity from diagnostic type and severity level.
    ///
    /// Mapping logic:
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
            clean_code_attribute: CleanCodeAttribute::Intentional,
            impacts: &[DEFAULT_IMPACT],
        }
    }
}

/// Helper macro to create DiagnosticMetadata with auto-derived Clean Code attributes.
///
/// Automatically derives `clean_code_attribute` from the first tag and diagnostic type,
/// and `impacts` from the diagnostic type and severity level.
///
/// Use the variant with `clean_code_attribute:` to explicitly set the attribute.
#[macro_export]
macro_rules! define_metadata {
    // Variant with explicit clean_code_attribute
    (
        diagnostic_type: $dtype:expr,
        severity: $severity:expr,
        scope: $scope:expr,
        modules: $modules:expr,
        minutes_to_fix: $mtf:expr,
        activated_by_default: $abd:expr,
        compatibility_mode: $cm:expr,
        tags: $tags:expr,
        can_locate_on_project: $clop:expr,
        extra_min_for_complexity: $emfc:expr,
        lsp_severity_override: $lso:expr,
        clean_code_attribute: $cca:expr $(,)?
    ) => {{
        const IMPACT: $crate::metadata::Impact =
            $crate::metadata::derive_primary_impact($dtype, $severity);

        $crate::metadata::DiagnosticMetadata {
            diagnostic_type: $dtype,
            severity: $severity,
            scope: $scope,
            modules: $modules,
            minutes_to_fix: $mtf,
            activated_by_default: $abd,
            compatibility_mode: $cm,
            tags: $tags,
            can_locate_on_project: $clop,
            extra_min_for_complexity: $emfc,
            lsp_severity_override: $lso,
            clean_code_attribute: $cca,
            impacts: &[IMPACT],
        }
    }};
    // Variant with auto-derived clean_code_attribute
    (
        diagnostic_type: $dtype:expr,
        severity: $severity:expr,
        scope: $scope:expr,
        modules: $modules:expr,
        minutes_to_fix: $mtf:expr,
        activated_by_default: $abd:expr,
        compatibility_mode: $cm:expr,
        tags: $tags:expr,
        can_locate_on_project: $clop:expr,
        extra_min_for_complexity: $emfc:expr,
        lsp_severity_override: $lso:expr $(,)?
    ) => {{
        // Extract first tag for clean code attribute derivation
        const FIRST_TAG: $crate::metadata::MetadataTag =
            if $tags.is_empty() { $crate::metadata::MetadataTag::Badpractice } else { $tags[0] };
        const CCA: $crate::metadata::CleanCodeAttribute =
            $crate::metadata::derive_clean_code_attribute(FIRST_TAG, $dtype);
        const IMPACT: $crate::metadata::Impact =
            $crate::metadata::derive_primary_impact($dtype, $severity);

        $crate::metadata::DiagnosticMetadata {
            diagnostic_type: $dtype,
            severity: $severity,
            scope: $scope,
            modules: $modules,
            minutes_to_fix: $mtf,
            activated_by_default: $abd,
            compatibility_mode: $cm,
            tags: $tags,
            can_locate_on_project: $clop,
            extra_min_for_complexity: $emfc,
            lsp_severity_override: $lso,
            clean_code_attribute: CCA,
            impacts: &[IMPACT],
        }
    }};
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
    fn test_derive_clean_code_attribute() {
        // Security issues -> Responsible
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Badpractice, DiagnosticType::Vulnerability),
            CleanCodeAttribute::Responsible
        );
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Error, DiagnosticType::SecurityHotspot),
            CleanCodeAttribute::Responsible
        );

        // Standard -> Consistent
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Standard, DiagnosticType::CodeSmell),
            CleanCodeAttribute::Consistent
        );

        // Design -> Adaptable
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Design, DiagnosticType::CodeSmell),
            CleanCodeAttribute::Adaptable
        );

        // Performance -> Adaptable
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Performance, DiagnosticType::CodeSmell),
            CleanCodeAttribute::Adaptable
        );

        // Localize -> Responsible
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Localize, DiagnosticType::CodeSmell),
            CleanCodeAttribute::Responsible
        );

        // Unused -> Adaptable
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Unused, DiagnosticType::CodeSmell),
            CleanCodeAttribute::Adaptable
        );

        // Error -> Intentional
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Error, DiagnosticType::Error),
            CleanCodeAttribute::Intentional
        );
        assert_eq!(
            derive_clean_code_attribute(MetadataTag::Error, DiagnosticType::CodeSmell),
            CleanCodeAttribute::Intentional
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
        // Categories serialize to valid SonarQube attribute values
        assert_eq!(
            serde_json::to_string(&CleanCodeAttribute::Consistent).unwrap(),
            "\"CONVENTIONAL\""
        );
        assert_eq!(serde_json::to_string(&CleanCodeAttribute::Intentional).unwrap(), "\"CLEAR\"");
        assert_eq!(serde_json::to_string(&CleanCodeAttribute::Adaptable).unwrap(), "\"FOCUSED\"");
        assert_eq!(
            serde_json::to_string(&CleanCodeAttribute::Responsible).unwrap(),
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
