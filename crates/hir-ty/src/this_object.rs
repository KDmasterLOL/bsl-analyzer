//! `Ty::ThisObject` → `Ty::MetadataRef` coercion used by field / method
//! lookup adapters.
//!
//! `Ty::ThisObject { owner: (kind, name) }` models the `ЭтотОбъект` /
//! `ThisObject` receiver. It carries the MDO provenance so diagnostics
//! and rename/refactor features can tell "explicitly self-referential"
//! apart from an arbitrary `CatalogObject` reference. For field and
//! method lookup the provenance is irrelevant — the receiver behaves
//! exactly like its MDO-object counterpart. This module applies that
//! coercion at the entry of each adapter so downstream logic stays
//! ignorant of `ThisObject` entirely.
//!
//! # Coverage
//!
//! Only MDO kinds that have a dedicated `*Object` companion in
//! [`MetadataKind`] are coercible: `Catalog`, `Document`, `ExchangePlan`,
//! `ChartOfAccounts`. Forms, record sets, command modules, and common
//! modules all fall through to `None` — their receiver surfaces are out
//! of scope for Task 5 and land as `Ty::Unknown`.
//!
//! # Why a helper
//!
//! Both `field_lookup::lookup_field` and `method_lookup::lookup_method`
//! apply the same coercion. Inlining the match twice would drift; a
//! single source of truth catches new `MetadataKind` variants with one
//! compile error instead of two.

use hir_def::ty::{MetadataKind, Ty};

/// If `receiver_ty` is a [`Ty::ThisObject`] whose owner kind has an
/// `*Object` companion in [`MetadataKind`], return the equivalent
/// [`Ty::MetadataRef`]. Otherwise return `None`.
///
/// Callers should treat a `Some(coerced)` result as the receiver for
/// the rest of the lookup: `let receiver = coerce_to_metadata_ref(ty)
/// .as_ref().unwrap_or(ty);`.
///
/// The `(MdoType → *Object MetadataKind)` mapping is owned by
/// [`MetadataKind::object_kind_for`] — same table the resolver uses to
/// gate `Ty::ThisObject` construction. Keeping both sides of the
/// coercion pipeline on a single table means a new `*Object` variant
/// (future `TaskObject`, `BusinessProcessObject`, …) needs exactly one
/// edit instead of two.
pub fn coerce_to_metadata_ref(receiver_ty: &Ty) -> Option<Ty> {
    let Ty::ThisObject { owner: (kind, name) } = receiver_ty else {
        return None;
    };

    let object_kind = MetadataKind::object_kind_for(*kind)?;

    Some(Ty::MetadataRef { kind: object_kind, name: name.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::MdoType;
    use hir_def::Name;

    #[test]
    fn coerces_catalog_this_object_to_catalog_object() {
        // The canonical case — `ЭтотОбъект` inside a Catalog
        // ObjectModule yields a `CatalogObject` ref.
        let owner = (MdoType::Catalog, Name::new("Номенклатура"));
        let coerced = coerce_to_metadata_ref(&Ty::ThisObject { owner }).expect("catalog coerces");
        assert_eq!(
            coerced,
            Ty::MetadataRef {
                kind: MetadataKind::CatalogObject, name: Name::new("Номенклатура")
            }
        );
    }

    #[test]
    fn coerces_document_this_object_to_document_object() {
        let owner = (MdoType::Document, Name::new("ПКО"));
        let coerced = coerce_to_metadata_ref(&Ty::ThisObject { owner }).expect("document coerces");
        assert_eq!(
            coerced,
            Ty::MetadataRef { kind: MetadataKind::DocumentObject, name: Name::new("ПКО") }
        );
    }

    #[test]
    fn coerces_exchange_plan_this_object_to_exchange_plan_object() {
        // Task 2b added `ExchangePlanObject` — proves the coercion
        // table tracks the full `*Object` set, not just Catalog /
        // Document.
        let owner = (MdoType::ExchangePlan, Name::new("Обмен"));
        let coerced = coerce_to_metadata_ref(&Ty::ThisObject { owner }).expect("exchange plan");
        assert_eq!(
            coerced,
            Ty::MetadataRef {
                kind: MetadataKind::ExchangePlanObject, name: Name::new("Обмен")
            }
        );
    }

    #[test]
    fn coerces_chart_of_accounts_this_object_to_chart_of_accounts_object() {
        let owner = (MdoType::ChartOfAccounts, Name::new("Основной"));
        let coerced = coerce_to_metadata_ref(&Ty::ThisObject { owner }).expect("chart of accounts");
        assert_eq!(
            coerced,
            Ty::MetadataRef {
                kind: MetadataKind::ChartOfAccountsObject,
                name: Name::new("Основной"),
            }
        );
    }

    #[test]
    fn no_coercion_for_non_object_kinds() {
        // Register flavours have `*RecordSet` / `*RecordManager`, not
        // `*Object`. Task 5 deliberately leaves them out of scope —
        // the coercion must stay `None` until a dedicated receiver
        // surface lands.
        let owner = (MdoType::InformationRegister, Name::new("РС"));
        assert!(coerce_to_metadata_ref(&Ty::ThisObject { owner }).is_none());

        // BusinessProcess / Task have `*Ref` but no `*Object` — again
        // out of scope.
        let owner_bp = (MdoType::BusinessProcess, Name::new("Процесс"));
        assert!(coerce_to_metadata_ref(&Ty::ThisObject { owner: owner_bp }).is_none());
    }

    #[test]
    fn no_coercion_for_non_this_object_receivers() {
        // The helper must stay a no-op for every other `Ty` — it is
        // called at every adapter entry, so a stray `Some(_)` here
        // would silently rewrite receivers the adapters are supposed
        // to handle directly.
        assert!(coerce_to_metadata_ref(&Ty::Number).is_none());
        assert!(coerce_to_metadata_ref(&Ty::Unknown).is_none());
        assert!(coerce_to_metadata_ref(&Ty::MetadataRef {
            kind: MetadataKind::CatalogRef,
            name: Name::new("X"),
        })
        .is_none());
    }
}
