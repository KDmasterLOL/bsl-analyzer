//! Member lookup on manager-global receivers.
//!
//! `Ty::ManagerCollection(kind)` (`Справочники`, `Документы`, …) and
//! `Ty::ObjectManager { kind, name }` (`Справочники.Валюты`) are the two
//! manager-global shapes that BSL surfaces on the global scope. Both
//! live outside the attribute/method tables used by
//! [`crate::field_lookup`]: their members come from the MDO itself
//! (enum cases, predefined items) or from the visible-configurations'
//! MDO index (when a plural manager is specialised by name).
//!
//! This module is the semantic complement to
//! [`crate::field_lookup`]: both are driven from `Expr::Field` in
//! `infer.rs` and share the same "walk visible configurations, pick
//! latest-wins" iteration order.
//!
//! # Dispatch table
//!
//! | Receiver | `.member` resolves to |
//! |---|---|
//! | `ManagerCollection(kind)` | `ObjectManager { kind, member }` if the MDO named `member` exists under `kind` (either the plain MDO vec or, for register flavours, the registers vec). |
//! | `ObjectManager { Enum, owner }` | `MetadataRef { EnumRef, owner }` when `member` matches `mdo.enum_values`. |
//! | `ObjectManager { Catalog, owner }` | `MetadataRef { CatalogRef, owner }` when `member` matches `mdo.predefined_items`. |
//! | `ObjectManager { ChartOfAccounts, owner }` | `MetadataRef { ChartOfAccountsRef, owner }` — same, via predefined items. |
//! | anything else | `None` (caller's fall-through). |
//!
//! The returned `MetadataRef` carries the OWNER's identifier (`owner_name`,
//! e.g. `"Валюты"`) — a predefined item / enum value is a value of the
//! owner's ref type, not a distinct type itself. This mirrors how
//! `bsl-platform` surfaces manager members in `platform_data.json`.
//!
//! # Scope boundary (M4 Task 3)
//!
//! The predefined-item / enum-value table is narrowed to the three
//! families the plan names (`Enum`, `Catalog`, `ChartOfAccounts`). Other
//! MDO families either carry no predefined items by construction
//! (`Document`) or need additional XML-parser coverage before their
//! `mdo.predefined_items` is populated (`ChartOfCharacteristicTypes`,
//! `ChartOfCalculationTypes`). Extending the match arm in
//! [`predefined_ref_kind_for`] is the single edit needed when they
//! land.

use bsl_config::VisibleConfig;
use bsl_metadata::{MdoType, MetadataObject};
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;
use hir_def::ty::{MetadataKind, Ty};
use hir_def::Name;

use crate::ty_bridge::typeid_to_ty;

/// Result of a successful manager-member lookup.
///
/// Parity with [`crate::field_lookup::FieldInfo`]: carries only the
/// lowered `Ty` today. Future additions (docs, provenance MDO handle)
/// extend this struct rather than widening [`lookup_manager_field`]'s
/// return signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerMemberInfo {
    /// Type of the member after promotion / predefined-item lookup
    /// (kernel handle, §4.G.2).
    pub ty: TypeId,
}

/// `Ty`-typed mirror of [`ManagerMemberInfo`] built by the db-free
/// helper tree; converted at the [`lookup_manager_field`] boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagerMemberInfoTy {
    pub ty: Ty,
}

fn manager_member_info_ty_to_kernel(
    db: &dyn TypeKernelDb,
    info: ManagerMemberInfoTy,
) -> ManagerMemberInfo {
    ManagerMemberInfo { ty: crate::ty_bridge::ty_to_typeid(db, &info.ty) }
}

/// Single adapter entry for `Expr::Field` on a manager-global receiver.
///
/// Dispatches on the receiver shape and delegates to the matching
/// helper; returns `None` for receivers outside the manager family so
/// the caller can keep its existing fall-through (`field_lookup`
/// already covered `Ty::MetadataRef`; everything else stays `Unknown`).
pub fn lookup_manager_field(
    db: &dyn TypeKernelDb,
    configs: &[VisibleConfig],
    receiver: TypeId,
    member: &Name,
) -> Option<ManagerMemberInfo> {
    let base_ty = typeid_to_ty(db, receiver);
    lookup_manager_field_ty(configs, &base_ty, member)
        .map(|info| manager_member_info_ty_to_kernel(db, info))
}

/// Verbatim `&Ty` manager-member pipeline behind the
/// [`lookup_manager_field`] boundary (§4.G.1 receiver flip). Builds the
/// db-free [`ManagerMemberInfoTy`]; the public entry interns (§4.G.2).
fn lookup_manager_field_ty(
    configs: &[VisibleConfig],
    base_ty: &Ty,
    member: &Name,
) -> Option<ManagerMemberInfoTy> {
    match base_ty {
        Ty::ManagerCollection(kind) => promote_collection_member(configs, *kind, member),
        Ty::ObjectManager { kind, name } => lookup_predefined(configs, *kind, name, member),
        _ => None,
    }
}

/// `ManagerCollection(kind).<MdoName>` → `ObjectManager { kind, MdoName }`.
///
/// The promotion only fires when the MDO named `mdo_name` actually
/// exists under `kind` in at least one visible configuration —
/// promoting a non-existent MDO would let typos silently type-check.
/// Walks both the plain `metadata_objects` vec (Catalog/Document/Enum/…)
/// and the `registers` vec (InformationRegister/…), so
/// `РегистрыСведений.РегистрСведений1` promotes too.
fn promote_collection_member(
    configs: &[VisibleConfig],
    kind: MdoType,
    mdo_name: &Name,
) -> Option<ManagerMemberInfoTy> {
    let needle = mdo_name.as_str();
    let exists = configs.iter().rev().any(|cfg| {
        cfg.configuration.find_metadata_object(kind, needle).is_some()
            || cfg.configuration.find_register_by_type_and_name(kind, needle).is_some()
    });

    exists.then(|| ManagerMemberInfoTy { ty: Ty::ObjectManager { kind, name: mdo_name.clone() } })
}

/// Resolve a predefined-item / enum-value member on an
/// `Ty::ObjectManager` receiver.
///
/// The lookup tests `mdo.enum_values` for `Enum` and `mdo.predefined_items`
/// for `Catalog` / `ChartOfAccounts`, both case-insensitively and
/// bilingually (the helpers on [`MetadataObject`] already handle that).
/// On hit, the returned `Ty::MetadataRef` carries the OWNER's name —
/// a predefined item is a value of the owner's ref kind, so the name
/// shouldn't switch to the member.
pub(crate) fn lookup_predefined(
    configs: &[VisibleConfig],
    kind: MdoType,
    owner_name: &Name,
    member_name: &Name,
) -> Option<ManagerMemberInfoTy> {
    let ref_kind = predefined_ref_kind_for(kind)?;
    let mdo = find_mdo(configs, kind, owner_name.as_str())?;
    let hit = match kind {
        MdoType::Enum => mdo.find_enum_value(member_name.as_str()).is_some(),
        MdoType::Catalog | MdoType::ChartOfAccounts => {
            mdo.find_predefined_item(member_name.as_str()).is_some()
        }
        _ => false,
    };

    hit.then(|| ManagerMemberInfoTy {
        ty: Ty::MetadataRef { kind: ref_kind, name: owner_name.clone() },
    })
}

/// Map an owner's `MdoType` to the `MetadataKind` of its reference form.
///
/// Returning `None` short-circuits the rest of the lookup: a `Document`
/// owner has no predefined-item surface, so the adapter bails before
/// walking the MDO's empty `predefined_items` vec.
fn predefined_ref_kind_for(kind: MdoType) -> Option<MetadataKind> {
    match kind {
        MdoType::Enum => Some(MetadataKind::EnumRef),
        MdoType::Catalog => Some(MetadataKind::CatalogRef),
        MdoType::ChartOfAccounts => Some(MetadataKind::ChartOfAccountsRef),
        _ => None,
    }
}

/// Look up an MDO in the visible configurations, latest-wins.
///
/// Same iteration order as `field_lookup::find_mdo`: reverse so
/// extensions override main on name collisions.
fn find_mdo<'a>(
    configs: &'a [VisibleConfig],
    kind: MdoType,
    name: &str,
) -> Option<&'a MetadataObject> {
    configs.iter().rev().find_map(|cfg| cfg.configuration.find_metadata_object(kind, name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_config::VisibleConfig;
    use bsl_metadata::metadata_object::{EnumValue, PredefinedItem};
    use bsl_metadata::Configuration;
    /// §4.G test shim: readable tests assert on a `Ty`-typed `.ty`. Route
    /// through the db-free [`lookup_manager_field_ty`] yielding the
    /// `Ty`-typed [`ManagerMemberInfoTy`].
    fn lookup_manager_field(
        configs: &[VisibleConfig],
        base_ty: &Ty,
        member: &Name,
    ) -> Option<ManagerMemberInfoTy> {
        lookup_manager_field_ty(configs, base_ty, member)
    }
    use std::sync::Arc;

    fn wrap(config: Configuration) -> Vec<VisibleConfig> {
        vec![VisibleConfig { name: None, configuration: Arc::new(config) }]
    }

    fn catalog(name: &str, predefined: Vec<&str>) -> MetadataObject {
        let mut mdo = MetadataObject::new(MdoType::Catalog, name);
        for n in predefined {
            mdo.predefined_items.push(PredefinedItem {
                name: n.to_string(),
                name_en: None,
                uuid: String::new(),
            });
        }
        mdo
    }

    fn enum_mdo(name: &str, values: Vec<&str>) -> MetadataObject {
        let mut mdo = MetadataObject::new(MdoType::Enum, name);
        for v in values {
            mdo.enum_values.push(EnumValue {
                name: v.to_string(),
                name_en: None,
                uuid: String::new(),
            });
        }
        mdo
    }

    fn chart_of_accounts(name: &str, predefined: Vec<&str>) -> MetadataObject {
        let mut mdo = MetadataObject::new(MdoType::ChartOfAccounts, name);
        for n in predefined {
            mdo.predefined_items.push(PredefinedItem {
                name: n.to_string(),
                name_en: None,
                uuid: String::new(),
            });
        }
        mdo
    }

    #[test]
    fn promotion_manager_collection_to_object_manager_when_mdo_exists() {
        // `Справочники.Валюты` → `ObjectManager { Catalog, "Валюты" }`.
        // Baseline for the 3-seg chain: step 2 must promote, not stay at
        // `Ty::Unknown`. Without this the plan's `Перечисления.Состояния.Активен`
        // gap example never gets past the first `.Состояния` hop.
        let mut config = Configuration::new("Test");
        config.add_metadata_object(catalog("Валюты", vec![]));
        let configs = wrap(config);

        let info = lookup_manager_field(
            &configs,
            &Ty::ManagerCollection(MdoType::Catalog),
            &Name::new("Валюты"),
        )
        .expect("ManagerCollection(Catalog).Валюты must promote when MDO exists");
        assert_eq!(
            info.ty,
            Ty::ObjectManager { kind: MdoType::Catalog, name: Name::new("Валюты") }
        );
    }

    #[test]
    fn promotion_returns_none_when_mdo_not_in_config() {
        // Typo safety: `Справочники.ОпечаткаВИмени` must stay None so the
        // caller can fall through to `Ty::Unknown` (and, eventually,
        // emit an UnresolvedName-style diagnostic). Promoting a
        // non-existent name would let typos silently type-check.
        let configs = wrap(Configuration::new("Test"));
        assert!(lookup_manager_field(
            &configs,
            &Ty::ManagerCollection(MdoType::Catalog),
            &Name::new("НеСуществует"),
        )
        .is_none());
    }

    #[test]
    fn promotion_works_for_registers_via_registers_vec() {
        // Registers live in `Configuration.registers` (separate from
        // `metadata_objects`). The promotion must consult both so
        // `РегистрыСведений.РегистрСведений1` resolves. Pins the dual
        // lookup in `promote_collection_member`.
        let mut config = Configuration::new("Test");
        config.add_register(
            bsl_metadata::Register::builder()
                .name("РегистрСведений1")
                .mdo_type(MdoType::InformationRegister)
                .build(),
        );
        let configs = wrap(config);

        let info = lookup_manager_field(
            &configs,
            &Ty::ManagerCollection(MdoType::InformationRegister),
            &Name::new("РегистрСведений1"),
        )
        .expect("register promotion must consult Configuration.registers");
        assert_eq!(
            info.ty,
            Ty::ObjectManager {
                kind: MdoType::InformationRegister,
                name: Name::new("РегистрСведений1"),
            }
        );
    }

    #[test]
    fn lookup_enum_value_resolves_to_enum_ref() {
        // `Перечисления.Состояния.Активен` → `EnumRef.Состояния`.
        // The returned `MetadataRef` name must be the OWNER's name, not
        // `Активен` — a member is a value of the owner's ref type.
        let mut config = Configuration::new("Test");
        config.add_metadata_object(enum_mdo("Состояния", vec!["Активен", "Закрыт"]));
        let configs = wrap(config);

        let info = lookup_manager_field(
            &configs,
            &Ty::ObjectManager { kind: MdoType::Enum, name: Name::new("Состояния") },
            &Name::new("Активен"),
        )
        .expect("enum value must resolve on ObjectManager<Enum, Состояния>");
        assert_eq!(
            info.ty,
            Ty::MetadataRef { kind: MetadataKind::EnumRef, name: Name::new("Состояния") }
        );
    }

    #[test]
    fn lookup_catalog_predefined_resolves_to_catalog_ref() {
        // `Справочники.Валюты.Доллар` (predefined item) → `CatalogRef.Валюты`.
        // Mirrors the plan's second example literally.
        let mut config = Configuration::new("Test");
        config.add_metadata_object(catalog("Валюты", vec!["Доллар", "Евро"]));
        let configs = wrap(config);

        let info = lookup_manager_field(
            &configs,
            &Ty::ObjectManager { kind: MdoType::Catalog, name: Name::new("Валюты") },
            &Name::new("Доллар"),
        )
        .expect("predefined item must resolve on ObjectManager<Catalog, Валюты>");
        assert_eq!(
            info.ty,
            Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("Валюты") }
        );
    }

    #[test]
    fn lookup_chart_of_accounts_predefined_resolves_to_chart_of_accounts_ref() {
        // Symmetry guard: ChartOfAccounts uses the same predefined-items
        // table as Catalog and must produce the matching `ChartOfAccountsRef`.
        let mut config = Configuration::new("Test");
        config.add_metadata_object(chart_of_accounts("Хозрасчетный", vec!["Касса"]));
        let configs = wrap(config);

        let info = lookup_manager_field(
            &configs,
            &Ty::ObjectManager {
                kind: MdoType::ChartOfAccounts, name: Name::new("Хозрасчетный")
            },
            &Name::new("Касса"),
        )
        .expect("chart-of-accounts predefined item must resolve");
        assert_eq!(
            info.ty,
            Ty::MetadataRef {
                kind: MetadataKind::ChartOfAccountsRef,
                name: Name::new("Хозрасчетный"),
            }
        );
    }

    #[test]
    fn lookup_unknown_member_returns_none() {
        // A member name that does not appear in the MDO's predefined /
        // enum-values list must return None. Keeps the adapter honest
        // so the caller's `UnresolvedField` path can fire on authoritative
        // receivers if/when that gets wired.
        let mut config = Configuration::new("Test");
        config.add_metadata_object(catalog("Валюты", vec!["Доллар"]));
        let configs = wrap(config);

        assert!(lookup_manager_field(
            &configs,
            &Ty::ObjectManager { kind: MdoType::Catalog, name: Name::new("Валюты") },
            &Name::new("Несуществующий"),
        )
        .is_none());
    }

    #[test]
    fn lookup_on_unsupported_owner_kind_returns_none() {
        // Task 3 deliberately excludes kinds without a predefined-items
        // / enum-values surface: `Document` has none, `Task` /
        // `BusinessProcess` / `ExchangePlan` aren't covered yet. The
        // adapter must return None so the caller's fall-through handles
        // the miss without fabricating a bogus MetadataRef.
        let mut config = Configuration::new("Test");
        let doc = MetadataObject::new(MdoType::Document, "ПКО");
        // Document carries no predefined_items / enum_values by
        // construction; the adapter must bail at `predefined_ref_kind_for`
        // without ever reaching for attributes / tabular sections.
        config.add_metadata_object(doc);
        let configs = wrap(config);

        assert!(lookup_manager_field(
            &configs,
            &Ty::ObjectManager { kind: MdoType::Document, name: Name::new("ПКО") },
            &Name::new("Любой"),
        )
        .is_none());
    }

    #[test]
    fn lookup_on_non_manager_receiver_returns_none() {
        // Receivers outside the manager family must not produce any
        // member info — `field_lookup` is the path for `MetadataRef`,
        // and everything else is a miss.
        let configs = wrap(Configuration::new("Test"));
        for ty in [
            Ty::Unknown,
            Ty::Number,
            Ty::String,
            Ty::Array,
            Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("X") },
            Ty::Union(vec![Ty::Number, Ty::String].into()),
        ] {
            assert!(
                lookup_manager_field(&configs, &ty, &Name::new("Любой")).is_none(),
                "no manager lookup on {ty:?}",
            );
        }
    }

    #[test]
    fn promotion_extension_wins_on_collision() {
        // Parity with `field_lookup`'s extension-override test:
        // extensions iterate last, so their MDO declarations win when a
        // main-config MDO has the same `(kind, name)`. Registers follow
        // the same rule via the `|| find_register_by_type_and_name(...)`
        // branch, covered by the promotion call.
        let mut main = Configuration::new("Main");
        main.add_metadata_object(catalog("Валюты", vec!["Доллар"]));
        let mut ext = Configuration::new("Ext");
        ext.add_metadata_object(catalog("Валюты", vec!["Евро"]));
        let configs = vec![
            VisibleConfig { name: None, configuration: Arc::new(main) },
            VisibleConfig { name: Some("Ext".into()), configuration: Arc::new(ext) },
        ];

        // Promotion doesn't distinguish main from ext here — both have
        // `Валюты`; the test pins that *some* config hit produces the
        // promotion. Predefined-item lookup on top:
        let info = lookup_manager_field(
            &configs,
            &Ty::ObjectManager { kind: MdoType::Catalog, name: Name::new("Валюты") },
            &Name::new("Евро"),
        )
        .expect("extension-declared predefined item must resolve");
        assert_eq!(
            info.ty,
            Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("Валюты") }
        );

        // `Доллар` (declared only in main) must NOT resolve — the ext
        // shadows the main MDO entirely, matching the reverse-iteration
        // rule.
        assert!(lookup_manager_field(
            &configs,
            &Ty::ObjectManager { kind: MdoType::Catalog, name: Name::new("Валюты") },
            &Name::new("Доллар"),
        )
        .is_none());
    }
}
