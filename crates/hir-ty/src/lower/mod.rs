//! TypeRef → Ty lowering.
//!
//! [`TyLoweringContext`] is the single entry point that turns the syntactic
//! [`TypeRef`] layer into the semantic [`Ty`]. Every source of BSL type
//! information — `Новый X`, `Тип("…")`, JSDoc parameter hints, XML metadata
//! attributes, `ОписаниеТипов("…")` literals — goes through the same
//! pipeline, so a future change (e.g. adding `Ty::Union` in M3) only needs a
//! single edit here instead of fanning out into per-source lowering code.
//!
//! M2 keeps the context stateless apart from the optional resolver. Bare-name
//! lowering therefore cannot tell a user-defined type from a platform object,
//! so it falls back to a platform object type id. Task 7 wires the context to
//! `Resolver` + `ConfigsDatabase` so three-segment paths and cross-module
//! lookups can go through the same adapter.
//!
//! # Invariants
//!
//! 1. `lower_bare_name_id("Документы")` → `ManagerCollection(Document)` —
//!    the plural-form check runs before the PlatformObject fallback so
//!    manager globals never degenerate into `PlatformObject`.
//! 2. `lower_qualified_id(<RefPrefix>.<Name>)` consults the builtin
//!    prefix-to-kind table; unknown prefixes land on `Unknown` rather
//!    than fabricating a platform object — the resolver (M2 Task 7) will
//!    handle user-facing diagnostics.
//! 3. Everything is inside `hir-ty`, so `hir-def` never sees a `Ty` built
//!    from a `TypeRef`. The syntactic layer stays db-free.

pub(crate) mod builtin_names;
pub(crate) mod metadata_resolver;
pub mod type_string;

use std::collections::HashSet;

use bsl_metadata::{resolve_defined_type_terminal, MdoType, MetadataResolver};
use bsl_types::builders::Builders;
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::{MetadataKind, TypeId};
use bsl_types::testing::RootConfigCtx;
use hir_def::path::QualifiedName;
use hir_def::type_ref::TypeRef;
use hir_def::Name;

/// Adapter that lowers a syntactic [`TypeRef`] into a semantic [`Ty`].
///
/// Carries an optional [`MetadataResolver`] so qualified names of the form
/// `ОпределяемыйТип.X` can be expanded to their underlying type at lowering
/// time. Without a resolver the context is otherwise stateless — every other
/// branch (builtins, plurals, MetadataKind prefixes, unions) ignores it and
/// produces the same `Ty` as before.
///
/// The resolver is the seam M2 Task 7 will widen into the full `Resolver` /
/// `ConfigsDatabase` plumbing without changing the lowering surface.
#[derive(Debug, Default, Clone, Copy)]
pub struct TyLoweringContext<'a> {
    resolver: Option<&'a dyn MetadataResolver>,
}

impl<'a> TyLoweringContext<'a> {
    /// Build an empty lowering context with no resolver attached.
    ///
    /// `ОпределяемыйТип.X` qualified names lower to `Ty::Unknown` because
    /// the underlying type is unreachable without a resolver. Every other
    /// branch behaves identically to a resolver-aware context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a context that resolves `ОпределяемыйТип.X` references through
    /// `resolver`.
    ///
    /// The resolver is borrowed for the lifetime of the context; pass a
    /// `ConfigsResolver(&configs)` from `hir-ty::lower::metadata_resolver`
    /// for BSL field enumeration, or an `Arc<Configuration>`-derived
    /// resolver for SDBL.
    pub fn with_resolver(resolver: &'a dyn MetadataResolver) -> Self {
        Self { resolver: Some(resolver) }
    }

    // ── §4.A kernel-native recursion ─────────────────────────────
    //
    // Native recursion minting `TypeId` directly through the kernel
    // [`Builders`]. `db` is the interning sink, passed per-call; the
    // context itself stays a db-free resolver holder.

    /// Lower a [`TypeRef`] into a kernel [`TypeId`].
    pub fn lower_type_ref_id(&self, db: &dyn TypeKernelDb, type_ref: &TypeRef) -> TypeId {
        let mut visited = HashSet::new();
        self.lower_type_ref_id_inner(db, type_ref, &mut visited)
    }

    fn lower_type_ref_id_inner(
        &self,
        db: &dyn TypeKernelDb,
        type_ref: &TypeRef,
        visited: &mut HashSet<String>,
    ) -> TypeId {
        match type_ref {
            TypeRef::Builtin(b) => builtin_names::builtin_to_typeid(db, *b),
            TypeRef::Array(Some(elem)) => {
                db.array(Some(self.lower_type_ref_id_inner(db, elem, visited)))
            }
            TypeRef::Array(None) => db.array(None),
            TypeRef::Map(_) => db.map(None, None),
            TypeRef::Name(qname) => match qname.len() {
                0 => db.unknown(),
                1 => self.lower_bare_name_id(db, qname.first()),
                _ => self.lower_qualified_id_inner(db, qname, visited),
            },
            TypeRef::Union(parts) => {
                let lowered: Vec<TypeId> =
                    parts.iter().map(|t| self.lower_type_ref_id_inner(db, t, visited)).collect();
                db.union(lowered)
            }
            TypeRef::AnyRef | TypeRef::Unknown => db.unknown(),
        }
    }

    /// Lower a single-segment bare name into a kernel [`TypeId`].
    pub fn lower_bare_name_id(&self, db: &dyn TypeKernelDb, name: &Name) -> TypeId {
        let raw = name.as_str();

        if let Some(tref) = TypeRef::from_bare_name(raw) {
            return self.lower_type_ref_id(db, &tref);
        }

        // MDO plural (`Документы` → manager collection). Gated on
        // `manager_type_prefix` exactly like `Ty::manager_collection`.
        if let Some(mdo) = MdoType::from_plural(raw) {
            if mdo.manager_type_prefix().is_some() {
                return db.manager_collection(mdo);
            }
        }

        // Bare metadata-reference prefix without an object name stays
        // `Unknown` — mirrors the `Ty::Unknown` guard.
        if metadata_kind_from_prefix(raw).is_some() {
            return db.unknown();
        }

        db.platform_object(raw.to_string())
    }

    /// Lower a multi-segment qualified name into a kernel [`TypeId`].
    pub fn lower_qualified_id(&self, db: &dyn TypeKernelDb, qname: &QualifiedName) -> TypeId {
        let mut visited = HashSet::new();
        self.lower_qualified_id_inner(db, qname, &mut visited)
    }

    fn lower_qualified_id_inner(
        &self,
        db: &dyn TypeKernelDb,
        qname: &QualifiedName,
        visited: &mut HashSet<String>,
    ) -> TypeId {
        if qname.len() != 2 {
            return db.unknown();
        }

        let prefix = qname.first().as_str();

        if is_defined_type_prefix(prefix) {
            let Some(resolver) = self.resolver else {
                return db.unknown();
            };
            let name = qname.last().as_str();
            let key = name.to_lowercase();

            if !visited.insert(key.clone()) {
                return db.unknown();
            }

            let mut chain_visited = HashSet::new();
            let result = resolve_defined_type_terminal(resolver, name, &mut chain_visited)
                .map(|underlying| {
                    let tref = TypeRef::from_attribute_type(underlying);
                    self.lower_type_ref_id_inner(db, &tref, visited)
                })
                .unwrap_or_else(|| db.unknown());

            visited.remove(&key);
            return result;
        }

        // Every MetadataKind prefix mints a plain `MetadataRef` via
        // `db.metadata_ref(.., &RootConfigCtx)`. Never `metadata_object` /
        // `register_*` here; those are richer kinds than qualified-name
        // lowering should produce.
        match metadata_kind_from_prefix(prefix) {
            Some(kind) => db.metadata_ref(kind, qname.last().as_str().to_string(), &RootConfigCtx),
            None => db.unknown(),
        }
    }
}

/// Recognise the `DefinedType` prefix in either language, case-insensitively.
///
/// `TypeRef::from_attribute_type` always emits the canonical Russian
/// `"ОпределяемыйТип"`, but BSL identifiers are case-insensitive at the
/// language level — `определяемыйтип.X` written in user source must reach
/// the same branch. Cyrillic case folding requires `to_lowercase`;
/// `eq_ignore_ascii_case` only normalises ASCII bytes.
fn is_defined_type_prefix(prefix: &str) -> bool {
    let lower = prefix.to_lowercase();
    lower == "определяемыйтип" || lower == "definedtype"
}

/// Prefix → [`MetadataKind`] table for the reference/object forms currently
/// modelled by [`Ty::MetadataRef`]. Both Russian and English variants are
/// accepted to keep the resolver case-insensitive and bilingual end-to-end.
///
/// # Coverage
///
/// Mirrors the XML tokens emitted by `type_ref::mdo_ref_prefix` — every
/// prefix there except `ChartOfCharacteristicTypesRef`,
/// `ChartOfCalculationTypesRef`, and `ConstantValueManager` has a matching
/// `MetadataKind` variant. `ExchangePlan` and `ChartOfAccounts` joined in
/// M4 Task 2b.
/// The Russian side uses the canonical 1C platform names
/// (`РегистрСведенийКлючЗаписи`, `ПеречислениеСсылка`, …) as recorded in
/// `bsl-platform`'s `platform_data.json`.
///
/// Tabular-section kinds (`TabularSection` / `TabularSectionRow`) are not
/// here because they are never named by a single-prefix path in source or
/// XML — `FieldLookup` (M3 Task 8) constructs them directly.
fn metadata_kind_from_prefix(prefix: &str) -> Option<MetadataKind> {
    match prefix.to_lowercase().as_str() {
        "catalogref" | "справочникссылка" => Some(MetadataKind::CatalogRef),
        "catalogobject" | "справочникобъект" => Some(MetadataKind::CatalogObject),
        "documentref" | "документссылка" => Some(MetadataKind::DocumentRef),
        "documentobject" | "документобъект" => Some(MetadataKind::DocumentObject),
        "enumref" | "перечислениессылка" => Some(MetadataKind::EnumRef),
        "taskref" | "задачассылка" => Some(MetadataKind::TaskRef),
        "taskobject" | "задачаобъект" => Some(MetadataKind::TaskObject),
        "businessprocessref" | "бизнеспроцессссылка" => {
            Some(MetadataKind::BusinessProcessRef)
        }
        "businessprocessobject" | "бизнеспроцессобъект" => {
            Some(MetadataKind::BusinessProcessObject)
        }
        "dataprocessorobject" | "обработкаобъект" => {
            Some(MetadataKind::DataProcessorObject)
        }
        "reportobject" | "отчётобъект" | "отчетобъект" => {
            Some(MetadataKind::ReportObject)
        }
        "exchangeplanref" | "планобменассылка" => {
            Some(MetadataKind::ExchangePlanRef)
        }
        "exchangeplanobject" | "планобменаобъект" => {
            Some(MetadataKind::ExchangePlanObject)
        }
        "chartofaccountsref" | "плансчетовссылка" => {
            Some(MetadataKind::ChartOfAccountsRef)
        }
        "chartofaccountsobject" | "плансчетовобъект" => {
            Some(MetadataKind::ChartOfAccountsObject)
        }
        "informationregisterrecordmanager" | "регистрсведенийменеджерзаписи" => {
            Some(MetadataKind::InformationRegisterRecordManager)
        }
        "informationregisterrecordset" | "регистрсведенийнаборзаписей" => {
            Some(MetadataKind::InformationRegisterRecordSet)
        }
        "informationregisterref" | "регистрсведенийключзаписи" => {
            Some(MetadataKind::InformationRegisterRef)
        }
        "accumulationregisterrecordset" | "регистрнакоплениянаборзаписей" => {
            Some(MetadataKind::AccumulationRegisterRecordSet)
        }
        "accumulationregisterref" | "регистрнакопленияключзаписи" => {
            Some(MetadataKind::AccumulationRegisterRef)
        }
        "accountingregisterrecordset" | "регистрбухгалтериинаборзаписей" => {
            Some(MetadataKind::AccountingRegisterRecordSet)
        }
        "accountingregisterref" | "регистрбухгалтерииключзаписи" => {
            Some(MetadataKind::AccountingRegisterRef)
        }
        "calculationregisterrecordset" | "регистррасчетанаборзаписей" => {
            Some(MetadataKind::CalculationRegisterRecordSet)
        }
        "calculationregisterref" | "регистррасчетаключзаписи" => {
            Some(MetadataKind::CalculationRegisterRef)
        }
        // Per-record element kinds — yielded by `Для каждого … Из …`
        // over a record-set. JSDoc / XML refs of the form
        // `РегистрСведенийЗапись.<Имя>` lower into the matching kind so
        // platform method/property lookup keeps working on iterated
        // records. Russian aliases below mirror the HBK syntax-help
        // names verbatim (lowercased) — see the matching record-set
        // arms above.
        "informationregisterrecord" => Some(MetadataKind::InformationRegisterRecord),
        "accumulationregisterrecord" => Some(MetadataKind::AccumulationRegisterRecord),
        "accountingregisterrecord" => Some(MetadataKind::AccountingRegisterRecord),
        "calculationregisterrecord" => Some(MetadataKind::CalculationRegisterRecord),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_types::kind::TypeKind;
    use bsl_types::testing::InMemoryDb;
    use hir_def::type_ref::BuiltinTypeRef;

    fn ctx() -> TyLoweringContext<'static> {
        TyLoweringContext::new()
    }

    fn assert_metadata_ref(
        db: &InMemoryDb,
        id: TypeId,
        expected_kind: MetadataKind,
        expected_name: &str,
    ) {
        match db.lookup_type(id) {
            TypeKind::MetadataRef(facet) => {
                assert_eq!(facet.kind, expected_kind);
                assert_eq!(facet.name.as_str(), expected_name);
            }
            other => {
                panic!("expected MetadataRef({expected_kind:?}, {expected_name}), got {other:?}")
            }
        }
    }

    #[test]
    fn ty_lowering_builtin_primitive() {
        let db = InMemoryDb::new();
        assert_eq!(
            ctx().lower_type_ref_id(&db, &TypeRef::Builtin(BuiltinTypeRef::Number)),
            db.number(None, None)
        );
        assert_eq!(
            ctx().lower_type_ref_id(&db, &TypeRef::Builtin(BuiltinTypeRef::String)),
            db.string(None, false)
        );
        assert_eq!(
            ctx().lower_type_ref_id(&db, &TypeRef::Builtin(BuiltinTypeRef::Undefined)),
            db.undefined()
        );
        assert_eq!(
            ctx().lower_type_ref_id(&db, &TypeRef::Builtin(BuiltinTypeRef::ValueTable)),
            db.value_table(None, bsl_types::facet::TableSource::Unknown)
        );
    }

    #[test]
    fn ty_lowering_array_with_elem_lowers_to_typed_array() {
        // `TypeRef::Array(Some(elem))` lowers to `Ty::TypedArray(<elem ty>)`
        // so JSDoc `Массив из X` and refined form-control payloads carry
        // the element through to iteration / field lookup. The
        // unparameterised `TypeRef::Array(None)` keeps the legacy
        // `Ty::Array` lowering — see `ty_lowering_array_none_stays_unparameterised`.
        let db = InMemoryDb::new();
        let array_with_elem =
            TypeRef::Array(Some(Box::new(TypeRef::Builtin(BuiltinTypeRef::Number))));
        assert_eq!(
            ctx().lower_type_ref_id(&db, &array_with_elem),
            db.array(Some(db.number(None, None)))
        );
    }

    #[test]
    fn ty_lowering_jsdoc_array_of_string_round_trip() {
        // End-to-end: parsing `// Возвращаемое значение: Массив из Строка`
        // produces `TypeRef::Array(Some(Builtin(String)))`, and lowering
        // it must surface `Ty::TypedArray(String)` so downstream
        // iteration / method lookup see the element type.
        let db = InMemoryDb::new();
        let doc = "// Возвращаемое значение:\n//   Массив из Строка - результат\n";
        let hints = hir_def::ty::doc_types::parse_method_doc_types(doc).unwrap();
        assert_eq!(
            ctx().lower_type_ref_id(&db, &hints.ret),
            db.array(Some(db.string(None, false)))
        );
    }

    #[test]
    fn ty_lowering_array_none_stays_unparameterised() {
        // `TypeRef::Array(None)` (e.g. `Новый Массив`, bare-name `Массив`)
        // has no recoverable element type — stays `Ty::Array` so platform
        // method lookup still resolves through the unparameterised page.
        let db = InMemoryDb::new();
        assert_eq!(ctx().lower_type_ref_id(&db, &TypeRef::Array(None)), db.array(None));
    }

    #[test]
    fn ty_lowering_map_drops_kv_pairs() {
        // `Ty::Map` is not parameterised yet — element types in
        // `TypeRef::Map(Some((k, v)))` are dropped. A future
        // `Ty::TypedMap(K, V)` would mirror `Ty::TypedArray`.
        let map_with_kv = TypeRef::Map(Some((
            Box::new(TypeRef::Builtin(BuiltinTypeRef::String)),
            Box::new(TypeRef::Builtin(BuiltinTypeRef::Number)),
        )));
        let db = InMemoryDb::new();
        assert_eq!(ctx().lower_type_ref_id(&db, &map_with_kv), db.map(None, None));
    }

    #[test]
    fn ty_lowering_bare_builtin_bilingual() {
        // Cascade step 1: `from_bare_name` catches builtins in both languages.
        let db = InMemoryDb::new();
        assert_eq!(ctx().lower_bare_name_id(&db, &Name::new("Число")), db.number(None, None));
        assert_eq!(ctx().lower_bare_name_id(&db, &Name::new("NUMBER")), db.number(None, None));
        assert_eq!(ctx().lower_bare_name_id(&db, &Name::new("Массив")), db.array(None));
        assert_eq!(ctx().lower_bare_name_id(&db, &Name::new("Соответствие")), db.map(None, None));
    }

    #[test]
    fn ty_lowering_manager_collection_plural() {
        // Cascade step 2: MDO plural → ManagerCollection.
        let db = InMemoryDb::new();
        assert_eq!(
            ctx().lower_bare_name_id(&db, &Name::new("Документы")),
            db.manager_collection(MdoType::Document)
        );
        assert_eq!(
            ctx().lower_bare_name_id(&db, &Name::new("Справочники")),
            db.manager_collection(MdoType::Catalog)
        );
    }

    #[test]
    fn ty_lowering_bare_unknown_falls_to_platform_object() {
        // Cascade step 4: unknown bare name → PlatformObject(name). Matches
        // the legacy `infer::Expr::New` fallback that lets `Новый Запрос`
        // type the expression as a platform object even without verifying
        // against `bsl_platform`.
        let db = InMemoryDb::new();
        let request = Name::new("Запрос");
        assert_eq!(
            ctx().lower_bare_name_id(&db, &request),
            db.platform_object("Запрос".to_string())
        );

        // Case is preserved verbatim — the caller owns display casing.
        let mixed = Name::new("HTTPЗапрос");
        assert_eq!(
            ctx().lower_bare_name_id(&db, &mixed),
            db.platform_object("HTTPЗапрос".to_string())
        );
    }

    #[test]
    fn ty_lowering_bare_metadata_prefix_without_name_is_unknown() {
        // Guard against the `AnyObjectRef` mis-routing Codex flagged:
        // a stray `СправочникСсылка` or `CatalogRef` without an object name
        // must never become `Ty::PlatformObject("CatalogRef")`. Both
        // languages covered because `metadata_kind_from_prefix` is
        // case-insensitive bilingual.
        let db = InMemoryDb::new();
        assert_eq!(ctx().lower_bare_name_id(&db, &Name::new("СправочникСсылка")), db.unknown());
        assert_eq!(ctx().lower_bare_name_id(&db, &Name::new("CatalogRef")), db.unknown());
        assert_eq!(ctx().lower_bare_name_id(&db, &Name::new("documentobject")), db.unknown());
    }

    #[test]
    fn ty_lowering_qualified_unmodelled_prefix_is_unknown() {
        // M3 left `ChartOfCharacteristicTypesRef`, `ChartOfCalculationTypesRef`,
        // and `ConstantValueManager` outside the model; these must land on
        // `Ty::Unknown` rather than producing a misleading `MetadataRef`
        // with a wrong kind. `ExchangePlanRef` and `ChartOfAccountsRef`
        // joined `MetadataKind` in M4 Task 2b — covered by the bilingual
        // test below instead.
        for prefix in
            ["ChartOfCharacteristicTypesRef", "ChartOfCalculationTypesRef", "ConstantValueManager"]
        {
            let db = InMemoryDb::new();
            let qname = QualifiedName::from_segments([Name::new(prefix), Name::new("Х")]);
            assert_eq!(
                ctx().lower_qualified_id(&db, &qname),
                db.unknown(),
                "expected Unknown for `{prefix}.Х`"
            );
        }
    }

    #[test]
    fn metadata_kind_exchange_plan_and_chart_of_accounts_lower_bilingual() {
        // M4 Task 2b: `ExchangePlan` and `ChartOfAccounts` joined
        // `MetadataKind` so field-lookup and the type facade can walk their
        // MDO metadata the same way Catalog/Document already do.
        for (prefix, expected) in [
            ("ExchangePlanRef", MetadataKind::ExchangePlanRef),
            ("ПланОбменаСсылка", MetadataKind::ExchangePlanRef),
            ("ExchangePlanObject", MetadataKind::ExchangePlanObject),
            ("ПланОбменаОбъект", MetadataKind::ExchangePlanObject),
            ("ChartOfAccountsRef", MetadataKind::ChartOfAccountsRef),
            ("ПланСчетовСсылка", MetadataKind::ChartOfAccountsRef),
            ("ChartOfAccountsObject", MetadataKind::ChartOfAccountsObject),
            ("ПланСчетовОбъект", MetadataKind::ChartOfAccountsObject),
        ] {
            let db = InMemoryDb::new();
            let qname = QualifiedName::from_segments([Name::new(prefix), Name::new("Х")]);
            let id = ctx().lower_qualified_id(&db, &qname);
            assert_metadata_ref(&db, id, expected, "Х");
        }
    }

    #[test]
    fn metadata_kind_enum_and_task_and_bp_lower_bilingual() {
        // M3 added EnumRef / TaskRef / BusinessProcessRef — the XML tokens and
        // 1C-canonical Russian names from `bsl-platform/data/platform_data.json`
        // must both lower to the correct MetadataKind.
        for (prefix, expected) in [
            ("EnumRef", MetadataKind::EnumRef),
            ("ПеречислениеСсылка", MetadataKind::EnumRef),
            ("TaskRef", MetadataKind::TaskRef),
            ("ЗадачаСсылка", MetadataKind::TaskRef),
            ("BusinessProcessRef", MetadataKind::BusinessProcessRef),
            ("БизнесПроцессСсылка", MetadataKind::BusinessProcessRef),
        ] {
            let db = InMemoryDb::new();
            let qname = QualifiedName::from_segments([Name::new(prefix), Name::new("Х")]);
            let id = ctx().lower_qualified_id(&db, &qname);
            assert_metadata_ref(&db, id, expected, "Х");
        }
    }

    #[test]
    fn metadata_kind_register_refs_lower_bilingual() {
        // Record-key forms for all four register families land on the
        // matching `*RegisterRef` kind. Keeps the `КлючЗаписи` form distinct
        // from the runtime `МенеджерЗаписи` / `НаборЗаписей` forms that
        // already exist in `MetadataKind`.
        for (prefix, expected) in [
            ("InformationRegisterRef", MetadataKind::InformationRegisterRef),
            ("РегистрСведенийКлючЗаписи", MetadataKind::InformationRegisterRef),
            ("AccumulationRegisterRef", MetadataKind::AccumulationRegisterRef),
            ("РегистрНакопленияКлючЗаписи", MetadataKind::AccumulationRegisterRef),
            ("AccountingRegisterRef", MetadataKind::AccountingRegisterRef),
            ("РегистрБухгалтерииКлючЗаписи", MetadataKind::AccountingRegisterRef),
            ("CalculationRegisterRef", MetadataKind::CalculationRegisterRef),
            ("РегистрРасчетаКлючЗаписи", MetadataKind::CalculationRegisterRef),
        ] {
            let db = InMemoryDb::new();
            let qname = QualifiedName::from_segments([Name::new(prefix), Name::new("Х")]);
            let id = ctx().lower_qualified_id(&db, &qname);
            assert_metadata_ref(&db, id, expected, "Х");
        }
    }

    #[test]
    fn ty_lowering_qualified_metadata_ref_english() {
        let db = InMemoryDb::new();
        let qname = QualifiedName::from_segments([Name::new("CatalogRef"), Name::new("Товары")]);
        let id = ctx().lower_qualified_id(&db, &qname);
        assert_metadata_ref(&db, id, MetadataKind::CatalogRef, "Товары");
    }

    #[test]
    fn ty_lowering_qualified_metadata_ref_russian() {
        let db = InMemoryDb::new();
        let qname = QualifiedName::from_segments([Name::new("ДокументСсылка"), Name::new("ПКО")]);
        let id = ctx().lower_qualified_id(&db, &qname);
        assert_metadata_ref(&db, id, MetadataKind::DocumentRef, "ПКО");
    }

    #[test]
    fn ty_lowering_qualified_unknown_prefix_is_unknown() {
        // Not a MetadataKind prefix; resolver will produce the user-facing
        // diagnostic in Task 7.
        let db = InMemoryDb::new();
        let qname = QualifiedName::from_segments([Name::new("ОбщийМодуль"), Name::new("Х")]);
        assert_eq!(ctx().lower_qualified_id(&db, &qname), db.unknown());
    }

    #[test]
    fn ty_lowering_qualified_three_segments_deferred_to_task7() {
        // 3-segment paths (`Документы.ПКО.СоздатьДокумент`) are the
        // resolver's job; the syntactic lowerer deliberately refuses them
        // so inference cannot silently observe a wrong tail.
        let three = QualifiedName::from_segments([
            Name::new("Документы"),
            Name::new("ПКО"),
            Name::new("СоздатьДокумент"),
        ]);
        let db = InMemoryDb::new();
        assert_eq!(ctx().lower_qualified_id(&db, &three), db.unknown());
    }

    #[test]
    fn ty_lowering_union_flows_through_ty_union_constructor() {
        // Each member lowers through the same `lower_type_ref_id`, then the
        // smart constructor normalises the result. Sibling primitives stay
        // distinct; `Ty::union` imposes a stable order so two syntactically
        // different composites with the same member set compare equal.
        let tr = TypeRef::Union(vec![
            TypeRef::Builtin(BuiltinTypeRef::Number),
            TypeRef::Builtin(BuiltinTypeRef::String),
        ]);
        let db = InMemoryDb::new();
        let ty = ctx().lower_type_ref_id(&db, &tr);
        match db.lookup_type(ty) {
            TypeKind::Union(parts) => assert_eq!(parts.len(), 2),
            other => panic!("expected TypeKind::Union, got {other:?}"),
        }

        // Flipping the order reaches the same semantic `Ty`.
        let flipped = TypeRef::Union(vec![
            TypeRef::Builtin(BuiltinTypeRef::String),
            TypeRef::Builtin(BuiltinTypeRef::Number),
        ]);
        assert_eq!(ctx().lower_type_ref_id(&db, &flipped), ty);
    }

    #[test]
    fn ty_lowering_union_singleton_collapses() {
        // `TypeRef::Union([x])` goes through `Ty::union([lowered_x])` which
        // unwraps to `lowered_x` — callers never have to pattern-match on a
        // one-element union.
        let tr = TypeRef::Union(vec![TypeRef::Builtin(BuiltinTypeRef::Number)]);
        let db = InMemoryDb::new();
        assert_eq!(ctx().lower_type_ref_id(&db, &tr), db.number(None, None));
    }

    #[test]
    fn ty_lowering_union_empty_becomes_unknown() {
        // Empty union has no type information — `Ty::union([])` returns
        // `Ty::Unknown`, keeping the "stated but empty" case distinguishable
        // from a truly absent type.
        let db = InMemoryDb::new();
        assert_eq!(ctx().lower_type_ref_id(&db, &TypeRef::Union(vec![])), db.unknown());
    }

    #[test]
    fn ty_lowering_type_ref_routes_through_name_branches() {
        // `TypeRef::Name([single])` → bare-name cascade.
        let db = InMemoryDb::new();
        let single = TypeRef::Name(QualifiedName::from_segments([Name::new("Массив")]));
        assert_eq!(ctx().lower_type_ref_id(&db, &single), db.array(None));

        // `TypeRef::Name([prefix, name])` → qualified cascade.
        let qualified = TypeRef::Name(QualifiedName::from_segments([
            Name::new("СправочникСсылка"),
            Name::new("Номенклатура"),
        ]));
        let id = ctx().lower_type_ref_id(&db, &qualified);
        assert_metadata_ref(&db, id, MetadataKind::CatalogRef, "Номенклатура");

        // AnyRef / Unknown remain Unknown until Ty::AnyRef lands.
        assert_eq!(ctx().lower_type_ref_id(&db, &TypeRef::AnyRef), db.unknown());
        assert_eq!(ctx().lower_type_ref_id(&db, &TypeRef::Unknown), db.unknown());
    }

    // -----------------------------------------------------------------------
    // ОпределяемыйТип resolution
    //
    // The lowering layer interacts with the resolver only through the
    // `MetadataResolver` trait — the production wire-up against
    // `Configuration` / `&[VisibleConfig]` is exercised by the integration
    // test in `field_enum.rs`. Here we use a HashMap-backed mock so the unit
    // test stays focused on the lowering logic (cycle guard, prefix
    // recognition, recursive Composite arms).
    // -----------------------------------------------------------------------

    use bsl_metadata::{AttributeType, MetadataResolver};

    #[derive(Debug, Default)]
    struct MockResolver(std::collections::HashMap<String, AttributeType>);

    impl MockResolver {
        fn with(entries: &[(&str, AttributeType)]) -> Self {
            let mut map = std::collections::HashMap::new();
            for (name, at) in entries {
                map.insert(name.to_lowercase(), at.clone());
            }
            Self(map)
        }
    }

    impl MetadataResolver for MockResolver {
        fn resolve_defined_type(&self, name: &str) -> Option<&AttributeType> {
            self.0.get(&name.to_lowercase())
        }
    }

    #[test]
    fn defined_type_without_resolver_stays_unknown() {
        // Stateless context (no resolver) — `ОпределяемыйТип.X` is unresolvable
        // by definition, so we deliberately produce `Ty::Unknown` instead of
        // guessing at a primitive. Mirrors the legacy M2 behaviour for the
        // same prefix.
        let qname = QualifiedName::from_segments([
            Name::new("ОпределяемыйТип"),
            Name::new("ДенежнаяСумма"),
        ]);
        let db = InMemoryDb::new();
        assert_eq!(ctx().lower_qualified_id(&db, &qname), db.unknown());
    }

    #[test]
    fn defined_type_with_resolver_lowers_to_underlying_primitive() {
        // The niagara_ut bug repro at the lowering layer: a DefinedType
        // backed by `xs:decimal` must lower to `Ty::Number` once a resolver
        // is attached. Numeric qualifiers (precision/scale) drop in M2
        // because `Ty::Number` does not carry them yet — that mirrors the
        // direct `xs:decimal` path.
        let resolver = MockResolver::with(&[(
            "ДенежнаяСумма",
            AttributeType::Number { precision: 15, scale: 2 },
        )]);

        let lowering = TyLoweringContext::with_resolver(&resolver);
        let qname = QualifiedName::from_segments([
            Name::new("ОпределяемыйТип"),
            Name::new("ДенежнаяСумма"),
        ]);
        let db = InMemoryDb::new();
        assert_eq!(lowering.lower_qualified_id(&db, &qname), db.number(None, None));
    }

    #[test]
    fn defined_type_chain_lowers_through_terminal_walk() {
        // Two-step chain — A points at B, B's underlying is `xs:string`.
        // The terminal walk collapses both hops in one go before lowering.
        let resolver = MockResolver::with(&[
            ("A", AttributeType::DefinedType { name: "B".to_string() }),
            ("B", AttributeType::String { length: Some(64) }),
        ]);

        let lowering = TyLoweringContext::with_resolver(&resolver);
        let qname = QualifiedName::from_segments([Name::new("ОпределяемыйТип"), Name::new("A")]);
        let db = InMemoryDb::new();
        assert_eq!(lowering.lower_qualified_id(&db, &qname), db.string(None, false));
    }

    #[test]
    fn defined_type_cycle_returns_unknown_without_overflow() {
        // Pathological metadata: `A → B → A`. `resolve_defined_type_terminal`
        // returns None on the second visit, lowering reports `Ty::Unknown`,
        // and crucially the recursion does not blow the stack.
        let resolver = MockResolver::with(&[
            ("A", AttributeType::DefinedType { name: "B".to_string() }),
            ("B", AttributeType::DefinedType { name: "A".to_string() }),
        ]);

        let lowering = TyLoweringContext::with_resolver(&resolver);
        let qname = QualifiedName::from_segments([Name::new("ОпределяемыйТип"), Name::new("A")]);
        let db = InMemoryDb::new();
        assert_eq!(lowering.lower_qualified_id(&db, &qname), db.unknown());
    }

    #[test]
    fn defined_type_composite_underlying_lowers_to_union() {
        // A DefinedType whose underlying is `Composite { Number, String }`
        // must lower to `Ty::Union([Number, String])` — the inner Composite
        // goes through `TypeRef::from_attribute_type` → `TypeRef::Union`,
        // and the outer `lower_type_ref_inner` recursively handles each arm.
        let resolver = MockResolver::with(&[(
            "ЛюбоеЧислоИлиСтрока",
            AttributeType::Composite {
                types: vec![
                    AttributeType::Number { precision: 10, scale: 0 },
                    AttributeType::String { length: None },
                ],
            },
        )]);

        let lowering = TyLoweringContext::with_resolver(&resolver);
        let qname = QualifiedName::from_segments([
            Name::new("ОпределяемыйТип"),
            Name::new("ЛюбоеЧислоИлиСтрока"),
        ]);
        let db = InMemoryDb::new();
        match db.lookup_type(lowering.lower_qualified_id(&db, &qname)) {
            TypeKind::Union(arms) => {
                assert!(arms.contains(&db.number(None, None)), "union must contain Number");
                assert!(arms.contains(&db.string(None, false)), "union must contain String");
            }
            other => panic!("expected TypeKind::Union, got {other:?}"),
        }
    }

    #[test]
    fn defined_type_sibling_arms_share_terminal_step_independently() {
        // A `Composite { DefT.A, DefT.B }` whose arms both chain through the
        // same intermediate `X → Number` must lower to `Ty::Number`
        // (singleton union collapse). The chain guard inside
        // `resolve_defined_type_terminal` is per-call-local; if it were
        // shared with the lowering-level `visited`, arm 2 would observe `X`
        // already visited from arm 1 and degrade to `Ty::Unknown`.
        let resolver = MockResolver::with(&[
            ("A", AttributeType::DefinedType { name: "X".to_string() }),
            ("B", AttributeType::DefinedType { name: "X".to_string() }),
            ("X", AttributeType::Number { precision: 10, scale: 0 }),
        ]);
        let lowering = TyLoweringContext::with_resolver(&resolver);

        let arm = |name: &str| {
            TypeRef::Name(QualifiedName::from_segments([
                Name::new("ОпределяемыйТип"),
                Name::new(name),
            ]))
        };
        let tref = TypeRef::Union(vec![arm("A"), arm("B")]);
        // Both arms collapse to `Ty::Number`; `Ty::union` then dedupes the
        // singleton — the assertion fails if either arm degrades to
        // `Ty::Unknown` (the bug case).
        let db = InMemoryDb::new();
        assert_eq!(lowering.lower_type_ref_id(&db, &tref), db.number(None, None));
    }

    #[test]
    fn defined_type_self_referential_composite_is_safe() {
        // `A → Composite{A, Number}`. The lowering-level guard must catch
        // re-entry into `A` while we're still lowering its underlying
        // composite, otherwise the recursion overflows the stack.
        let resolver = MockResolver::with(&[(
            "A",
            AttributeType::Composite {
                types: vec![
                    AttributeType::DefinedType { name: "A".to_string() },
                    AttributeType::Number { precision: 10, scale: 0 },
                ],
            },
        )]);
        let lowering = TyLoweringContext::with_resolver(&resolver);
        let qname = QualifiedName::from_segments([Name::new("ОпределяемыйТип"), Name::new("A")]);
        // Self-reference inside the composite collapses to Unknown, the
        // other arm to Number. The kernel union canonicalizer drops the
        // unknown arm, preserving the concrete witness while still proving
        // that recursion terminates.
        let db = InMemoryDb::new();
        assert_eq!(lowering.lower_qualified_id(&db, &qname), db.number(None, None));
    }

    #[test]
    fn russian_prefix_is_case_insensitive() {
        // BSL identifiers are case-insensitive; `определяемыйтип.X` written
        // in user source must reach the same DefinedType branch as the
        // canonical `ОпределяемыйТип.X` form emitted by
        // `TypeRef::from_attribute_type`.
        let resolver = MockResolver::with(&[("X", AttributeType::Boolean)]);
        let lowering = TyLoweringContext::with_resolver(&resolver);
        for prefix in ["ОпределяемыйТип", "определяемыйтип", "ОПРЕДЕЛЯЕМЫЙТИП"]
        {
            let qname = QualifiedName::from_segments([Name::new(prefix), Name::new("X")]);
            let db = InMemoryDb::new();
            assert_eq!(
                lowering.lower_qualified_id(&db, &qname),
                db.boolean(),
                "case-insensitive lookup failed for `{prefix}`"
            );
        }
    }

    #[test]
    fn defined_type_english_prefix_also_resolves() {
        // `TypeRef::from_attribute_type` always emits Russian, but defensive
        // code accepts English `DefinedType.X` too — JSDoc and future XML
        // sources may use either. Case-insensitive on the ASCII spelling.
        let resolver = MockResolver::with(&[("X", AttributeType::Boolean)]);
        let lowering = TyLoweringContext::with_resolver(&resolver);
        let qname = QualifiedName::from_segments([Name::new("DefinedType"), Name::new("X")]);
        let db = InMemoryDb::new();
        assert_eq!(lowering.lower_qualified_id(&db, &qname), db.boolean());
    }

    #[test]
    fn lower_type_ref_id_covers_resolver_free_branches() {
        let db = InMemoryDb::new();
        let lowering = ctx();

        let name = |s: &str| TypeRef::Name(QualifiedName::from_segments([Name::new(s)]));
        let qual = |a: &str, b: &str| {
            TypeRef::Name(QualifiedName::from_segments([Name::new(a), Name::new(b)]))
        };

        let cases = vec![
            (TypeRef::Builtin(BuiltinTypeRef::Number), db.number(None, None)),
            (
                TypeRef::Builtin(BuiltinTypeRef::ValueTable),
                db.value_table(None, bsl_types::facet::TableSource::Unknown),
            ),
            (
                TypeRef::Array(Some(Box::new(TypeRef::Builtin(BuiltinTypeRef::String)))),
                db.array(Some(db.string(None, false))),
            ),
            (TypeRef::Array(None), db.array(None)),
            (
                TypeRef::Map(Some((
                    Box::new(TypeRef::Builtin(BuiltinTypeRef::String)),
                    Box::new(TypeRef::Builtin(BuiltinTypeRef::Number)),
                ))),
                db.map(None, None),
            ),
            (TypeRef::AnyRef, db.unknown()),
            (TypeRef::Unknown, db.unknown()),
            // bare names: builtin, MDO plural, RefPrefix-without-name guard,
            // platform-object fallback.
            (name("Число"), db.number(None, None)),
            (name("Документы"), db.manager_collection(MdoType::Document)),
            (name("СправочникСсылка"), db.unknown()),
            (name("Запрос"), db.platform_object("Запрос".to_string())),
            // qualified: MetadataRef prefix, non-metadata prefix, DefinedType
            // (no resolver → Unknown), 3-segment overflow.
            (
                qual("СправочникСсылка", "Товары"),
                db.metadata_ref(MetadataKind::CatalogRef, "Товары".to_string(), &RootConfigCtx),
            ),
            (qual("ОбщийМодуль", "Х"), db.unknown()),
            (qual("ОпределяемыйТип", "ДенежнаяСумма"), db.unknown()),
            (
                TypeRef::Name(QualifiedName::from_segments([
                    Name::new("Документы"),
                    Name::new("ПКО"),
                    Name::new("Создать"),
                ])),
                db.unknown(),
            ),
            // unions: concrete arms, arm with Unknown, all-Unknown, dup arms.
            (
                TypeRef::Union(vec![
                    TypeRef::Builtin(BuiltinTypeRef::Number),
                    TypeRef::Builtin(BuiltinTypeRef::String),
                ]),
                db.union(vec![db.number(None, None), db.string(None, false)]),
            ),
            (
                TypeRef::Union(vec![TypeRef::Builtin(BuiltinTypeRef::Number), TypeRef::Unknown]),
                db.union(vec![db.number(None, None), db.unknown()]),
            ),
            (TypeRef::Union(vec![TypeRef::Unknown, TypeRef::AnyRef]), db.unknown()),
            (
                TypeRef::Union(vec![
                    TypeRef::Builtin(BuiltinTypeRef::Boolean),
                    TypeRef::Builtin(BuiltinTypeRef::Boolean),
                ]),
                db.boolean(),
            ),
        ];

        for (tr, expected) in &cases {
            assert_eq!(lowering.lower_type_ref_id(&db, tr), *expected, "lowering drift for {tr:?}");
        }
    }

    #[test]
    fn lower_qualified_id_resolves_defined_types() {
        let db = InMemoryDb::new();
        let resolver = MockResolver::with(&[
            ("ДенежнаяСумма", AttributeType::Number { precision: 15, scale: 2 }),
            ("A", AttributeType::DefinedType { name: "B".to_string() }),
            ("B", AttributeType::String { length: Some(64) }),
            (
                "ЛюбоеЧислоИлиСтрока",
                AttributeType::Composite {
                    types: vec![
                        AttributeType::Number { precision: 10, scale: 0 },
                        AttributeType::String { length: None },
                    ],
                },
            ),
        ]);
        let lowering = TyLoweringContext::with_resolver(&resolver);

        let number_qname = QualifiedName::from_segments([
            Name::new("ОпределяемыйТип"),
            Name::new("ДенежнаяСумма"),
        ]);
        assert_eq!(lowering.lower_qualified_id(&db, &number_qname), db.number(None, None));

        let chained_qname =
            QualifiedName::from_segments([Name::new("ОпределяемыйТип"), Name::new("A")]);
        assert_eq!(lowering.lower_qualified_id(&db, &chained_qname), db.string(None, false));

        let union_qname = QualifiedName::from_segments([
            Name::new("ОпределяемыйТип"),
            Name::new("ЛюбоеЧислоИлиСтрока"),
        ]);
        assert_eq!(
            lowering.lower_qualified_id(&db, &union_qname),
            db.union(vec![db.number(None, None), db.string(None, false)])
        );
    }
}
