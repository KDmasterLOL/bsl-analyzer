use hir_def::body::Body;
use hir_def::hir::Expr;
use hir_def::{ExprId, IdConversion};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnloadIteration {
    Linear,
    Hierarchical,
    Dynamic,
}

pub(crate) fn classify_unload_arg(body: &Body, args: &[ExprId]) -> UnloadIteration {
    let Some(first) = args.first().copied() else {
        return UnloadIteration::Linear;
    };
    let Expr::Field { base, field } = body.expr(first) else {
        return UnloadIteration::Dynamic;
    };
    let base_id = ExprId::from_idx(*base);
    let Expr::Path(base_name) = body.expr(base_id) else {
        return UnloadIteration::Dynamic;
    };
    if !is_iteration_enum_name(base_name.as_str()) {
        return UnloadIteration::Dynamic;
    }
    classify_iteration_member(field.as_str())
}

fn is_iteration_enum_name(s: &str) -> bool {
    let lower = s.to_lowercase();
    matches!(lower.as_str(), "обходрезультатазапроса" | "queryresultiteration")
}

fn classify_iteration_member(s: &str) -> UnloadIteration {
    match s.to_lowercase().as_str() {
        "прямой" | "linear" => UnloadIteration::Linear,
        "погруппировкам" | "погруппировкамсиерархией" | "bygroups" | "bygroupswithhierarchy" => {
            UnloadIteration::Hierarchical
        }
        _ => UnloadIteration::Dynamic,
    }
}
