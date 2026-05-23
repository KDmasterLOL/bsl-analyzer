//! `TypeKernelDb` trait — the interning gateway.
//!
//! Production interning lives behind this trait. Implementations choose
//! their own storage: sandbox uses `elsa::FrozenVec<Box<TypeKind>>`
//! (see [`crate::testing::InMemoryDb`]); production crates may use a
//! Salsa input + manual table, or `RwLock<FrozenVec>`, etc.
//!
//! Canonicalisation rules ([`canonicalise`]) are applied inside
//! `intern_type` before hashing. Rules are spec'd in
//! `.omc/plans/type-kernel-phase-1-sandbox.md` §1.D.

use std::sync::Arc;

use crate::facet::{
    DateFacet, FunctionFacet, NumberFacet, ProjectionFacet, StringFacet, TableFacet,
};
use crate::kind::{
    Projection, ProjectionField, ProjectionFieldSource, ProjectionOrigin, TypeId, TypeKind,
};

/// The interning gateway. Implementations canonicalise on input so two
/// callers that build semantically equal `TypeKind` literals via
/// different paths get the same `TypeId`.
///
/// **Cross-db caveat:** `TypeId` carries no db-identity tag. Callers
/// must not mix `TypeId`s obtained from one db with `lookup_type` on a
/// different db — IDs are opaque indices into a particular db's intern
/// table.
pub trait TypeKernelDb {
    /// Intern a `TypeKind`, returning the canonical `TypeId`.
    fn intern_type(&self, kind: TypeKind) -> TypeId;

    /// Look up the `TypeKind` interned at the given handle. Returned
    /// reference borrows from `&self` of the db; it cannot outlive the
    /// db borrow.
    ///
    /// **Lifetime-escape compile_fail pin** — Phase 1.F asserts the
    /// invariant via doc-test: a `&TypeKind` from `lookup_type` MUST
    /// NOT survive the db being dropped. The snippet below is
    /// expected NOT to compile; if it ever does, the lifetime
    /// contract loosened and the test fails:
    ///
    /// ```compile_fail
    /// use bsl_types::intern::TypeKernelDb;
    /// use bsl_types::kind::TypeKind;
    /// use bsl_types::testing::InMemoryDb;
    /// use bsl_types::builders::Builders;
    ///
    /// fn require_borrow(_x: &TypeKind) {}
    ///
    /// let kind_ref: &TypeKind = {
    ///     let db = InMemoryDb::new();
    ///     let id = db.number(None, None);
    ///     db.lookup_type(id)
    /// };
    /// require_borrow(kind_ref); // ERROR: kind_ref outlives db
    /// ```
    fn lookup_type(&self, id: TypeId) -> &TypeKind;
}

/// Canonicalise a `TypeKind` before interning.
///
/// Applies the rules from `.omc/plans/type-kernel-phase-1-sandbox.md`
/// §1.D:
///
/// - **`Union` algebra:** flatten nested unions, drop `Never`, absorb
///   `Unknown`, dominate by `Any`, dedupe, sort by `TypeId`. Single
///   member → return that member's `TypeKind`; empty → `Unknown`.
/// - **Provenance stripping:** zero `*_origin` / `*_source` fields on
///   `NumberFacet`, `StringFacet`, `DateFacet`, `TableFacet`,
///   `ProjectionFacet`, `FunctionFacet`, and inside projections
///   (`Projection.origin` and `ProjectionField.source`).
/// - **Projection field order:** preserved (NOT dedup'd; the design
///   says projections are ordered lists, not sets).
/// - **`MetaRefFacet` / `MetaObjFacet`:** `config_id` is opaque
///   identity — no defaulting, no rewriting.
///
/// `Union` canonicalisation needs to look up member kinds (to detect
/// nested unions / sentinels), hence the `&dyn TypeKernelDb` parameter.
/// Members are already-interned `TypeId`s; canonicalise does not
/// recursively re-intern them, so termination is straightforward.
pub(crate) fn canonicalise(db: &dyn TypeKernelDb, kind: TypeKind) -> TypeKind {
    match kind {
        // Provenance stripping on primitive facets.
        TypeKind::Number(NumberFacet { precision, scale, .. }) => {
            TypeKind::Number(NumberFacet { precision, scale, origin: None })
        }
        TypeKind::String(StringFacet { length, fixed, .. }) => {
            TypeKind::String(StringFacet { length, fixed, origin: None })
        }
        TypeKind::Date(DateFacet { component, .. }) => {
            TypeKind::Date(DateFacet { component, origin: None })
        }

        // Provenance stripping on table / projection facets, including
        // the nested `Projection.origin` and `ProjectionField.source`.
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

        // Provenance stripping on function facet.
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

        // Union algebra.
        TypeKind::Union(members) => canonicalise_union(db, members),

        // Variants without provenance pass through.
        other => other,
    }
}

/// Rebuilds a projection with provenance zeroed
/// (`Projection.origin` and `ProjectionField.source`).
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
        Arc::new(Projection { fields, origin: ProjectionOrigin::Unknown })
    })
}

/// Strips provenance from each entry of a `Query`/`QueryBatchResult`
/// projection slice.
fn strip_projection_slice(
    slice: &Arc<[Option<Arc<Projection>>]>,
) -> Arc<[Option<Arc<Projection>>]> {
    slice.iter().map(|p| strip_projection(p.clone())).collect()
}

/// Apply the `Union(members)` canonicalisation algebra.
fn canonicalise_union(db: &dyn TypeKernelDb, members: Arc<[TypeId]>) -> TypeKind {
    // 1. Flatten nested unions (single level — recursive flatten works
    //    because every existing Union was itself canonicalised on
    //    interning, so its members are already flat).
    let mut flat: Vec<TypeId> = Vec::with_capacity(members.len());
    for &m in members.iter() {
        match db.lookup_type(m) {
            TypeKind::Union(inner) => flat.extend(inner.iter().copied()),
            _ => flat.push(m),
        }
    }

    // 2. Drop `Never` IFF at least one non-Never arm remains.
    //    `Union([Never, X]) → X`, but `Union([Never])` must stay
    //    `Never` (plan §1.D rule 4 — proven-unreachable single arm
    //    is meaningful and must not collapse to `Unknown`).
    let has_non_never = flat.iter().any(|&m| !matches!(db.lookup_type(m), TypeKind::Never));
    if has_non_never {
        flat.retain(|&m| !matches!(db.lookup_type(m), TypeKind::Never));
    }

    // 3. Detect `Any` (top type — dominates).
    if flat.iter().any(|&m| matches!(db.lookup_type(m), TypeKind::Any)) {
        return TypeKind::Any;
    }

    // 4. Absorb `Unknown` (analysis-incomplete bottom contributes
    //    nothing once at least one concrete arm exists). Filter only
    //    if at least one non-Unknown arm remains; otherwise the union
    //    is effectively all-Unknown and step 7 handles it via
    //    single-member unwrap or empty-union fallback.
    let has_non_unknown = flat.iter().any(|&m| !matches!(db.lookup_type(m), TypeKind::Unknown));
    if has_non_unknown {
        flat.retain(|&m| !matches!(db.lookup_type(m), TypeKind::Unknown));
    }

    // 5. Dedupe + 6. sort by raw `TypeId` value.
    flat.sort_by_key(|id| id.raw());
    flat.dedup();

    // 7. Single-member unwrap: return that member's TypeKind directly.
    if flat.len() == 1 {
        return db.lookup_type(flat[0]).clone();
    }

    // 8. Empty union: `Unknown`.
    if flat.is_empty() {
        return TypeKind::Unknown;
    }

    TypeKind::Union(flat.into())
}
