use std::sync::Arc;

use bsl_types::builders::Builders;
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;
use hir_def::docs::{MethodDocs, ParameterDoc};
use hir_def::symbol_tree::ParamSymbol;
use hir_def::{MethodIdInput, Name};

use crate::db::HirDatabase;
use crate::lower::type_string::{lower_param_type_string_typeid, lower_return_type_string_typeid};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcSignature {
    pub params: Vec<TypeId>,
    pub return_ty: TypeId,
}

// Cross-module call-resolution currency (declared params + return type, cheap):
// high cap keeps it across chunk-boundary LRU trims so dependent chunks reuse it.
#[salsa::tracked(lru = 262144)]
pub fn proc_signature_query<'db>(
    db: &'db dyn HirDatabase,
    method_input: MethodIdInput<'db>,
) -> Arc<ProcSignature> {
    let method_id = method_input.method_id(db);

    let symbol_tree = db.symbol_tree(method_id.module);
    let Some(method_symbol) = symbol_tree.find_method_by_id(method_id) else {
        return Arc::new(ProcSignature { params: Vec::new(), return_ty: db.unknown() });
    };

    let docs = method_symbol.docs.as_deref();
    let params = lower_params(db, method_symbol.params.as_slice(), docs);

    let return_ty = if !method_symbol.is_function {
        db.undefined()
    } else if let Some(docs_return_ty) = docs.and_then(|d| lower_return_from_docs(db, d)) {
        docs_return_ty
    } else {
        db.unknown()
    };

    Arc::new(ProcSignature { params, return_ty })
}

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

fn lower_params(
    db: &dyn TypeKernelDb,
    decl_params: &[ParamSymbol],
    docs: Option<&MethodDocs>,
) -> Vec<TypeId> {
    decl_params
        .iter()
        .map(|p| match docs.and_then(|d| find_param_doc(d, &p.name)) {
            Some(param_doc) => lower_one_param_doc(db, param_doc),
            None => db.unknown(),
        })
        .collect()
}

fn find_param_doc<'a>(docs: &'a MethodDocs, name: &Name) -> Option<&'a ParameterDoc> {
    let needle = name.as_str().to_lowercase();
    docs.parameters.iter().find(|p| p.name.to_lowercase() == needle)
}

fn lower_one_param_doc(db: &dyn TypeKernelDb, param: &ParameterDoc) -> TypeId {
    if param.types.is_empty() {
        return db.unknown();
    }
    if param.types.len() == 1 {
        return lower_param_type_string_typeid(db, &param.types[0].name);
    }
    let joined = param.types.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ");
    lower_param_type_string_typeid(db, &joined)
}

fn lower_return_from_docs(db: &dyn TypeKernelDb, docs: &MethodDocs) -> Option<TypeId> {
    if docs.returned_value.is_empty() {
        return None;
    }
    let joined = docs.returned_value.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ");
    Some(lower_return_type_string_typeid(db, &joined))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_types::testing::InMemoryDb;
    use hir_def::docs::TypeDoc;
    use hir_def::{Body, ExprId, IdConversion, Stmt};

    fn typedoc(name: &str) -> TypeDoc {
        TypeDoc::simple(name.to_string(), None)
    }

    fn param_doc(name: &str, types: Vec<TypeDoc>) -> ParameterDoc {
        ParameterDoc::new(name.to_string(), types)
    }

    fn decl_param(name: &str) -> ParamSymbol {
        ParamSymbol { name: Name::new(name), is_val: false, has_default: false, type_ref: None }
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
        let db = InMemoryDb::new();
        let d = docs(Vec::new(), Vec::new());
        assert!(lower_params(&db, &[], Some(&d)).is_empty());
    }

    #[test]
    fn missing_doc_for_decl_param_stays_unknown() {
        let db = InMemoryDb::new();
        let d = docs(vec![param_doc("А", vec![typedoc("Число")])], Vec::new());
        let params = lower_params(&db, &[decl_param("А"), decl_param("Б")], Some(&d));
        assert_eq!(params, vec![db.number(None, None), db.unknown()]);
    }

    #[test]
    fn out_of_order_doc_matches_by_name_not_position() {
        let db = InMemoryDb::new();
        let d = docs(
            vec![param_doc("Б", vec![typedoc("Строка")]), param_doc("А", vec![typedoc("Число")])],
            Vec::new(),
        );
        let params = lower_params(&db, &[decl_param("А"), decl_param("Б")], Some(&d));
        assert_eq!(params, vec![db.number(None, None), db.string(None, false)]);
    }

    #[test]
    fn name_match_is_case_insensitive() {
        let db = InMemoryDb::new();
        let d = docs(vec![param_doc("ПАРАМЕТР", vec![typedoc("Число")])], Vec::new());
        let params = lower_params(&db, &[decl_param("параметр")], Some(&d));
        assert_eq!(params, vec![db.number(None, None)]);
    }

    #[test]
    fn no_docs_at_all_keeps_every_slot_unknown() {
        let db = InMemoryDb::new();
        let params = lower_params(&db, &[decl_param("А"), decl_param("Б")], None);
        assert_eq!(params, vec![db.unknown(), db.unknown()]);
    }

    #[test]
    fn single_unrecognised_param_stays_unknown_for_gradual_typing() {
        let db = InMemoryDb::new();
        let d = docs(vec![param_doc("X", vec![typedoc("СтрокаТабличнойЧасти")])], Vec::new());
        let params = lower_params(&db, &[decl_param("X")], Some(&d));
        assert_eq!(params, vec![db.unknown()]);
    }

    #[test]
    fn multi_typedoc_param_joins_into_union() {
        let db = InMemoryDb::new();
        let d = docs(vec![param_doc("X", vec![typedoc("Число"), typedoc("Строка")])], Vec::new());
        let params = lower_params(&db, &[decl_param("X")], Some(&d));
        assert_eq!(params, vec![db.union(vec![db.number(None, None), db.string(None, false)])]);
    }

    #[test]
    fn return_section_lowers_through_return_pipeline() {
        let db = InMemoryDb::new();
        let d = docs(Vec::new(), vec![typedoc("Булево"), typedoc("Неопределено")]);
        let ret = lower_return_from_docs(&db, &d);
        assert_eq!(ret, Some(db.union(vec![db.boolean(), db.undefined()])));
    }

    #[test]
    fn return_arbitrary_returns_some_any_not_none() {
        let db = InMemoryDb::new();
        let d = docs(Vec::new(), vec![typedoc("Произвольный"), typedoc("Неопределено")]);
        let ret = lower_return_from_docs(&db, &d);
        assert_eq!(ret, Some(db.any()));
    }

    #[test]
    fn return_section_absent_returns_none() {
        let db = InMemoryDb::new();
        let d = docs(Vec::new(), Vec::new());
        assert_eq!(lower_return_from_docs(&db, &d), None);
    }

    #[test]
    fn collect_return_value_exprs_picks_value_bearing_returns() {
        use hir_def::{Expr, Literal};

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
        let mut body = Body::default();
        let return_void = body.stmts_mut().alloc(Stmt::Return { value: None });
        body.set_body_stmts(vec![return_void].into());

        assert!(collect_return_value_exprs(&body).is_empty());
    }
}
