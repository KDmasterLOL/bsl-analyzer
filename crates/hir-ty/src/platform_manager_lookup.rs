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
//! it doesn't recognise falls through to [`crate::method_lookup::lower_platform_type_name`]
//! (primitives, `ValueTable`, opaque `Ty::PlatformObject`).
//!
//! ## Workspace > platform priority
//!
//! The 3-segment-call caller consults `Resolver::resolve_three_level_method`
//! first (workspace `ManagerModule.bsl` with exported method) and only
//! falls back here on `MethodNotFound`. A user-defined override therefore
//! always wins; platform fills the gap when no workspace method exists.

use bsl_metadata::MdoType;
use bsl_platform::{find_prefixed_method, PlatformMethod};
use bsl_types::builders::Builders;
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;
use bsl_types::testing::RootConfigCtx;
use hir_def::ty::{FunctionSignature, MetadataKind};
use hir_def::Name;

use crate::lower::type_string::{lower_param_type_string_typeid, lower_platform_type_name_typeid};
use crate::method_lookup::lower_overloads_typeid;

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
    /// Signature lowered to type-kernel ids (parameter types + return type).
    pub signature: FunctionSignature,
    /// Convenience clone of `signature.ret` — matches the shape of
    /// `MethodResolution.return_type` so inference call-sites can
    /// read without dereferencing the signature.
    pub return_ty: TypeId,
    /// Per-overload parameter lists for multi-overload composite methods
    /// (`InformationRegisterManager.Get`,
    /// `AccountingRegisterRecordSet.Move`,
    /// `BusinessProcessManager.FindByNumber` …). Empty for
    /// single-signature methods — `signature.params` already covers
    /// them. Argument-type checks accept the call when ANY overload
    /// accepts it; without this, `arg_diagnostics_query` saw composite
    /// multi-overload methods as strictly typed against the first
    /// signature only and false-fired on legitimate alternative call
    /// shapes.
    pub overloads: Vec<Vec<TypeId>>,
}

/// Resolve `<manager-collective>.<mdo_name>.<method>()` through platform data.
///
/// Returns `None` when:
/// - `mdo_type` has no `manager_type_prefix` (e.g. `Cube`,
///   `DimensionTable`, `CommonModule`);
/// - no indexed method under that prefix matches `method_name`
///   bilingually.
pub fn resolve_platform_manager_method(
    db: &dyn TypeKernelDb,
    mdo_type: MdoType,
    mdo_name: &Name,
    method_name: &Name,
) -> Option<PlatformMethodResolution> {
    let prefix = mdo_type.manager_type_prefix()?;
    let method = find_prefixed_method(prefix, method_name.as_str())?;
    Some(build_resolution(db, &method, mdo_type, mdo_name))
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
    db: &dyn TypeKernelDb,
    kind: MetadataKind,
    mdo_name: &Name,
    method_name: &Name,
) -> Option<PlatformMethodResolution> {
    let (prefix, parent_mdo) = metadata_kind_to_prefix_and_mdo(kind)?;
    let method = find_prefixed_method(prefix, method_name.as_str())?;
    Some(build_resolution(db, &method, parent_mdo, mdo_name))
}

/// Build a `PlatformMethodResolution` from a resolved platform method in
/// the context of `(mdo_type, mdo_name)`.
///
/// Parameter types and the return type lower through
/// [`lower_platform_type_name`] for the scalar cases and
/// [`map_generic_metadata_return_type`] for the manager-relative
/// generics (`"СправочникОбъект"` → `Ty::MetadataRef { CatalogObject, mdo_name }`).
pub(crate) fn build_resolution(
    db: &dyn TypeKernelDb,
    method: &PlatformMethod,
    mdo_type: MdoType,
    mdo_name: &Name,
) -> PlatformMethodResolution {
    let params: Vec<TypeId> = method
        .parameters
        .iter()
        .map(|p| {
            p.param_type
                .as_ref()
                .map(|t| lower_param_type_string_typeid(db, t))
                .unwrap_or(db.unknown())
        })
        .collect();
    let defaults: Vec<bool> = method.parameters.iter().map(|p| p.is_optional).collect();

    let return_ty = method
        .return_type
        .as_ref()
        .map(|raw| {
            map_generic_metadata_return_type_typeid(db, raw, mdo_type, mdo_name)
                .unwrap_or_else(|| lower_platform_type_name_typeid(db, raw))
        })
        .unwrap_or(db.undefined());

    let signature = FunctionSignature {
        max_args: Some(params.len() as u32),
        params: params.into_boxed_slice(),
        defaults: defaults.into_boxed_slice(),
        ret: return_ty,
    };
    PlatformMethodResolution { signature, return_ty, overloads: lower_overloads_typeid(db, method) }
}

/// Kernel-native counterpart of [`map_generic_metadata_return_type`].
pub(crate) fn map_generic_metadata_return_type_typeid(
    db: &dyn TypeKernelDb,
    raw: &str,
    mdo_type: MdoType,
    mdo_name: &Name,
) -> Option<TypeId> {
    let kind = map_generic_metadata_return_type(raw, mdo_type)?;
    Some(db.metadata_ref(kind, mdo_name.as_str().to_string(), &RootConfigCtx))
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
/// resources, attributes, tabular sections, the synthetic
/// `RegisterFilter`, and the bare `*Ref` register reference forms whose
/// methods are not indexed under a composite prefix).
pub(crate) fn metadata_kind_to_prefix_and_mdo(
    kind: MetadataKind,
) -> Option<(&'static str, MdoType)> {
    let prefix = kind.platform_prefix()?;
    let parent_mdo = match kind {
        MetadataKind::CatalogObject | MetadataKind::CatalogRef => MdoType::Catalog,
        MetadataKind::DocumentObject | MetadataKind::DocumentRef => MdoType::Document,
        MetadataKind::EnumRef => MdoType::Enum,
        MetadataKind::TaskRef | MetadataKind::TaskObject => MdoType::Task,
        MetadataKind::BusinessProcessRef | MetadataKind::BusinessProcessObject => {
            MdoType::BusinessProcess
        }
        MetadataKind::DataProcessorObject => MdoType::DataProcessor,
        MetadataKind::ReportObject => MdoType::Report,
        MetadataKind::ExchangePlanRef | MetadataKind::ExchangePlanObject => MdoType::ExchangePlan,
        MetadataKind::ChartOfAccountsRef | MetadataKind::ChartOfAccountsObject => {
            MdoType::ChartOfAccounts
        }
        // Register-record kinds — manager / record-set / record platform
        // surfaces. The per-record variants (`*Record`) are the element
        // types yielded by `Для каждого … Из …` over a record-set; their
        // platform methods are indexed under `<Flavour>Record.<Имя>`.
        MetadataKind::InformationRegisterRecordManager
        | MetadataKind::InformationRegisterRecordSet
        | MetadataKind::InformationRegisterRecord => MdoType::InformationRegister,
        MetadataKind::AccumulationRegisterRecordSet | MetadataKind::AccumulationRegisterRecord => {
            MdoType::AccumulationRegister
        }
        MetadataKind::AccountingRegisterRecordSet | MetadataKind::AccountingRegisterRecord => {
            MdoType::AccountingRegister
        }
        MetadataKind::CalculationRegisterRecordSet | MetadataKind::CalculationRegisterRecord => {
            MdoType::CalculationRegister
        }
        // No-platform-prefix kinds: `platform_prefix` already returned
        // `None` for these via `?` above, so these arms are unreachable
        // in practice. Listed explicitly (no wildcard) so a new
        // `MetadataKind` variant surfaces as a compiler error here and
        // forces an authorial decision rather than silently bypassing
        // the manager-prefix dispatch.
        MetadataKind::InformationRegisterRef
        | MetadataKind::AccumulationRegisterRef
        | MetadataKind::AccountingRegisterRef
        | MetadataKind::CalculationRegisterRef
        | MetadataKind::RegisterDimension { .. }
        | MetadataKind::RegisterResource { .. }
        | MetadataKind::RegisterAttribute { .. }
        | MetadataKind::RegisterFilter { .. }
        | MetadataKind::TabularSection { .. }
        | MetadataKind::TabularSectionRow { .. } => return None,
    };
    Some((prefix, parent_mdo))
}

/// Rewrite a generic platform return-type string to a concrete
/// `Ty::MetadataRef` bound to `(mdo_type, mdo_name)`.
///
/// Returns `None` when `raw` is not a recognised manager-relative
/// generic; the caller then falls through to
/// [`lower_platform_type_name`] (primitives, value-types,
/// `Ty::PlatformObject`).
///
/// The table mirrors [`MetadataKind::object_kind_for`] plus the
/// corresponding `*Ref` variants — both directions are kept in sync by
/// the `(raw, mdo_type) → MetadataKind` pair: producing an
/// `ExchangePlanObject` for an `MdoType::Document` context would be
/// a bug, so the match is keyed on both.
pub(crate) fn map_generic_metadata_return_type(
    raw: &str,
    mdo_type: MdoType,
) -> Option<MetadataKind> {
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
        ("ЗадачаОбъект" | "TaskObject", MdoType::Task) => MetadataKind::TaskObject,
        ("БизнесПроцессОбъект" | "BusinessProcessObject", MdoType::BusinessProcess) => {
            MetadataKind::BusinessProcessObject
        }
        ("ОбработкаОбъект" | "DataProcessorObject", MdoType::DataProcessor) => {
            MetadataKind::DataProcessorObject
        }
        ("ОтчётОбъект" | "ОтчетОбъект" | "ReportObject", MdoType::Report) => {
            MetadataKind::ReportObject
        }
        // Register-record return forms: manager methods like
        // `РегистрыСведений.X.СоздатьМенеджерЗаписи()` and
        // `РегистрыНакопления.X.СоздатьНаборЗаписей()` produce a
        // register-record receiver that platform method lookup and the
        // workspace `RecordSetModule.bsl` resolver can act on.
        (
            "РегистрСведенийМенеджерЗаписи" | "InformationRegisterRecordManager",
            MdoType::InformationRegister,
        ) => MetadataKind::InformationRegisterRecordManager,
        (
            "РегистрСведенийНаборЗаписей" | "InformationRegisterRecordSet",
            MdoType::InformationRegister,
        ) => MetadataKind::InformationRegisterRecordSet,
        (
            "РегистрНакопленияНаборЗаписей" | "AccumulationRegisterRecordSet",
            MdoType::AccumulationRegister,
        ) => MetadataKind::AccumulationRegisterRecordSet,
        (
            "РегистрБухгалтерииНаборЗаписей" | "AccountingRegisterRecordSet",
            MdoType::AccountingRegister,
        ) => MetadataKind::AccountingRegisterRecordSet,
        (
            "РегистрРасчетаНаборЗаписей" | "CalculationRegisterRecordSet",
            MdoType::CalculationRegister,
        ) => MetadataKind::CalculationRegisterRecordSet,
        // Per-record element forms: yielded by `Для каждого … Из …`
        // over a register record-set. `iteration_lookup` calls into
        // this same table, threading the `(record_kind, mdo_name)`
        // pair so iteration over a register-set produces the matching
        // `Ty::MetadataRef { *Record, mdo_name }` element.
        ("РегистрСведенийЗапись" | "InformationRegisterRecord", MdoType::InformationRegister) => {
            MetadataKind::InformationRegisterRecord
        }
        (
            "РегистрНакопленияЗапись" | "AccumulationRegisterRecord",
            MdoType::AccumulationRegister,
        ) => MetadataKind::AccumulationRegisterRecord,
        ("РегистрБухгалтерииЗапись" | "AccountingRegisterRecord", MdoType::AccountingRegister) => {
            MetadataKind::AccountingRegisterRecord
        }
        ("РегистрРасчетаЗапись" | "CalculationRegisterRecord", MdoType::CalculationRegister) => {
            MetadataKind::CalculationRegisterRecord
        }
        _ => return None,
    };
    Some(kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty_bridge::typeid_to_ty;
    use bsl_types::builders::Builders;
    use bsl_types::testing::InMemoryDb;
    use hir_def::ty::Ty;

    fn ret_ty(db: &InMemoryDb, res: &PlatformMethodResolution) -> Ty {
        typeid_to_ty(db, res.return_ty)
    }

    fn params_ty(db: &InMemoryDb, res: &PlatformMethodResolution) -> Vec<Ty> {
        res.signature.params.iter().map(|id| typeid_to_ty(db, *id)).collect()
    }

    /// §4.C drift-detector: kernel-native accessors mirror return_ty / overloads.
    #[test]
    fn platform_manager_typeid_round_trips_via_ty() {
        let db = InMemoryDb::new();
        let res = PlatformMethodResolution {
            signature: FunctionSignature {
                params: Box::new([]),
                defaults: Box::new([]),
                ret: db.number(None, None),
                max_args: Some(0),
            },
            return_ty: db.number(None, None),
            overloads: vec![vec![db.string(None, false)]],
        };
        assert_eq!(typeid_to_ty(&db, res.return_ty), Ty::Number);
        let overloads_via_ty: Vec<Vec<Ty>> = res
            .overloads
            .iter()
            .map(|row| row.iter().map(|id| typeid_to_ty(&db, *id)).collect())
            .collect();
        assert_eq!(overloads_via_ty, vec![vec![Ty::String]]);
    }

    #[test]
    fn manager_create_item_on_catalog_returns_catalog_object() {
        // `Справочники.<Name>.СоздатьЭлемент()` must bind the generic
        // `СправочникОбъект` return to `MetadataRef { CatalogObject, Name }`.
        let db = InMemoryDb::new();
        let res = resolve_platform_manager_method(
            &db,
            MdoType::Catalog,
            &Name::new("Номенклатура"),
            &Name::new("СоздатьЭлемент"),
        )
        .expect("platform data indexes CreateItem under CatalogManager");

        assert_eq!(
            ret_ty(&db, &res),
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
        let db = InMemoryDb::new();
        let res = resolve_platform_manager_method(
            &db,
            MdoType::Catalog,
            &Name::new("Валюты"),
            &Name::new("НайтиПоКоду"),
        )
        .expect("platform data indexes FindByCode under CatalogManager");

        assert_eq!(
            ret_ty(&db, &res),
            Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("Валюты") }
        );
    }

    #[test]
    fn manager_find_by_code_param_lowers_to_union() {
        // `НайтиПоКоду`'s first param is `param_type = "Число, Строка"` in
        // platform_data. The bug: `build_resolution` used to lower this
        // through `lower_platform_type_name` directly, which doesn't
        // split on `,` — the whole string became `Ty::PlatformObject(
        // "Число, Строка")` and `String → that` failed structural equality.
        // Pin that comma-joined param_type lowers to a `Ty::Union` so
        // `is_assignable(String, Number|String)` passes via the
        // union-right rule.
        let db = InMemoryDb::new();
        let res = resolve_platform_manager_method(
            &db,
            MdoType::Catalog,
            &Name::new("Валюты"),
            &Name::new("НайтиПоКоду"),
        )
        .expect("platform data indexes FindByCode under CatalogManager");

        assert_eq!(
            params_ty(&db, &res).first(),
            Some(&Ty::union(vec![Ty::Number, Ty::String])),
            "first param of FindByCode must be a Union, not a single PlatformObject; got {:?}",
            params_ty(&db, &res).first(),
        );
    }

    #[test]
    fn manager_unknown_method_returns_none() {
        // Not a platform method — lookup must fail, diagnostic stays.
        let db = InMemoryDb::new();
        assert!(resolve_platform_manager_method(
            &db,
            MdoType::Catalog,
            &Name::new("Валюты"),
            &Name::new("НетТакогоМетода"),
        )
        .is_none());
    }

    #[test]
    fn manager_english_method_name_resolves() {
        // Bilingual gate — the English canonical name must hit too.
        let db = InMemoryDb::new();
        let res = resolve_platform_manager_method(
            &db,
            MdoType::Catalog,
            &Name::new("Номенклатура"),
            &Name::new("CreateItem"),
        )
        .expect("English 'CreateItem' must also resolve to CatalogManager.CreateItem");
        assert!(matches!(
            ret_ty(&db, &res),
            Ty::MetadataRef { kind: MetadataKind::CatalogObject, .. }
        ));
    }

    #[test]
    fn manager_mdo_without_prefix_returns_none() {
        // CommonModule / Cube / DimensionTable have no `manager_type_prefix`.
        let db = InMemoryDb::new();
        assert!(resolve_platform_manager_method(
            &db,
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
        let db = InMemoryDb::new();
        let res = resolve_platform_metadata_ref_method(
            &db,
            MetadataKind::CatalogObject,
            &Name::new("Номенклатура"),
            &Name::new("Записать"),
        )
        .expect("platform data indexes Write under CatalogObject");
        assert_eq!(ret_ty(&db, &res), Ty::Undefined);
    }

    #[test]
    fn metadata_ref_register_record_manager_resolves_write() {
        // Phase C: Register-record kinds were de-authoritized in
        // Phase 0 (returning `None`) because their platform surface
        // wasn't wired. Phase C wires `platform_prefix()` so methods
        // declared under the
        // `InformationRegisterRecordManager.<Имя>` composite typename
        // (`Записать`, `Прочитать`, …) now resolve.
        let db = InMemoryDb::new();
        let res = resolve_platform_metadata_ref_method(
            &db,
            MetadataKind::InformationRegisterRecordManager,
            &Name::new("Курсы"),
            &Name::new("Записать"),
        )
        .expect("platform data indexes Write under InformationRegisterRecordManager");
        // `Записать` is a procedure → `Ty::Undefined` return.
        assert_eq!(ret_ty(&db, &res), Ty::Undefined);
    }

    #[test]
    fn manager_create_record_set_on_information_register_returns_record_set() {
        // `РегистрыСведений.<X>.СоздатьНаборЗаписей()` must rebind the
        // generic `РегистрСведенийНаборЗаписей` return to a concrete
        // `MetadataRef { InformationRegisterRecordSet, X }`. Without
        // this rebinding the receiver type degrades to
        // `Ty::PlatformObject("РегистрСведенийНаборЗаписей")` and the
        // composite-prefixed methods (`Записать`, `Загрузить`, …)
        // become unreachable.
        let db = InMemoryDb::new();
        let res = resolve_platform_manager_method(
            &db,
            MdoType::InformationRegister,
            &Name::new("Курсы"),
            &Name::new("СоздатьНаборЗаписей"),
        )
        .expect("platform data indexes CreateRecordSet under InformationRegisterManager");
        assert_eq!(
            ret_ty(&db, &res),
            Ty::MetadataRef {
                kind: MetadataKind::InformationRegisterRecordSet,
                name: Name::new("Курсы"),
            }
        );
    }

    #[test]
    fn manager_create_record_set_on_accumulation_register_returns_record_set() {
        let db = InMemoryDb::new();
        let res = resolve_platform_manager_method(
            &db,
            MdoType::AccumulationRegister,
            &Name::new("ПродажиОбороты"),
            &Name::new("СоздатьНаборЗаписей"),
        )
        .expect("platform data indexes CreateRecordSet under AccumulationRegisterManager");
        assert_eq!(
            ret_ty(&db, &res),
            Ty::MetadataRef {
                kind: MetadataKind::AccumulationRegisterRecordSet,
                name: Name::new("ПродажиОбороты"),
            }
        );
    }

    #[test]
    fn manager_create_record_set_on_accounting_register_returns_record_set() {
        let db = InMemoryDb::new();
        let res = resolve_platform_manager_method(
            &db,
            MdoType::AccountingRegister,
            &Name::new("Хозрасчетный"),
            &Name::new("СоздатьНаборЗаписей"),
        )
        .expect("platform data indexes CreateRecordSet under AccountingRegisterManager");
        assert_eq!(
            ret_ty(&db, &res),
            Ty::MetadataRef {
                kind: MetadataKind::AccountingRegisterRecordSet,
                name: Name::new("Хозрасчетный"),
            }
        );
    }

    #[test]
    fn manager_create_record_set_on_calculation_register_returns_record_set() {
        let db = InMemoryDb::new();
        let res = resolve_platform_manager_method(
            &db,
            MdoType::CalculationRegister,
            &Name::new("Начисления"),
            &Name::new("СоздатьНаборЗаписей"),
        )
        .expect("platform data indexes CreateRecordSet under CalculationRegisterManager");
        assert_eq!(
            ret_ty(&db, &res),
            Ty::MetadataRef {
                kind: MetadataKind::CalculationRegisterRecordSet,
                name: Name::new("Начисления"),
            }
        );
    }

    #[test]
    fn metadata_ref_information_register_record_set_resolves_load() {
        // After the new variant is wired through `platform_prefix`,
        // platform-indexed methods on `InformationRegisterRecordSet.<X>`
        // (e.g. `Загрузить`) must resolve via the metadata-ref path.
        let db = InMemoryDb::new();
        let res = resolve_platform_metadata_ref_method(
            &db,
            MetadataKind::InformationRegisterRecordSet,
            &Name::new("Курсы"),
            &Name::new("Загрузить"),
        )
        .expect("platform data indexes Load under InformationRegisterRecordSet");
        // `Загрузить` is a procedure → `Ty::Undefined` return.
        assert_eq!(ret_ty(&db, &res), Ty::Undefined);
    }
}
