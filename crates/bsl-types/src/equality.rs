//! Equality helpers on `TypeKind`.
//!
//! - [`semantic_eq`] — structural equality ignoring provenance hints
//!   (`*_origin`, `*_source`). Two `Number(NumberFacet { precision:
//!   Some(15), scale: Some(2), origin: A })` and
//!   `Number(NumberFacet { ..., origin: B })` are `semantic_eq`.
//! - [`type_eq`] — strict equality, distinguishes precision/scale.
//!   `Number(15, 2)` vs `Number(20, 4)` are NOT `type_eq`.
//!
//! These mirror the derived `PartialEq` on `TypeKind` but with
//! provenance fields zeroed out. `PartialEq` itself remains strict and
//! is what `intern_type` uses for the canonical identity map (after
//! canonicalisation has stripped provenance — see Phase 1.D).
//!
//! Sandbox callers may invoke these directly for debug or structural
//! comparison. They must NOT use raw `TypeKind` instances as cache
//! keys or dispatch input — intern first to get a `TypeId`.

use std::sync::Arc;

use crate::facet::{
    DateFacet, FunctionFacet, NumberFacet, ProjectionFacet, StringFacet, TableFacet,
};
use crate::kind::{Projection, ProjectionField, TypeKind};

/// Structural equality ignoring provenance fields.
///
/// Used by tests and diagnostics that want to compare type shapes
/// without caring where they came from. For canonical identity, use
/// [`crate::intern::TypeKernelDb::intern_type`] which canonicalises
/// (and hashes) provenance-free.
pub fn semantic_eq(a: &TypeKind, b: &TypeKind) -> bool {
    match (a, b) {
        (TypeKind::Number(x), TypeKind::Number(y)) => number_eq_no_origin(x, y),
        (TypeKind::String(x), TypeKind::String(y)) => string_eq_no_origin(x, y),
        (TypeKind::Date(x), TypeKind::Date(y)) => date_eq_no_origin(x, y),
        (TypeKind::ValueTable(x), TypeKind::ValueTable(y)) => table_eq_no_source(x, y),
        (TypeKind::ValueTableRow(x), TypeKind::ValueTableRow(y)) => table_eq_no_source(x, y),
        (TypeKind::QueryResult(x), TypeKind::QueryResult(y)) => projection_facet_eq(x, y),
        (TypeKind::QueryResultSelection(x), TypeKind::QueryResultSelection(y)) => {
            projection_facet_eq(x, y)
        }
        (
            TypeKind::QueryBatchResult { per_query: a },
            TypeKind::QueryBatchResult { per_query: b },
        ) => projection_slice_eq(a, b),
        (TypeKind::Query { projections: a }, TypeKind::Query { projections: b }) => {
            projection_slice_eq(a, b)
        }
        (TypeKind::Function(x), TypeKind::Function(y)) => function_eq_no_origin(x, y),
        // All other variants have no provenance — fall back to `==`.
        _ => a == b,
    }
}

/// Strict equality.
///
/// Distinguishes precision/scale/length on primitives. Provenance is
/// still ignored. Today this is identical to [`semantic_eq`] —
/// kept as a separate entry point because the design contract
/// distinguishes them (§4.4 of the architecture doc).
pub fn type_eq(a: &TypeKind, b: &TypeKind) -> bool {
    semantic_eq(a, b)
}

// ── Helper functions ─────────────────────────────────────────

fn number_eq_no_origin(x: &NumberFacet, y: &NumberFacet) -> bool {
    x.precision == y.precision && x.scale == y.scale
}

fn string_eq_no_origin(x: &StringFacet, y: &StringFacet) -> bool {
    x.length == y.length && x.fixed == y.fixed
}

fn date_eq_no_origin(x: &DateFacet, y: &DateFacet) -> bool {
    x.component == y.component
}

fn table_eq_no_source(x: &TableFacet, y: &TableFacet) -> bool {
    projection_arc_eq(&x.projection, &y.projection)
}

fn projection_facet_eq(x: &ProjectionFacet, y: &ProjectionFacet) -> bool {
    projection_arc_eq(&x.projection, &y.projection)
}

/// Compares an `Option<Arc<Projection>>` pair ignoring `Projection.origin`
/// and `ProjectionField.source` — both provenance.
fn projection_arc_eq(a: &Option<Arc<Projection>>, b: &Option<Arc<Projection>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(p), Some(q)) => projection_eq(p, q),
        _ => false,
    }
}

fn projection_eq(p: &Projection, q: &Projection) -> bool {
    p.fields.len() == q.fields.len()
        && p.fields.iter().zip(q.fields.iter()).all(|(a, b)| projection_field_eq(a, b))
}

fn projection_field_eq(a: &ProjectionField, b: &ProjectionField) -> bool {
    a.name == b.name && a.ty == b.ty
}

/// Per-element comparison for `Query.projections` /
/// `QueryBatchResult.per_query`. Each element is `Option<Arc<Projection>>`;
/// uses [`projection_arc_eq`] to strip provenance per element.
fn projection_slice_eq(
    a: &Arc<[Option<Arc<Projection>>]>,
    b: &Arc<[Option<Arc<Projection>>]>,
) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| projection_arc_eq(x, y))
}

fn function_eq_no_origin(x: &FunctionFacet, y: &FunctionFacet) -> bool {
    x.params == y.params
        && x.defaults == y.defaults
        && x.min_args == y.min_args
        && x.max_args == y.max_args
        && x.returns == y.returns
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bsl_metadata::MdoType;

    use crate::facet::{
        FormBindingFacet, FormBindingTargetFacet, FormDataFacet, FormElementFacet, MdoRefFacet,
        NumberFacet,
    };
    use crate::kind::{ConfigId, TypeKind, TypeOrigin};

    #[test]
    fn semantic_eq_ignores_number_origin() {
        let a = TypeKind::Number(NumberFacet {
            precision: Some(15),
            scale: Some(2),
            origin: Some(TypeOrigin::SdblCast),
        });
        let b = TypeKind::Number(NumberFacet {
            precision: Some(15),
            scale: Some(2),
            origin: Some(TypeOrigin::BslLiteral),
        });
        assert!(super::semantic_eq(&a, &b));
    }

    #[test]
    fn type_eq_distinguishes_precision() {
        let a = TypeKind::Number(NumberFacet::with_scale(15, 2));
        let b = TypeKind::Number(NumberFacet::with_scale(20, 4));
        assert!(!super::type_eq(&a, &b));
    }

    #[test]
    fn semantic_eq_distinguishes_unsized_vs_sized_number() {
        let a = TypeKind::Number(NumberFacet::unsized_());
        let b = TypeKind::Number(NumberFacet::with_scale(15, 2));
        assert!(!super::semantic_eq(&a, &b));
    }

    #[test]
    fn semantic_eq_passes_through_for_simple_variants() {
        assert!(super::semantic_eq(&TypeKind::Boolean, &TypeKind::Boolean));
        assert!(!super::semantic_eq(&TypeKind::Boolean, &TypeKind::Null));
    }

    #[test]
    fn semantic_eq_ignores_projection_provenance() {
        use std::sync::Arc;

        use crate::facet::{ProjectionFacet, ProjectionSource};
        use crate::kind::{
            Projection, ProjectionField, ProjectionFieldSource, ProjectionOrigin, TypeId,
        };

        let make_field = |name: &str, ty: u64, field_source, origin| {
            Arc::new(Projection {
                fields: Arc::new([ProjectionField {
                    name: name.to_string(),
                    ty: TypeId(ty),
                    source: field_source,
                }]),
                origin,
            })
        };

        let a = TypeKind::QueryResult(ProjectionFacet {
            projection: Some(make_field(
                "Цена",
                1,
                ProjectionFieldSource::Cast,
                ProjectionOrigin::SdblQuery,
            )),
            source: ProjectionSource::Sdbl,
        });
        let b = TypeKind::QueryResult(ProjectionFacet {
            projection: Some(make_field(
                "Цена",
                1,
                ProjectionFieldSource::Column,
                ProjectionOrigin::Unknown,
            )),
            source: ProjectionSource::Unknown,
        });

        // Same field name + same TypeId; provenance differs everywhere
        // (field source, projection origin, facet source). All three are
        // ignored.
        assert!(super::semantic_eq(&a, &b));

        // Negative checks — structural differences MUST surface even
        // when provenance is identical.
        let different_name = TypeKind::QueryResult(ProjectionFacet {
            projection: Some(make_field(
                "Стоимость",
                1,
                ProjectionFieldSource::Cast,
                ProjectionOrigin::SdblQuery,
            )),
            source: ProjectionSource::Sdbl,
        });
        assert!(!super::semantic_eq(&a, &different_name));

        let different_ty = TypeKind::QueryResult(ProjectionFacet {
            projection: Some(make_field(
                "Цена",
                2, // different TypeId
                ProjectionFieldSource::Cast,
                ProjectionOrigin::SdblQuery,
            )),
            source: ProjectionSource::Sdbl,
        });
        assert!(!super::semantic_eq(&a, &different_ty));
    }

    #[test]
    fn this_variants_match_metadata_config_identity_rules() {
        let owner =
            MdoRefFacet { mdo_type: MdoType::Catalog, name: "Контрагенты".to_string() };
        let root = TypeKind::ThisObject { config_id: ConfigId::Root, owner: owner.clone() };
        let resolved =
            TypeKind::ThisObject { config_id: ConfigId::Resolved(1), owner: owner.clone() };
        let same = TypeKind::ThisObject { config_id: ConfigId::Root, owner };

        assert!(super::semantic_eq(&root, &same));
        assert!(super::type_eq(&root, &same));
        assert!(!super::semantic_eq(&root, &resolved));
        assert!(!super::type_eq(&root, &resolved));
    }

    #[test]
    fn form_variants_use_structural_equality() {
        let owner =
            MdoRefFacet { mdo_type: MdoType::Catalog, name: "Контрагенты".to_string() };
        let form_a =
            TypeKind::FormData { kind: FormDataFacet::Structure, underlying: Some(owner.clone()) };
        let form_b =
            TypeKind::FormData { kind: FormDataFacet::Structure, underlying: Some(owner.clone()) };
        let form_c =
            TypeKind::FormData { kind: FormDataFacet::Collection, underlying: Some(owner.clone()) };

        assert!(super::semantic_eq(&form_a, &form_b));
        assert!(!super::semantic_eq(&form_a, &form_c));

        let binding = FormBindingFacet {
            path: Arc::from(["Объект".to_string(), "Цена".to_string()]),
            target: FormBindingTargetFacet::Attribute { ty: crate::kind::TypeId(42) },
        };
        let control_a =
            TypeKind::FormControl { kind: FormElementFacet::Field, binding: Some(binding.clone()) };
        let control_b =
            TypeKind::FormControl { kind: FormElementFacet::Field, binding: Some(binding) };
        let control_c = TypeKind::FormControl { kind: FormElementFacet::Button, binding: None };

        assert!(super::semantic_eq(&control_a, &control_b));
        assert!(!super::semantic_eq(&control_a, &control_c));
    }
}
