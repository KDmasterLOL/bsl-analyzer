//! Argument-driven narrowing for `РезультатЗапроса.Выгрузить()` (Phase H).
//!
//! `Запрос.Выполнить().Выгрузить()` declares the platform return as
//! `Union([PlatformObject("ТаблицаЗначений"), PlatformObject
//! ("ДеревоЗначений")])` because the runtime shape depends on the
//! `ОбходРезультатаЗапроса` argument. The default (missing arg) is
//! `Прямой` which produces `ТаблицаЗначений`; `ПоГруппировкам[СИерархией]`
//! produces `ДеревоЗначений`.
//!
//! This module classifies the first call argument syntactically (no
//! resolution; no Salsa) and reports a [`UnloadIteration`] verdict that
//! the caller in `method_lookup` uses to drop the wrong arm from the
//! union. Dynamic / non-recognised shapes collapse to
//! [`UnloadIteration::Dynamic`] so the union is preserved — the user
//! still sees both methods, just without narrowing.
//!
//! Enum member names are bilingual and case-insensitive, verified
//! against `crates/bsl-platform/data/platform_data.json:9716,91163-91194`.

use hir_def::body::Body;
use hir_def::hir::Expr;
use hir_def::{ExprId, IdConversion};

/// Iteration shape selected by the `.Выгрузить(ТипОбхода)` argument.
///
/// * [`UnloadIteration::Linear`] — runtime returns `ТаблицаЗначений`.
/// * [`UnloadIteration::Hierarchical`] — runtime returns `ДеревоЗначений`.
/// * [`UnloadIteration::Dynamic`] — arg shape can't be statically
///   resolved (variable, expression, unrecognised enum member); the
///   caller must keep the platform union intact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnloadIteration {
    Linear,
    Hierarchical,
    Dynamic,
}

/// Inspect the first call argument of `.Выгрузить(arg)` and classify
/// the iteration shape.
///
/// Recognises `Expr::Field { base: Path("ОбходРезультатаЗапроса" |
/// "QueryResultIteration"), field: <member> }` with bilingual member
/// names. Missing first argument resolves to [`UnloadIteration::Linear`]
/// (platform default per `platform_data.json:91164`). Anything else —
/// dynamic variable, expression, unrecognised member — falls through to
/// [`UnloadIteration::Dynamic`].
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
