//! RedundantAccessToObject diagnostic
//!
//! Detects redundant access to object via ЭтотОбъект/ThisObject or module name.
//!
//! Examples:
//! - ObjectModule: `ЭтотОбъект.Контрагент` → should be `Контрагент`
//! - FormModule: `ЭтотОбъект.Элементы` → should be `Элементы`
//! - CommonModule: `МойМодуль.МояФункция()` → should be `МояФункция()`
//! - ManagerModule: `Справочники.Справочник1.Метод()` → should be `Метод()`
//!
//! Exclusions:
//! - `ЭтотОбъект["Поле"]` is NOT an error (INDEX_EXPR handled separately)
//! - CommonModule with ReturnValueReuse != DontUse are NOT checked
//!
//! Ported from: RedundantAccessToObjectDiagnostic.java

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use bsl_metadata::traits::MdObject;
use bsl_metadata::{ModuleType, ReturnValueReuse};
use hir::RedundantAccessKind;
use ide_db::TextRange;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Info,
    scope: DiagnosticScope::Bsl,
    modules: &[
        bsl_metadata::ModuleType::CommonModule,
        bsl_metadata::ModuleType::ObjectModule,
        bsl_metadata::ModuleType::ManagerModule,
        bsl_metadata::ModuleType::FormModule,
        bsl_metadata::ModuleType::RecordSetModule,
    ],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Clumsy],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Adaptable,
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from hir_dispatch when `BodyDiagnostic::RedundantAccessToObject` is encountered.
///
/// This function validates the candidate against module metadata:
/// 1. For ThisObject: module must be ObjectModule/FormModule/RecordSetModule
/// 2. For TwoLevel (CommonModule): module name must match, ReturnValueReuse == DontUse
/// 3. For ThreeLevel (ManagerModule): mdo_type and mdo_name must match current module
pub fn from_hir(
    kind: &RedundantAccessKind,
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::RedundantAccessToObject;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    let metadata = ctx.module_metadata();

    match kind {
        RedundantAccessKind::ThisObject { prefix: _ } => {
            // Validate for ObjectModule/FormModule/RecordSetModule
            let should_check = match metadata.module_type {
                ModuleType::ObjectModule => get_check_object_module(ctx),
                ModuleType::FormModule => get_check_form_module(ctx),
                ModuleType::RecordSetModule => get_check_record_set_module(ctx),
                _ => false,
            };
            if !should_check {
                return None;
            }
            Some(create_diagnostic(range, code, ctx))
        }
        RedundantAccessKind::TwoLevel { module } => {
            // Validate for CommonModule: module name must match current module
            if metadata.module_type != ModuleType::CommonModule {
                return None;
            }
            let cm = metadata.common_module.as_ref()?;

            // Skip if ReturnValueReuse is active (caching requires full path)
            if cm.return_values_reuse() != ReturnValueReuse::DontUse {
                return None;
            }

            // Check if module name matches current common module name
            if !module.eq_ignore_ascii_case(cm.name()) {
                return None;
            }
            Some(create_diagnostic(range, code, ctx))
        }
        RedundantAccessKind::ThreeLevel { mdo_type, mdo_name } => {
            // Validate for ManagerModule: mdo_type and mdo_name must match
            if metadata.module_type != ModuleType::ManagerModule {
                return None;
            }
            let mdo = metadata.mdo.as_ref()?;

            // Check if mdo_type matches manager collection name
            let expected_plural = get_plural_collection_name(mdo.mdo_type)?;

            // Compare mdo_type (case-insensitive, bilingual)
            if !is_matching_mdo_type(mdo_type, expected_plural) {
                return None;
            }

            // Check if mdo_name matches
            if !mdo_name.eq_ignore_ascii_case(&mdo.name) {
                return None;
            }
            Some(create_diagnostic(range, code, ctx))
        }
    }
}

fn create_diagnostic(
    range: TextRange,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    Diagnostic {
        code,
        message: "Избыточное обращение к объекту".into(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

/// Get plural collection name for MdoType (Russian form).
fn get_plural_collection_name(
    mdo_type: bsl_metadata::MdoType,
) -> Option<(&'static str, &'static str)> {
    use bsl_metadata::MdoType;
    match mdo_type {
        MdoType::Catalog => Some(("справочники", "catalogs")),
        MdoType::Document => Some(("документы", "documents")),
        MdoType::InformationRegister => Some(("регистрысведений", "informationregisters")),
        MdoType::AccumulationRegister => Some(("регистрынакопления", "accumulationregisters")),
        MdoType::AccountingRegister => Some(("регистрыбухгалтерии", "accountingregisters")),
        MdoType::CalculationRegister => Some(("регистрырасчета", "calculationregisters")),
        MdoType::ChartOfCharacteristicTypes => {
            Some(("планывидовхарактеристик", "chartsofcharacteristictypes"))
        }
        MdoType::ChartOfAccounts => Some(("планысчетов", "chartsofaccounts")),
        MdoType::ChartOfCalculationTypes => Some(("планывидоврасчета", "chartsofcalculationtypes")),
        MdoType::BusinessProcess => Some(("бизнеспроцессы", "businessprocesses")),
        MdoType::Task => Some(("задачи", "tasks")),
        MdoType::Enum => Some(("перечисления", "enums")),
        MdoType::ExchangePlan => Some(("планыобмена", "exchangeplans")),
        MdoType::ExternalDataSource => Some(("внешниеисточникиданных", "externaldatasources")),
        MdoType::Constant => Some(("константы", "constants")),
        MdoType::DataProcessor => Some(("обработки", "dataprocessors")),
        MdoType::Report => Some(("отчеты", "reports")),
        // These don't have manager modules
        MdoType::Cube | MdoType::DimensionTable | MdoType::CommonModule => None,
    }
}

/// Check if mdo_type string matches expected plural forms (bilingual, case-insensitive).
fn is_matching_mdo_type(mdo_type: &str, expected: (&str, &str)) -> bool {
    let lower = mdo_type.to_lowercase();
    lower == expected.0 || lower == expected.1
}

// Configuration parameter getters (with defaults)
fn get_check_object_module(ctx: &DiagnosticsContext) -> bool {
    ctx.config
        .get_bool(DiagnosticCode::RedundantAccessToObject, "checkObjectModule")
        .unwrap_or(true)
}

fn get_check_form_module(ctx: &DiagnosticsContext) -> bool {
    ctx.config.get_bool(DiagnosticCode::RedundantAccessToObject, "checkFormModule").unwrap_or(true)
}

fn get_check_record_set_module(ctx: &DiagnosticsContext) -> bool {
    ctx.config
        .get_bool(DiagnosticCode::RedundantAccessToObject, "checkRecordSetModule")
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use crate::test_utils::check_hir_diagnostic;
    use crate::DiagnosticCode;
    #[test]
    fn test_this_object_field_access() {
        // ThisObject field access - should emit candidate (but no diagnostic without metadata)
        let code = r#"Процедура Тест()
    ЭтотОбъект.Контрагент = Данные.Контрагент;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        // Without metadata context (ObjectModule), no diagnostics
        let redundant_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::RedundantAccessToObject)
            .collect();
        assert_eq!(redundant_diags.len(), 0);
    }

    #[test]
    fn test_this_object_index_access_no_diagnostic() {
        // INDEX_EXPR access - should NOT emit candidate (handled separately)
        let code = r#"Процедура Тест()
    ЭтотОбъект["ПолеКонтактнойИнформации"] = Данные.Телефон;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let redundant_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::RedundantAccessToObject)
            .collect();
        // INDEX_EXPR is not a FIELD_EXPR, so no candidate emitted
        assert_eq!(redundant_diags.len(), 0);
    }

    #[test]
    fn test_this_object_english() {
        // English ThisObject
        let code = r#"Procedure Test()
    ThisObject.Counterparty = Data.Counterparty;
EndProcedure"#;

        let diagnostics = check_hir_diagnostic(code);
        // Without metadata context, no diagnostics
        let redundant_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::RedundantAccessToObject)
            .collect();
        assert_eq!(redundant_diags.len(), 0);
    }
}
