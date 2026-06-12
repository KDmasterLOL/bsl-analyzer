use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use bsl_metadata::traits::MdObject;
use bsl_metadata::{ModuleType, ReturnValueReuse};
use hir::RedundantAccessKind;
use ide_db::TextRange;
use stdx::case::CaseExt;

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
            if metadata.module_type != ModuleType::CommonModule {
                return None;
            }
            let cm = metadata.common_module.as_ref()?;

            if cm.return_values_reuse() != ReturnValueReuse::DontUse {
                return None;
            }

            if !module.eq_ignore_ascii_case(cm.name()) {
                return None;
            }
            Some(create_diagnostic(range, code, ctx))
        }
        RedundantAccessKind::ThreeLevel { mdo_type, mdo_name } => {
            if metadata.module_type != ModuleType::ManagerModule {
                return None;
            }
            let mdo = metadata.mdo.as_ref()?;

            let expected_plural = get_plural_collection_name(mdo.mdo_type)?;

            if !is_matching_mdo_type(mdo_type, expected_plural) {
                return None;
            }

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
        MdoType::Cube
        | MdoType::DimensionTable
        | MdoType::CommonModule
        | MdoType::EventSubscription
        | MdoType::Subsystem
        | MdoType::Role => None,
    }
}

fn is_matching_mdo_type(mdo_type: &str, expected: (&str, &str)) -> bool {
    let lower = mdo_type.fold_lower();
    lower == expected.0 || lower == expected.1
}

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
    use crate::test_utils::{check_diagnostics_snapshot_for, format_diags};
    use crate::DiagnosticCode;
    use expect_test::expect;
    #[test]
    fn test_this_object_field_access() {
        let code = r#"Процедура Тест()
    ЭтотОбъект.Контрагент = Данные.Контрагент;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::RedundantAccessToObject,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_this_object_index_access_no_diagnostic() {
        let code = r#"Процедура Тест()
    ЭтотОбъект["ПолеКонтактнойИнформации"] = Данные.Телефон;
КонецПроцедуры"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::RedundantAccessToObject,
            expect![[r#""#]],
        );
    }

    #[test]
    fn test_this_object_chained_access_single_diagnostic() {
        use crate::test_utils::{check_metadata_diagnostic, make_non_common_module_metadata};

        let metadata = make_non_common_module_metadata(bsl_metadata::ModuleType::ObjectModule);
        let code = r#"Процедура Тест()
    Если ТипЗНЧ(ЭтотОбъект.Отбор.Регистратор.Значение) = Тип("ДокументСсылка.ЭтапПроизводства2_2") Тогда
        Возврат;
    КонецЕсли;
КонецПроцедуры"#;

        let diagnostics =
            check_metadata_diagnostic(metadata, code, |_meta, ctx| crate::diagnostics(ctx));
        let redundant_diags = diagnostics
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::RedundantAccessToObject)
            .collect::<Vec<_>>();
        expect![[r#"
            RedundantAccessToObject @ 2:17..2:33
              message: Избыточное обращение к объекту
              severity: Hint"#]]
        .assert_eq(&format_diags(code, &redundant_diags));
    }

    #[test]
    fn test_this_object_english() {
        let code = r#"Procedure Test()
    ThisObject.Counterparty = Data.Counterparty;
EndProcedure"#;

        check_diagnostics_snapshot_for(
            code,
            DiagnosticCode::RedundantAccessToObject,
            expect![[r#""#]],
        );
    }
}
