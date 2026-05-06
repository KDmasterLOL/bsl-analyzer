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
//! Coercible MDO kinds — those with a dedicated `*Object` companion in
//! [`MetadataKind`] (single source of truth: [`MetadataKind::object_kind_for`]):
//! `Catalog`, `Document`, `ExchangePlan`, `ChartOfAccounts`, `Task`,
//! `BusinessProcess`, `DataProcessor`, `Report`. Form modules, record
//! sets, command modules, common modules, registers, and enums all fall
//! through to `None` — their receiver surfaces don't have an `*Object`
//! shape and land as `Ty::Unknown`.
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
/// needs exactly one edit instead of two.
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
        // `*Object`, and `MetadataKind::object_kind_for` rejects them.
        // The coercion must stay `None` until a dedicated receiver
        // surface lands.
        for mdo in [
            MdoType::InformationRegister,
            MdoType::AccumulationRegister,
            MdoType::AccountingRegister,
            MdoType::CalculationRegister,
            MdoType::Enum,
        ] {
            let owner = (mdo, Name::new("X"));
            assert!(
                coerce_to_metadata_ref(&Ty::ThisObject { owner }).is_none(),
                "expected no coercion for {mdo:?}"
            );
        }
    }

    #[test]
    fn coerces_business_process_and_task_and_data_processor_and_report_this_object() {
        // The four MDO kinds that joined the *Object set together with
        // form `Объект` projection: each must coerce to its `*Object`
        // MetadataRef so field/method dispatch works inside ObjectModule
        // and through the FormData wrapper.
        for (mdo, expected_kind) in [
            (MdoType::BusinessProcess, MetadataKind::BusinessProcessObject),
            (MdoType::Task, MetadataKind::TaskObject),
            (MdoType::DataProcessor, MetadataKind::DataProcessorObject),
            (MdoType::Report, MetadataKind::ReportObject),
        ] {
            let owner = (mdo, Name::new("X"));
            let coerced = coerce_to_metadata_ref(&Ty::ThisObject { owner })
                .unwrap_or_else(|| panic!("must coerce {mdo:?}"));
            assert_eq!(
                coerced,
                Ty::MetadataRef { kind: expected_kind, name: Name::new("X") },
                "{mdo:?} must coerce to {expected_kind:?}"
            );
        }
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
