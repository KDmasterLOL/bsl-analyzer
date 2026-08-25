use bsl_metadata::MdoType;
use bsl_platform::{find_prefixed_methods, PlatformMethod};
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;
use hir_def::ty::{FunctionSignature, MetadataKind};
use hir_def::Name;

use crate::call_resolution::CallCandidateSet;

mod record_resolution;
mod return_mapping;

use record_resolution::{build_any_metadata_ref_method_record, build_method_record_resolution};
pub(crate) use return_mapping::{
    map_generic_metadata_return_type, map_generic_metadata_return_type_typeid,
    metadata_kind_to_prefix_and_mdo,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformMethodResolution {
    pub signature: FunctionSignature,
    pub return_ty: TypeId,
    pub overloads: Vec<Vec<TypeId>>,
    /// Execution environments the method is available in.
    pub env: hir_def::execution_env::EnvFlags,
    pub candidates: CallCandidateSet,
    pub records: Vec<PlatformMethodRecordResolution>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformMethodRecordResolution {
    pub method_id: u32,
    pub signature: FunctionSignature,
    pub return_ty: TypeId,
    pub overloads: Vec<Vec<TypeId>>,
    pub env: hir_def::execution_env::EnvFlags,
    pub candidates: CallCandidateSet,
}

pub fn resolve_platform_manager_method(
    db: &dyn TypeKernelDb,
    mdo_type: MdoType,
    mdo_name: &Name,
    method_name: &Name,
) -> Option<PlatformMethodResolution> {
    let prefix = mdo_type.manager_type_prefix()?;
    build_prefixed_resolution(
        db,
        find_prefixed_methods(prefix, method_name.as_str()),
        mdo_type,
        mdo_name,
    )
}

pub fn resolve_platform_metadata_ref_method(
    db: &dyn TypeKernelDb,
    kind: MetadataKind,
    mdo_name: &Name,
    method_name: &Name,
) -> Option<PlatformMethodResolution> {
    let (prefix, parent_mdo) = metadata_kind_to_prefix_and_mdo(kind)?;
    build_prefixed_resolution(
        db,
        find_prefixed_methods(prefix, method_name.as_str()),
        parent_mdo,
        mdo_name,
    )
}

pub fn platform_methods_for_metadata_kind(kind: MetadataKind) -> Vec<PlatformMethod> {
    metadata_kind_to_prefix_and_mdo(kind)
        .map(|(prefix, _)| {
            bsl_platform::PlatformData::instance()
                .get_manager_methods(prefix)
                .into_iter()
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

pub fn platform_methods_for_manager(mdo_type: MdoType) -> Vec<PlatformMethod> {
    mdo_type
        .manager_type_prefix()
        .map(|prefix| {
            bsl_platform::PlatformData::instance()
                .get_manager_methods(prefix)
                .into_iter()
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn build_resolution(
    db: &dyn TypeKernelDb,
    method: &PlatformMethod,
    mdo_type: MdoType,
    mdo_name: &Name,
) -> PlatformMethodResolution {
    build_method_record_resolution(db, method, mdo_type, mdo_name).into_resolution()
}

fn build_prefixed_resolution(
    db: &dyn TypeKernelDb,
    methods: Vec<PlatformMethod>,
    mdo_type: MdoType,
    mdo_name: &Name,
) -> Option<PlatformMethodResolution> {
    let records = methods
        .iter()
        .map(|method| build_method_record_resolution(db, method, mdo_type, mdo_name))
        .collect();
    PlatformMethodResolution::from_records(records)
}

pub fn resolve_platform_any_metadata_ref_method(
    db: &dyn TypeKernelDb,
    mdo_type: MdoType,
    method_name: &Name,
) -> Option<PlatformMethodResolution> {
    let ref_kind = MetadataKind::ref_kind_for(mdo_type)?;
    let (prefix, parent_mdo) = metadata_kind_to_prefix_and_mdo(ref_kind)?;
    let records = find_prefixed_methods(prefix, method_name.as_str())
        .iter()
        .map(|method| build_any_metadata_ref_method_record(db, method, parent_mdo))
        .collect();
    PlatformMethodResolution::from_records(records)
}

impl PlatformMethodRecordResolution {
    fn into_resolution(self) -> PlatformMethodResolution {
        PlatformMethodResolution {
            signature: self.signature.clone(),
            return_ty: self.return_ty,
            overloads: self.overloads.clone(),
            env: self.env,
            candidates: self.candidates.clone(),
            records: vec![self],
        }
    }
}

impl PlatformMethodResolution {
    fn from_records(records: Vec<PlatformMethodRecordResolution>) -> Option<Self> {
        let candidates = CallCandidateSet::try_from(
            records
                .iter()
                .flat_map(|record| record.candidates.as_slice().iter().cloned())
                .collect::<Vec<_>>(),
        )
        .ok()?;
        let primary = records.first()?.clone();
        let mut resolution = primary.into_resolution();
        resolution.candidates = candidates;
        resolution.records = records;
        Some(resolution)
    }
}

#[cfg(test)]
mod tests;
