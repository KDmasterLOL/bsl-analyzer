//! Platform-method lookup for managers and metadata refs.
//!
//! Bridges two receiver shapes into the platform-data catalogue:
//!
//! - `Ty::ObjectManager { kind, name }` → methods indexed under
//!   `"CatalogManager.<Имя>"` / `"DocumentManager.<Имя>"` …. The user
//!   surface is `Справочники.Номенклатура.СоздатьЭлемент()` (3-segment
//!   call) or `М.СоздатьЭлемент()` after `М = Справочники.Номенклатура`
//!   (aliased manager — method-lookup path).
//! - `Ty::MetadataRef { kind, name }` for object/ref flavours →
//!   methods indexed under `"CatalogObject.<Имя>"` / `"CatalogRef.<Имя>"`
//!   …. The user surface is `Спр.Записать()` after
//!   `Спр = Справочники.Номенклатура.СоздатьЭлемент()`.
//!
//! Platform-data stores these method groups with composite `type_name`
//! (the `"ManagerType.<Имя справочника>"` shape) and placeholder
//! `name = "<Имя"`; the real Russian method name lives in
//! `docs.syntax`, and the English name lives in `english_name` after
//! the last `.`. This mirrors the matching logic first introduced in
//! `symbol-info::adapters::platform_manager::build`.
//!
//! ## Return-type rewriting
//!
//! `PlatformMethod::return_type` is a generic string (`"СправочникОбъект"`,
//! `"СправочникСсылка"`, …) that intentionally drops the concrete MDO
//! name. Inference has to re-bind it to the current `(mdo_type, mdo_name)`
//! context so chained calls like `Спр.Записать()` keep resolving. The
//! rewrite table lives in [`map_generic_metadata_return_type`]; anything
//! it doesn't recognise falls through to [`crate::method_lookup::resolve_platform_type_name`]
//! (primitives, `ValueTable`, opaque `Ty::PlatformObject`).
//!
//! ## Workspace > platform priority
//!
//! The 3-segment-call caller consults `Resolver::resolve_three_level_method`
//! first (workspace `ManagerModule.bsl` with exported method) and only
//! falls back here on `MethodNotFound`. A user-defined override therefore
//! always wins; platform fills the gap when no workspace method exists.

use bsl_metadata::MdoType;
use bsl_platform::{PlatformData, PlatformDataInner, PlatformMethod};
use hir_def::ty::{FunctionSignature, MetadataKind, Ty};
use hir_def::Name;

use crate::method_lookup::resolve_platform_type_name;

/// Outcome of a successful platform-method lookup.
///
/// Deliberately does not carry `method_id` / `is_export` — platform
/// methods live outside the workspace symbol tree and are always
/// callable. This is the stripped mirror of
/// [`crate::method_resolution::MethodResolution`] with only the fields
/// inference actually uses (signature for arg-count / arg-type checks,
/// return type for the call's `Ty`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformMethodResolution {
    /// Signature lowered to `Ty`s (parameter types + return type).
    pub signature: FunctionSignature,
    /// Convenience clone of `signature.ret` — matches the shape of
    /// `MethodResolution.return_type` so inference call-sites can
    /// read without dereferencing the `Box`.
    pub return_ty: Ty,
}

/// Resolve `<manager-collective>.<mdo_name>.<method>()` through platform data.
///
/// Returns `None` when:
/// - `mdo_type` has no `manager_type_prefix` (e.g. `Cube`,
///   `DimensionTable`, `CommonModule`);
/// - no indexed method under that prefix matches `method_name`
///   bilingually.
pub fn resolve_platform_manager_method(
    mdo_type: MdoType,
    mdo_name: &Name,
    method_name: &Name,
) -> Option<PlatformMethodResolution> {
    let prefix = mdo_type.manager_type_prefix()?;
    let method = find_prefixed_method(prefix, method_name)?;
    Some(build_resolution(&method, mdo_type, mdo_name))
}

/// Resolve `<metadata-ref>.<method>()` through platform data.
///
/// Covers `Ty::MetadataRef { kind, name }` for the object/ref flavours
/// (`CatalogObject`, `CatalogRef`, `DocumentObject`, …). The prefix and
/// the parent `MdoType` (used for context-aware return rewriting) both
/// come from [`metadata_kind_to_prefix_and_mdo`]; kinds without a
/// platform surface (register dimensions, tabular sections) return
/// `None`.
pub fn resolve_platform_metadata_ref_method(
    kind: MetadataKind,
    mdo_name: &Name,
    method_name: &Name,
) -> Option<PlatformMethodResolution> {
    let (prefix, parent_mdo) = metadata_kind_to_prefix_and_mdo(kind)?;
    let method = find_prefixed_method(prefix, method_name)?;
    Some(build_resolution(&method, parent_mdo, mdo_name))
}

/// Build a `PlatformMethodResolution` from a resolved platform method in
/// the context of `(mdo_type, mdo_name)`.
///
/// Parameter types and the return type lower through
/// [`resolve_platform_type_name`] for the scalar cases and
/// [`map_generic_metadata_return_type`] for the manager-relative
/// generics (`"СправочникОбъект"` → `Ty::MetadataRef { CatalogObject, mdo_name }`).
fn build_resolution(
    method: &PlatformMethod,
    mdo_type: MdoType,
    mdo_name: &Name,
) -> PlatformMethodResolution {
    let params: Vec<Ty> = method
        .parameters
        .iter()
        .map(|p| {
            p.param_type.as_ref().map(|t| resolve_platform_type_name(t)).unwrap_or(Ty::Unknown)
        })
        .collect();
    let defaults: Vec<bool> = method.parameters.iter().map(|p| p.is_optional).collect();

    let return_ty = method
        .return_type
        .as_ref()
        .map(|raw| {
            map_generic_metadata_return_type(raw, mdo_type, mdo_name)
                .unwrap_or_else(|| resolve_platform_type_name(raw))
        })
        .unwrap_or(Ty::Undefined);

    let signature = FunctionSignature::new_with_defaults(params, defaults, return_ty.clone());
    PlatformMethodResolution { signature, return_ty }
}

/// Find one platform method whose `type_name` starts with `"{prefix}."`
/// and whose name (bilingual) matches `method_name`.
///
/// Platform data stores manager / object / ref methods with composite
/// `type_name` (`"CatalogManager.<Catalog name>"` etc.) and placeholder
/// `name = "<Имя"`, so neither the (type_name, method_name) index nor
/// a direct name comparison works. Two matchers are tried:
///
/// 1. `docs.syntax.split('(').next()` — the Russian method name is
///    stored in the HBK-derived docs under "ИмяМенеджера.Метод(...)".
/// 2. `english_name.rsplit_once('.')` — the English canonical name is
///    `"ManagerType.Method"`, so the part after the last `.` is the
///    bare method name.
fn find_prefixed_method(prefix: &str, method_name: &Name) -> Option<PlatformMethod> {
    let method_lower = method_name.as_str().to_lowercase();
    let data = PlatformData::instance();
    let docs_db = PlatformDataInner::instance();

    data.get_manager_methods(prefix)
        .into_iter()
        .find(|m| {
            let docs = docs_db.get_method_docs(m.id);
            let ru_match = docs
                .as_ref()
                .and_then(|d| d.syntax.split('(').next())
                .is_some_and(|ru| ru.to_lowercase() == method_lower);
            if ru_match {
                return true;
            }
            let en_name =
                m.english_name.rsplit_once('.').map(|(_, n)| n).unwrap_or(&m.english_name);
            en_name.to_lowercase() == method_lower
        })
        .cloned()
}

/// Map a `MetadataKind` to `(prefix, parent_mdo)` for platform lookup.
///
/// - `prefix` is the English composite-type prefix used in
///   `PlatformMethod::type_name` (`"CatalogObject"`, `"CatalogRef"`, …).
/// - `parent_mdo` is the owning MDO flavour — threaded into
///   [`map_generic_metadata_return_type`] so a method on
///   `MetadataRef { CatalogObject, "Валюты" }` returning generic
///   `"СправочникСсылка"` re-binds to
///   `MetadataRef { CatalogRef, "Валюты" }`.
///
/// `None` for kinds without a platform surface (register dimensions,
/// resources, attributes, tabular sections) or kinds whose platform
/// methods are not yet covered by the generic table
/// (`InformationRegisterRecordManager`, `AccumulationRegisterRecordSet`).
fn metadata_kind_to_prefix_and_mdo(kind: MetadataKind) -> Option<(&'static str, MdoType)> {
    let prefix = kind.platform_prefix()?;
    let parent_mdo = match kind {
        MetadataKind::CatalogObject | MetadataKind::CatalogRef => MdoType::Catalog,
        MetadataKind::DocumentObject | MetadataKind::DocumentRef => MdoType::Document,
        MetadataKind::EnumRef => MdoType::Enum,
        MetadataKind::TaskRef => MdoType::Task,
        MetadataKind::BusinessProcessRef => MdoType::BusinessProcess,
        MetadataKind::ExchangePlanRef | MetadataKind::ExchangePlanObject => MdoType::ExchangePlan,
        MetadataKind::ChartOfAccountsRef | MetadataKind::ChartOfAccountsObject => {
            MdoType::ChartOfAccounts
        }
        // Register-record kinds — Phase C platform-side wiring.
        MetadataKind::InformationRegisterRecordManager => MdoType::InformationRegister,
        MetadataKind::AccumulationRegisterRecordSet => MdoType::AccumulationRegister,
        // `platform_prefix` already returned `None` for the remaining
        // variants via `?` above, so this arm is unreachable in
        // practice — the guard below documents the invariant and keeps
        // the match exhaustive.
        _ => return None,
    };
    Some((prefix, parent_mdo))
}

/// Rewrite a generic platform return-type string to a concrete
/// `Ty::MetadataRef` bound to `(mdo_type, mdo_name)`.
///
/// Returns `None` when `raw` is not a recognised manager-relative
/// generic; the caller then falls through to
/// [`resolve_platform_type_name`] (primitives, value-types,
/// `Ty::PlatformObject`).
///
/// The table mirrors [`MetadataKind::object_kind_for`] plus the
/// corresponding `*Ref` variants — both directions are kept in sync by
/// the `(raw, mdo_type) → MetadataKind` pair: producing an
/// `ExchangePlanObject` for an `MdoType::Document` context would be
/// a bug, so the match is keyed on both.
fn map_generic_metadata_return_type(raw: &str, mdo_type: MdoType, mdo_name: &Name) -> Option<Ty> {
    let kind = match (raw, mdo_type) {
        ("СправочникОбъект" | "CatalogObject", MdoType::Catalog) => {
            MetadataKind::CatalogObject
        }
        ("СправочникСсылка" | "CatalogRef", MdoType::Catalog) => {
            MetadataKind::CatalogRef
        }
        ("ДокументОбъект" | "DocumentObject", MdoType::Document) => {
            MetadataKind::DocumentObject
        }
        ("ДокументСсылка" | "DocumentRef", MdoType::Document) => {
            MetadataKind::DocumentRef
        }
        ("ПеречислениеСсылка" | "EnumRef", MdoType::Enum) => {
            MetadataKind::EnumRef
        }
        ("ЗадачаСсылка" | "TaskRef", MdoType::Task) => MetadataKind::TaskRef,
        ("БизнесПроцессСсылка" | "BusinessProcessRef", MdoType::BusinessProcess) => {
            MetadataKind::BusinessProcessRef
        }
        ("ПланОбменаСсылка" | "ExchangePlanRef", MdoType::ExchangePlan) => {
            MetadataKind::ExchangePlanRef
        }
        ("ПланОбменаОбъект" | "ExchangePlanObject", MdoType::ExchangePlan) => {
            MetadataKind::ExchangePlanObject
        }
        ("ПланСчетовСсылка" | "ChartOfAccountsRef", MdoType::ChartOfAccounts) => {
            MetadataKind::ChartOfAccountsRef
        }
        ("ПланСчетовОбъект" | "ChartOfAccountsObject", MdoType::ChartOfAccounts) => {
            MetadataKind::ChartOfAccountsObject
        }
        // Register-record return forms (Phase C): manager methods like
        // `РегистрыСведений.X.СоздатьМенеджерЗаписи()` and
        // `РегистрыНакопления.X.СоздатьНаборЗаписей()` produce a
        // register-record receiver that the workspace
        // `RecordSetModule.bsl` resolver can act on.
        (
            "РегистрСведенийМенеджерЗаписи" | "InformationRegisterRecordManager",
            MdoType::InformationRegister,
        ) => MetadataKind::InformationRegisterRecordManager,
        (
            "РегистрНакопленияНаборЗаписей" | "AccumulationRegisterRecordSet",
            MdoType::AccumulationRegister,
        ) => MetadataKind::AccumulationRegisterRecordSet,
        _ => return None,
    };
    Some(Ty::MetadataRef { kind, name: mdo_name.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_create_item_on_catalog_returns_catalog_object() {
        // `Справочники.<Name>.СоздатьЭлемент()` must bind the generic
        // `СправочникОбъект` return to `MetadataRef { CatalogObject, Name }`.
        let res = resolve_platform_manager_method(
            MdoType::Catalog,
            &Name::new("Номенклатура"),
            &Name::new("СоздатьЭлемент"),
        )
        .expect("platform data indexes CreateItem under CatalogManager");

        assert_eq!(
            res.return_ty,
            Ty::MetadataRef {
                kind: MetadataKind::CatalogObject, name: Name::new("Номенклатура")
            }
        );
    }

    #[test]
    fn manager_find_by_code_on_catalog_returns_catalog_ref() {
        // `НайтиПоКоду` returns the generic `СправочникСсылка` — must
        // rebind to `MetadataRef { CatalogRef, Name }`, not a bare
        // `PlatformObject("СправочникСсылка")`.
        let res = resolve_platform_manager_method(
            MdoType::Catalog,
            &Name::new("Валюты"),
            &Name::new("НайтиПоКоду"),
        )
        .expect("platform data indexes FindByCode under CatalogManager");

        assert_eq!(
            res.return_ty,
            Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("Валюты") }
        );
    }

    #[test]
    fn manager_unknown_method_returns_none() {
        // Not a platform method — lookup must fail, diagnostic stays.
        assert!(resolve_platform_manager_method(
            MdoType::Catalog,
            &Name::new("Валюты"),
            &Name::new("НетТакогоМетода"),
        )
        .is_none());
    }

    #[test]
    fn manager_english_method_name_resolves() {
        // Bilingual gate — the English canonical name must hit too.
        let res = resolve_platform_manager_method(
            MdoType::Catalog,
            &Name::new("Номенклатура"),
            &Name::new("CreateItem"),
        )
        .expect("English 'CreateItem' must also resolve to CatalogManager.CreateItem");
        assert!(matches!(res.return_ty, Ty::MetadataRef { kind: MetadataKind::CatalogObject, .. }));
    }

    #[test]
    fn manager_mdo_without_prefix_returns_none() {
        // CommonModule / Cube / DimensionTable have no `manager_type_prefix`.
        assert!(resolve_platform_manager_method(
            MdoType::CommonModule,
            &Name::new("AnyName"),
            &Name::new("СоздатьЭлемент"),
        )
        .is_none());
    }

    #[test]
    fn metadata_ref_catalog_object_resolves_write_as_procedure() {
        // `CatalogObject.Записать()` is a procedure (return=None) —
        // lookup must succeed with `Ty::Undefined` return.
        let res = resolve_platform_metadata_ref_method(
            MetadataKind::CatalogObject,
            &Name::new("Номенклатура"),
            &Name::new("Записать"),
        )
        .expect("platform data indexes Write under CatalogObject");
        assert_eq!(res.return_ty, Ty::Undefined);
    }

    #[test]
    fn metadata_ref_register_record_manager_resolves_write() {
        // Phase C: Register-record kinds were de-authoritized in
        // Phase 0 (returning `None`) because their platform surface
        // wasn't wired. Phase C wires `platform_prefix()` so methods
        // declared under the
        // `InformationRegisterRecordManager.<Имя>` composite typename
        // (`Записать`, `Прочитать`, …) now resolve.
        let res = resolve_platform_metadata_ref_method(
            MetadataKind::InformationRegisterRecordManager,
            &Name::new("Курсы"),
            &Name::new("Записать"),
        )
        .expect("platform data indexes Write under InformationRegisterRecordManager");
        // `Записать` is a procedure → `Ty::Undefined` return.
        assert_eq!(res.return_ty, Ty::Undefined);
    }
}
