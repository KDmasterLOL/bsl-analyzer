use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::{MetadataKind, TypeId, TypeKind};

pub fn is_assignable(db: &dyn TypeKernelDb, from: TypeId, to: TypeId) -> bool {
    if from == to {
        return true;
    }

    let from_kind = db.lookup_type(from);
    let to_kind = db.lookup_type(to);

    if matches!(from_kind, TypeKind::Unknown | TypeKind::Any)
        || matches!(to_kind, TypeKind::Unknown | TypeKind::Any)
    {
        return true;
    }

    if matches!(from_kind, TypeKind::Never) {
        return true;
    }

    if let TypeKind::Union(parts) = from_kind {
        return parts.iter().all(|p| is_assignable(db, *p, to));
    }
    if let TypeKind::Union(parts) = to_kind {
        return parts.iter().any(|p| is_assignable(db, from, *p));
    }

    if matches!(from_kind, TypeKind::Null) && is_ref_kind(to_kind) {
        return true;
    }

    if matches!(to_kind, TypeKind::AnyRef) && is_ref_kind(from_kind) {
        return true;
    }
    if let TypeKind::AnyMetadataRef { mdo_type } = to_kind {
        if let TypeKind::MetadataRef(facet) = from_kind {
            if facet.kind.ref_mdo_type() == Some(*mdo_type) {
                return true;
            }
        }
    }

    if is_tabular_row_bridge(from_kind, to_kind) {
        return true;
    }

    if is_array_bridge(db, from_kind, to_kind) {
        return true;
    }

    if matches!(from_kind, TypeKind::ThisObject { .. } | TypeKind::ThisManager { .. }) {
        if let Some(coerced) = crate::this_object::coerce_to_metadata_ref_id(db, from) {
            if coerced == to {
                return true;
            }
        }
    }

    if let (TypeKind::Function(from_fn), TypeKind::Function(to_fn)) = (from_kind, to_kind) {
        if from_fn.params.len() != to_fn.params.len() {
            return false;
        }
        let params_ok = from_fn
            .params
            .iter()
            .zip(to_fn.params.iter())
            .all(|(from_p, to_p)| is_assignable(db, to_p.ty, from_p.ty));
        return params_ok && is_assignable(db, from_fn.returns, to_fn.returns);
    }

    false
}

fn is_ref_kind(kind: &TypeKind) -> bool {
    match kind {
        TypeKind::MetadataRef(facet) => facet.kind.ref_mdo_type().is_some(),
        TypeKind::AnyMetadataRef { .. } | TypeKind::AnyRef => true,
        _ => false,
    }
}

pub fn is_ref_ty(db: &dyn TypeKernelDb, ty: TypeId) -> bool {
    is_ref_kind(db.lookup_type(ty))
}

fn is_tabular_row_bridge(a: &TypeKind, b: &TypeKind) -> bool {
    fn is_row_metadata_ref(ty: &TypeKind) -> bool {
        matches!(
            ty,
            TypeKind::MetadataRef(facet)
                if matches!(facet.kind, MetadataKind::TabularSectionRow { .. })
        )
    }
    fn is_row_platform_object(ty: &TypeKind) -> bool {
        matches!(ty, TypeKind::PlatformObject(facet)
            if facet.name.eq_ignore_ascii_case("Line of a tabular section")
                || facet.name.to_lowercase() == "строка табличной части")
    }
    (is_row_metadata_ref(a) && is_row_platform_object(b))
        || (is_row_platform_object(a) && is_row_metadata_ref(b))
}

fn is_array_bridge(db: &dyn TypeKernelDb, from: &TypeKind, to: &TypeKind) -> bool {
    match (from, to) {
        (TypeKind::Array(a), TypeKind::Array(b)) => match (a.element, b.element) {
            (Some(_), None) | (None, Some(_)) => true,
            (Some(ae), Some(be)) => is_assignable(db, ae, be),
            (None, None) => false,
        },
        _ => false,
    }
}

pub fn is_coercible_to(db: &dyn TypeKernelDb, from: TypeId, to: TypeId) -> bool {
    if matches!(db.lookup_type(to), TypeKind::String(_)) {
        return true;
    }
    is_assignable(db, from, to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_types::builders::Builders;
    use bsl_types::facet::{ArgArity, FunctionFacet, FunctionOrigin, ParamPassing, ParamSpec};
    use bsl_types::testing::InMemoryDb;
    use std::sync::Arc;

    fn fn_ty(db: &dyn TypeKernelDb, params: Vec<TypeId>, ret: TypeId) -> TypeId {
        let params: Arc<[ParamSpec]> = params
            .into_iter()
            .enumerate()
            .map(|(idx, ty)| {
                ParamSpec::new(format!("p{}", idx + 1), ty, ParamPassing::ByRef, false)
            })
            .collect();
        db.function(FunctionFacet::new(
            params.clone(),
            vec![None; params.len()].into(),
            params.len() as u16,
            ArgArity::Fixed(params.len() as u16),
            ret,
            FunctionOrigin::Unknown,
        ))
    }

    fn metadata_ref_id(db: &dyn TypeKernelDb, kind: MetadataKind, name: &str) -> TypeId {
        db.metadata_ref(kind, name.to_string(), &bsl_types::testing::RootConfigCtx)
    }

    #[test]
    fn function_reflexive() {
        let db = InMemoryDb::new();
        let f = fn_ty(&db, vec![db.number(None, None), db.string(None, false)], db.boolean());
        assert!(is_assignable(&db, f, f));
    }

    #[test]
    fn function_arity_mismatch_is_rejected() {
        let db = InMemoryDb::new();
        let one = fn_ty(&db, vec![db.number(None, None)], db.boolean());
        let two = fn_ty(&db, vec![db.number(None, None), db.string(None, false)], db.boolean());
        assert!(!is_assignable(&db, one, two));
        assert!(!is_assignable(&db, two, one));
    }

    #[test]
    fn function_covariant_return_widens() {
        let db = InMemoryDb::new();
        let narrow = fn_ty(&db, vec![], db.number(None, None));
        let wide =
            fn_ty(&db, vec![], db.union(vec![db.number(None, None), db.string(None, false)]));
        assert!(
            is_assignable(&db, narrow, wide),
            "Number return ≤ Union return (covariant widening)"
        );
        assert!(
            !is_assignable(&db, wide, narrow),
            "Union return ≤ Number return must fail — String leg cannot satisfy Number callers"
        );
    }

    #[test]
    fn function_contravariant_param_widens() {
        let db = InMemoryDb::new();
        let wide_param = fn_ty(
            &db,
            vec![db.union(vec![db.number(None, None), db.string(None, false)])],
            db.boolean(),
        );
        let narrow_param = fn_ty(&db, vec![db.number(None, None)], db.boolean());
        assert!(
            is_assignable(&db, wide_param, narrow_param),
            "Fn(Union) ≤ Fn(Number) (contravariant — wider accepting subtype)"
        );
        assert!(
            !is_assignable(&db, narrow_param, wide_param),
            "Fn(Number) ≤ Fn(Union) must fail — String callers would slip through"
        );
    }

    #[test]
    fn function_mixed_variance() {
        let db = InMemoryDb::new();
        let from = fn_ty(
            &db,
            vec![db.union(vec![db.number(None, None), db.string(None, false)])],
            db.number(None, None),
        );
        let to = fn_ty(
            &db,
            vec![db.number(None, None)],
            db.union(vec![db.number(None, None), db.string(None, false)]),
        );
        assert!(is_assignable(&db, from, to));
        assert!(!is_assignable(&db, to, from));
    }

    #[test]
    fn tabular_row_metadata_ref_assignable_to_platform_object() {
        let db = InMemoryDb::new();
        let row = metadata_ref_id(
            &db,
            MetadataKind::TabularSectionRow { parent: bsl_metadata::MdoType::Catalog },
            "X.Y",
        );
        let generic_ru = db.platform_object("Строка табличной части".to_string());
        let generic_en = db.platform_object("Line of a tabular section".to_string());
        assert!(is_assignable(&db, row, generic_ru));
        assert!(is_assignable(&db, row, generic_en));
    }

    #[test]
    fn tabular_row_platform_object_assignable_to_metadata_ref() {
        let db = InMemoryDb::new();
        let row = metadata_ref_id(
            &db,
            MetadataKind::TabularSectionRow { parent: bsl_metadata::MdoType::Document },
            "X.Y",
        );
        let generic = db.platform_object("Строка табличной части".to_string());
        assert!(is_assignable(&db, generic, row));
    }

    #[test]
    fn tabular_row_bridge_does_not_open_unrelated_platform_objects() {
        let db = InMemoryDb::new();
        let row = metadata_ref_id(
            &db,
            MetadataKind::TabularSectionRow { parent: bsl_metadata::MdoType::Catalog },
            "X.Y",
        );
        let unrelated = db.platform_object("ТаблицаЗначений".to_string());
        assert!(!is_assignable(&db, row, unrelated));
        assert!(!is_assignable(&db, unrelated, row));
    }

    #[test]
    fn coercible_anything_to_string() {
        let db = InMemoryDb::new();
        assert!(is_coercible_to(&db, db.number(None, None), db.string(None, false)));
        assert!(is_coercible_to(
            &db,
            db.date(bsl_types::facet::DateComponent::DateTime),
            db.string(None, false)
        ));
        assert!(is_coercible_to(&db, db.boolean(), db.string(None, false)));
        assert!(is_coercible_to(&db, db.null(), db.string(None, false)));
        assert!(is_coercible_to(&db, db.undefined(), db.string(None, false)));
        assert!(is_coercible_to(
            &db,
            db.union(vec![
                db.number(None, None),
                db.date(bsl_types::facet::DateComponent::DateTime)
            ]),
            db.string(None, false)
        ));
    }

    #[test]
    fn coercion_does_not_open_reverse_direction() {
        let db = InMemoryDb::new();
        assert!(!is_coercible_to(&db, db.string(None, false), db.number(None, None)));
        assert!(!is_coercible_to(
            &db,
            db.string(None, false),
            db.date(bsl_types::facet::DateComponent::DateTime)
        ));
        assert!(!is_coercible_to(&db, db.string(None, false), db.boolean()));
    }

    #[test]
    fn coercion_does_not_leak_into_is_assignable() {
        let db = InMemoryDb::new();
        assert!(!is_assignable(&db, db.number(None, None), db.string(None, false)));
        assert!(!is_assignable(
            &db,
            db.date(bsl_types::facet::DateComponent::DateTime),
            db.string(None, false)
        ));
        assert!(!is_assignable(
            &db,
            db.union(vec![
                db.number(None, None),
                db.date(bsl_types::facet::DateComponent::DateTime)
            ]),
            db.string(None, false)
        ));
    }

    #[test]
    fn typed_array_assignable_to_unparameterised_array() {
        let db = InMemoryDb::new();
        let typed = db.array(Some(db.string(None, false)));
        assert!(is_assignable(&db, typed, db.array(None)));
    }

    #[test]
    fn unparameterised_array_assignable_to_typed_array_gradual() {
        let db = InMemoryDb::new();
        assert!(is_assignable(&db, db.array(None), db.array(Some(db.number(None, None)))));
    }

    #[test]
    fn typed_array_covariant_on_element() {
        let db = InMemoryDb::new();
        let narrow = db.array(Some(db.number(None, None)));
        let wide = db.array(Some(db.union(vec![db.number(None, None), db.string(None, false)])));
        assert!(is_assignable(&db, narrow, wide), "TypedArray covariant: Number ≤ Number|String");
        assert!(
            !is_assignable(&db, wide, narrow),
            "TypedArray covariant: Number|String ≰ Number — String leg cannot satisfy Number callers"
        );
    }

    #[test]
    fn typed_array_unrelated_elements_rejected() {
        let db = InMemoryDb::new();
        let str_arr = db.array(Some(db.string(None, false)));
        let num_arr = db.array(Some(db.number(None, None)));
        assert!(!is_assignable(&db, str_arr, num_arr));
        assert!(!is_assignable(&db, num_arr, str_arr));
    }

    #[test]
    fn typed_array_reflexivity_holds() {
        let db = InMemoryDb::new();
        let ta = db.array(Some(db.string(None, false)));
        assert!(is_assignable(&db, ta, ta));
    }

    #[test]
    fn typed_array_bridge_does_not_open_unrelated_ty_pairs() {
        let db = InMemoryDb::new();
        assert!(!is_assignable(&db, db.string(None, false), db.array(None)));
        assert!(!is_assignable(&db, db.array(None), db.string(None, false)));
        assert!(!is_assignable(&db, db.number(None, None), db.array(Some(db.number(None, None)))));
    }

    #[test]
    fn function_unknown_short_circuit_wins_over_variance() {
        let db = InMemoryDb::new();
        let f = fn_ty(&db, vec![db.number(None, None)], db.boolean());
        assert!(is_assignable(&db, db.unknown(), f));
        assert!(is_assignable(&db, f, db.unknown()));
    }

    #[test]
    fn null_assignable_to_ref_types() {
        let db = InMemoryDb::new();
        let cat_ref = metadata_ref_id(&db, MetadataKind::CatalogRef, "Контрагенты");
        assert!(is_assignable(&db, db.null(), cat_ref));
        assert!(!is_assignable(&db, db.null(), db.number(None, None)));
    }

    #[test]
    fn any_is_universal_both_directions() {
        let db = InMemoryDb::new();
        let any = db.any();
        let number = db.number(None, None);
        assert!(is_assignable(&db, number, any), "A ≤ Any (universal top)");
        assert!(is_assignable(&db, any, number), "Any ≤ A (universal, gradual)");
    }

    #[test]
    fn concrete_ref_assignable_to_any_ref() {
        let db = InMemoryDb::new();
        let cat = metadata_ref_id(&db, MetadataKind::CatalogRef, "Контрагенты");
        let doc = metadata_ref_id(&db, MetadataKind::DocumentRef, "ПКО");
        assert!(is_assignable(&db, cat, db.any_ref()));
        assert!(is_assignable(&db, doc, db.any_ref()));
    }

    #[test]
    fn any_metadata_ref_assignable_to_any_ref() {
        let db = InMemoryDb::new();
        assert!(is_assignable(
            &db,
            db.any_metadata_ref(bsl_metadata::MdoType::Catalog),
            db.any_ref()
        ));
    }

    #[test]
    fn any_ref_not_assignable_to_concrete_ref() {
        let db = InMemoryDb::new();
        let cat = metadata_ref_id(&db, MetadataKind::CatalogRef, "Контрагенты");
        assert!(!is_assignable(&db, db.any_ref(), cat));
        assert!(!is_assignable(
            &db,
            db.any_ref(),
            db.any_metadata_ref(bsl_metadata::MdoType::Catalog)
        ));
    }

    #[test]
    fn concrete_ref_assignable_to_matching_flavour() {
        let db = InMemoryDb::new();
        let cat = metadata_ref_id(&db, MetadataKind::CatalogRef, "Контрагенты");
        let doc = metadata_ref_id(&db, MetadataKind::DocumentRef, "ПКО");
        let any_catalog = db.any_metadata_ref(bsl_metadata::MdoType::Catalog);
        assert!(is_assignable(&db, cat, any_catalog), "CatalogRef ≤ AnyMetadataRef{{Catalog}}");
        assert!(
            !is_assignable(&db, doc, any_catalog),
            "DocumentRef ≰ AnyMetadataRef{{Catalog}} — wrong flavour"
        );
    }

    #[test]
    fn any_metadata_ref_not_assignable_to_concrete_ref() {
        let db = InMemoryDb::new();
        let cat = metadata_ref_id(&db, MetadataKind::CatalogRef, "Контрагенты");
        let any_catalog = db.any_metadata_ref(bsl_metadata::MdoType::Catalog);
        assert!(!is_assignable(&db, any_catalog, cat));
    }

    #[test]
    fn null_assignable_to_any_ref() {
        let db = InMemoryDb::new();
        assert!(is_assignable(&db, db.null(), db.any_ref()));
        assert!(is_assignable(&db, db.null(), db.any_metadata_ref(bsl_metadata::MdoType::Catalog)));
    }

    #[test]
    fn number_not_assignable_to_any_ref() {
        let db = InMemoryDb::new();
        assert!(!is_assignable(&db, db.number(None, None), db.any_ref()));
        assert!(!is_assignable(&db, db.string(None, false), db.any_ref()));
    }

    #[test]
    fn never_is_bottom() {
        let db = InMemoryDb::new();
        let never = db.never();
        let number = db.number(None, None);
        assert!(is_assignable(&db, never, number), "Never ≤ A (bottom)");
        assert!(!is_assignable(&db, number, never), "A ≤ Never must fail (not reflexive)");
        assert!(is_assignable(&db, never, never), "Never ≤ Never (reflexive)");
    }
}
