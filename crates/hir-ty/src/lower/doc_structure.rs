//! Lowering of a documented inline structure into a typed structure.
//!
//! The 1C convention declares a structure's fields in the doc-comment itself:
//!
//! ```text
//! // Возвращаемое значение:
//! //   Структура:
//! //    * Имя - Строка - имя профиля.
//! ```
//!
//! Two entry points with opposite defaults, and they must not be merged. [`doc_structure_ty`]
//! decides whether the slot carries a richer type at all, so it answers `None` for everything the
//! existing `TypeRef` lowering already handles. [`field_ty`] types one documented field, where
//! `None` is not an option — a field without a type cannot go into a projection.
//!
//! What a cross-module `см. Модуль.Метод` lowers to is a parameter of the traversal rather than a
//! constant of it — see [`SeePolicy`]. Everything reachable from a slot goes through one
//! traversal, so a reference is handled the same whether it stands alone, under an array, or in a
//! field of a documented structure.

use std::sync::Arc;

use bsl_types::builders::Builders;
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::{
    Projection, ProjectionField, ProjectionFieldSource, ProjectionOrigin, TypeId, TypeOrigin,
};
use hir_def::docs::{DocField, DocTypeExpr};
use hir_def::QualifiedName;

use crate::lower::TyLoweringContext;

/// A documented structure together with how deep the documentation puts it: `Структура:` is depth
/// zero, `Массив из Структура:` depth one.
///
/// The depth is what keeps one documented slot from colouring another. A slot may hold several
/// untyped structures — `Структура` in one union arm and `Массив из Структура` in the next — and
/// only the arm the bullets belong to may receive them.
pub(crate) struct DocumentedStructure {
    ty: TypeId,
    depth: usize,
}

/// What to do with a reference to another method's documentation while lowering.
///
/// One lowering, two behaviours — not two lowerings. A second traversal beside this one would
/// answer differently on the forms only one of them recognises, and the difference would show up
/// as a slot silently losing its fields rather than as a failure.
pub(crate) enum SeePolicy<'a> {
    /// Leave the reference the permissive top type. An unresolvable reference must not narrow the
    /// slot: the kernel drops `Unknown` from a union (`T | Unknown == T`) and would leave the slot
    /// as whatever else happened to be documented beside it.
    Permissive,
    /// Resolve the reference with the given function. `None` from it means the same as
    /// [`SeePolicy::Permissive`] — the reference named a target that gives nothing.
    Resolve(&'a dyn Fn(&QualifiedName) -> Option<TypeId>),
}

impl SeePolicy<'_> {
    fn apply(&self, db: &dyn TypeKernelDb, name: &QualifiedName) -> TypeId {
        match self {
            Self::Permissive => db.any(),
            Self::Resolve(resolve) => resolve(name).unwrap_or_else(|| db.any()),
        }
    }
}

/// The documented structure a slot carries, or `None` when the slot lowers exactly as it does
/// today. `None` is the common answer: only an inline structure body adds anything.
///
/// A collection wrapper is deliberately NOT rebuilt here. `Массив из Структура:` already lowers to
/// an array of an untyped structure, so [`substitute`] only has to put this structure where that
/// untyped one stands — rebuilding the wrapper here would nest a second array inside the first.
pub(crate) fn doc_structure_ty(
    db: &dyn TypeKernelDb,
    expr: &DocTypeExpr,
    policy: &SeePolicy<'_>,
) -> Option<DocumentedStructure> {
    match expr {
        DocTypeExpr::Structure { fields } if !fields.is_empty() => {
            Some(DocumentedStructure { ty: structure_ty(db, fields, policy), depth: 0 })
        }
        DocTypeExpr::Array(inner) => doc_structure_ty(db, inner, policy)
            .map(|documented| DocumentedStructure { depth: documented.depth + 1, ..documented }),
        // A `См.` slot keeps the permissive `Any` the existing lowering gives it, a plain type
        // keeps its existing lowering, and a structure without documented fields is the untyped
        // structure it always was.
        DocTypeExpr::Structure { .. } | DocTypeExpr::TypeRef(_) | DocTypeExpr::See(_) => None,
    }
}

/// Puts a documented structure where the untyped one stands in an already lowered slot.
///
/// Only ever replaces a structure that has no fields, so the rule cannot narrow a slot: the other
/// union members, the array wrapper and every unrelated type stay exactly as they were. A slot
/// whose declared type holds no untyped structure is left alone — the documentation then disagrees
/// with the declaration, and the declaration wins.
pub(crate) fn substitute(
    db: &dyn TypeKernelDb,
    base: TypeId,
    documented: &DocumentedStructure,
) -> TypeId {
    substitute_at(db, base, documented.ty, documented.depth)
}

/// Union arms are all candidates — the documentation does not say which arm it describes — but the
/// collection depth must match: bullets written under `Массив из Структура` belong to the element
/// of an array, never to a bare structure standing beside it.
fn substitute_at(db: &dyn TypeKernelDb, base: TypeId, structure: TypeId, depth: usize) -> TypeId {
    use bsl_types::kind::TypeKind;
    match db.lookup_type(base) {
        TypeKind::Structure(facet) if depth == 0 && facet.fields.is_none() => structure,
        TypeKind::Array(facet) if depth > 0 => match facet.element {
            Some(element) => {
                let replaced = substitute_at(db, element, structure, depth - 1);
                if replaced == element {
                    base
                } else {
                    db.array(Some(replaced))
                }
            }
            None => base,
        },
        TypeKind::Union(members) => {
            let members = members.clone();
            let replaced: Vec<TypeId> =
                members.iter().map(|member| substitute_at(db, *member, structure, depth)).collect();
            if replaced.iter().zip(members.iter()).all(|(new, old)| new == old) {
                base
            } else {
                db.union(replaced)
            }
        }
        _ => base,
    }
}

/// Puts a structure into every untyped structure the slot carries directly — a bare one, or one
/// standing in a union arm. Used where the richer structure comes from the body rather than from
/// the documentation, so it carries no collection depth of its own.
pub(crate) fn substitute_bare(db: &dyn TypeKernelDb, base: TypeId, structure: TypeId) -> TypeId {
    substitute_at(db, base, structure, 0)
}

/// The structures carrying fields that a type holds directly: itself, or the arms of a union.
/// A method that returns `Неопределено` on one path and a structure on another has both in one
/// type, and only the structure can fill a documented slot.
pub(crate) fn structures_with_fields(db: &dyn TypeKernelDb, ty: TypeId) -> Vec<TypeId> {
    use bsl_types::kind::TypeKind;
    match db.lookup_type(ty) {
        TypeKind::Structure(facet) if facet.fields.is_some() => vec![ty],
        TypeKind::Union(members) => {
            members.iter().flat_map(|member| structures_with_fields(db, *member)).collect()
        }
        _ => Vec::new(),
    }
}

/// Whether a type carries a structure whose keys were proved by a body rather than declared in
/// documentation. Such keys are the only thing a caller has for that slot, so nothing documented
/// may replace them — documentation adds and never removes.
pub(crate) fn has_body_proven_structure(db: &dyn TypeKernelDb, ty: TypeId) -> bool {
    use bsl_types::kind::TypeKind;
    match db.lookup_type(ty) {
        TypeKind::Structure(facet) => facet
            .fields
            .as_ref()
            .is_some_and(|projection| projection.origin == ProjectionOrigin::StructureLiteral),
        TypeKind::Union(members) => {
            members.iter().any(|member| has_body_proven_structure(db, *member))
        }
        _ => false,
    }
}

/// Whether the slot has an untyped structure that [`substitute_bare`] would fill.
pub(crate) fn has_bare_untyped_structure(db: &dyn TypeKernelDb, ty: TypeId) -> bool {
    use bsl_types::kind::TypeKind;
    match db.lookup_type(ty) {
        TypeKind::Structure(facet) => facet.fields.is_none(),
        TypeKind::Union(members) => {
            members.iter().any(|member| has_bare_untyped_structure(db, *member))
        }
        _ => false,
    }
}

/// Whether a lowered type carries a documented structure anywhere a member lookup can reach it:
/// directly, as an array element, or in one arm of a union. The union arm is the common shape —
/// `Параметры - Неопределено, Структура:` documents a structure beside its absent value.
pub(crate) fn is_doc_structure(db: &dyn TypeKernelDb, ty: TypeId) -> bool {
    use bsl_types::kind::TypeKind;
    match db.lookup_type(ty) {
        TypeKind::Structure(facet) => facet.origin == Some(TypeOrigin::DocComment),
        TypeKind::Array(facet) => {
            facet.element.is_some_and(|element| is_doc_structure(db, element))
        }
        TypeKind::Union(members) => members.iter().any(|member| is_doc_structure(db, *member)),
        _ => false,
    }
}

fn structure_ty(db: &dyn TypeKernelDb, fields: &[DocField], policy: &SeePolicy<'_>) -> TypeId {
    let lowered: Vec<ProjectionField> = fields
        .iter()
        .map(|field| {
            ProjectionField::new(
                field.name.clone(),
                field_ty(db, &field.types, policy),
                ProjectionFieldSource::DocComment,
            )
        })
        .collect();
    let projection = Projection::new(lowered.into(), ProjectionOrigin::DocComment, None);
    db.structure_typed(Arc::new(projection), TypeOrigin::DocComment)
}

/// The type of the alternatives documented for one slot or one field. Unlike [`doc_structure_ty`]
/// this always answers: an alternative whose type cannot be named is `Unknown`, which the field
/// lookup already treats permissively.
pub(crate) fn field_ty(
    db: &dyn TypeKernelDb,
    exprs: &[DocTypeExpr],
    policy: &SeePolicy<'_>,
) -> TypeId {
    let types: Vec<TypeId> = exprs.iter().map(|expr| lower_expr(db, expr, policy)).collect();
    match types.len() {
        0 => db.unknown(),
        1 => types[0],
        _ => db.union(types),
    }
}

/// Lowers one documented type expression whole, wherever it stands — a slot's alternative, a
/// field's alternative, the element under an array. A reference is reached at any of those
/// depths, so the policy travels the whole traversal rather than being consulted at the root.
pub(crate) fn lower_expr(
    db: &dyn TypeKernelDb,
    expr: &DocTypeExpr,
    policy: &SeePolicy<'_>,
) -> TypeId {
    match expr {
        DocTypeExpr::TypeRef(type_ref) => TyLoweringContext::new().lower_type_ref_id(db, type_ref),
        DocTypeExpr::See(name) => policy.apply(db, name),
        DocTypeExpr::Structure { fields } if !fields.is_empty() => structure_ty(db, fields, policy),
        DocTypeExpr::Structure { .. } => db.structure(None),
        DocTypeExpr::Array(inner) => db.array(Some(lower_expr(db, inner, policy))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_types::kind::TypeKind;
    use bsl_types::testing::InMemoryDb;

    fn documented(db: &InMemoryDb) -> TypeId {
        let field = ProjectionField::new(
            "Адрес".to_string(),
            db.string(None, false),
            ProjectionFieldSource::DocComment,
        );
        let projection = Projection::new(vec![field].into(), ProjectionOrigin::DocComment, None);
        db.structure_typed(Arc::new(projection), TypeOrigin::DocComment)
    }

    fn bare(ty: TypeId) -> DocumentedStructure {
        DocumentedStructure { ty, depth: 0 }
    }

    fn in_array(ty: TypeId) -> DocumentedStructure {
        DocumentedStructure { ty, depth: 1 }
    }

    fn see(target: &str) -> DocTypeExpr {
        let segments = target.split('.').map(hir_def::Name::new);
        DocTypeExpr::See(QualifiedName::from_segments(segments))
    }

    #[test]
    fn a_reference_stays_permissive_at_every_depth_by_default() {
        // The default must be the top type wherever the reference stands. `Unknown` would be
        // dropped from a union by the kernel and would narrow the slot to whatever was documented
        // beside the reference — the shape of defect that once produced 903 false positives.
        let db = InMemoryDb::new();
        let permissive = SeePolicy::Permissive;

        assert_eq!(lower_expr(&db, &see("База.Создать"), &permissive), db.any());
        assert_eq!(
            lower_expr(&db, &DocTypeExpr::Array(Box::new(see("База.Создать"))), &permissive),
            db.array(Some(db.any())),
        );
    }

    #[test]
    fn a_resolving_policy_is_consulted_at_every_depth() {
        // The whole point of one traversal: a reference is reached at the root, under an array and
        // inside a field of a documented structure. A policy consulted only at the root leaves the
        // other two forms exactly as they were, and the feature silently covers one case of three.
        let db = InMemoryDb::new();
        let resolved = documented(&db);
        let resolve = |_: &QualifiedName| Some(resolved);
        let policy = SeePolicy::Resolve(&resolve);

        assert_eq!(lower_expr(&db, &see("База.Создать"), &policy), resolved, "в корне слота");
        assert_eq!(
            lower_expr(&db, &DocTypeExpr::Array(Box::new(see("База.Создать"))), &policy),
            db.array(Some(resolved)),
            "под массивом",
        );

        let in_field = DocTypeExpr::Structure {
            fields: vec![DocField {
                name: "Ключ".to_string(), types: vec![see("База.Создать")]
            }],
        };
        let TypeKind::Structure(facet) = db.lookup_type(lower_expr(&db, &in_field, &policy)) else {
            panic!("ожидалась структура");
        };
        let fields = facet.fields.as_ref().expect("поля документированы");
        assert_eq!(fields.fields[0].ty, resolved, "в поле структуры");
    }

    #[test]
    fn an_unresolvable_reference_falls_back_to_the_permissive_type() {
        // A policy that answers `None` must leave the slot exactly where the default leaves it.
        let db = InMemoryDb::new();
        let resolve = |_: &QualifiedName| None;

        assert_eq!(lower_expr(&db, &see("Нет.Такого"), &SeePolicy::Resolve(&resolve)), db.any());
    }

    #[test]
    fn substitute_fills_a_bare_untyped_structure() {
        let db = InMemoryDb::new();
        let rich = documented(&db);
        assert_eq!(substitute(&db, db.structure(None), &bare(rich)), rich);
    }

    #[test]
    fn substitute_keeps_every_other_union_member() {
        // The whole no-narrowing guarantee lives here: filling in the structure must not cost the
        // slot its `Неопределено` arm, or every guarded caller starts reporting a mismatch.
        let db = InMemoryDb::new();
        let rich = documented(&db);
        let base = db.union(vec![db.undefined(), db.structure(None)]);

        let result = substitute(&db, base, &bare(rich));

        let TypeKind::Union(members) = db.lookup_type(result) else {
            panic!("expected a union, got {:?}", db.lookup_type(result));
        };
        assert_eq!(members.len(), 2);
        assert!(members.contains(&db.undefined()), "the Неопределено arm must survive");
        assert!(members.contains(&rich), "the structure arm must carry the fields");
    }

    #[test]
    fn substitute_reaches_the_element_of_an_array() {
        let db = InMemoryDb::new();
        let rich = documented(&db);
        let base = db.array(Some(db.structure(None)));

        let result = substitute(&db, base, &in_array(rich));

        assert_eq!(result, db.array(Some(rich)), "the array wrapper stays, its element is filled");
    }

    #[test]
    fn substitute_leaves_a_slot_that_declares_something_else() {
        // Documentation may fill in a structure, never replace a declared type with one.
        let db = InMemoryDb::new();
        let rich = documented(&db);
        let base = db.number(None, None);

        assert_eq!(substitute(&db, base, &bare(rich)), base);
    }

    #[test]
    fn substitute_leaves_a_structure_that_already_has_fields() {
        let db = InMemoryDb::new();
        let rich = documented(&db);
        assert_eq!(substitute(&db, rich, &bare(db.structure(None))), rich);
    }

    #[test]
    fn fields_of_an_array_element_stay_out_of_a_bare_structure_beside_it() {
        // `Структура` in one arm and `Массив из Структура:` with bullets in the next: the bullets
        // describe the element, and the bare arm must come out of this untouched.
        let db = InMemoryDb::new();
        let rich = documented(&db);
        let base = db.union(vec![db.structure(None), db.array(Some(db.structure(None)))]);

        let result = substitute(&db, base, &in_array(rich));

        let TypeKind::Union(members) = db.lookup_type(result) else {
            panic!("expected a union, got {:?}", db.lookup_type(result));
        };
        assert!(members.contains(&db.structure(None)), "the bare arm keeps no foreign fields");
        assert!(members.contains(&db.array(Some(rich))), "the array element carries the fields");
    }

    #[test]
    fn fields_of_a_bare_structure_stay_out_of_an_array_element() {
        let db = InMemoryDb::new();
        let rich = documented(&db);
        let base = db.array(Some(db.structure(None)));

        assert_eq!(substitute(&db, base, &bare(rich)), base);
    }

    #[test]
    fn a_union_arm_makes_the_slot_a_documented_structure() {
        // `Параметры - Неопределено, Структура:` is the shape most parameters are documented in;
        // without this the seeding step skips exactly the common case.
        let db = InMemoryDb::new();
        let rich = documented(&db);

        assert!(is_doc_structure(&db, db.union(vec![db.undefined(), rich])));
        assert!(!is_doc_structure(&db, db.union(vec![db.undefined(), db.structure(None)])));
    }
}
