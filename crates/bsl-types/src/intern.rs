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

    flat.sort_by_key(|id| id.raw());
    flat.dedup();

    if flat.len() == 1 {
        return db.lookup_type(flat[0]).clone();
    }

    if flat.is_empty() {
        return TypeKind::Unknown;
    }

    TypeKind::Union(flat.into())
}
