//! Diagnostic metadata matching bsl-language-server @DiagnosticMetadata.
//!
//! Provides compile-time const metadata definitions with zero-cost abstraction.
//! Runtime config can override metadata through JSON configuration.

use crate::Severity;

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
}
