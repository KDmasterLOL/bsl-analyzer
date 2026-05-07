//! Signature lowering for workspace-defined methods.
//!
//! Salsa-tracked query that derives a `(params, return_ty)` signature
//! for a workspace-defined procedure / function from its docstring.
//! Slot defaults to `Ty::Unknown` (gradual typing) for anything the
//! docstring does not declare.
//!
//! # Scope of this slice
//!
//! Phase 1 — docstring only. The query reads `db.method_docs(method)`
//! and pipes the parsed `ParameterDoc` / `TypeDoc` through the unified
//! [`crate::lower::type_string`] pipeline. Cycle risk is zero today
//! because the query never consults `db.infer` or any other tracked
//! query that reads back into `lookup_method` / `proc_signature_query`.
//!
//! Phase 2 (a follow-up slice) widens the algorithm with a `Body`-walk
//! fallback for return-from-body inference, at which point a
//! `salsa::cycle_fn` returning `ProcSignature { params: <doc-derived>,
//! return_ty: Unknown }` becomes mandatory — recursive `A → B → A`
//! callers would otherwise drive the solver into an unbounded
//! `infer ↔ lookup_method ↔ proc_signature_query` loop. See plan
//! §2.4 / risk 9.

use std::sync::Arc;

use hir_def::docs::{MethodDocs, ParameterDoc};
use hir_def::symbol_tree::ParamSymbol;
use hir_def::{MethodIdInput, Name};

use crate::db::HirDatabase;
use crate::lower::type_string::{lower_param_type_string, lower_return_type_string};
use crate::Ty;

/// Lowered signature of a workspace-defined procedure / function.
///
/// Mirrors the `(params, return_ty)` half of [`crate::method_lookup::MethodInfo`]
/// so the [`crate::proc_signature_lookup`] adapter (added in a follow-up
/// slice) can hand workspace methods to the same call-arg checking path
/// that platform methods use today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcSignature {
    /// Parameter types, in declaration order. Slots that the docstring
    /// omits stay `Ty::Unknown` so call-site `is_assignable` accepts any
    /// actual via gradual typing.
    pub params: Vec<Ty>,
    /// Return type. `Ty::Unknown` when the docstring omits the
    /// `Возвращаемое значение:` section. Phase 2 will walk the body for
    /// `Возврат X` expressions when the docstring is silent.
    pub return_ty: Ty,
}

/// Salsa-tracked query: lower a workspace method's signature.
///
/// Iterates the **declaration-order** parameter list from the method's
/// `MethodSymbol` and overlays a docstring `ParameterDoc` whose `name`
/// matches case-insensitively. Slots without a matching doc entry stay
/// `Ty::Unknown` (gradual). The return type comes from the docstring
/// `returned_value`; absent → `Ty::Unknown`.
///
/// Driving from the declaration rather than the docstring is what
/// guarantees the resulting `params.len() == method_symbol.params.len()`
/// — out-of-order or missing doc entries never shift slots, never
/// truncate or extend the signature.
#[salsa::tracked]
pub fn proc_signature_query<'db>(
    db: &'db dyn HirDatabase,
    method_input: MethodIdInput<'db>,
) -> Arc<ProcSignature> {
    let method_id = method_input.method_id(db);

    let symbol_tree = db.symbol_tree(method_id.module);
    let Some(method_symbol) = symbol_tree.find_method_by_id(method_id) else {
        return Arc::new(ProcSignature { params: Vec::new(), return_ty: Ty::Unknown });
    };

    let docs = method_symbol.docs.as_deref();
    let params = lower_params(method_symbol.params.as_slice(), docs);
    let return_ty = docs.map(lower_return_from_docs).unwrap_or(Ty::Unknown);

    Arc::new(ProcSignature { params, return_ty })
}

fn lower_params(decl_params: &[ParamSymbol], docs: Option<&MethodDocs>) -> Vec<Ty> {
    decl_params
        .iter()
        .map(|p| match docs.and_then(|d| find_param_doc(d, &p.name)) {
            Some(param_doc) => lower_one_param_doc(param_doc),
            None => Ty::Unknown,
        })
        .collect()
}

/// Look up a docstring parameter entry by name, BSL-case-insensitive.
fn find_param_doc<'a>(docs: &'a MethodDocs, name: &Name) -> Option<&'a ParameterDoc> {
    let needle = name.as_str().to_lowercase();
    docs.parameters.iter().find(|p| p.name.to_lowercase() == needle)
}

fn lower_one_param_doc(param: &ParameterDoc) -> Ty {
    if param.types.is_empty() {
        return Ty::Unknown;
    }
    if param.types.len() == 1 {
        return lower_param_type_string(&param.types[0].name);
    }
    // Multiple `TypeDoc` entries are already a parsed union per the
    // doc grammar. Re-joining with `, ` and routing through the
    // unified pipeline keeps the gradual-typing rules
    // (single-unrecognised → Unknown, multi all-valid → Union) and
    // the `Произвольный` collapse in one place rather than
    // re-implementing them here.
    let joined = param.types.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ");
    lower_param_type_string(&joined)
}

fn lower_return_from_docs(docs: &MethodDocs) -> Ty {
    if docs.returned_value.is_empty() {
        return Ty::Unknown;
    }
    let joined = docs.returned_value.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ");
    lower_return_type_string(&joined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hir_def::docs::TypeDoc;

    fn typedoc(name: &str) -> TypeDoc {
        TypeDoc::simple(name.to_string(), None)
    }

    fn param_doc(name: &str, types: Vec<TypeDoc>) -> ParameterDoc {
        ParameterDoc::new(name.to_string(), types)
    }

    fn decl_param(name: &str) -> ParamSymbol {
        ParamSymbol {
            name: Name::new(name),
            is_val: false,
            has_default: false,
            ty: hir_def::ty::Ty::Unknown,
            type_ref: None,
        }
    }

    fn docs(parameters: Vec<ParameterDoc>, returned_value: Vec<TypeDoc>) -> MethodDocs {
        MethodDocs {
            raw: String::new(),
            purpose: None,
            parameters,
            returned_value,
            examples: Vec::new(),
            call_options: Vec::new(),
            deprecation: None,
            link: None,
        }
    }

    #[test]
    fn no_decl_params_yields_empty_signature() {
        let d = docs(Vec::new(), Vec::new());
        assert_eq!(lower_params(&[], Some(&d)), Vec::<Ty>::new());
    }

    #[test]
    fn missing_doc_for_decl_param_stays_unknown() {
        // Headline contract: declaration drives arity. The doc has nothing
        // for `Б`, so slot index 1 must be `Ty::Unknown`, not collapse the
        // signature.
        let d = docs(vec![param_doc("А", vec![typedoc("Число")])], Vec::new());
        let params = lower_params(&[decl_param("А"), decl_param("Б")], Some(&d));
        assert_eq!(params, vec![Ty::Number, Ty::Unknown]);
    }

    #[test]
    fn out_of_order_doc_matches_by_name_not_position() {
        let d = docs(
            vec![param_doc("Б", vec![typedoc("Строка")]), param_doc("А", vec![typedoc("Число")])],
            Vec::new(),
        );
        let params = lower_params(&[decl_param("А"), decl_param("Б")], Some(&d));
        assert_eq!(params, vec![Ty::Number, Ty::String]);
    }

    #[test]
    fn name_match_is_case_insensitive() {
        // BSL identifiers are case-insensitive; the matcher follows.
        let d = docs(vec![param_doc("ПАРАМЕТР", vec![typedoc("Число")])], Vec::new());
        let params = lower_params(&[decl_param("параметр")], Some(&d));
        assert_eq!(params, vec![Ty::Number]);
    }

    #[test]
    fn no_docs_at_all_keeps_every_slot_unknown() {
        let params = lower_params(&[decl_param("А"), decl_param("Б")], None);
        assert_eq!(params, vec![Ty::Unknown, Ty::Unknown]);
    }

    #[test]
    fn single_unrecognised_param_stays_unknown_for_gradual_typing() {
        // Same asymmetry as the platform-method path.
        let d = docs(vec![param_doc("X", vec![typedoc("СтрокаТабличнойЧасти")])], Vec::new());
        let params = lower_params(&[decl_param("X")], Some(&d));
        assert_eq!(params, vec![Ty::Unknown]);
    }

    #[test]
    fn multi_typedoc_param_joins_into_union() {
        let d = docs(vec![param_doc("X", vec![typedoc("Число"), typedoc("Строка")])], Vec::new());
        let params = lower_params(&[decl_param("X")], Some(&d));
        assert_eq!(params, vec![Ty::union(vec![Ty::Number, Ty::String])]);
    }

    #[test]
    fn return_section_lowers_through_return_pipeline() {
        let d = docs(Vec::new(), vec![typedoc("Булево"), typedoc("Неопределено")]);
        assert_eq!(lower_return_from_docs(&d), Ty::union(vec![Ty::Boolean, Ty::Undefined]),);
    }

    #[test]
    fn return_arbitrary_collapses_to_unknown() {
        let d = docs(Vec::new(), vec![typedoc("Произвольный"), typedoc("Неопределено")]);
        assert_eq!(lower_return_from_docs(&d), Ty::Unknown);
    }
}
