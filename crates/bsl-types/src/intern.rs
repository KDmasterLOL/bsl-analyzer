use std::sync::Arc;

use crate::facet::{
    DateFacet, FormBindingFacet, FormBindingTargetFacet, FunctionFacet, MdoRefFacet, NumberFacet,
    ProjectionFacet, StringFacet, TableFacet,
};
use crate::kind::{
    Projection, ProjectionField, ProjectionFieldSource, ProjectionOrigin, TypeId, TypeKind,
};

pub trait TypeKernelDb {
    fn intern_type(&self, kind: TypeKind) -> TypeId;

    fn lookup_type(&self, id: TypeId) -> &TypeKind;
}

pub fn canonicalise(db: &dyn TypeKernelDb, kind: TypeKind) -> TypeKind {
    match kind {
        TypeKind::Number(NumberFacet { precision, scale, .. }) => {
            TypeKind::Number(NumberFacet { precision, scale, origin: None })
        }
        TypeKind::String(StringFacet { length, fixed, .. }) => {
            TypeKind::String(StringFacet { length, fixed, origin: None })
        }
        TypeKind::Date(DateFacet { component, .. }) => {
            TypeKind::Date(DateFacet { component, origin: None })
        }

        TypeKind::ValueTable(TableFacet { projection, .. }) => TypeKind::ValueTable(TableFacet {
            projection: strip_projection(projection),
            source: crate::facet::TableSource::Unknown,
        }),
        TypeKind::ValueTableRow(TableFacet { projection, .. }) => {
            TypeKind::ValueTableRow(TableFacet {
                projection: strip_projection(projection),
                source: crate::facet::TableSource::Unknown,
            })
        }
        TypeKind::QueryResult(ProjectionFacet { projection, .. }) => {
            TypeKind::QueryResult(ProjectionFacet {
                projection: strip_projection(projection),
                source: crate::facet::ProjectionSource::Unknown,
            })
        }
        TypeKind::QueryResultSelection(ProjectionFacet { projection, .. }) => {
            TypeKind::QueryResultSelection(ProjectionFacet {
                projection: strip_projection(projection),
                source: crate::facet::ProjectionSource::Unknown,
            })
        }
        TypeKind::Query { projections } => {
            TypeKind::Query { projections: strip_projection_slice(&projections) }
        }
        TypeKind::QueryBatchResult { per_query } => {
            TypeKind::QueryBatchResult { per_query: strip_projection_slice(&per_query) }
        }

        TypeKind::Function(FunctionFacet {
            params, defaults, min_args, max_args, returns, ..
        }) => TypeKind::Function(FunctionFacet {
            params,
            defaults,
            min_args,
            max_args,
            returns,
            origin: crate::facet::FunctionOrigin::Unknown,
        }),

        TypeKind::FormData { kind, underlying } => {
            TypeKind::FormData { kind, underlying: underlying.map(canonicalise_mdo_ref) }
        }
        TypeKind::FormControl { kind, binding } => TypeKind::FormControl {
            kind,
            binding: binding.map(|b| canonicalise_form_binding(db, b)),
        },
        TypeKind::ThisObject { config_id, owner } => {
            TypeKind::ThisObject { config_id, owner: canonicalise_mdo_ref(owner) }
        }
        TypeKind::ThisManager { config_id, owner } => {
            TypeKind::ThisManager { config_id, owner: canonicalise_mdo_ref(owner) }
        }

        TypeKind::Union(members) => canonicalise_union(db, members),

        other => other,
    }
}

fn canonicalise_mdo_ref(owner: MdoRefFacet) -> MdoRefFacet {
    owner
}

fn canonicalise_form_binding(
    db: &dyn TypeKernelDb,
    FormBindingFacet { path, target }: FormBindingFacet,
) -> FormBindingFacet {
    FormBindingFacet { path, target: canonicalise_form_binding_target(db, target) }
}

fn canonicalise_form_binding_target(
    db: &dyn TypeKernelDb,
    target: FormBindingTargetFacet,
) -> FormBindingTargetFacet {
    match target {
        FormBindingTargetFacet::TabularSection { mdo_ref, section } => {
            FormBindingTargetFacet::TabularSection {
                mdo_ref: canonicalise_mdo_ref(mdo_ref),
                section,
            }
        }
        FormBindingTargetFacet::Attribute { ty } => {
            let ty = db.intern_type(db.lookup_type(ty).clone());
            FormBindingTargetFacet::Attribute { ty }
        }
    }
}

fn strip_projection(p: Option<Arc<Projection>>) -> Option<Arc<Projection>> {
    p.map(|arc| {
        let fields: Arc<[ProjectionField]> = arc
            .fields
            .iter()
            .map(|f| ProjectionField {
                name: f.name.clone(),
                ty: f.ty,
                source: ProjectionFieldSource::Unknown,
            })
            .collect();
        Arc::new(Projection {
            fields,
            origin: ProjectionOrigin::Unknown,
            raw_sdbl_types: arc.raw_sdbl_types.clone(),
        })
    })
}

fn strip_projection_slice(
    slice: &Arc<[Option<Arc<Projection>>]>,
) -> Arc<[Option<Arc<Projection>>]> {
    slice.iter().map(|p| strip_projection(p.clone())).collect()
}

fn canonicalise_union(db: &dyn TypeKernelDb, members: Arc<[TypeId]>) -> TypeKind {
    let mut flat: Vec<TypeId> = Vec::with_capacity(members.len());
    for &m in members.iter() {
        match db.lookup_type(m) {
            TypeKind::Union(inner) => flat.extend(inner.iter().copied()),
            _ => flat.push(m),
        }
    }

    let has_non_never = flat.iter().any(|&m| !matches!(db.lookup_type(m), TypeKind::Never));
    if has_non_never {
        flat.retain(|&m| !matches!(db.lookup_type(m), TypeKind::Never));
    }

    if flat.iter().any(|&m| matches!(db.lookup_type(m), TypeKind::Any)) {
        return TypeKind::Any;
    }

    let has_non_unknown = flat.iter().any(|&m| !matches!(db.lookup_type(m), TypeKind::Unknown));
    if has_non_unknown {
        flat.retain(|&m| !matches!(db.lookup_type(m), TypeKind::Unknown));
    }

    // Order members by a deterministic structural key, NOT the raw `TypeId`
    // (whose value follows interning order, which is scheduler-dependent under
    // parallel analysis). This keeps a union's canonical member order — and thus
    // its display text and the arm picked by union method/overload resolution —
    // stable across runs. Stable sort preserves construction order on key ties.
    flat.sort_by_cached_key(|&id| union_order_key(db, id));
    flat.dedup();

    if flat.len() == 1 {
        return db.lookup_type(flat[0]).clone();
    }

    if flat.is_empty() {
        return TypeKind::Unknown;
    }

    TypeKind::Union(flat.into())
}

/// Deterministic structural ordering key for union members, independent of
/// `TypeId` assignment order. Variants without nested `TypeId`s key off their
/// `Debug` (only names / enums / config-ids — no ids); the variants that carry
/// nested `TypeId`s recurse so the key reflects structure, not interning order.
fn union_order_key(db: &dyn TypeKernelDb, id: TypeId) -> String {
    let mut out = String::new();
    write_type_order_key(db, id, 0, &mut out);
    out
}

fn write_type_order_key(db: &dyn TypeKernelDb, id: TypeId, depth: u8, out: &mut String) {
    use std::fmt::Write as _;

    if depth >= 24 {
        out.push('~');
        return;
    }
    let d = depth + 1;
    let opt = |db: &dyn TypeKernelDb, t: Option<TypeId>, out: &mut String| match t {
        Some(t) => write_type_order_key(db, t, d, out),
        None => out.push('_'),
    };

    match db.lookup_type(id) {
        TypeKind::Array(f) => {
            out.push_str("Array<");
            opt(db, f.element, out);
            out.push('>');
        }
        TypeKind::Map(f) => {
            out.push_str("Map<");
            opt(db, f.key, out);
            out.push(',');
            opt(db, f.value, out);
            out.push('>');
        }
        TypeKind::ValueList(el) => {
            out.push_str("ValueList<");
            opt(db, *el, out);
            out.push('>');
        }
        TypeKind::ValueTable(f) => {
            out.push_str("ValueTable");
            write_projection_order_key(db, &f.projection, d, out);
        }
        TypeKind::ValueTableRow(f) => {
            out.push_str("ValueTableRow");
            write_projection_order_key(db, &f.projection, d, out);
        }
        TypeKind::QueryResult(f) => {
            out.push_str("QueryResult");
            write_projection_order_key(db, &f.projection, d, out);
        }
        TypeKind::QueryResultSelection(f) => {
            out.push_str("QueryResultSelection");
            write_projection_order_key(db, &f.projection, d, out);
        }
        TypeKind::QueryBatchResult { per_query } => {
            out.push_str("QueryBatchResult");
            for proj in per_query.iter() {
                write_projection_order_key(db, proj, d, out);
            }
        }
        TypeKind::Query { projections } => {
            out.push_str("Query");
            for proj in projections.iter() {
                write_projection_order_key(db, proj, d, out);
            }
        }
        TypeKind::Function(f) => {
            out.push_str("Function(");
            for p in f.params.iter() {
                let _ = write!(out, "{:?}{:?}:", p.name, (&p.passing, p.variadic));
                write_type_order_key(db, p.ty, d, out);
                out.push(',');
            }
            out.push_str("->");
            write_type_order_key(db, f.returns, d, out);
            let _ = write!(out, ";{:?}{:?}{:?}{:?}", f.min_args, f.max_args, f.origin, f.defaults);
            out.push(')');
        }
        TypeKind::FormControl { kind, binding } => {
            let _ = write!(out, "FormControl{{{kind:?},");
            match binding {
                Some(b) => {
                    let _ = write!(out, "{:?}:", b.path);
                    match &b.target {
                        crate::facet::FormBindingTargetFacet::Attribute { ty } => {
                            out.push_str("Attr(");
                            write_type_order_key(db, *ty, d, out);
                            out.push(')');
                        }
                        other => {
                            let _ = write!(out, "{other:?}");
                        }
                    }
                }
                None => out.push('_'),
            }
            out.push('}');
        }
        TypeKind::Union(members) => {
            // A union nested inside another type (Array<A|B>, a projection field,
            // …) must recurse, not hit the `Debug` fallback (which embeds raw
            // member `TypeId`s). Sort the member keys so the nested-union key is
            // itself member-order-independent.
            out.push_str("Union(");
            let mut keys: Vec<String> = members
                .iter()
                .map(|&m| {
                    let mut s = String::new();
                    write_type_order_key(db, m, d, &mut s);
                    s
                })
                .collect();
            keys.sort();
            for k in &keys {
                out.push_str(k);
                out.push('|');
            }
            out.push(')');
        }
        // Every remaining variant carries only names / enums / config-ids (verified
        // against `kind.rs`), so `Debug` is already interning-order-independent.
        other => {
            let _ = write!(out, "{other:?}");
        }
    }
}

fn write_projection_order_key(
    db: &dyn TypeKernelDb,
    projection: &Option<Arc<crate::kind::Projection>>,
    depth: u8,
    out: &mut String,
) {
    use std::fmt::Write as _;
    out.push('[');
    if let Some(p) = projection {
        for fld in p.fields.iter() {
            let _ = write!(out, "{:?}=", fld.name);
            write_type_order_key(db, fld.ty, depth, out);
            out.push(';');
        }
    }
    out.push(']');
}
