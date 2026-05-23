//! Signature lowering for workspace-defined methods.
//!
//! Salsa-tracked query that derives a `(params, return_ty)` signature
//! for a workspace-defined procedure / function from its docstring.
//! Slot defaults to `Ty::Unknown` (gradual typing) for anything the
//! docstring does not declare.
//!
//! # Cycle status (Phase O.16b — body-walk dropped)
//!
//! Phase O.16b removed the docstring-less `Body`-walk fallback that
//! previously called `db.infer(file_id)` for return-from-body
//! inference. After O.16a turned `infer_query` into a thin fan-out
//! wrapper over `db.infer_method` (Lni.5 / O.15), keeping the
//! body-walk here would close the self-edge
//! `proc_signature_query → infer_query → infer_method →
//! proc_signature_query` (`infer_method`'s
//! `InferenceContext::infer_all` consults proc signatures during
//! qualified-call resolution).
//!
//! Dropping the body-walk to `Ty::Unknown` for docstring-less
//! functions is observably a no-op: the cascade-typing path
//! (`materialise_signature_enriched` in `method_resolution.rs`,
//! shipped Phase O.11) is the production consumer that wanted a
//! body-derived return type, and it queries
//! `method_return_type_query` directly — which has its own
//! cycle-safe handlers (Phase J / O.10). PLAN-v3 §R9 verified zero
//! production callers depended on `proc_signature_query`'s
//! body-walked return type before O.16b.
//!
//! `collect_return_value_exprs` is retained under `#[cfg(test)]`
//! because two unit tests still exercise its Stmt::Return walking
//! shape against hand-rolled bodies.

use std::sync::Arc;

use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;
use hir_def::docs::{MethodDocs, ParameterDoc};
use hir_def::symbol_tree::ParamSymbol;
use hir_def::{MethodIdInput, Name};

use crate::db::HirDatabase;
use crate::lower::type_string::{lower_param_type_string, lower_return_type_string};
use crate::ty_bridge::ty_to_typeid;
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
    /// `Возвращаемое значение:` section. Body-walked return-from-`Возврат`
    /// inference was dropped in Phase O.16b to break the
    /// `proc_signature_query → infer_query → infer_method →
    /// proc_signature_query` self-edge introduced by O.16a; cascade
    /// typing recovers the same precision via
    /// [`crate::method_graph::method_return_type_query`] at the call
    /// site.
    pub return_ty: Ty,
}

impl ProcSignature {
    /// Kernel-native projection of [`Self::return_ty`].
    ///
    /// §4.C accessor — bridges via §4.A `ty_to_typeid`.
    #[allow(dead_code, reason = "Phase 3 §4.C — consumers migrate in 4.D-4.E")]
    pub fn return_typeid(&self, db: &dyn TypeKernelDb) -> TypeId {
        ty_to_typeid(db, &self.return_ty)
    }

    /// Kernel-native projection of [`Self::params`].
    #[allow(dead_code, reason = "Phase 3 §4.C — consumers migrate in 4.D-4.E")]
    pub fn params_typeid(&self, db: &dyn TypeKernelDb) -> Vec<TypeId> {
        self.params.iter().map(|t| ty_to_typeid(db, t)).collect()
    }
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
#[salsa::tracked(lru = 1024)]
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

    // Procedures never carry a return type — match the platform-method
    // path (`return_type: None` → `Ty::Undefined`) so consumers can
    // use a single sentinel for "no return".
    let return_ty = if !method_symbol.is_function {
        Ty::Undefined
    } else if let Some(docs_return_ty) = docs.and_then(lower_return_from_docs) {
        // Docstring `Возвращаемое значение:` is present — its lowered
        // type wins, even when it lowers to `Ty::Unknown` (the user
        // wrote `Произвольный` and we honour that gradual claim
        // instead of second-guessing via body inference).
        docs_return_ty
    } else {
        // Phase O.16b: docstring-less return drops to `Ty::Unknown`.
        // The previous body-walk path (`db.infer(file_id)` →
        // `expr_types_by_body[Method(local_id)]` → union over
        // `Stmt::Return`s) was a self-edge through the O.16a
        // `infer_query` wrapper. Cascade typing recovers the same
        // precision at the call site via
        // [`crate::method_graph::method_return_type_query`].
        Ty::Unknown
    };

    Arc::new(ProcSignature { params, return_ty })
}

/// Walk a method body for every `Stmt::Return { value: Some(_) }`
/// and collect the value-bearing expression ids.
///
/// Retained under `#[cfg(test)]` after Phase O.16b dropped the
/// production caller; the local unit tests still exercise the
/// Stmt::Return walking shape against hand-rolled bodies.
#[cfg(test)]
fn collect_return_value_exprs(body: &hir_def::Body) -> Vec<hir_def::ExprId> {
    use hir_def::{ExprId, IdConversion, Stmt};
    body.stmts_iter()
        .filter_map(|(_, stmt)| match stmt {
            Stmt::Return { value: Some(expr_idx) } => Some(ExprId::from_idx(*expr_idx)),
            _ => None,
        })
        .collect()
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

/// Lower the docstring's `Возвращаемое значение:` section.
///
/// Returns `None` when the section is absent — that's the signal the
/// caller uses to fall through to body-walk inference. Returns
/// `Some(ty)` whenever the section is present, even when `ty` collapses
/// to `Ty::Unknown` (e.g., the user wrote `Произвольный`): an explicit
/// "any" claim is still the user's claim and wins over body inference.
fn lower_return_from_docs(docs: &MethodDocs) -> Option<Ty> {
    if docs.returned_value.is_empty() {
        return None;
    }
    let joined = docs.returned_value.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ");
    Some(lower_return_type_string(&joined))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty_bridge::typeid_to_ty;
    use bsl_types::testing::InMemoryDb;
    use hir_def::docs::TypeDoc;
    use hir_def::{Body, ExprId, IdConversion, Stmt};

    /// §4.C drift-detector: kernel-native accessor mirrors the Ty fields.
    #[test]
    fn proc_signature_typeid_round_trips_via_ty() {
        let db = InMemoryDb::new();
        let sig = ProcSignature { params: vec![Ty::Number, Ty::String], return_ty: Ty::Boolean };
        assert_eq!(typeid_to_ty(&db, sig.return_typeid(&db)), sig.return_ty);
        let pids = sig.params_typeid(&db);
        let pids_via_ty: Vec<Ty> = pids.iter().map(|id| typeid_to_ty(&db, *id)).collect();
        assert_eq!(pids_via_ty, sig.params);
    }

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
        assert_eq!(lower_return_from_docs(&d), Some(Ty::union(vec![Ty::Boolean, Ty::Undefined])),);
    }

    #[test]
    fn return_arbitrary_returns_some_unknown_not_none() {
        // Explicit `Произвольный` is the user's gradual claim and
        // collapses to `Ty::Unknown`, but the section IS present —
        // wrap in `Some` so the caller knows not to fall through to
        // body inference.
        let d = docs(Vec::new(), vec![typedoc("Произвольный"), typedoc("Неопределено")]);
        assert_eq!(lower_return_from_docs(&d), Some(Ty::Unknown));
    }

    #[test]
    fn return_section_absent_returns_none() {
        // No `Возвращаемое значение:` line in the docstring — the
        // caller falls through to body inference.
        let d = docs(Vec::new(), Vec::new());
        assert_eq!(lower_return_from_docs(&d), None);
    }

    #[test]
    fn collect_return_value_exprs_picks_value_bearing_returns() {
        use hir_def::{Expr, Literal};

        // Procedure-shape `Возврат;` (no value) is excluded; only
        // `Возврат <expr>;` contributes to the return-from-body union.
        let mut body = Body::default();
        let lit_true = body.exprs_mut().alloc(Expr::Literal(Literal::Bool(true)));
        let return_value = body.stmts_mut().alloc(Stmt::Return { value: Some(lit_true) });
        let return_void = body.stmts_mut().alloc(Stmt::Return { value: None });
        body.set_body_stmts(vec![return_value, return_void].into());

        let returns = collect_return_value_exprs(&body);
        assert_eq!(returns, vec![ExprId::from_idx(lit_true)]);
    }

    #[test]
    fn collect_return_value_exprs_returns_empty_for_procedure_body() {
        // Procedures with only `Возврат;` (no value) → empty vector,
        // so the caller falls back to `Ty::Unknown` via `Ty::union(vec![])`.
        let mut body = Body::default();
        let return_void = body.stmts_mut().alloc(Stmt::Return { value: None });
        body.set_body_stmts(vec![return_void].into());

        assert!(collect_return_value_exprs(&body).is_empty());
    }
}
