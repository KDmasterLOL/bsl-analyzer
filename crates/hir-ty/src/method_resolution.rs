use bsl_types::builders::Builders;
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;
use hir_def::resolver::{QualifiedMethodError, Resolver};
use hir_def::symbol_tree::MethodSymbol;
use hir_def::ty::FunctionSignature;
use hir_def::{MethodId, Name};

use crate::db::HirDatabase;
use crate::lower::TyLoweringContext;
#[cfg(test)]
use vfs::FileId;

use crate::infer::UnresolvedMethodKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodResolution {
    pub method_id: MethodId,

    pub is_export: bool,

    pub signature: FunctionSignature,

    pub return_type: TypeId,
}

impl MethodResolution {
    pub fn new(method_id: MethodId, is_export: bool, signature: FunctionSignature) -> Self {
        let return_type = signature.ret;
        Self { method_id, is_export, signature, return_type }
    }
}

pub fn resolve_qualified_call(
    db: &dyn HirDatabase,
    module_name: &Name,
    method_name: &Name,
    resolver: &Resolver,
) -> Result<MethodResolution, UnresolvedMethodKind> {
    let resolution =
        resolver.resolve_qualified_method(db, module_name, method_name).map_err(|e| match e {
            QualifiedMethodError::NotVisibleInConfigs | QualifiedMethodError::NotFound => {
                UnresolvedMethodKind::MethodNotFound
            }
            QualifiedMethodError::BodyUnread => UnresolvedMethodKind::BodyUnread,
        })?;

    let symbol_tree = db.symbol_tree_ref(resolution.method_id.module);
    let method_symbol = symbol_tree.find_method_by_id(resolution.method_id).expect(
        "method_id returned by Resolver must exist in symbol_tree — \
         symbol_tree / Resolver are out of sync",
    );

    let signature = materialise_signature_enriched(db, resolution.method_id, method_symbol);
    Ok(MethodResolution::new(resolution.method_id, resolution.is_export, signature))
}

fn object_kind_to_mdo(kind: hir_def::ty::MetadataKind) -> Option<bsl_metadata::MdoType> {
    use bsl_metadata::MdoType;
    use hir_def::ty::MetadataKind;
    Some(match kind {
        MetadataKind::CatalogObject => MdoType::Catalog,
        MetadataKind::DocumentObject => MdoType::Document,
        MetadataKind::ExchangePlanObject => MdoType::ExchangePlan,
        MetadataKind::ChartOfAccountsObject => MdoType::ChartOfAccounts,
        MetadataKind::TaskObject => MdoType::Task,
        MetadataKind::BusinessProcessObject => MdoType::BusinessProcess,
        MetadataKind::DataProcessorObject => MdoType::DataProcessor,
        MetadataKind::ReportObject => MdoType::Report,
        MetadataKind::ChartOfCharacteristicTypesObject => MdoType::ChartOfCharacteristicTypes,
        MetadataKind::ChartOfCalculationTypesObject => MdoType::ChartOfCalculationTypes,
        MetadataKind::CatalogRef
        | MetadataKind::DocumentRef
        | MetadataKind::EnumRef
        | MetadataKind::TaskRef
        | MetadataKind::BusinessProcessRef
        | MetadataKind::ExchangePlanRef
        | MetadataKind::ChartOfAccountsRef
        | MetadataKind::ChartOfCharacteristicTypesRef
        | MetadataKind::ChartOfCalculationTypesRef
        | MetadataKind::InformationRegisterRef
        | MetadataKind::AccumulationRegisterRef
        | MetadataKind::AccountingRegisterRef
        | MetadataKind::CalculationRegisterRef => return None,
        MetadataKind::InformationRegisterRecordManager
        | MetadataKind::InformationRegisterRecordSet
        | MetadataKind::InformationRegisterRecord
        | MetadataKind::AccumulationRegisterRecordSet
        | MetadataKind::AccumulationRegisterRecord
        | MetadataKind::AccountingRegisterRecordSet
        | MetadataKind::AccountingRegisterRecord
        | MetadataKind::CalculationRegisterRecordSet
        | MetadataKind::CalculationRegisterRecord => return None,
        MetadataKind::RegisterDimension { .. }
        | MetadataKind::RegisterResource { .. }
        | MetadataKind::RegisterAttribute { .. }
        | MetadataKind::RegisterFilter { .. }
        | MetadataKind::TabularSection { .. }
        | MetadataKind::TabularSectionRow { .. } => return None,
    })
}

fn record_set_kind_to_mdo(kind: hir_def::ty::MetadataKind) -> Option<bsl_metadata::MdoType> {
    use bsl_metadata::MdoType;
    use hir_def::ty::MetadataKind;
    Some(match kind {
        MetadataKind::InformationRegisterRecordSet => MdoType::InformationRegister,
        MetadataKind::AccumulationRegisterRecordSet => MdoType::AccumulationRegister,
        MetadataKind::AccountingRegisterRecordSet => MdoType::AccountingRegister,
        MetadataKind::CalculationRegisterRecordSet => MdoType::CalculationRegister,
        MetadataKind::InformationRegisterRecordManager => return None,
        MetadataKind::CatalogObject
        | MetadataKind::DocumentObject
        | MetadataKind::ExchangePlanObject
        | MetadataKind::ChartOfAccountsObject
        | MetadataKind::TaskObject
        | MetadataKind::BusinessProcessObject
        | MetadataKind::DataProcessorObject
        | MetadataKind::ChartOfCharacteristicTypesObject
        | MetadataKind::ChartOfCalculationTypesObject
        | MetadataKind::ReportObject => return None,
        MetadataKind::CatalogRef
        | MetadataKind::DocumentRef
        | MetadataKind::EnumRef
        | MetadataKind::TaskRef
        | MetadataKind::BusinessProcessRef
        | MetadataKind::ExchangePlanRef
        | MetadataKind::ChartOfAccountsRef
        | MetadataKind::ChartOfCharacteristicTypesRef
        | MetadataKind::ChartOfCalculationTypesRef
        | MetadataKind::InformationRegisterRef
        | MetadataKind::AccumulationRegisterRef
        | MetadataKind::AccountingRegisterRef
        | MetadataKind::CalculationRegisterRef => return None,
        MetadataKind::InformationRegisterRecord
        | MetadataKind::AccumulationRegisterRecord
        | MetadataKind::AccountingRegisterRecord
        | MetadataKind::CalculationRegisterRecord => return None,
        MetadataKind::RegisterDimension { .. }
        | MetadataKind::RegisterResource { .. }
        | MetadataKind::RegisterAttribute { .. }
        | MetadataKind::RegisterFilter { .. }
        | MetadataKind::TabularSection { .. }
        | MetadataKind::TabularSectionRow { .. } => return None,
    })
}

pub fn resolve_record_set_module_call(
    db: &dyn HirDatabase,
    kind: hir_def::ty::MetadataKind,
    mdo_name: &Name,
    method_name: &Name,
    resolver: &Resolver,
) -> Result<MethodResolution, UnresolvedMethodKind> {
    let mdo_type = record_set_kind_to_mdo(kind).ok_or(UnresolvedMethodKind::MethodNotFound)?;

    let resolution = resolver
        .resolve_record_set_module_method(db, mdo_type, mdo_name, method_name)
        .map_err(|e| match e {
            QualifiedMethodError::NotVisibleInConfigs | QualifiedMethodError::NotFound => {
                UnresolvedMethodKind::MethodNotFound
            }
            QualifiedMethodError::BodyUnread => UnresolvedMethodKind::BodyUnread,
        })?;

    let symbol_tree = db.symbol_tree_ref(resolution.method_id.module);
    let method_symbol = symbol_tree.find_method_by_id(resolution.method_id).expect(
        "method_id returned by Resolver must exist in symbol_tree — \
         symbol_tree / Resolver are out of sync",
    );

    let signature = materialise_signature_enriched(db, resolution.method_id, method_symbol);
    Ok(MethodResolution::new(resolution.method_id, resolution.is_export, signature))
}

pub fn resolve_object_module_call(
    db: &dyn HirDatabase,
    kind: hir_def::ty::MetadataKind,
    mdo_name: &Name,
    method_name: &Name,
    resolver: &Resolver,
) -> Result<MethodResolution, UnresolvedMethodKind> {
    let mdo_type = object_kind_to_mdo(kind).ok_or(UnresolvedMethodKind::MethodNotFound)?;

    let resolution = resolver
        .resolve_object_module_method(db, mdo_type, mdo_name, method_name)
        .map_err(|e| match e {
            QualifiedMethodError::NotVisibleInConfigs | QualifiedMethodError::NotFound => {
                UnresolvedMethodKind::MethodNotFound
            }
            QualifiedMethodError::BodyUnread => UnresolvedMethodKind::BodyUnread,
        })?;

    let symbol_tree = db.symbol_tree_ref(resolution.method_id.module);
    let method_symbol = symbol_tree.find_method_by_id(resolution.method_id).expect(
        "method_id returned by Resolver must exist in symbol_tree — \
         symbol_tree / Resolver are out of sync",
    );

    let signature = materialise_signature_enriched(db, resolution.method_id, method_symbol);
    Ok(MethodResolution::new(resolution.method_id, resolution.is_export, signature))
}

pub fn resolve_aliased_manager_call(
    db: &dyn HirDatabase,
    mdo_type: bsl_metadata::MdoType,
    mdo_name: &Name,
    method_name: &Name,
    resolver: &Resolver,
) -> Result<MethodResolution, UnresolvedMethodKind> {
    let resolution = resolver
        .resolve_aliased_manager_method(db, mdo_type, mdo_name, method_name)
        .map_err(|e| match e {
        QualifiedMethodError::NotVisibleInConfigs | QualifiedMethodError::NotFound => {
            UnresolvedMethodKind::MethodNotFound
        }
        QualifiedMethodError::BodyUnread => UnresolvedMethodKind::BodyUnread,
    })?;

    let symbol_tree = db.symbol_tree_ref(resolution.method_id.module);
    let method_symbol = symbol_tree.find_method_by_id(resolution.method_id).expect(
        "method_id returned by Resolver must exist in symbol_tree — \
         symbol_tree / Resolver are out of sync",
    );

    let signature = materialise_signature_enriched(db, resolution.method_id, method_symbol);
    Ok(MethodResolution::new(resolution.method_id, resolution.is_export, signature))
}

pub(crate) fn materialise_signature(
    db: &dyn TypeKernelDb,
    method_symbol: &MethodSymbol,
) -> FunctionSignature {
    let ctx = TyLoweringContext::new();
    // Absent documentation is the common case, and then nothing below changes a single type.
    let docs = method_symbol.docs.as_deref();

    let params: Box<[TypeId]> = method_symbol
        .params
        .iter()
        .map(|p| {
            let base =
                p.type_ref.as_ref().map(|t| ctx.lower_type_ref_id(db, t)).unwrap_or(db.unknown());
            let documented = docs
                .and_then(|docs| find_param_doc(docs, &p.name))
                .map(|param_doc| param_doc.types.as_slice());
            enrich_with_documented_structure(db, base, documented)
        })
        .collect();
    let defaults: Box<[bool]> = method_symbol.params.iter().map(|p| p.has_default).collect();

    let ret = method_symbol
        .return_type_ref
        .as_ref()
        .map(|t| ctx.lower_type_ref_id(db, t))
        .unwrap_or_else(|| if method_symbol.is_function { db.unknown() } else { db.undefined() });
    let ret =
        enrich_with_documented_structure(db, ret, docs.map(|docs| docs.returned_value.as_slice()));

    let max_args = Some(params.len() as u32);
    FunctionSignature { params, defaults, ret, max_args, from_doc_comment: true }
}

/// Puts the fields a doc-comment declares into the slot's already lowered type.
///
/// Documentation is advisory here: it may only fill in the fields of a structure the declared type
/// already carries. When the slot declares something else, or the documentation names no inline
/// structure, the lowered type is returned untouched.
fn enrich_with_documented_structure(
    db: &dyn TypeKernelDb,
    base: TypeId,
    documented: Option<&[hir_def::docs::TypeDoc]>,
) -> TypeId {
    let Some(documented) = documented else {
        return base;
    };
    let mut enriched = base;
    for type_doc in documented {
        let Some(expr) = hir_def::docs::parse_type_expr(type_doc) else {
            continue;
        };
        if let Some(documented) = crate::lower::doc_structure::doc_structure_ty(db, &expr) {
            enriched = crate::lower::doc_structure::substitute(db, enriched, &documented);
        }
    }
    enriched
}

/// Matched by name without regard to case, the same way the syntactic parameter hints are.
fn find_param_doc<'a>(
    docs: &'a hir_def::docs::MethodDocs,
    name: &hir_def::Name,
) -> Option<&'a hir_def::docs::ParameterDoc> {
    use stdx::case::CaseExt;
    let needle = name.as_str().fold_lower();
    docs.parameters.iter().find(|param| param.name.fold_lower() == needle)
}

pub(crate) fn materialise_signature_enriched(
    db: &dyn HirDatabase,
    method_id: hir_def::MethodId,
    method_symbol: &MethodSymbol,
) -> FunctionSignature {
    let mut sig: FunctionSignature = materialise_signature(db, method_symbol);
    // A slot documented as a bare `Структура` says nothing the body does not already say, and the
    // keys the body proves are the only thing anyone can use. Reading the body for it keeps the
    // rule that documentation adds and never removes.
    let unknown = sig.ret == db.unknown();
    if unknown || is_fieldless_structure(db, sig.ret) {
        let method_input = hir_def::MethodIdInput::new(db, method_id);
        let inferred = crate::method_graph::method_return_type_query(db, method_input);
        if unknown {
            if inferred != db.unknown() {
                sig.ret = inferred;
            }
        } else if has_structure_fields(db, inferred) {
            sig.ret = inferred;
        }
    }

    sig
}

fn is_fieldless_structure(db: &dyn TypeKernelDb, ty: TypeId) -> bool {
    matches!(db.lookup_type(ty), bsl_types::kind::TypeKind::Structure(facet) if facet.fields.is_none())
}

fn has_structure_fields(db: &dyn TypeKernelDb, ty: TypeId) -> bool {
    matches!(db.lookup_type(ty), bsl_types::kind::TypeKind::Structure(facet) if facet.fields.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_types::testing::InMemoryDb;

    #[test]
    fn test_method_resolution_new() {
        let db = InMemoryDb::new();
        let method_id = MethodId { module: hir_def::ModuleId { file_id: FileId(0) }, local_id: 0 };
        let signature = FunctionSignature {
            params: Box::new([db.string(None, false)]),
            defaults: Box::new([false]),
            ret: db.number(None, None),
            max_args: Some(1),
            from_doc_comment: true,
        };

        let resolution = MethodResolution::new(method_id, true, signature.clone());

        assert_eq!(resolution.method_id, method_id);
        assert!(resolution.is_export);
        assert_eq!(resolution.return_type, db.number(None, None));
        assert_eq!(resolution.signature, signature);
    }

    #[test]
    fn test_method_resolution_not_export() {
        let db = InMemoryDb::new();
        let method_id = MethodId { module: hir_def::ModuleId { file_id: FileId(0) }, local_id: 0 };
        let signature = FunctionSignature {
            params: Box::new([]),
            defaults: Box::new([]),
            ret: db.undefined(),
            max_args: Some(0),
            from_doc_comment: true,
        };

        let resolution = MethodResolution::new(method_id, false, signature);

        assert!(!resolution.is_export);
    }

    #[test]
    fn object_kind_to_mdo_accepts_only_object_variants() {
        use bsl_metadata::MdoType;
        use hir_def::ty::MetadataKind;
        assert_eq!(object_kind_to_mdo(MetadataKind::CatalogObject), Some(MdoType::Catalog));
        assert_eq!(object_kind_to_mdo(MetadataKind::DocumentObject), Some(MdoType::Document));
        assert_eq!(
            object_kind_to_mdo(MetadataKind::ExchangePlanObject),
            Some(MdoType::ExchangePlan),
        );
        assert_eq!(
            object_kind_to_mdo(MetadataKind::ChartOfAccountsObject),
            Some(MdoType::ChartOfAccounts),
        );
        assert_eq!(object_kind_to_mdo(MetadataKind::CatalogRef), None);
        assert_eq!(object_kind_to_mdo(MetadataKind::DocumentRef), None);
        assert_eq!(object_kind_to_mdo(MetadataKind::InformationRegisterRecordManager), None);
        assert_eq!(object_kind_to_mdo(MetadataKind::AccumulationRegisterRecordSet), None);
    }

    #[test]
    fn record_set_kind_to_mdo_accepts_only_register_set_variants() {
        use bsl_metadata::MdoType;
        use hir_def::ty::MetadataKind;
        assert_eq!(
            record_set_kind_to_mdo(MetadataKind::InformationRegisterRecordSet),
            Some(MdoType::InformationRegister),
        );
        assert_eq!(
            record_set_kind_to_mdo(MetadataKind::AccumulationRegisterRecordSet),
            Some(MdoType::AccumulationRegister),
        );
        assert_eq!(
            record_set_kind_to_mdo(MetadataKind::AccountingRegisterRecordSet),
            Some(MdoType::AccountingRegister),
        );
        assert_eq!(
            record_set_kind_to_mdo(MetadataKind::CalculationRegisterRecordSet),
            Some(MdoType::CalculationRegister),
        );
        assert_eq!(record_set_kind_to_mdo(MetadataKind::InformationRegisterRecordManager), None);
        assert_eq!(
            record_set_kind_to_mdo(MetadataKind::RegisterFilter {
                parent: MdoType::InformationRegister,
            }),
            None,
        );
        assert_eq!(record_set_kind_to_mdo(MetadataKind::CatalogObject), None);
        assert_eq!(record_set_kind_to_mdo(MetadataKind::DocumentObject), None);
        assert_eq!(record_set_kind_to_mdo(MetadataKind::CatalogRef), None);
        assert_eq!(record_set_kind_to_mdo(MetadataKind::InformationRegisterRef), None);
        assert_eq!(record_set_kind_to_mdo(MetadataKind::AccumulationRegisterRef), None);
        assert_eq!(record_set_kind_to_mdo(MetadataKind::AccountingRegisterRef), None);
        assert_eq!(record_set_kind_to_mdo(MetadataKind::CalculationRegisterRef), None);
    }
}
