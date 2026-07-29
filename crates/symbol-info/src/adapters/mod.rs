use ide_db::{base_db::Locale, RootDatabase};
use smol_str::SmolStr;
use vfs::FileId;

use crate::domain::{CalleeKind, SymbolSignature};

mod common_module;
pub mod global_function;
mod local_method;
mod manager_module;
mod mdo_naming;
mod params;
mod platform_constructor;
mod platform_manager;
pub mod platform_method;

pub use global_function::from_global_function;
pub use platform_method::from_platform_method;

pub fn build_signature(
    db: &dyn RootDatabase,
    file_id: FileId,
    callee: &CalleeKind,
) -> Option<Vec<SymbolSignature>> {
    match callee {
        CalleeKind::PlatformMethod { type_name, method_name } => {
            platform_method::build(db, type_name, method_name)
        }
        CalleeKind::GlobalFunction { name } => global_function::build(db, name),
        CalleeKind::CommonModuleMethod { module, method } => {
            common_module::build(db, file_id, module, method)
        }
        CalleeKind::ManagerModuleMethod { mdo_type, object, method } => {
            manager_module::build(db, file_id, *mdo_type, object, method)
        }
        CalleeKind::PlatformManagerMethod { mdo_type, method } => {
            platform_manager::build(*mdo_type, method)
        }
        CalleeKind::LocalMethod { module_id, method } => {
            local_method::build(db, *module_id, method)
        }
        CalleeKind::PlatformConstructor { type_name } => platform_constructor::build(db, type_name),
    }
}

pub fn build_signature_from_resolution(
    db: &dyn RootDatabase,
    binding: &hir::CandidateCallBinding,
) -> Option<Vec<SymbolSignature>> {
    let signatures = binding
        .candidates
        .as_slice()
        .iter()
        .filter_map(|candidate| build_signature_from_candidate(db, candidate))
        .collect::<Vec<_>>();
    (!signatures.is_empty()).then_some(signatures)
}

/// The index in `signatures` of the candidate inference actually selected.
///
/// `None` when the call is ambiguous or rejected, or the selected candidate has
/// no rendered signature — every consumer of a resolved call needs the same
/// answer, so the mapping from `CallSelection` to a rendered signature lives
/// here rather than in each IDE feature.
pub fn selected_signature_index(
    binding: &hir::CandidateCallBinding,
    signatures: &[SymbolSignature],
) -> Option<usize> {
    match binding.resolution.selection {
        hir::CallSelection::Unique { candidate } => match candidate {
            hir::CandidateId::Platform {
                method_id,
                signature: hir::PlatformSignatureSlot::Base,
            } => signatures.iter().position(|signature| {
                signature.platform_id == Some(method_id) && signature.candidate_ordinal.is_none()
            }),
            hir::CandidateId::Platform {
                method_id,
                signature: hir::PlatformSignatureSlot::Variant(candidate_ordinal),
            } => signatures.iter().position(|signature| {
                signature.platform_id == Some(method_id)
                    && signature.candidate_ordinal == Some(candidate_ordinal)
            }),
            hir::CandidateId::User { signature_ordinal: candidate_ordinal, .. }
            | hir::CandidateId::Builtin { signature_ordinal: candidate_ordinal, .. } => signatures
                .iter()
                .position(|signature| signature.candidate_ordinal == Some(candidate_ordinal)),
            hir::CandidateId::FunctionValue => None,
        },
        hir::CallSelection::Ambiguous { .. } | hir::CallSelection::Rejected(_) => None,
    }
}

fn build_signature_from_candidate(
    db: &dyn RootDatabase,
    candidate: &hir::CallSignature,
) -> Option<SymbolSignature> {
    match candidate.provenance {
        hir::CandidateProvenance::PlatformMethod { method_id, signature } => {
            let data = bsl_platform::PlatformDataInner::instance();
            if let Some(projected) = constructor_signature(db, candidate) {
                return Some(projected);
            }
            let method = data.all_methods().iter().find(|method| method.id == method_id);
            match method {
                Some(method) => {
                    let docs = data.get_method_docs(method_id);
                    from_platform_method(method, docs.as_ref()).into_iter().find(|projected| {
                        projected.platform_id == Some(method_id)
                            && projected.candidate_ordinal == platform_ordinal(signature)
                    })
                }
                None => None,
            }
        }
        hir::CandidateProvenance::Builtin(hir::BuiltinCallableId::PlatformGlobal(id)) => {
            let data = bsl_platform::PlatformDataInner::instance();
            let function = data.all_global_functions().iter().find(|function| function.id == id)?;
            let docs = data.get_global_function_docs(id);
            let ordinal = match candidate.id {
                hir::CandidateId::Builtin { signature_ordinal, .. }
                    if function.variants.is_empty() && signature_ordinal == 0 =>
                {
                    None
                }
                hir::CandidateId::Builtin { signature_ordinal, .. } => Some(signature_ordinal),
                hir::CandidateId::Platform { .. }
                | hir::CandidateId::User { .. }
                | hir::CandidateId::FunctionValue => return None,
            };
            from_global_function(function, docs.as_ref())
                .into_iter()
                .find(|projected| projected.candidate_ordinal == ordinal)
        }
        hir::CandidateProvenance::Builtin(hir::BuiltinCallableId::Intrinsic(id)) => {
            let callable = hir::BuiltinCallableId::Intrinsic(id);
            let name = hir::builtin_functions().canonical_name(callable)?;
            Some(minimal_signature(db, candidate, name, candidate_ordinal(candidate)))
        }
        hir::CandidateProvenance::UserMethod(method) => {
            let mut projected = local_method::build_from_method_id(db, method.into())?;
            projected.candidate_ordinal = candidate_ordinal(candidate);
            Some(projected)
        }
        hir::CandidateProvenance::FunctionValue => {
            Some(minimal_signature(db, candidate, "Функция", None))
        }
    }
}

fn constructor_signature(
    db: &dyn RootDatabase,
    candidate: &hir::CallSignature,
) -> Option<SymbolSignature> {
    if matches!(db.lookup_type(candidate.return_ty), hir::TypeKind::Undefined) {
        return None;
    }
    let hir::CandidateId::Platform { method_id: id, .. } = candidate.id else {
        return None;
    };
    let data = bsl_platform::PlatformDataInner::instance();
    let constructor = data.all_constructors().iter().find(|constructor| {
        constructor.id == id
            && constructor.parameters.len() == candidate.params.len()
            && constructor
                .parameters
                .iter()
                .zip(candidate.params.iter())
                .all(|(parameter, candidate)| parameter.name == candidate.name)
    })?;
    let type_name = data
        .get_type(&constructor.type_name)
        .map(|platform_type| platform_type.name.as_str())
        .unwrap_or(constructor.type_name.as_str());
    platform_constructor::build(db, type_name)?
        .into_iter()
        .find(|projected| projected.platform_id == Some(id))
}

fn minimal_signature(
    db: &dyn RootDatabase,
    candidate: &hir::CallSignature,
    name: &str,
    candidate_ordinal: Option<usize>,
) -> SymbolSignature {
    let params = candidate
        .params
        .iter()
        .map(|param| crate::domain::SignatureParam {
            name: param.name.clone(),
            types: vec![crate::domain::TypeRef {
                russian: SmolStr::new(hir::kernel_type_label(db, param.ty, Locale::Ru, false)),
                english: None,
                description: None,
                is_hyperlink: false,
            }],
            is_optional: param.has_default,
            default_value: None,
            description: None,
            is_val: false,
        })
        .collect();
    SymbolSignature {
        candidate_ordinal,
        kind: crate::domain::MethodKind::Function,
        name_russian: SmolStr::new(name),
        name_english: None,
        qualifier: None,
        prefix: None,
        params,
        returns: vec![crate::domain::TypeRef {
            russian: SmolStr::new(hir::kernel_type_label(
                db,
                candidate.return_ty,
                Locale::Ru,
                false,
            )),
            english: None,
            description: None,
            is_hyperlink: false,
        }],
        purpose: None,
        description: None,
        examples: Vec::new(),
        notes: None,
        deprecation: None,
        is_export: true,
        source: crate::domain::SignatureSource::GlobalFunction,
        method_id: None,
        platform_id: None,
    }
}

fn candidate_ordinal(candidate: &hir::CallSignature) -> Option<usize> {
    match candidate.id {
        hir::CandidateId::Platform { signature, .. } => platform_ordinal(signature),
        hir::CandidateId::User { signature_ordinal, .. }
        | hir::CandidateId::Builtin { signature_ordinal, .. } => Some(signature_ordinal),
        hir::CandidateId::FunctionValue => None,
    }
}

fn platform_ordinal(signature: hir::PlatformSignatureSlot) -> Option<usize> {
    match signature {
        hir::PlatformSignatureSlot::Base => None,
        hir::PlatformSignatureSlot::Variant(ordinal) => Some(ordinal),
    }
}
