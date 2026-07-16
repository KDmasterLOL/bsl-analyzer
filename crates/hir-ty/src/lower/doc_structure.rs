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

/// The documented structure a slot carries, or `None` when the slot lowers exactly as it does
/// today. `None` is the common answer: only an inline structure body adds anything.
///
/// A collection wrapper is deliberately NOT rebuilt here. `Массив из Структура:` already lowers to
/// an array of an untyped structure, so [`substitute`] only has to put this structure where that
/// untyped one stands — rebuilding the wrapper here would nest a second array inside the first.
pub(crate) fn doc_structure_ty(db: &dyn TypeKernelDb, expr: &DocTypeExpr) -> Option<TypeId> {
    match expr {
        DocTypeExpr::Structure { fields } if !fields.is_empty() => Some(structure_ty(db, fields)),
        DocTypeExpr::Array(inner) => doc_structure_ty(db, inner),
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
pub(crate) fn substitute(db: &dyn TypeKernelDb, base: TypeId, structure: TypeId) -> TypeId {
    use bsl_types::kind::TypeKind;
    match db.lookup_type(base) {
        TypeKind::Structure(facet) if facet.fields.is_none() => structure,
        TypeKind::Array(facet) => match facet.element {
            Some(element) => {
                let replaced = substitute(db, element, structure);
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
                members.iter().map(|member| substitute(db, *member, structure)).collect();
            if replaced.iter().zip(members.iter()).all(|(new, old)| new == old) {
                base
            } else {
                db.union(replaced)
            }
        }
        _ => base,
    }
}

/// Whether a lowered type is a documented structure, directly or as an array element.
pub(crate) fn is_doc_structure(db: &dyn TypeKernelDb, ty: TypeId) -> bool {
    use bsl_types::kind::TypeKind;
    match db.lookup_type(ty) {
        TypeKind::Structure(facet) => facet.origin == Some(TypeOrigin::DocComment),
        TypeKind::Array(facet) => {
            facet.element.is_some_and(|element| is_doc_structure(db, element))
        }
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

    #[test]
    fn substitute_fills_a_bare_untyped_structure() {
        let db = InMemoryDb::new();
        let rich = documented(&db);
        assert_eq!(substitute(&db, db.structure(None), rich), rich);
    }

    #[test]
    fn substitute_keeps_every_other_union_member() {
        // The whole no-narrowing guarantee lives here: filling in the structure must not cost the
        // slot its `Неопределено` arm, or every guarded caller starts reporting a mismatch.
        let db = InMemoryDb::new();
        let rich = documented(&db);
        let base = db.union(vec![db.undefined(), db.structure(None)]);

        let result = substitute(&db, base, rich);

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

        let result = substitute(&db, base, rich);

        assert_eq!(result, db.array(Some(rich)), "the array wrapper stays, its element is filled");
    }

    #[test]
    fn substitute_leaves_a_slot_that_declares_something_else() {
        // Documentation may fill in a structure, never replace a declared type with one.
        let db = InMemoryDb::new();
        let rich = documented(&db);
        let base = db.number(None, None);

        assert_eq!(substitute(&db, base, rich), base);
    }

    #[test]
    fn substitute_leaves_a_structure_that_already_has_fields() {
        let db = InMemoryDb::new();
        let rich = documented(&db);
        assert_eq!(substitute(&db, rich, db.structure(None)), rich);
    }
}
