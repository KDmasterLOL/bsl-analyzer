use std::fmt;

use bsl_platform::{MethodParam, PlatformMethod};
use bsl_types::builders::Builders;
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;
use hir_def::execution_env::EnvFlags;
use smol_str::SmolStr;
use vfs::FileId;

use crate::lower::type_string::lower_param_type_string_typeid;

mod applicability;
mod resolution;

pub use applicability::{
    evaluate_applicability, evaluate_arity, ArgumentApplicability, ArgumentEvaluation,
    ArgumentIndeterminateReason, ArgumentParameter, ArityMismatch, ArityUsage,
    CandidateApplicability, CandidateEvaluation,
};
pub use resolution::{
    resolve_candidates, ArityFallback, CallRejection, CallResolution, CallSelection,
    CandidateDisposition, CandidateFact, CandidateRejection, CandidateScore,
};

/// Stable position of a platform signature within one method record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PlatformSignatureSlot {
    /// The method record's own signature.
    Base,
    /// A signature from `PlatformMethod::variants`, indexed in source order.
    Variant(usize),
}

/// Stable workspace method identity independent of filesystem paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UserMethodId {
    file_id: FileId,
    local_id: u32,
}

impl UserMethodId {
    /// Creates an identity from Salsa-stable file and local method IDs.
    pub const fn new(file_id: FileId, local_id: u32) -> Self {
        Self { file_id, local_id }
    }
}

impl From<hir_def::MethodId> for UserMethodId {
    fn from(method: hir_def::MethodId) -> Self {
        Self::new(method.module.file_id, method.local_id)
    }
}

/// Stable identity of a built-in callable's defining registry entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BuiltinCallableId {
    /// A global function from the platform database.
    PlatformGlobal(u32),
    /// A language intrinsic from the analyzer's numeric registry.
    Intrinsic(u32),
}

/// Semantic identity of one callable signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CandidateId {
    Platform { method_id: u32, signature: PlatformSignatureSlot },
    User { method: UserMethodId, signature_ordinal: usize },
    Builtin { callable: BuiltinCallableId, signature_ordinal: usize },
}

/// Layer that supplied a call candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CandidateOrigin {
    Platform,
    User,
    Builtin,
}

/// Stable source record from which a candidate was lowered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CandidateProvenance {
    PlatformMethod { method_id: u32, signature: PlatformSignatureSlot },
    UserMethod(UserMethodId),
    Builtin(BuiltinCallableId),
}

/// How a parameter contributes to the call's arity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallParamMode {
    Positional,
    Variadic,
}

/// One lowered call parameter with its source name and arity metadata.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CallParam {
    pub name: SmolStr,
    pub ty: TypeId,
    pub has_default: bool,
    pub mode: CallParamMode,
}

/// Complete, Salsa-safe signature metadata for one call candidate.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CallSignature {
    pub id: CandidateId,
    pub params: Box<[CallParam]>,
    pub required_args: usize,
    pub max_args: Option<usize>,
    pub return_ty: TypeId,
    pub origin: CandidateOrigin,
    pub environment: EnvFlags,
    pub provenance: CandidateProvenance,
    pub from_doc_comment: bool,
}

impl CallSignature {
    /// Returns the variadic element parameter when the source declares one.
    pub fn variadic_param(&self) -> Option<&CallParam> {
        self.params.iter().find(|param| param.mode == CallParamMode::Variadic)
    }
}

/// Deterministically ordered signatures with unique semantic identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallCandidateSet(Box<[CallSignature]>);

impl CallCandidateSet {
    /// Returns candidates in stable identity order.
    pub fn as_slice(&self) -> &[CallSignature] {
        &self.0
    }

    pub(crate) fn from_platform_method(
        db: &dyn TypeKernelDb,
        method: &PlatformMethod,
        return_ty: TypeId,
    ) -> Self {
        let context = PlatformCandidateContext {
            db,
            method,
            return_ty,
            environment: EnvFlags::from_platform_context(method.context.as_ref()),
        };
        let mut candidates = Vec::with_capacity(method.variants.len() + 1);
        candidates.push(context.lower(PlatformSignatureSlot::Base, &method.parameters));
        candidates.extend(method.variants.iter().enumerate().map(|(ordinal, variant)| {
            context.lower(PlatformSignatureSlot::Variant(ordinal), &variant.parameters)
        }));
        Self(candidates.into_boxed_slice())
    }
}

impl TryFrom<Vec<CallSignature>> for CallCandidateSet {
    type Error = DuplicateCandidateId;

    fn try_from(mut candidates: Vec<CallSignature>) -> Result<Self, Self::Error> {
        candidates.sort_by_key(|candidate| candidate.id);
        if let Some(duplicate) =
            candidates.windows(2).find(|pair| pair[0].id == pair[1].id).map(|pair| pair[0].id)
        {
            return Err(DuplicateCandidateId { id: duplicate });
        }
        Ok(Self(candidates.into_boxed_slice()))
    }
}

/// Error returned when two signatures claim the same semantic identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicateCandidateId {
    pub id: CandidateId,
}

impl fmt::Display for DuplicateCandidateId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "duplicate call candidate identity: {:?}", self.id)
    }
}

impl std::error::Error for DuplicateCandidateId {}

struct PlatformCandidateContext<'a> {
    db: &'a dyn TypeKernelDb,
    method: &'a PlatformMethod,
    return_ty: TypeId,
    environment: EnvFlags,
}

impl PlatformCandidateContext<'_> {
    fn lower(&self, signature: PlatformSignatureSlot, parameters: &[MethodParam]) -> CallSignature {
        let params: Box<[CallParam]> = parameters
            .iter()
            .map(|param| CallParam {
                name: param.name.clone(),
                ty: param
                    .param_type
                    .as_deref()
                    .map(|raw| lower_param_type_string_typeid(self.db, raw))
                    .unwrap_or_else(|| self.db.unknown()),
                has_default: param.is_optional,
                mode: if param.is_variadic {
                    CallParamMode::Variadic
                } else {
                    CallParamMode::Positional
                },
            })
            .collect();
        let required_args = params
            .iter()
            .rposition(|param| !param.has_default && param.mode == CallParamMode::Positional)
            .map_or(0, |index| index + 1);
        let max_args = params
            .iter()
            .all(|param| param.mode == CallParamMode::Positional)
            .then_some(params.len());
        let id = CandidateId::Platform { method_id: self.method.id, signature };
        CallSignature {
            id,
            params,
            required_args,
            max_args,
            return_ty: self.return_ty,
            origin: CandidateOrigin::Platform,
            environment: self.environment,
            provenance: CandidateProvenance::PlatformMethod {
                method_id: self.method.id,
                signature,
            },
            from_doc_comment: false,
        }
    }
}

#[cfg(test)]
mod tests;
