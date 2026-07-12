use bsl_metadata::MdoType;
use bsl_platform::{find_prefixed_method, PlatformMethod};
use bsl_types::builders::Builders;
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;
use bsl_types::testing::RootConfigCtx;
use hir_def::ty::{FunctionSignature, MetadataKind};
use hir_def::Name;

use crate::lower::type_string::{lower_param_type_string_typeid, lower_platform_type_name_typeid};
use crate::method_lookup::lower_overloads_typeid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformMethodResolution {
    pub signature: FunctionSignature,
    pub return_ty: TypeId,
    pub overloads: Vec<Vec<TypeId>>,
    /// Execution environments the method is available in.
    pub env: hir_def::execution_env::EnvFlags,
}

pub fn resolve_platform_manager_method(
    db: &dyn TypeKernelDb,
    mdo_type: MdoType,
    mdo_name: &Name,
    method_name: &Name,
) -> Option<PlatformMethodResolution> {
    let prefix = mdo_type.manager_type_prefix()?;
    let method = find_prefixed_method(prefix, method_name.as_str())?;
    Some(build_resolution(db, &method, mdo_type, mdo_name))
}

pub fn resolve_platform_metadata_ref_method(
    db: &dyn TypeKernelDb,
    kind: MetadataKind,
    mdo_name: &Name,
    method_name: &Name,
) -> Option<PlatformMethodResolution> {
    let (prefix, parent_mdo) = metadata_kind_to_prefix_and_mdo(kind)?;
    let method = find_prefixed_method(prefix, method_name.as_str())?;
    Some(build_resolution(db, &method, parent_mdo, mdo_name))
}

pub(crate) fn build_resolution(
    db: &dyn TypeKernelDb,
    method: &PlatformMethod,
    mdo_type: MdoType,
    mdo_name: &Name,
) -> PlatformMethodResolution {
    let params: Vec<TypeId> = method
        .parameters
        .iter()
        .map(|p| {
            p.param_type
                .as_ref()
                .map(|t| lower_param_type_string_typeid(db, t))
                .unwrap_or(db.unknown())
        })
        .collect();
    let defaults: Vec<bool> = method.parameters.iter().map(|p| p.is_optional).collect();

    let return_ty = method
        .return_type
        .as_ref()
        .map(|raw| {
            map_generic_metadata_return_type_typeid(db, raw, mdo_type, mdo_name)
                .unwrap_or_else(|| lower_platform_type_name_typeid(db, raw))
        })
        .unwrap_or(db.undefined());

    let signature = FunctionSignature {
        max_args: Some(params.len() as u32),
        params: params.into_boxed_slice(),
        defaults: defaults.into_boxed_slice(),
        ret: return_ty,
        from_doc_comment: false,
    };
    PlatformMethodResolution {
        signature,
        return_ty,
        overloads: lower_overloads_typeid(db, method),
        env: hir_def::execution_env::EnvFlags::from_platform_context(method.context.as_ref()),
    }
}

pub fn resolve_platform_any_metadata_ref_method(
    db: &dyn TypeKernelDb,
    mdo_type: MdoType,
    method_name: &Name,
) -> Option<PlatformMethodResolution> {
    let ref_kind = MetadataKind::ref_kind_for(mdo_type)?;
    let (prefix, parent_mdo) = metadata_kind_to_prefix_and_mdo(ref_kind)?;
    let method = find_prefixed_method(prefix, method_name.as_str())?;

    let params: Vec<TypeId> = method
        .parameters
        .iter()
        .map(|p| {
            p.param_type
                .as_ref()
                .map(|t| lower_param_type_string_typeid(db, t))
                .unwrap_or(db.unknown())
        })
        .collect();
    let defaults: Vec<bool> = method.parameters.iter().map(|p| p.is_optional).collect();

    let return_ty = method
        .return_type
        .as_ref()
        .map(|raw| match map_generic_metadata_return_type(raw, parent_mdo) {
            Some(kind) if kind.ref_mdo_type().is_some() => db.any_metadata_ref(parent_mdo),
            Some(_) => db.unknown(),
            None => lower_platform_type_name_typeid(db, raw),
        })
        .unwrap_or(db.undefined());

    let signature = FunctionSignature {
        max_args: Some(params.len() as u32),
        params: params.into_boxed_slice(),
        defaults: defaults.into_boxed_slice(),
        ret: return_ty,
        from_doc_comment: false,
    };
    Some(PlatformMethodResolution {
        signature,
        return_ty,
        overloads: lower_overloads_typeid(db, &method),
        env: hir_def::execution_env::EnvFlags::from_platform_context(method.context.as_ref()),
    })
}

pub(crate) fn map_generic_metadata_return_type_typeid(
    db: &dyn TypeKernelDb,
    raw: &str,
    mdo_type: MdoType,
    mdo_name: &Name,
) -> Option<TypeId> {
    let kind = map_generic_metadata_return_type(raw, mdo_type)?;
    Some(db.metadata_ref(kind, mdo_name.as_str().to_string(), &RootConfigCtx))
}

pub(crate) fn metadata_kind_to_prefix_and_mdo(
    kind: MetadataKind,
) -> Option<(&'static str, MdoType)> {
    let prefix = kind.platform_prefix()?;
    let parent_mdo = match kind {
        MetadataKind::CatalogObject | MetadataKind::CatalogRef => MdoType::Catalog,
        MetadataKind::DocumentObject | MetadataKind::DocumentRef => MdoType::Document,
        MetadataKind::EnumRef => MdoType::Enum,
        MetadataKind::TaskRef | MetadataKind::TaskObject => MdoType::Task,
        MetadataKind::BusinessProcessRef | MetadataKind::BusinessProcessObject => {
            MdoType::BusinessProcess
        }
        MetadataKind::DataProcessorObject => MdoType::DataProcessor,
        MetadataKind::ReportObject => MdoType::Report,
        MetadataKind::ExchangePlanRef | MetadataKind::ExchangePlanObject => MdoType::ExchangePlan,
        MetadataKind::ChartOfAccountsRef | MetadataKind::ChartOfAccountsObject => {
            MdoType::ChartOfAccounts
        }
        MetadataKind::ChartOfCharacteristicTypesRef
        | MetadataKind::ChartOfCharacteristicTypesObject => MdoType::ChartOfCharacteristicTypes,
        MetadataKind::ChartOfCalculationTypesRef | MetadataKind::ChartOfCalculationTypesObject => {
            MdoType::ChartOfCalculationTypes
        }
        MetadataKind::InformationRegisterRecordManager
        | MetadataKind::InformationRegisterRecordSet
        | MetadataKind::InformationRegisterRecord => MdoType::InformationRegister,
        MetadataKind::AccumulationRegisterRecordSet | MetadataKind::AccumulationRegisterRecord => {
            MdoType::AccumulationRegister
        }
        MetadataKind::AccountingRegisterRecordSet | MetadataKind::AccountingRegisterRecord => {
            MdoType::AccountingRegister
        }
        MetadataKind::CalculationRegisterRecordSet | MetadataKind::CalculationRegisterRecord => {
            MdoType::CalculationRegister
        }
        MetadataKind::InformationRegisterRef
        | MetadataKind::AccumulationRegisterRef
        | MetadataKind::AccountingRegisterRef
        | MetadataKind::CalculationRegisterRef
        | MetadataKind::RegisterDimension { .. }
        | MetadataKind::RegisterResource { .. }
        | MetadataKind::RegisterAttribute { .. }
        | MetadataKind::RegisterFilter { .. }
        | MetadataKind::TabularSection { .. }
        | MetadataKind::TabularSectionRow { .. } => return None,
    };
    Some((prefix, parent_mdo))
}

pub(crate) fn map_generic_metadata_return_type(
    raw: &str,
    mdo_type: MdoType,
) -> Option<MetadataKind> {
    let kind = match (raw, mdo_type) {
        ("СправочникОбъект" | "CatalogObject", MdoType::Catalog) => {
            MetadataKind::CatalogObject
        }
        ("СправочникСсылка" | "CatalogRef", MdoType::Catalog) => {
            MetadataKind::CatalogRef
        }
        ("ДокументОбъект" | "DocumentObject", MdoType::Document) => {
            MetadataKind::DocumentObject
        }
        ("ДокументСсылка" | "DocumentRef", MdoType::Document) => {
            MetadataKind::DocumentRef
        }
        ("ПеречислениеСсылка" | "EnumRef", MdoType::Enum) => {
            MetadataKind::EnumRef
        }
        ("ЗадачаСсылка" | "TaskRef", MdoType::Task) => MetadataKind::TaskRef,
        ("БизнесПроцессСсылка" | "BusinessProcessRef", MdoType::BusinessProcess) => {
            MetadataKind::BusinessProcessRef
        }
        ("ПланОбменаСсылка" | "ExchangePlanRef", MdoType::ExchangePlan) => {
            MetadataKind::ExchangePlanRef
        }
        ("ПланОбменаОбъект" | "ExchangePlanObject", MdoType::ExchangePlan) => {
            MetadataKind::ExchangePlanObject
        }
        ("ПланСчетовСсылка" | "ChartOfAccountsRef", MdoType::ChartOfAccounts) => {
            MetadataKind::ChartOfAccountsRef
        }
        ("ПланСчетовОбъект" | "ChartOfAccountsObject", MdoType::ChartOfAccounts) => {
            MetadataKind::ChartOfAccountsObject
        }
        (
            "ПланВидовХарактеристикСсылка" | "ChartOfCharacteristicTypesRef",
            MdoType::ChartOfCharacteristicTypes,
        ) => MetadataKind::ChartOfCharacteristicTypesRef,
        (
            "ПланВидовХарактеристикОбъект" | "ChartOfCharacteristicTypesObject",
            MdoType::ChartOfCharacteristicTypes,
        ) => MetadataKind::ChartOfCharacteristicTypesObject,
        (
            "ПланВидовРасчетаСсылка" | "ChartOfCalculationTypesRef",
            MdoType::ChartOfCalculationTypes,
        ) => MetadataKind::ChartOfCalculationTypesRef,
        (
            "ПланВидовРасчетаОбъект" | "ChartOfCalculationTypesObject",
            MdoType::ChartOfCalculationTypes,
        ) => MetadataKind::ChartOfCalculationTypesObject,
        ("ЗадачаОбъект" | "TaskObject", MdoType::Task) => MetadataKind::TaskObject,
        ("БизнесПроцессОбъект" | "BusinessProcessObject", MdoType::BusinessProcess) => {
            MetadataKind::BusinessProcessObject
        }
        ("ОбработкаОбъект" | "DataProcessorObject", MdoType::DataProcessor) => {
            MetadataKind::DataProcessorObject
        }
        ("ОтчётОбъект" | "ОтчетОбъект" | "ReportObject", MdoType::Report) => {
            MetadataKind::ReportObject
        }
        (
            "РегистрСведенийМенеджерЗаписи" | "InformationRegisterRecordManager",
            MdoType::InformationRegister,
        ) => MetadataKind::InformationRegisterRecordManager,
        (
            "РегистрСведенийНаборЗаписей" | "InformationRegisterRecordSet",
            MdoType::InformationRegister,
        ) => MetadataKind::InformationRegisterRecordSet,
        (
            "РегистрНакопленияНаборЗаписей" | "AccumulationRegisterRecordSet",
            MdoType::AccumulationRegister,
        ) => MetadataKind::AccumulationRegisterRecordSet,
        (
            "РегистрБухгалтерииНаборЗаписей" | "AccountingRegisterRecordSet",
            MdoType::AccountingRegister,
        ) => MetadataKind::AccountingRegisterRecordSet,
        (
            "РегистрРасчетаНаборЗаписей" | "CalculationRegisterRecordSet",
            MdoType::CalculationRegister,
        ) => MetadataKind::CalculationRegisterRecordSet,
        ("РегистрСведенийЗапись" | "InformationRegisterRecord", MdoType::InformationRegister) => {
            MetadataKind::InformationRegisterRecord
        }
        (
            "РегистрНакопленияЗапись" | "AccumulationRegisterRecord",
            MdoType::AccumulationRegister,
        ) => MetadataKind::AccumulationRegisterRecord,
        ("РегистрБухгалтерииЗапись" | "AccountingRegisterRecord", MdoType::AccountingRegister) => {
            MetadataKind::AccountingRegisterRecord
        }
        ("РегистрРасчетаЗапись" | "CalculationRegisterRecord", MdoType::CalculationRegister) => {
            MetadataKind::CalculationRegisterRecord
        }
        _ => return None,
    };
    Some(kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_types::builders::Builders;
    use bsl_types::kind::TypeKind;
    use bsl_types::testing::InMemoryDb;

    #[test]
    fn platform_manager_typeid_round_trips_via_ty() {
        let db = InMemoryDb::new();
        let res = PlatformMethodResolution {
            signature: FunctionSignature {
                params: Box::new([]),
                defaults: Box::new([]),
                ret: db.number(None, None),
                max_args: Some(0),
                from_doc_comment: false,
            },
            return_ty: db.number(None, None),
            overloads: vec![vec![db.string(None, false)]],
            env: hir_def::execution_env::EnvFlags::ALL,
        };
        assert_eq!(res.return_ty, db.number(None, None));
        assert_eq!(res.overloads, vec![vec![db.string(None, false)]]);
    }

    #[test]
    fn manager_create_item_on_catalog_returns_catalog_object() {
        let db = InMemoryDb::new();
        let res = resolve_platform_manager_method(
            &db,
            MdoType::Catalog,
            &Name::new("Номенклатура"),
            &Name::new("СоздатьЭлемент"),
        )
        .expect("platform data indexes CreateItem under CatalogManager");

        assert_eq!(
            res.return_ty,
            db.metadata_ref(
                MetadataKind::CatalogObject,
                "Номенклатура".to_string(),
                &RootConfigCtx
            )
        );
    }

    #[test]
    fn manager_find_by_code_on_catalog_returns_catalog_ref() {
        let db = InMemoryDb::new();
        let res = resolve_platform_manager_method(
            &db,
            MdoType::Catalog,
            &Name::new("Валюты"),
            &Name::new("НайтиПоКоду"),
        )
        .expect("platform data indexes FindByCode under CatalogManager");

        assert_eq!(
            res.return_ty,
            db.metadata_ref(MetadataKind::CatalogRef, "Валюты".to_string(), &RootConfigCtx)
        );
    }

    #[test]
    fn manager_find_by_code_param_lowers_to_union() {
        let db = InMemoryDb::new();
        let res = resolve_platform_manager_method(
            &db,
            MdoType::Catalog,
            &Name::new("Валюты"),
            &Name::new("НайтиПоКоду"),
        )
        .expect("platform data indexes FindByCode under CatalogManager");

        assert_eq!(
            res.signature.params.first(),
            Some(&db.union(vec![db.number(None, None), db.string(None, false)])),
            "first param of FindByCode must be a Union, not a single PlatformObject; got {:?}",
            res.signature.params.first(),
        );
    }

    #[test]
    fn manager_unknown_method_returns_none() {
        let db = InMemoryDb::new();
        assert!(resolve_platform_manager_method(
            &db,
            MdoType::Catalog,
            &Name::new("Валюты"),
            &Name::new("НетТакогоМетода"),
        )
        .is_none());
    }

    #[test]
    fn manager_english_method_name_resolves() {
        let db = InMemoryDb::new();
        let res = resolve_platform_manager_method(
            &db,
            MdoType::Catalog,
            &Name::new("Номенклатура"),
            &Name::new("CreateItem"),
        )
        .expect("English 'CreateItem' must also resolve to CatalogManager.CreateItem");
        match db.lookup_type(res.return_ty) {
            TypeKind::MetadataRef(facet) => assert_eq!(facet.kind, MetadataKind::CatalogObject),
            other => panic!("expected MetadataRef{{CatalogObject}}, got {other:?}"),
        }
    }

    #[test]
    fn manager_mdo_without_prefix_returns_none() {
        let db = InMemoryDb::new();
        assert!(resolve_platform_manager_method(
            &db,
            MdoType::CommonModule,
            &Name::new("AnyName"),
            &Name::new("СоздатьЭлемент"),
        )
        .is_none());
    }

    #[test]
    fn metadata_ref_catalog_object_resolves_write_as_procedure() {
        let db = InMemoryDb::new();
        let res = resolve_platform_metadata_ref_method(
            &db,
            MetadataKind::CatalogObject,
            &Name::new("Номенклатура"),
            &Name::new("Записать"),
        )
        .expect("platform data indexes Write under CatalogObject");
        assert_eq!(res.return_ty, db.undefined());
    }

    #[test]
    fn any_metadata_ref_resolves_common_method_without_name() {
        let db = InMemoryDb::new();
        let res = resolve_platform_any_metadata_ref_method(
            &db,
            MdoType::Catalog,
            &Name::new("Метаданные"),
        );
        assert!(res.is_some(), "Metadata() must resolve on AnyMetadataRef{{Catalog}}");
    }

    #[test]
    fn any_metadata_ref_object_return_degrades_to_unknown() {
        let db = InMemoryDb::new();
        let res = resolve_platform_any_metadata_ref_method(
            &db,
            MdoType::Catalog,
            &Name::new("ПолучитьОбъект"),
        )
        .expect("GetObject must resolve on AnyMetadataRef{Catalog}");
        assert_eq!(res.return_ty, db.unknown(), "object return has no name to bind → Unknown");
    }

    #[test]
    fn any_metadata_ref_unknown_method_is_none() {
        let db = InMemoryDb::new();
        assert!(resolve_platform_any_metadata_ref_method(
            &db,
            MdoType::Catalog,
            &Name::new("НесуществующийМетод"),
        )
        .is_none());
    }

    #[test]
    fn any_metadata_ref_register_flavour_has_no_ref_surface() {
        let db = InMemoryDb::new();
        assert!(resolve_platform_any_metadata_ref_method(
            &db,
            MdoType::InformationRegister,
            &Name::new("Метаданные"),
        )
        .is_none());
    }

    #[test]
    fn metadata_ref_register_record_manager_resolves_write() {
        let db = InMemoryDb::new();
        let res = resolve_platform_metadata_ref_method(
            &db,
            MetadataKind::InformationRegisterRecordManager,
            &Name::new("Курсы"),
            &Name::new("Записать"),
        )
        .expect("platform data indexes Write under InformationRegisterRecordManager");
        assert_eq!(res.return_ty, db.undefined());
    }

    #[test]
    fn manager_create_record_set_on_information_register_returns_record_set() {
        let db = InMemoryDb::new();
        let res = resolve_platform_manager_method(
            &db,
            MdoType::InformationRegister,
            &Name::new("Курсы"),
            &Name::new("СоздатьНаборЗаписей"),
        )
        .expect("platform data indexes CreateRecordSet under InformationRegisterManager");
        assert_eq!(
            res.return_ty,
            db.metadata_ref(
                MetadataKind::InformationRegisterRecordSet,
                "Курсы".to_string(),
                &RootConfigCtx,
            )
        );
    }

    #[test]
    fn manager_create_record_set_on_accumulation_register_returns_record_set() {
        let db = InMemoryDb::new();
        let res = resolve_platform_manager_method(
            &db,
            MdoType::AccumulationRegister,
            &Name::new("ПродажиОбороты"),
            &Name::new("СоздатьНаборЗаписей"),
        )
        .expect("platform data indexes CreateRecordSet under AccumulationRegisterManager");
        assert_eq!(
            res.return_ty,
            db.metadata_ref(
                MetadataKind::AccumulationRegisterRecordSet,
                "ПродажиОбороты".to_string(),
                &RootConfigCtx,
            )
        );
    }

    #[test]
    fn manager_create_record_set_on_accounting_register_returns_record_set() {
        let db = InMemoryDb::new();
        let res = resolve_platform_manager_method(
            &db,
            MdoType::AccountingRegister,
            &Name::new("Хозрасчетный"),
            &Name::new("СоздатьНаборЗаписей"),
        )
        .expect("platform data indexes CreateRecordSet under AccountingRegisterManager");
        assert_eq!(
            res.return_ty,
            db.metadata_ref(
                MetadataKind::AccountingRegisterRecordSet,
                "Хозрасчетный".to_string(),
                &RootConfigCtx,
            )
        );
    }

    #[test]
    fn manager_create_record_set_on_calculation_register_returns_record_set() {
        let db = InMemoryDb::new();
        let res = resolve_platform_manager_method(
            &db,
            MdoType::CalculationRegister,
            &Name::new("Начисления"),
            &Name::new("СоздатьНаборЗаписей"),
        )
        .expect("platform data indexes CreateRecordSet under CalculationRegisterManager");
        assert_eq!(
            res.return_ty,
            db.metadata_ref(
                MetadataKind::CalculationRegisterRecordSet,
                "Начисления".to_string(),
                &RootConfigCtx,
            )
        );
    }

    #[test]
    fn metadata_ref_information_register_record_set_resolves_load() {
        let db = InMemoryDb::new();
        let res = resolve_platform_metadata_ref_method(
            &db,
            MetadataKind::InformationRegisterRecordSet,
            &Name::new("Курсы"),
            &Name::new("Загрузить"),
        )
        .expect("platform data indexes Load under InformationRegisterRecordSet");
        assert_eq!(res.return_ty, db.undefined());
    }
}
