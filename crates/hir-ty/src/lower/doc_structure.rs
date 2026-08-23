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
//! Cross-module `см. Модуль.Метод` is deliberately not resolved here: it stays the permissive
//! `Any` it is today.

use std::sync::Arc;

use bsl_types::builders::Builders;
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::{
    Projection, ProjectionField, ProjectionFieldSource, ProjectionOrigin, TypeId, TypeOrigin,
};
use hir_def::docs::{DocField, DocTypeExpr};

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

/// The documented structure a slot carries, or `None` when the slot lowers exactly as it does
/// today. `None` is the common answer: only an inline structure body adds anything.
///
/// A collection wrapper is deliberately NOT rebuilt here. `Массив из Структура:` already lowers to
/// an array of an untyped structure, so [`substitute`] only has to put this structure where that
/// untyped one stands — rebuilding the wrapper here would nest a second array inside the first.
pub(crate) fn doc_structure_ty(
    db: &dyn TypeKernelDb,
    expr: &DocTypeExpr,
) -> Option<DocumentedStructure> {
    match expr {
        DocTypeExpr::Structure { fields } if !fields.is_empty() => {
            Some(DocumentedStructure { ty: structure_ty(db, fields), depth: 0 })
        }
        DocTypeExpr::Array(inner) => doc_structure_ty(db, inner)
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

fn structure_ty(db: &dyn TypeKernelDb, fields: &[DocField]) -> TypeId {
    let lowered: Vec<ProjectionField> = fields
        .iter()
        .map(|field| {
            ProjectionField::new(
                field.name.clone(),
                field_ty(db, &field.types),
                ProjectionFieldSource::DocComment,
            )
        })
        .collect();
    let projection = Projection::new(lowered.into(), ProjectionOrigin::DocComment, None);
    db.structure_typed(Arc::new(projection), TypeOrigin::DocComment)
}

/// The type of one documented field. Unlike [`doc_structure_ty`] this always answers: a field
/// whose type cannot be named is `Unknown`, which the field lookup already treats permissively.
fn field_ty(db: &dyn TypeKernelDb, exprs: &[DocTypeExpr]) -> TypeId {
    let types: Vec<TypeId> = exprs.iter().map(|expr| one_field_ty(db, expr)).collect();
    match types.len() {
        0 => db.unknown(),
        1 => types[0],
        _ => db.union(types),
    }
}

fn one_field_ty(db: &dyn TypeKernelDb, expr: &DocTypeExpr) -> TypeId {
    match expr {
        DocTypeExpr::TypeRef(type_ref) => TyLoweringContext::new().lower_type_ref_id(db, type_ref),
        // Same reason the doc-type parser lowers `См.` to `Any`: an unresolvable reference must
        // stay the top type, or the kernel drops it from a union and narrows the field to the
        // members that happened to resolve.
        DocTypeExpr::See(_) => db.any(),
        DocTypeExpr::Structure { fields } if !fields.is_empty() => structure_ty(db, fields),
        DocTypeExpr::Structure { .. } => db.structure(None),
        DocTypeExpr::Array(inner) => db.array(Some(one_field_ty(db, inner))),
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
