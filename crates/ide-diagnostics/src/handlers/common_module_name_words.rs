//! CommonModuleNameWords diagnostic
//!
//! CommonModule name should not contain generic words like "Procedures", "Functions", "Module", etc.
//!
//! Ported from: CommonModuleNameWordsDiagnostic.java

use crate::{Diagnostic, DiagnosticCode};
use bsl_metadata::traits::MdObject;
use hir::ModuleMetadata;
use ide_db::TextRange;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[bsl_metadata::ModuleType::CommonModule],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Consistent,
};

const DEFAULT_WORDS: &str = r"процедуры|procedures|функции|functions|обработчики|handlers|модуль|module|функциональность|functionality";

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

    let words_pattern = ctx.config.get_string(code, "words").unwrap_or(DEFAULT_WORDS);

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
    use crate::test_utils::*;

    #[test]
    fn test_with_forbidden_russian_word() {
        let module = bsl_metadata::CommonModule::builder().name("МойМодуль").build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_with_forbidden_english_word() {
        let module = bsl_metadata::CommonModule::builder().name("MyModule").build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_without_forbidden_word() {
        let module = bsl_metadata::CommonModule::builder().name("РаботаСДанными").build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_with_procedures_word() {
        let module = bsl_metadata::CommonModule::builder().name("ОбщиеПроцедуры").build();
        let metadata = make_common_module_metadata(module);
        let diagnostics = check_metadata_diagnostic(metadata, "", from_metadata);
        assert_eq!(diagnostics.len(), 1);
    }
}
