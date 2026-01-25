//! CommonModuleNameWords diagnostic
//!
//! CommonModule name should not contain generic words like "Procedures", "Functions", "Module", etc.
//!
//! Ported from: CommonModuleNameWordsDiagnostic.java

use crate::{Diagnostic, DiagnosticCode};
use bsl_metadata::traits::MdObject;
use ide_db::hir_def::ModuleMetadata;
use ide_db::TextRange;

const DEFAULT_WORDS: &str = r"процедуры|procedures|функции|functions|обработчики|handlers|модуль|module|функциональность|functionality";

/// Check metadata-based diagnostics using ModuleMetadata.
pub fn from_metadata(
    metadata: &ModuleMetadata,
    ctx: &crate::DiagnosticsContext,
) -> Vec<Diagnostic> {
    let code = DiagnosticCode::CommonModuleNameWords;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    if !matches!(metadata.module_type, bsl_metadata::ModuleType::CommonModule) {
        return Vec::new();
    }

    let module = match &metadata.common_module {
        Some(m) => m.as_ref(),
        None => return Vec::new(),
    };

    // Get words pattern from config, or use default
    let words_pattern = ctx.config.get_string(code, "words").unwrap_or(DEFAULT_WORDS);

    // Split pattern by | (regex alternation) and check if name contains any word
    let name_lower = module.name().to_lowercase();
    for word in words_pattern.split('|') {
        if name_lower.contains(&word.to_lowercase()) {
            return vec![Diagnostic {
                code,
                message: "Имя общего модуля не должно содержать общих слов типа 'Процедуры', 'Функции', 'Модуль'"
                    .to_string(),
                severity: ctx.severity(code),
                range: TextRange::empty(0.into()),
                tags: ctx.tags(code),
                fixes: vec![],
            }];
        }
    }

    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;

    #[test]
    fn test_from_metadata_with_forbidden_russian_word() {
        let module = bsl_metadata::CommonModule::builder().name("МойМодуль").build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
            http_service: None,
            form: None,
        };

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_from_metadata_with_forbidden_english_word() {
        let module = bsl_metadata::CommonModule::builder().name("MyModule").build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
            http_service: None,
            form: None,
        };

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_from_metadata_without_forbidden_word() {
        let module = bsl_metadata::CommonModule::builder().name("РаботаСДанными").build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
            http_service: None,
            form: None,
        };

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_from_metadata_with_procedures_word() {
        let module = bsl_metadata::CommonModule::builder().name("ОбщиеПроцедуры").build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
            register: None,
            http_service: None,
            form: None,
        };

        let _config = DiagnosticsConfig::default();
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 1);
    }
}
