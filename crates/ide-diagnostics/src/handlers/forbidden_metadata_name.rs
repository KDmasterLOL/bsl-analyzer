//! ForbiddenMetadataName diagnostic
//!
//! Checks that metadata objects are not named using reserved query language words.
//!
//! Ported from: ForbiddenMetadataNameDiagnostic.java

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use bsl_metadata::traits::MdObject;
use hir::ModuleMetadata;
use ide_db::TextRange;
use once_cell::sync::Lazy;
use rustc_hash::FxHashSet;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Blocker,
    scope: DiagnosticScope::Bsl,
    modules: &[
        bsl_metadata::ModuleType::ManagerModule,
        bsl_metadata::ModuleType::ObjectModule,
        bsl_metadata::ModuleType::ValueManagerModule,
        bsl_metadata::ModuleType::SessionModule,
    ],
    minutes_to_fix: 30,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Standard, MetadataTag::Sql, MetadataTag::Design],
    can_locate_on_project: true,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
    clean_code_attribute: CleanCodeAttribute::Consistent,
};

static FORBIDDEN_NAMES: Lazy<FxHashSet<&'static str>> = Lazy::new(|| {
    [
        // English
        "accountingregister",
        "accountingregisters",
        "accumulationregister",
        "accumulationregisters",
        "businessprocess",
        "businessprocesses",
        "calculationregister",
        "calculationregisters",
        "catalog",
        "catalogs",
        "chartofaccounts",
        "chartofcalculationtypes",
        "chartofcharacteristictypes",
        "chartsofaccounts",
        "chartsofcalculationtypes",
        "chartsofcharacteristictypes",
        "constant",
        "constants",
        "document",
        "documents",
        "documentjournal",
        "documentjournals",
        "enum",
        "enums",
        "exchangeplan",
        "exchangeplans",
        "filtercriteria",
        "filtercriterion",
        "informationregister",
        "informationregisters",
        "task",
        "tasks",
        // Russian (lowercase)
        "бизнеспроцесс",
        "бизнеспроцессы",
        "документ",
        "документы",
        "журналдокументов",
        "журналыдокументов",
        "задача",
        "задачи",
        "константа",
        "константы",
        "критерииотбора",
        "критерийотбора",
        "перечисление",
        "перечисления",
        "планвидоврасчета",
        "планвидовхарактеристик",
        "планобмена",
        "плансчетов",
        "планывидоврасчета",
        "планывидовхарактеристик",
        "планыобмена",
        "планысчетов",
        "регистрбухгалтерии",
        "регистрнакопления",
        "регистррасчета",
        "регистрсведений",
        "регистрыбухгалтерии",
        "регистрынакопления",
        "регистрырасчета",
        "регистрысведений",
        "справочник",
        "справочники",
    ]
    .into_iter()
    .collect()
});

fn is_forbidden_name(name: &str) -> bool {
    FORBIDDEN_NAMES.contains(name.to_lowercase().as_str())
}

fn make_diagnostic(
    name: &str,
    mdo_ref: &str,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    Diagnostic {
        code,
        message: format!("Запрещено использовать имя `{}` для `{}`", name, mdo_ref),
        severity: ctx.severity(code),
        range: TextRange::empty(0.into()),
        tags: ctx.tags(code),
        fixes: vec![],
    }
}

fn get_mdo_type_russian(mdo_type: &bsl_metadata::MdoType) -> &'static str {
    use bsl_metadata::MdoType;
    match mdo_type {
        MdoType::Catalog => "Справочник",
        MdoType::Document => "Документ",
        MdoType::InformationRegister => "РегистрСведений",
        MdoType::AccumulationRegister => "РегистрНакопления",
        MdoType::AccountingRegister => "РегистрБухгалтерии",
        MdoType::CalculationRegister => "РегистрРасчета",
        MdoType::ChartOfCharacteristicTypes => "ПланВидовХарактеристик",
        MdoType::ChartOfAccounts => "ПланСчетов",
        MdoType::ChartOfCalculationTypes => "ПланВидовРасчета",
        MdoType::BusinessProcess => "БизнесПроцесс",
        MdoType::Task => "Задача",
        MdoType::Enum => "Перечисление",
        MdoType::ExchangePlan => "ПланОбмена",
        MdoType::Constant => "Константа",
        MdoType::DataProcessor => "Обработка",
        MdoType::Report => "Отчет",
        MdoType::CommonModule => "ОбщийМодуль",
        MdoType::ExternalDataSource => "ВнешнийИсточникДанных",
        MdoType::Cube => "Куб",
        MdoType::DimensionTable => "ТаблицаИзмерения",
    }
}

pub fn from_metadata(metadata: &ModuleMetadata, ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::ForbiddenMetadataName;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    if let Some(ref common_module) = metadata.common_module {
        check_common_module(common_module, code, ctx, &mut diagnostics);
    }

    if let Some(ref mdo) = metadata.mdo {
        check_metadata_object(mdo, code, ctx, &mut diagnostics);
    }

    if let Some(ref register) = metadata.register {
        check_register(register, code, ctx, &mut diagnostics);
    }

    diagnostics
}

fn check_common_module(
    module: &bsl_metadata::CommonModule,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let name = module.name();
    if is_forbidden_name(name) {
        let mdo_ref = format!("ОбщийМодуль.{}", name);
        diagnostics.push(make_diagnostic(name, &mdo_ref, code, ctx));
    }
}

fn check_metadata_object(
    mdo: &bsl_metadata::MetadataObject,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let type_name = get_mdo_type_russian(&mdo.mdo_type);
    let base_ref = format!("{}.{}", type_name, mdo.name);

    if is_forbidden_name(&mdo.name) {
        diagnostics.push(make_diagnostic(&mdo.name, &base_ref, code, ctx));
    }

    for attr in &mdo.attributes {
        if is_forbidden_name(&attr.name) {
            let mdo_ref = format!("{}.Реквизит.{}", base_ref, attr.name);
            diagnostics.push(make_diagnostic(&attr.name, &mdo_ref, code, ctx));
        }
    }

    for ts in &mdo.tabular_sections {
        if is_forbidden_name(ts.name()) {
            let mdo_ref = format!("{}.ТабличнаяЧасть.{}", base_ref, ts.name());
            diagnostics.push(make_diagnostic(ts.name(), &mdo_ref, code, ctx));
        }

        for ts_attr in ts.attributes() {
            if is_forbidden_name(ts_attr.name()) {
                let mdo_ref = format!(
                    "{}.ТабличнаяЧасть.{}.Реквизит.{}",
                    base_ref,
                    ts.name(),
                    ts_attr.name()
                );
                diagnostics.push(make_diagnostic(ts_attr.name(), &mdo_ref, code, ctx));
            }
        }
    }
}

fn check_register(
    register: &bsl_metadata::Register,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let type_name = get_mdo_type_russian(&register.mdo_type());
    let base_ref = format!("{}.{}", type_name, register.name());

    if is_forbidden_name(register.name()) {
        diagnostics.push(make_diagnostic(register.name(), &base_ref, code, ctx));
    }

    for dim in register.dimensions() {
        if is_forbidden_name(dim.name()) {
            let mdo_ref = format!("{}.Измерение.{}", base_ref, dim.name());
            diagnostics.push(make_diagnostic(dim.name(), &mdo_ref, code, ctx));
        }
    }

    for attr in register.attributes() {
        if is_forbidden_name(attr.name()) {
            let mdo_ref = format!("{}.Реквизит.{}", base_ref, attr.name());
            diagnostics.push(make_diagnostic(attr.name(), &mdo_ref, code, ctx));
        }
    }

    for resource in register.resources() {
        if is_forbidden_name(resource.name()) {
            let mdo_ref = format!("{}.Ресурс.{}", base_ref, resource.name());
            diagnostics.push(make_diagnostic(resource.name(), &mdo_ref, code, ctx));
        }
    }
}

pub fn check_session_module(
    configuration: &bsl_metadata::Configuration,
    ctx: &DiagnosticsContext,
) -> Vec<Diagnostic> {
    let code = DiagnosticCode::ForbiddenMetadataName;

    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();

    for mdo in configuration.metadata_objects() {
        if !has_modules(&mdo.mdo_type) {
            check_metadata_object_full(mdo, code, ctx, &mut diagnostics);
        }
    }

    diagnostics
}

fn has_modules(mdo_type: &bsl_metadata::MdoType) -> bool {
    use bsl_metadata::MdoType;
    matches!(
        mdo_type,
        MdoType::Catalog
            | MdoType::Document
            | MdoType::BusinessProcess
            | MdoType::Task
            | MdoType::ChartOfAccounts
            | MdoType::ChartOfCalculationTypes
            | MdoType::ChartOfCharacteristicTypes
            | MdoType::DataProcessor
            | MdoType::Report
            | MdoType::ExchangePlan
    )
}

fn check_metadata_object_full(
    mdo: &bsl_metadata::MetadataObject,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let type_name = get_mdo_type_russian(&mdo.mdo_type);
    let base_ref = format!("{}.{}", type_name, mdo.name);

    if is_forbidden_name(&mdo.name) {
        diagnostics.push(make_diagnostic(&mdo.name, &base_ref, code, ctx));
    }

    for attr in &mdo.attributes {
        if is_forbidden_name(&attr.name) {
            let mdo_ref = format!("{}.Реквизит.{}", base_ref, attr.name);
            diagnostics.push(make_diagnostic(&attr.name, &mdo_ref, code, ctx));
        }
    }

    for ts in &mdo.tabular_sections {
        if is_forbidden_name(ts.name()) {
            let mdo_ref = format!("{}.ТабличнаяЧасть.{}", base_ref, ts.name());
            diagnostics.push(make_diagnostic(ts.name(), &mdo_ref, code, ctx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn make_metadata_with_common_module(module: bsl_metadata::CommonModule) -> ModuleMetadata {
        ModuleMetadata {
            module_type: bsl_metadata::ModuleType::CommonModule,
            execution_context: None,
            common_module: Some(Arc::new(module)),
            mdo: None,
            register: None,
            http_service: None,
            web_service: None,
            form: None,
        }
    }

    fn make_metadata_with_mdo(mdo: bsl_metadata::MetadataObject) -> ModuleMetadata {
        ModuleMetadata {
            module_type: bsl_metadata::ModuleType::ObjectModule,
            execution_context: None,
            common_module: None,
            mdo: Some(Arc::new(mdo)),
            register: None,
            http_service: None,
            web_service: None,
            form: None,
        }
    }

    fn make_metadata_with_register(register: bsl_metadata::Register) -> ModuleMetadata {
        ModuleMetadata {
            module_type: bsl_metadata::ModuleType::ManagerModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: Some(Arc::new(register)),
            http_service: None,
            web_service: None,
            form: None,
        }
    }

    #[test]
    fn test_forbidden_names_pattern() {
        assert!(is_forbidden_name("Справочник"));
        assert!(is_forbidden_name("справочник"));
        assert!(is_forbidden_name("СПРАВОЧНИК"));
        assert!(is_forbidden_name("Catalog"));
        assert!(is_forbidden_name("catalog"));
        assert!(is_forbidden_name("CATALOG"));
        assert!(is_forbidden_name("РегистрСведений"));
        assert!(is_forbidden_name("InformationRegister"));

        assert!(!is_forbidden_name("МойСправочник"));
        assert!(!is_forbidden_name("MyCatalog"));
        assert!(!is_forbidden_name("Товары"));
        assert!(!is_forbidden_name("Products"));
    }

    #[test]
    fn test_common_module_forbidden_name() {
        let module = bsl_metadata::CommonModule::builder().name("Справочник").build();
        let metadata = make_metadata_with_common_module(module);
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Справочник"));
        assert!(diagnostics[0].message.contains("ОбщийМодуль.Справочник"));
    }

    #[test]
    fn test_common_module_allowed_name() {
        let module = bsl_metadata::CommonModule::builder().name("ОбщегоНазначения").build();
        let metadata = make_metadata_with_common_module(module);
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_metadata_object_forbidden_name() {
        let mdo = bsl_metadata::MetadataObject::new(bsl_metadata::MdoType::Catalog, "Справочник");
        let metadata = make_metadata_with_mdo(mdo);
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("Справочник.Справочник"));
    }

    #[test]
    fn test_metadata_object_with_forbidden_attribute() {
        let mut mdo = bsl_metadata::MetadataObject::new(bsl_metadata::MdoType::Catalog, "Товары");
        mdo.attributes.push(bsl_metadata::Attribute {
            name: "РегистрСведений".to_string(),
            name_en: None,
            attr_type: bsl_metadata::AttributeType::String { length: Some(100) },
        });
        let metadata = make_metadata_with_mdo(mdo);
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("РегистрСведений"));
        assert!(diagnostics[0].message.contains("Справочник.Товары.Реквизит.РегистрСведений"));
    }

    #[test]
    fn test_register_forbidden_name() {
        let register = bsl_metadata::Register::builder()
            .name("РегистрСведений")
            .mdo_type(bsl_metadata::MdoType::InformationRegister)
            .build();
        let metadata = make_metadata_with_register(register);
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("РегистрСведений.РегистрСведений"));
    }

    #[test]
    fn test_register_allowed_name() {
        let register = bsl_metadata::Register::builder()
            .name("ОстаткиТоваров")
            .mdo_type(bsl_metadata::MdoType::AccumulationRegister)
            .build();
        let metadata = make_metadata_with_register(register);
        let diagnostics = crate::test_utils::check_metadata_diagnostic(metadata, "", from_metadata);

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_disabled_diagnostic() {
        let module = bsl_metadata::CommonModule::builder().name("Справочник").build();
        let metadata = make_metadata_with_common_module(module);

        let mut config = crate::DiagnosticsConfig::default();
        config.disabled.push(DiagnosticCode::ForbiddenMetadataName);

        let diagnostics = crate::test_utils::check_metadata_diagnostic_with_config(
            metadata,
            "",
            config,
            from_metadata,
        );

        assert!(diagnostics.is_empty());
    }
}
