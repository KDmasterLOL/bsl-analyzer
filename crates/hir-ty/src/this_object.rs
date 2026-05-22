//! `Ty::ThisObject` / `Ty::ThisManager` → dispatch-ready Ty coercion
//! used by field / method lookup adapters.
//!
//! `Ty::ThisObject { owner: (kind, name) }` models `ЭтотОбъект` inside
//! an `ObjectModule`; `Ty::ThisManager { owner: (kind, name) }` models
//! the same identifier inside a `ManagerModule`. Both carry the MDO
//! provenance so diagnostics and rename/refactor features can tell
//! "explicitly self-referential" apart from an arbitrary
//! `CatalogObject` / `Справочники.Номенклатура` reference. For field
//! and method lookup the provenance is irrelevant — each receiver
//! behaves exactly like its dispatch counterpart:
//!
//! - `ThisObject` → `Ty::MetadataRef { kind: <*Object>, name }` —
//!   the same shape `Записать()` / `ПолучитьОбъект()` already operate
//!   on inside an `ObjectModule`.
//! - `ThisManager` → `Ty::ObjectManager { kind: <MdoType>, name }` —
//!   the same shape the user reaches via `Справочники.<Name>`. Method
//!   dispatch then routes through `bsl_platform::get_manager_methods`
//!   keyed on `MdoType` (and the workspace `ManagerModule.bsl`
//!   resolver) — `MetadataKind` deliberately has *no* `*Manager` arm,
//!   the manager axis lives entirely on `MdoType`.
//!
//! This module applies both coercions at the entry of each adapter so
//! downstream logic stays ignorant of `ThisObject` / `ThisManager`
//! entirely.
//!
//! # Coverage
//!
//! Coercible MDO kinds:
//!
//! - `ThisObject` — those with a dedicated `*Object` companion in
//!   [`MetadataKind`] (single source of truth:
//!   [`MetadataKind::object_kind_for`]): `Catalog`, `Document`,
//!   `ExchangePlan`, `ChartOfAccounts`, `Task`, `BusinessProcess`,
//!   `DataProcessor`, `Report`. Form modules, record sets, command
//!   modules, common modules, registers, and enums all fall through
//!   to `None`.
//! - `ThisManager` — those with a manager surface (gated by
//!   `MdoType::manager_type_prefix() != None`, same table
//!   `Ty::ManagerCollection` factory uses). Both
//!   `this_object::resolve_this_manager_owner` and this coercion read that
//!   single gate, so a new MDO with a manager surface grows
//!   `ЭтотОбъект` support automatically.
//!
//! # Why a helper
//!
//! Both `field_lookup::lookup_field` and `method_lookup::lookup_method`
//! apply the same coercion. Inlining the match twice would drift; a
//! single source of truth catches new variants with one compile error
//! instead of two.

use bsl_metadata::{MdoType, ModuleType};
use hir_def::resolver::Resolver;
use hir_def::ty::{MetadataKind, Ty};
use hir_def::{DefDatabase, Name};

/// Coerce `ЭтотОбъект` receivers to their dispatch-ready Ty.
///
/// Returns `Some(coerced)` for both [`Ty::ThisObject`] (→
/// `Ty::MetadataRef { *Object, name }`) and [`Ty::ThisManager`] (→
/// `Ty::ObjectManager { kind, name }`); `None` for everything else
/// **and** for `This*` receivers whose MDO kind has no dispatch
/// counterpart (e.g. `ThisObject` for a register flavour).
///
/// Callers should treat a `Some(coerced)` result as the receiver for
/// the rest of the lookup: `let receiver = coerce_to_metadata_ref(ty)
/// .as_ref().unwrap_or(ty);`.
///
/// The function name retains the historical "_to_metadata_ref" suffix
/// even though `ThisManager` coerces to `Ty::ObjectManager` (not
/// `Ty::MetadataRef`) — the original naming reflects the
/// `ObjectModule`-only world before Step J. Renaming is a separate
/// cleanup; the contract is "receiver-position coercion to a
/// dispatch-ready Ty" and that has not changed.
pub fn coerce_to_metadata_ref(receiver_ty: &Ty) -> Option<Ty> {
    match receiver_ty {
        Ty::ThisObject { owner: (kind, name) } => {
            let object_kind = MetadataKind::object_kind_for(*kind)?;
            Some(Ty::MetadataRef { kind: object_kind, name: name.clone() })
        }
        Ty::ThisManager { owner: (kind, name) } => {
            // Gate on the same table `resolve_this_manager_owner` uses, so
            // an unreachable case here would only fire if a caller
            // bypassed the resolver and synthesised a `ThisManager`
            // for an MDO without a manager surface.
            kind.manager_type_prefix()?;
            Some(Ty::ObjectManager { kind: *kind, name: name.clone() })
        }
        _ => None,
    }
}

/// Resolve `ЭтотОбъект` / `ThisObject` inside an `ObjectModule.bsl`.
///
/// Reads the owner-module metadata via [`DefDatabase::module_metadata`]
/// and returns `Some((mdo_type, name))` only when **both** conditions
/// hold:
///
/// 1. The module is an `ObjectModule` — the single module type where
///    `ЭтотОбъект` semantically means "the current MDO as a `*Object`
///    reference" (record-set / form / manager / common / command
///    modules have their own `ЭтотОбъект` semantics).
/// 2. The MDO flavour has a matching `*Object` companion in
///    [`MetadataKind`] (checked via [`MetadataKind::object_kind_for`]).
///    Without a coercion target the downstream `FieldLookup` /
///    `MethodLookup` adapters have nothing to resolve against, so a
///    `Ty::ThisObject` constructed here would dangle.
///
/// Covered kinds today: `Catalog`, `Document`, `ExchangePlan`,
/// `ChartOfAccounts`, `Task`, `BusinessProcess`, `DataProcessor`,
/// `Report`. `ChartOfCharacteristicTypes`, registers, and enums still
/// sit in this gap — their ObjectModule `ЭтотОбъект` stays
/// `Ty::Unknown` until dedicated `*Object` variants land.
///
/// # Why in `hir-ty` (and not `Resolver`)
///
/// This is a **type-system** decision (`MetadataKind`-gated, produces
/// the seed for `Ty::ThisObject`), not name resolution. The function
/// reads the `Resolver`'s current module scope as input but returns
/// type-system entities. Keeping it in `hir-ty` matches the layer
/// rule from `CLAUDE.md`: `hir-def` is for syntactic / scope
/// decisions; type-aware decisions live in `hir-ty`.
pub fn resolve_this_object_owner(
    db: &dyn DefDatabase,
    resolver: &Resolver,
) -> Option<(MdoType, Name)> {
    let module_id = resolver.module_id()?;
    let metadata = db.module_metadata(module_id);
    let mdo = metadata.mdo.as_ref()?;

    if metadata.module_type != ModuleType::ObjectModule {
        return None;
    }

    MetadataKind::object_kind_for(mdo.mdo_type)?;

    Some((mdo.mdo_type, Name::new(&mdo.name)))
}

/// Resolve `ЭтотОбъект` / `ThisObject` inside a `ManagerModule.bsl`.
///
/// Sibling of [`resolve_this_object_owner`] for the manager axis.
/// Returns `Some((MdoType, Name))` only when the resolver's enclosing
/// module is a `ModuleType::ManagerModule` whose MDO has a manager
/// surface — gated on [`MdoType::manager_type_prefix`] returning
/// `Some(_)`, the same table that `Ty::ManagerCollection` factory uses,
/// so a flavour without a manager (constants, common modules, forms,
/// HTTP-services, web-services, event subscriptions, scheduled jobs …)
/// returns `None` rather than dangle a `Ty::ThisManager` no adapter
/// can dispatch.
///
/// Two storage slots: `metadata.mdo` for non-register flavours
/// (Catalog, Document, ChartOfAccounts, …); `metadata.register` for
/// the four register flavours (Information / Accumulation / Accounting
/// / Calculation), where `metadata.mdo` stays `None`. Both carry the
/// `(MdoType, name)` pair this gate needs.
pub fn resolve_this_manager_owner(
    db: &dyn DefDatabase,
    resolver: &Resolver,
) -> Option<(MdoType, Name)> {
    let module_id = resolver.module_id()?;
    let metadata = db.module_metadata(module_id);

    if metadata.module_type != ModuleType::ManagerModule {
        return None;
    }

    let (mdo_type, name) = match (metadata.mdo.as_ref(), metadata.register.as_ref()) {
        (Some(mdo), _) => (mdo.mdo_type, Name::new(&mdo.name)),
        (None, Some(reg)) => (reg.mdo_type(), Name::new(reg.name())),
        (None, None) => return None,
    };

    mdo_type.manager_type_prefix()?;

    Some((mdo_type, name))
}

/// Resolve `ЭтотОбъект` / `ThisObject` inside
/// `<Register>/Ext/RecordSetModule.bsl`.
///
/// Returns `Some((MdoType, Name))` only when the enclosing module is
/// `ModuleType::RecordSetModule` whose MDO is one of the four register
/// flavours — gated through [`MetadataKind::record_set_kind_for`] so
/// the downstream `*RecordSet` companion always exists.
///
/// Two storage slots, same as [`resolve_this_manager_owner`]: register
/// flavours populate `metadata.register`, not `metadata.mdo`.
pub fn resolve_this_record_set_owner(
    db: &dyn DefDatabase,
    resolver: &Resolver,
) -> Option<(MdoType, Name)> {
    let module_id = resolver.module_id()?;
    let metadata = db.module_metadata(module_id);

    if metadata.module_type != ModuleType::RecordSetModule {
        return None;
    }

    let (mdo_type, name) = match (metadata.mdo.as_ref(), metadata.register.as_ref()) {
        (Some(mdo), _) => (mdo.mdo_type, Name::new(&mdo.name)),
        (None, Some(reg)) => (reg.mdo_type(), Name::new(reg.name())),
        (None, None) => return None,
    };

    MetadataKind::record_set_kind_for(mdo_type)?;

    Some((mdo_type, name))
}

/// Returns `true` when the resolver's enclosing module is a managed
/// form.
///
/// Sibling to [`resolve_this_object_owner`]: same input shape and
/// parallel module-metadata gate, but answers a different question.
/// `resolve_this_object_owner` returns the `(MdoType, Name)` pair that
/// lets `infer_path_name` build a `Ty::ThisObject` for an object
/// module's `ЭтотОбъект`. Forms have no `MdoType` companion (they live
/// outside the catalog/document/exchange-plan/chart-of-accounts axis),
/// so the form path returns just a flag — the caller maps it to the
/// platform type `ФормаКлиентскогоПриложения` directly.
///
/// Gate is strict: only `ModuleType::FormModule` *and* an attached
/// managed `Form` payload qualifies. Ordinary forms and form modules
/// without a loaded `Form.xml` return `false` (conservative — we'd
/// rather miss type info than mistype an ordinary form as managed).
pub fn is_managed_form_module(db: &dyn DefDatabase, resolver: &Resolver) -> bool {
    let Some(module_id) = resolver.module_id() else { return false };
    let metadata = db.module_metadata(module_id);

    if metadata.module_type != ModuleType::FormModule {
        return false;
    }

    metadata.form.as_ref().is_some_and(|f| f.is_managed())
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
        // `Ty::ObjectManager` is itself a coercion target — a stray
        // `Some(_)` here would mean we coerce a manager-receiver back
        // to itself in a way that confuses the dispatch chain.
        assert!(coerce_to_metadata_ref(&Ty::ObjectManager {
            kind: MdoType::Catalog,
            name: Name::new("X"),
        })
        .is_none());
    }

    // -------------------------------------------------------------
    //  ThisManager coercion (Step J)
    // -------------------------------------------------------------

    #[test]
    fn coerces_catalog_this_manager_to_object_manager() {
        // The canonical case — `ЭтотОбъект` inside a Catalog
        // ManagerModule yields a `Ty::ObjectManager` keyed on
        // `MdoType::Catalog`. Method dispatch then routes through
        // `bsl_platform::get_manager_methods` and the workspace
        // `ManagerModule.bsl` resolver — *not* through any
        // `MetadataKind::CatalogManager` arm (no such arm exists).
        let owner = (MdoType::Catalog, Name::new("Номенклатура"));
        let coerced =
            coerce_to_metadata_ref(&Ty::ThisManager { owner }).expect("catalog manager coerces");
        assert_eq!(
            coerced,
            Ty::ObjectManager {
                kind: MdoType::Catalog, name: Name::new("Номенклатура")
            }
        );
    }

    #[test]
    fn coerces_document_this_manager_to_object_manager() {
        let owner = (MdoType::Document, Name::new("ПКО"));
        let coerced =
            coerce_to_metadata_ref(&Ty::ThisManager { owner }).expect("document manager coerces");
        assert_eq!(coerced, Ty::ObjectManager { kind: MdoType::Document, name: Name::new("ПКО") });
    }

    #[test]
    fn coerces_register_this_manager_to_object_manager() {
        // Registers have a manager surface (`РегистрыСведений.X.<метод>()`)
        // but no `*Object` companion — so they coerce as `ThisManager`
        // here while `ThisObject` for the same MDO would correctly stay
        // `None`. This is the asymmetry that makes a single-discriminator
        // Ty insufficient (Step J architectural choice).
        let owner = (MdoType::InformationRegister, Name::new("Курс"));
        let coerced =
            coerce_to_metadata_ref(&Ty::ThisManager { owner }).expect("register manager coerces");
        assert_eq!(
            coerced,
            Ty::ObjectManager { kind: MdoType::InformationRegister, name: Name::new("Курс") }
        );
        // And `ThisObject` for the same kind must NOT coerce — the
        // resolver's `resolve_this_object_owner` already gates on
        // `MetadataKind::object_kind_for`, but the coercion table is
        // the second wall of defence: a synthesised `ThisObject` on a
        // register kind has no dispatch target.
        assert!(
            coerce_to_metadata_ref(&Ty::ThisObject {
                owner: (MdoType::InformationRegister, Name::new("Курс"))
            })
            .is_none(),
            "register kind has no `*Object` companion"
        );
    }

    #[test]
    fn coerces_business_process_and_task_and_data_processor_and_report_this_manager() {
        // The four MDO kinds that joined the *Object set together also
        // have managers; verify both axes coerce on the same MDO.
        for mdo in
            [MdoType::BusinessProcess, MdoType::Task, MdoType::DataProcessor, MdoType::Report]
        {
            let owner = (mdo, Name::new("X"));
            let coerced = coerce_to_metadata_ref(&Ty::ThisManager { owner })
                .unwrap_or_else(|| panic!("must coerce manager for {mdo:?}"));
            assert_eq!(
                coerced,
                Ty::ObjectManager { kind: mdo, name: Name::new("X") },
                "{mdo:?} ThisManager must coerce to its own ObjectManager"
            );
        }
    }

    #[test]
    fn no_coercion_for_this_manager_on_kinds_without_manager_surface() {
        // `MdoType::CommonModule` has no `manager_type_prefix`, so a
        // synthesised `ThisManager` for that kind has no dispatch
        // target. The coercion table is the second wall of defence —
        // the resolver gate on the same predicate is the first.
        // (At the time of writing this is the only kind without a
        // manager prefix that is reachable through a `ManagerModule.bsl`
        // hierarchy at all, since `Constant`, `Form`, `HTTPService` etc.
        // never *have* a `ManagerModule.bsl` directory; the test still
        // pins the algebraic invariant.)
        let owner = (MdoType::CommonModule, Name::new("X"));
        assert!(
            coerce_to_metadata_ref(&Ty::ThisManager { owner }).is_none(),
            "kinds without a manager surface must not coerce"
        );
    }
}
