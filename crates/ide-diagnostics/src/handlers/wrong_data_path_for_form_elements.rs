//! WrongDataPathForFormElements diagnostic.
//!
//! Checks form elements for DataPath starting with `~` (unresolved reference).
//! Such elements indicate that the form attribute was deleted or renamed.
//!
//! ## Why?
//! When a form attribute is deleted or renamed, but the form element still
//! references it, the platform marks the DataPath with `~` prefix. This indicates
//! a broken binding that will cause runtime errors or incorrect form behavior.
//!
//! ## Bad practice
//! ```xml
//! <InputField name="НесуществующийРеквизит" id="4">
//!     <DataPath>~Объект.НесуществующийРеквизит</DataPath>  <!-- ← ERROR! -->
//! </InputField>
//! ```
//!
//! ## Good practice
//! ```xml
//! <InputField name="Код" id="1">
//!     <DataPath>Объект.Code</DataPath>  <!-- ← OK -->
//! </InputField>
//! ```
//!
//! ## Implementation
//!
//! Tier 3 diagnostic: Requires metadata (Form with elements).
//! Applies to FormModule (checks current form's elements).

use crate::{Diagnostic, DiagnosticCode};
use hir::ModuleMetadata;
use ide_db::TextRange;
use crate::define_metadata;
use crate::metadata::*;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Critical,
    scope: DiagnosticScope::Bsl,
    modules: &[
        bsl_metadata::ModuleType::FormModule,
        bsl_metadata::ModuleType::ManagedApplicationModule,
    ],
    minutes_to_fix: 5,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Unpredictable],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Collect diagnostics from module metadata.
///
/// Checks form elements for DataPath starting with `~`.
pub fn from_metadata(
    metadata: &ModuleMetadata,
    ctx: &crate::DiagnosticsContext,
) -> Vec<Diagnostic> {
    let code = DiagnosticCode::WrongDataPathForFormElements;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    // Only process FormModule
    if metadata.module_type != bsl_metadata::ModuleType::FormModule {
        return Vec::new();
    }

    // Check if we have form metadata
    let Some(ref form) = metadata.form else {
        return Vec::new();
    };

    let mut diagnostics = Vec::new();
    let file_text = ctx.file_text();

    for element in form.elements_with_wrong_data_path() {
        let message = format!(
            "Не указан путь к данным у реквизита формы \"{}\". Форма \"{}\".",
            element.name,
            form.name()
        );

        // Use range [0, min(9, file_len)) to avoid exceeding file bounds
        let file_len = file_text.len();
        let end_offset = std::cmp::min(9, file_len);
        let range = TextRange::new(0.into(), (end_offset as u32).into());

        diagnostics.push(Diagnostic {
            code,
            message,
            severity: ctx.severity(code),
            range,
            tags: ctx.tags(code),
            fixes: vec![],
        });
    }

    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticsConfig;
    use std::sync::Arc;
    fn make_metadata_with_form(form: bsl_metadata::Form) -> ModuleMetadata {
        ModuleMetadata {
            module_type: bsl_metadata::ModuleType::FormModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: None,
            http_service: None,
            web_service: None,
            form: Some(Arc::new(form)),
        }
    }

    #[test]
    fn test_element_with_wrong_data_path() {
        let form = bsl_metadata::Form::with_elements(
            "ФормаЭлемента".to_string(),
            bsl_metadata::FormType::Managed,
            uuid::Uuid::nil(),
            vec![bsl_metadata::FormElement {
                name: "НесуществующийРеквизит".to_string(),
                id: 1,
                data_path: Some("~Объект.НесуществующийРеквизит".to_string()),
            }],
        );

        let metadata = make_metadata_with_form(form);
        let file_text = "Процедура Тест()\nКонецПроцедуры";
        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, file_text, from_metadata);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("НесуществующийРеквизит"));
        assert!(diagnostics[0].message.contains("ФормаЭлемента"));
    }

    #[test]
    fn test_element_with_valid_data_path() {
        let form = bsl_metadata::Form::with_elements(
            "ФормаЭлемента".to_string(),
            bsl_metadata::FormType::Managed,
            uuid::Uuid::nil(),
            vec![bsl_metadata::FormElement {
                name: "Код".to_string(),
                id: 1,
                data_path: Some("Объект.Code".to_string()),
            }],
        );

        let metadata = make_metadata_with_form(form);
        let file_text = "Процедура Тест()\nКонецПроцедуры";
        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, file_text, from_metadata);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_multiple_elements_mixed() {
        let form = bsl_metadata::Form::with_elements(
            "ФормаЭлемента".to_string(),
            bsl_metadata::FormType::Managed,
            uuid::Uuid::nil(),
            vec![
                bsl_metadata::FormElement {
                    name: "Код".to_string(),
                    id: 1,
                    data_path: Some("Объект.Code".to_string()),
                },
                bsl_metadata::FormElement {
                    name: "НесуществующийРеквизит1".to_string(),
                    id: 2,
                    data_path: Some("~Объект.НесуществующийРеквизит1".to_string()),
                },
                bsl_metadata::FormElement {
                    name: "Наименование".to_string(),
                    id: 3,
                    data_path: Some("Объект.Description".to_string()),
                },
                bsl_metadata::FormElement {
                    name: "НесуществующийРеквизит2".to_string(),
                    id: 4,
                    data_path: Some("~Объект.НесуществующийРеквизит2".to_string()),
                },
            ],
        );

        let metadata = make_metadata_with_form(form);
        let file_text = "Процедура Тест()\nКонецПроцедуры";
        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, file_text, from_metadata);

        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics[0].message.contains("НесуществующийРеквизит1"));
        assert!(diagnostics[1].message.contains("НесуществующийРеквизит2"));
    }

    #[test]
    fn test_not_a_form_module() {
        let metadata = ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: None,
            http_service: None,
            web_service: None,
            form: None,
        };

        let file_text = "Процедура Тест()\nКонецПроцедуры";
        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, file_text, from_metadata);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_form_without_elements() {
        let form = bsl_metadata::Form::new(
            "ФормаЭлемента".to_string(),
            bsl_metadata::FormType::Managed,
            uuid::Uuid::nil(),
        );

        let metadata = make_metadata_with_form(form);
        let file_text = "Процедура Тест()\nКонецПроцедуры";
        let diagnostics =
            crate::test_utils::check_metadata_diagnostic(metadata, file_text, from_metadata);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_disabled_diagnostic() {
        let form = bsl_metadata::Form::with_elements(
            "ФормаЭлемента".to_string(),
            bsl_metadata::FormType::Managed,
            uuid::Uuid::nil(),
            vec![bsl_metadata::FormElement {
                name: "НесуществующийРеквизит".to_string(),
                id: 1,
                data_path: Some("~Объект.НесуществующийРеквизит".to_string()),
            }],
        );

        let metadata = make_metadata_with_form(form);

        let mut config = DiagnosticsConfig::default();
        config.disabled.push(DiagnosticCode::WrongDataPathForFormElements);

        let file_text = "Процедура Тест()\nКонецПроцедуры";
        let diagnostics = crate::test_utils::check_metadata_diagnostic_with_config(
            metadata,
            file_text,
            config,
            from_metadata,
        );

        assert!(diagnostics.is_empty());
    }
}
