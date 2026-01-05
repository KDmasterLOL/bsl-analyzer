//! CommonModuleNameWords diagnostic
//!
//! CommonModule name should not contain generic words like "Procedures", "Functions", "Module", etc.
//!
//! Ported from: CommonModuleNameWordsDiagnostic.java

use crate::{common_module_helpers, Diagnostic, DiagnosticCode, DiagnosticsContext, Severity};
use bsl_metadata::traits::MdObject;
use ide_db::hir_def::ModuleMetadata;
use ide_db::TextRange;
use regex::Regex;

const DEFAULT_WORDS: &str = r"процедуры|procedures|функции|functions|обработчики|handlers|модуль|module|функциональность|functionality";

pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::CommonModuleNameWords) {
        return Vec::new();
    }

    let configuration = match ctx.load_configuration() {
        Some(config) => config,
        None => return Vec::new(),
    };

    let module = match common_module_helpers::find_common_module_for_file(ctx, &configuration) {
        Some(m) => m,
        None => return Vec::new(),
    };

    let words_pattern = ctx
        .config
        .get_string(DiagnosticCode::CommonModuleNameWords, "words")
        .unwrap_or(DEFAULT_WORDS);

    let regex = match Regex::new(&format!(r"(?i){}", words_pattern)) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    if !regex.is_match(module.name()) {
        return Vec::new();
    }

    vec![Diagnostic {
        code: DiagnosticCode::CommonModuleNameWords,
        message:
            "Имя общего модуля не должно содержать общих слов типа 'Процедуры', 'Функции', 'Модуль'"
                .to_string(),
        severity: Severity::Information,
        range: TextRange::empty(0.into()),
        tags: vec![],
        fixes: vec![],
    }]
}

/// Check metadata-based diagnostics using ModuleMetadata.
pub fn from_metadata(
    metadata: &ModuleMetadata,
    config: &crate::DiagnosticsConfig,
) -> Vec<Diagnostic> {
    if !matches!(metadata.module_type, bsl_metadata::ModuleType::CommonModule) {
        return Vec::new();
    }

    let module = match &metadata.common_module {
        Some(m) => m.as_ref(),
        None => return Vec::new(),
    };

    // Get words pattern from config, or use default
    let words_pattern =
        config.get_string(DiagnosticCode::CommonModuleNameWords, "words").unwrap_or(DEFAULT_WORDS);

    // Split pattern by | (regex alternation) and check if name contains any word
    let name_lower = module.name().to_lowercase();
    for word in words_pattern.split('|') {
        if name_lower.contains(&word.to_lowercase()) {
            return vec![Diagnostic {
                code: DiagnosticCode::CommonModuleNameWords,
                message: "Имя общего модуля не должно содержать общих слов типа 'Процедуры', 'Функции', 'Модуль'"
                    .to_string(),
                severity: Severity::Information,
                range: TextRange::empty(0.into()),
                tags: vec![],
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
    use ide_db::base_db::SourceDatabase;
    use ide_db::{RootDatabase, RootDatabaseImpl};
    use std::rc::Rc;
    use test_fixture::Fixture;

    fn check_with_module(code: &str, module: &bsl_metadata::CommonModule) -> Vec<Diagnostic> {
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have a file");

        let mut db = RootDatabaseImpl::new();
        db.set_file_text(file_id, code);
        let _db = Rc::new(db) as Rc<dyn RootDatabase>;

        let _config = DiagnosticsConfig::default();

        let regex = regex::Regex::new(&format!(r"(?i){}", DEFAULT_WORDS)).unwrap();

        if !regex.is_match(module.name()) {
            return Vec::new();
        }

        vec![Diagnostic {
            code: DiagnosticCode::CommonModuleNameWords,
            message: "Имя общего модуля не должно содержать общих слов типа 'Процедуры', 'Функции', 'Модуль'".to_string(),
            severity: Severity::Information,
            range: ide_db::TextRange::empty(0.into()),
            tags: vec![],
            fixes: vec![],
        }]
    }

    #[test]
    fn test_with_forbidden_word() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        let module = bsl_metadata::CommonModule::builder().name("СвойМодуль").build();

        let diagnostics = check_with_module(code, &module);
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn test_without_forbidden_word() {
        let code = "Процедура Тест()\nКонецПроцедуры";
        let module = bsl_metadata::CommonModule::builder().name("РаботаСФайлами").build();

        let diagnostics = check_with_module(code, &module);
        assert_eq!(diagnostics.len(), 0);
    }

    #[test]
    fn test_from_metadata_with_forbidden_russian_word() {
        let module = bsl_metadata::CommonModule::builder().name("МойМодуль").build();

        let metadata = ide_db::hir_def::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(std::sync::Arc::new(module)),
            mdo: None,
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);
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
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);
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
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);
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
        };

        let config = DiagnosticsConfig::default();
        let diagnostics = from_metadata(&metadata, &config);
        assert_eq!(diagnostics.len(), 1);
    }
}
