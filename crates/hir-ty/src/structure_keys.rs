//! Inference of `Структура` keys from literal construction.
//!
//! A local built as `Новый Структура("Ключ1, Ключ2")` and/or extended with
//! `.Вставить("Ключ", Значение)` in the same body has a knowable set of keys. This module collects
//! that shape syntactically (Phase 0, no `db`) and materialises it into a typed
//! [`bsl_types::facet::StructureFacet`] during inference (Phase 1), so member completion and hover
//! after a dot can surface the keys (and the keys of a simple nested structure value).
//!
//! Soft by construction: it only ever *adds* fields to a structure; it is never consulted by a
//! diagnostic. Interprocedural key propagation (keys added inside called helpers) is Stage 2.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use stdx::case::{eq_ignore_case, CaseExt};

use bsl_types::builders::Builders;
use bsl_types::facet::StructureFacet;
use bsl_types::kind::{
    Projection, ProjectionField, ProjectionFieldSource, ProjectionOrigin, TypeId, TypeOrigin,
};
use cfg_types::{ExprId, IdConversion};
use hir_def::body::Body;
use hir_def::hir::{Expr, Literal, Stmt};

use crate::db::HirDatabase;

/// Hard cap on nested-structure recursion and receiver-chain depth, applied in the collector and
/// the materialiser. Beyond it a nested value degrades to an untyped structure.
const MAX_NEST_DEPTH: usize = 4;

/// Where a structure key's value comes from. Resolved to a `TypeId` only at materialisation time —
/// an `ExprId` must never reach the interned [`Projection`].
pub(crate) enum ValueSource {
    /// The value is itself a literal `Новый Структура(...)` whose shape we collected.
    Literal(StructureShape),
    /// The value is an arbitrary expression; its type is taken from the inference cache if available.
    Expr(ExprId),
    /// A type already resolved elsewhere — used by Stage 2 to inject a callee summary's field types
    /// (an interned `TypeId`, never an `ExprId`).
    Resolved(TypeId),
    /// No value, or a non-literal we do not type (key still surfaced).
    Unknown,
}

struct StructField {
    /// Original-case key name (display + projection).
    name: String,
    source: ValueSource,
}

/// Collected keys (and nested shapes) of one structure local, flow-insensitive whole-body union.
/// A non-literal key (`.Вставить(ИмяКлюча, …)`) is simply not recorded — the structure still
/// surfaces the keys it does know.
#[derive(Default)]
pub(crate) struct StructureShape {
    /// Keys in first-seen order; de-duplicated last-wins on value by case-insensitive name.
    fields: Vec<StructField>,
}

impl StructureShape {
    pub(crate) fn upsert(&mut self, name: &str, source: ValueSource) {
        if let Some(existing) = self.fields.iter_mut().find(|f| eq_ignore_case(&f.name, name)) {
            existing.source = source;
        } else {
            self.fields.push(StructField { name: name.to_string(), source });
        }
    }

    fn merge(&mut self, other: StructureShape) {
        for f in other.fields {
            self.upsert(&f.name, f.source);
        }
    }
}

/// What seeds the tracked roots of a structure-shape collection.
#[derive(Clone, Copy)]
pub(crate) enum SeedRoots<'a> {
    /// Stage 1: roots are locals assigned a `Новый Структура` literal in this body.
    NewLiterals,
    /// Stage 2: roots are the (by-reference) parameter names — pre-tracked even though they are not
    /// constructed here; only `.Вставить` and forwarding contribute their keys.
    ParamNames(&'a [String]),
}

/// Collect, per local/param name (lowercased), the structure shape built in this body.
///
/// A single pass in statement (≈ source) order, so a seed assignment and a `.Вставить` writing the
/// same key interleave correctly: the last write in the body wins. An insert before its root is
/// tracked is ignored (you cannot insert into an unconstructed structure).
pub(crate) fn collect_structure_shapes(
    body: &Body,
    seed_roots: SeedRoots,
    forwarder: Option<&crate::structure_param_keys::Forwarder>,
) -> FxHashMap<String, StructureShape> {
    let mut shapes: FxHashMap<String, StructureShape> = FxHashMap::default();
    // Roots that may no longer be extended: a by-reference param that has been reassigned keeps the
    // keys accumulated while it still aliased the caller, but ignores later inserts/forwards.
    let mut frozen: FxHashSet<String> = FxHashSet::default();

    if let SeedRoots::ParamNames(names) = seed_roots {
        for name in names {
            shapes.entry(name.clone()).or_default();
        }
    }

    // One pass in statement (≈ source) order. Seeds, `.Вставить` inserts, and interprocedural
    // forwarding all interleave here, so a root is only ever extended after it is live and the last
    // write to a key wins.
    for (_id, stmt) in body.stmts_iter() {
        match stmt {
            Stmt::Assign { target, value } => {
                // Seed: `Local = Новый Структура(...)` — Stage-1 (`NewLiterals`) only; a constructor
                // assignment to a parameter is a reassignment that breaks aliasing (handled below).
                if matches!(seed_roots, SeedRoots::NewLiterals) {
                    if let Expr::Path(name) = body.expr_idx(*target) {
                        if let Some(shape) = constructor_shape_of(body, ExprId::from_idx(*value), 0)
                        {
                            shapes.entry(name.as_str().fold_lower()).or_default().merge(shape);
                            continue;
                        }
                    }
                }
                // The RHS may forward a tracked root to a helper (`Х = Заполнить(С)`), including a
                // by-ref param filled during the call (`П = F(П)`) — those keys reach the caller
                // before the reassignment completes, so fold BEFORE freezing the root below.
                if let Some(fw) = forwarder {
                    fw.fold_call(body, &mut shapes, &frozen, body.expr_idx(*value));
                }
                // Stage 2: reassigning a tracked parameter breaks its aliasing with the caller's
                // argument. Freeze it in source order — keys inserted earlier still reach the
                // caller, but later inserts into the fresh value do not.
                if matches!(seed_roots, SeedRoots::ParamNames(_)) {
                    if let Expr::Path(name) = body.expr_idx(*target) {
                        let key = name.as_str().fold_lower();
                        if shapes.contains_key(&key) {
                            frozen.insert(key);
                        }
                    }
                }
            }
            Stmt::Expr(expr_idx) => {
                let call = body.expr_idx(*expr_idx);
                // Insert: `Receiver.Вставить("Ключ", Значение)` on an already-tracked root.
                if let Some((receiver, method, args)) = as_method_call(body, call) {
                    if is_insert_method(method) {
                        if let Some((root, path)) = receiver_root_path(body, receiver, 0) {
                            if shapes.contains_key(&root) && !frozen.contains(&root) {
                                let root_shape =
                                    shapes.get_mut(&root).expect("checked contains_key");
                                if let Some(target_shape) = navigate_mut(root_shape, &path) {
                                    apply_insert(body, target_shape, args);
                                }
                            }
                        }
                        continue;
                    }
                }
                // A non-insert call statement may forward a tracked root to a helper (`Заполнить(С)`).
                if let Some(fw) = forwarder {
                    fw.fold_call(body, &mut shapes, &frozen, call);
                }
            }
            _ => {}
        }
    }

    shapes
}

/// Decompose a method/qualified call into `(receiver, method_name, args)`. A BSL `recv.Method(args)`
/// lowers to `Expr::Call { callee: Field { base, field }, .. }`; the dedicated `Expr::MethodCall`
/// form is also accepted defensively.
fn as_method_call<'b>(
    body: &'b Body,
    expr: &'b Expr,
) -> Option<(ExprId, &'b hir_def::Name, &'b [hir_def::hir::ExprIdx])> {
    match expr {
        Expr::MethodCall { receiver, method, args } => {
            Some((ExprId::from_idx(*receiver), method, args))
        }
        Expr::Call { callee, args } => match body.expr_idx(*callee) {
            Expr::Field { base, field } => Some((ExprId::from_idx(*base), field, args)),
            _ => None,
        },
        _ => None,
    }
}

/// Materialise the typed structure for `local_lower` using value types known so far.
pub(crate) fn materialize(
    db: &dyn HirDatabase,
    shapes: &FxHashMap<String, StructureShape>,
    expr_types: &FxHashMap<ExprId, TypeId>,
    local_lower: &str,
) -> Option<TypeId> {
    let shape = shapes.get(local_lower)?;
    // No known keys → keep the plain untyped structure (unchanged display/behaviour).
    let projection = shape_to_projection(db, expr_types, shape, 0)?;
    Some(db.structure_typed(projection, TypeOrigin::BslLiteral))
}

/// The value type of a structure field (`facet.fields`), matched case-insensitively. `None` if the
/// structure is untyped or has no such key — callers stay permissive (no diagnostic).
pub(crate) fn structure_projection_field(facet: &StructureFacet, field: &str) -> Option<TypeId> {
    let projection = facet.fields.as_ref()?;
    projection.fields.iter().find(|f| eq_ignore_case(&f.name, field)).map(|f| f.ty)
}

/// The full typed projection of a structure, if known.
pub(crate) fn structure_projection_fields(facet: &StructureFacet) -> Option<Arc<Projection>> {
    facet.fields.clone()
}

/// Build a shape's typed projection, or `None` if it has no keys (or the depth cap is hit). Shared
/// by Stage-1 local materialisation and the Stage-2 summary's per-param projections.
pub(crate) fn shape_to_projection(
    db: &dyn HirDatabase,
    expr_types: &FxHashMap<ExprId, TypeId>,
    shape: &StructureShape,
    depth: usize,
) -> Option<Arc<Projection>> {
    if depth > MAX_NEST_DEPTH || shape.fields.is_empty() {
        return None;
    }
    let fields: Vec<ProjectionField> = shape
        .fields
        .iter()
        .map(|f| {
            let ty = match &f.source {
                ValueSource::Literal(child) => {
                    match shape_to_projection(db, expr_types, child, depth + 1) {
                        Some(p) => db.structure_typed(p, TypeOrigin::BslLiteral),
                        None => db.structure(None),
                    }
                }
                // Never infer ahead of the read site: only use an already-cached value type.
                ValueSource::Expr(e) => expr_types.get(e).copied().unwrap_or_else(|| db.unknown()),
                ValueSource::Resolved(t) => *t,
                ValueSource::Unknown => db.unknown(),
            };
            ProjectionField::new(f.name.clone(), ty, ProjectionFieldSource::StructureLiteral)
        })
        .collect();
    Some(Arc::new(Projection::new(fields.into(), ProjectionOrigin::StructureLiteral, None)))
}

/// The shape of a `Новый Структура(...)` constructor expression, or `None` if `expr` is not one.
fn constructor_shape_of(body: &Body, expr: ExprId, depth: usize) -> Option<StructureShape> {
    if depth > MAX_NEST_DEPTH {
        return Some(StructureShape::default());
    }
    let Expr::New { type_name: Some(name), args } = body.expr(expr) else { return None };
    if !is_structure_name(name) {
        return None;
    }
    let mut shape = StructureShape::default();
    let Some(first) = args.first() else { return Some(shape) };
    let Expr::Literal(Literal::String(keys_str)) = body.expr_idx(*first) else {
        // First constructor arg is not a literal key string → no nameable keys.
        return Some(shape);
    };
    for (i, key) in keys_str.split(',').map(str::trim).filter(|k| !k.is_empty()).enumerate() {
        // Constructor positional values map to keys: `Новый Структура("К1,К2", З1, З2)`.
        let source = match args.get(i + 1) {
            Some(value) => value_source_of(body, ExprId::from_idx(*value), depth),
            None => ValueSource::Unknown,
        };
        shape.upsert(key, source);
    }
    Some(shape)
}

/// Classify a value expression: a nested literal structure, or an opaque expression.
fn value_source_of(body: &Body, expr: ExprId, depth: usize) -> ValueSource {
    match constructor_shape_of(body, expr, depth + 1) {
        Some(nested) => ValueSource::Literal(nested),
        None => ValueSource::Expr(expr),
    }
}

fn apply_insert(body: &Body, shape: &mut StructureShape, args: &[hir_def::hir::ExprIdx]) {
    let Some(first) = args.first() else { return };
    let Expr::Literal(Literal::String(key)) = body.expr_idx(*first) else {
        // Non-literal key (`.Вставить(ИмяКлюча, …)`) — not nameable, leave known keys intact.
        return;
    };
    let key = key.trim();
    if key.is_empty() {
        return;
    }
    let source = match args.get(1) {
        Some(value) => value_source_of(body, ExprId::from_idx(*value), 0),
        None => ValueSource::Unknown,
    };
    shape.upsert(key, source);
}

/// Decompose a receiver expression into `(root_local_lowercased, field_path_lowercased)`.
/// `С.Адрес.Вставить(…)` → `("с", ["адрес"])`; bounded by [`MAX_NEST_DEPTH`].
fn receiver_root_path(body: &Body, expr: ExprId, depth: usize) -> Option<(String, Vec<String>)> {
    if depth > MAX_NEST_DEPTH {
        return None;
    }
    match body.expr(expr) {
        Expr::Path(name) => Some((name.as_str().fold_lower(), Vec::new())),
        Expr::Field { base, field } => {
            let (root, mut path) = receiver_root_path(body, ExprId::from_idx(*base), depth + 1)?;
            path.push(field.as_str().fold_lower());
            Some((root, path))
        }
        _ => None,
    }
}

/// Descend into the nested literal shape addressed by `path`; `None` if a segment is missing or is
/// not itself a literal structure.
fn navigate_mut<'a>(
    shape: &'a mut StructureShape,
    path: &[String],
) -> Option<&'a mut StructureShape> {
    let Some((seg, rest)) = path.split_first() else { return Some(shape) };
    let idx = shape.fields.iter().position(|f| eq_ignore_case(&f.name, seg))?;
    match &mut shape.fields[idx].source {
        ValueSource::Literal(child) => navigate_mut(child, rest),
        _ => None,
    }
}

fn is_structure_name(name: &hir_def::Name) -> bool {
    crate::method_lookup::is_platform_name(name, "Структура", "Structure")
}

fn is_insert_method(name: &hir_def::Name) -> bool {
    crate::method_lookup::is_platform_name(name, "Вставить", "Insert")
}
