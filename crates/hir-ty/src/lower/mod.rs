//! TypeRef → Ty lowering.
//!
//! [`TyLoweringContext`] is the single entry point that turns the syntactic
//! [`TypeRef`] layer into the semantic [`Ty`]. Every source of BSL type
//! information — `Новый X`, `Тип("…")`, JSDoc parameter hints, XML metadata
//! attributes, `ОписаниеТипов("…")` literals — goes through the same
//! pipeline, so a future change (e.g. adding `Ty::Union` in M3) only needs a
//! single edit here instead of fanning out into per-source lowering code.
//!
//! M2 keeps the context stateless: there is no resolver or database yet, and
//! `lower_bare_name` therefore cannot tell a user-defined type from a
//! platform object — it falls back to `Ty::PlatformObject(name)`, mirroring
//! the legacy `Ty::from_type_name` behaviour. Task 7 wires the context to
//! `Resolver` + `ConfigsDatabase` so three-segment paths and cross-module
//! lookups can go through the same adapter.
//!
//! # Invariants
//!
//! 1. `lower_bare_name("Документы")` → `Ty::ManagerCollection(Document)` —
//!    the plural-form check runs before the PlatformObject fallback so
//!    manager globals never degenerate into `PlatformObject`.
//! 2. `lower_qualified(<RefPrefix>.<Name>)` consults the builtin
//!    prefix-to-kind table; unknown prefixes land on `Ty::Unknown` rather
//!    than fabricating a platform object — the resolver (M2 Task 7) will
//!    handle user-facing diagnostics.
//! 3. Everything is inside `hir-ty`, so `hir-def` never sees a `Ty` built
//!    from a `TypeRef`. The syntactic layer stays db-free.

pub(crate) mod builtin_names;
pub(crate) mod metadata_resolver;

use std::collections::HashSet;

use bsl_metadata::{resolve_defined_type_terminal, MdoType, MetadataResolver};
use hir_def::path::QualifiedName;
use hir_def::ty::{MetadataKind, Ty};
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

    /// Lower a [`TypeRef`] into a [`Ty`].
    ///
    /// Dispatches over the `TypeRef` variants:
    /// - `Builtin(b)` → the fixed primitive from [`builtin_names::builtin_to_ty`].
    /// - `Array(_)` / `Map(_)` → `Ty::Array` / `Ty::Map` (element types are
    ///   dropped in M2 — `Ty` does not carry them yet).
    /// - `Name(qname)` → [`Self::lower_qualified`] for 2+ segments, else
    ///   [`Self::lower_bare_name`].
    /// - `Union(parts)` → [`Ty::union`] after recursive lowering of each
    ///   component. The smart constructor flattens nested unions (including
    ///   those emerging from XML `Composite of Composite`), deduplicates,
    ///   and collapses singletons.
    /// - `AnyRef` / `Unknown` → `Ty::Unknown` (deliberately: we have no
    ///   `Ty::AnyRef` variant yet, and narrowing it to a concrete kind would
    ///   lie to callers).
    pub fn lower_type_ref(&self, type_ref: &TypeRef) -> Ty {
        let mut visited = HashSet::new();
        self.lower_type_ref_inner(type_ref, &mut visited)
    }

    fn lower_type_ref_inner(&self, type_ref: &TypeRef, visited: &mut HashSet<String>) -> Ty {
        match type_ref {
            TypeRef::Builtin(b) => builtin_names::builtin_to_ty(*b),
            TypeRef::Array(_) => Ty::Array,
            TypeRef::Map(_) => Ty::Map,
            TypeRef::Name(qname) => match qname.len() {
                0 => Ty::Unknown,
                1 => self.lower_bare_name(qname.first()),
                _ => self.lower_qualified_inner(qname, visited),
            },
            TypeRef::Union(parts) => {
                let lowered: Vec<Ty> =
                    parts.iter().map(|t| self.lower_type_ref_inner(t, visited)).collect();
                Ty::union(lowered)
            }
            TypeRef::AnyRef | TypeRef::Unknown => Ty::Unknown,
        }
    }

    /// Lower a single-segment bare name (`Массив`, `Документы`, `Запрос`).
    ///
    /// Consolidated cascade:
    /// 1. Primitive or collection builtin (via [`TypeRef::from_bare_name`]).
    /// 2. MDO plural (`Документы` → `Ty::ManagerCollection(Document)`).
    /// 3. Metadata-reference prefix without an object name (`СправочникСсылка`
    ///    standalone) → `Ty::Unknown`. Prevents producing bogus
    ///    `Ty::PlatformObject("CatalogRef")` from stray XML `cfg:*Ref`
    ///    tokens that arrive without a concrete object name.
    /// 4. Fallback `Ty::PlatformObject(name)` — matches the legacy
    ///    `Expr::New` fallback in `infer::infer_new_expr` for `Новый Запрос`
    ///    and other unverified platform objects.
    ///
    /// The cascade never returns `Ty::Unknown` for a syntactically valid
    /// bare name, apart from the explicit RefPrefix guard above — the real
    /// "unknown type" diagnostic is the resolver's job in Task 7.
    pub fn lower_bare_name(&self, name: &Name) -> Ty {
        let raw = name.as_str();

        if let Some(tref) = TypeRef::from_bare_name(raw) {
            return self.lower_type_ref(&tref);
        }

        if let Some(mdo) = MdoType::from_plural(raw) {
            if let Some(ty) = Ty::manager_collection(mdo) {
                return ty;
            }
        }

        // Guard against stray metadata-reference prefixes — a bare
        // `СправочникСсылка` is not a platform object and must not degrade
        // into one. The resolver (Task 7) will eventually turn this into a
        // diagnostic; for now Unknown is the honest answer.
        if metadata_kind_from_prefix(raw).is_some() {
            return Ty::Unknown;
        }

        Ty::PlatformObject(name.clone())
    }

    /// Lower a multi-segment qualified name.
    ///
    /// Two patterns are decoded:
    /// - 2-segment metadata reference (`СправочникСсылка.Товары`,
    ///   `DocumentObject.ПКО`).
    /// - 2-segment `ОпределяемыйТип.X` (or `DefinedType.X`) — when a
    ///   resolver is attached, the underlying [`bsl_metadata::AttributeType`]
    ///   is fetched via the resolver and lowered through this same context,
    ///   so a `DefinedType` whose underlying is `xs:decimal` becomes
    ///   `Ty::Number`. Without a resolver — or when the chain is unresolved
    ///   or cyclic — the result is `Ty::Unknown`.
    ///
    /// Three-segment paths (`Документы.ПКО.СоздатьДокумент`) are delegated
    /// to the resolver in Task 7 — this method returns `Ty::Unknown` for
    /// anything beyond the 2-segment case so callers cannot silently observe
    /// a wrong tail.
    pub fn lower_qualified(&self, qname: &QualifiedName) -> Ty {
        let mut visited = HashSet::new();
        self.lower_qualified_inner(qname, &mut visited)
    }

    fn lower_qualified_inner(&self, qname: &QualifiedName, visited: &mut HashSet<String>) -> Ty {
        if qname.len() != 2 {
            return Ty::Unknown;
        }

        let prefix = qname.first().as_str();

        // `ОпределяемыйТип.X` / `DefinedType.X` — resolve through the
        // attached resolver, then lower the underlying `AttributeType`
        // recursively. Two distinct cycle layers operate here:
        //
        // 1. **Lowering-level guard (`visited`)** — tracks DefinedTypes that
        //    are currently in the process of being lowered up the call stack,
        //    snapshot/restore-style. Without it, a self-referential
        //    `A → Composite{A, …}` would recurse forever between the
        //    `Composite` arm lowering and re-entry into `lower_qualified`.
        //    The set must be popped when the arm exits so sibling arms of
        //    the same `Composite` start from the same shared ancestry but do
        //    not see *each other's* chain.
        // 2. **Chain guard (inside `resolve_defined_type_terminal`)** — a
        //    fresh, *local* set per call protects against `A → B → A`
        //    chains. Keeping this set local is essential: two sibling arms
        //    `DefT.A` and `DefT.B` that happen to chain through the same
        //    intermediate `X → terminal` must each be free to walk through
        //    `X`, otherwise the second arm collapses to `Ty::Unknown`.
        if is_defined_type_prefix(prefix) {
            let Some(resolver) = self.resolver else {
                return Ty::Unknown;
            };
            let name = qname.last().as_str();
            let key = name.to_lowercase();

            if !visited.insert(key.clone()) {
                // Already inside a lowering of this DefinedType higher up
                // the stack — break the recursion.
                return Ty::Unknown;
            }

            let mut chain_visited = HashSet::new();
            let result = resolve_defined_type_terminal(resolver, name, &mut chain_visited)
                .map(|underlying| {
                    let tref = TypeRef::from_attribute_type(underlying);
                    self.lower_type_ref_inner(&tref, visited)
                })
                .unwrap_or(Ty::Unknown);

            visited.remove(&key);
            return result;
        }

        match metadata_kind_from_prefix(prefix) {
            Some(kind) => Ty::MetadataRef { kind, name: qname.last().clone() },
            None => Ty::Unknown,
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
        "businessprocessref" | "бизнеспроцессссылка" => {
            Some(MetadataKind::BusinessProcessRef)
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
    use hir_def::type_ref::BuiltinTypeRef;

    fn ctx() -> TyLoweringContext<'static> {
        TyLoweringContext::new()
    }

    #[test]
    fn ty_lowering_builtin_primitive() {
        // `BuiltinTypeRef` → concrete `Ty` primitive.
        assert_eq!(ctx().lower_type_ref(&TypeRef::Builtin(BuiltinTypeRef::Number)), Ty::Number);
        assert_eq!(ctx().lower_type_ref(&TypeRef::Builtin(BuiltinTypeRef::String)), Ty::String);
        assert_eq!(
            ctx().lower_type_ref(&TypeRef::Builtin(BuiltinTypeRef::Undefined)),
            Ty::Undefined
        );
        assert_eq!(
            ctx().lower_type_ref(&TypeRef::Builtin(BuiltinTypeRef::ValueTable)),
            Ty::ValueTable
        );
    }

    #[test]
    fn ty_lowering_bare_array_and_map_drop_element_types() {
        // Element types inside `TypeRef::Array` / `::Map` are deliberately
        // dropped in M2: `Ty::Array` / `Ty::Map` do not yet carry them. Once
        // parameterised collections land, this test will be replaced.
        let array_with_elem =
            TypeRef::Array(Some(Box::new(TypeRef::Builtin(BuiltinTypeRef::Number))));
        assert_eq!(ctx().lower_type_ref(&array_with_elem), Ty::Array);

        let map_with_kv = TypeRef::Map(Some((
            Box::new(TypeRef::Builtin(BuiltinTypeRef::String)),
            Box::new(TypeRef::Builtin(BuiltinTypeRef::Number)),
        )));
        assert_eq!(ctx().lower_type_ref(&map_with_kv), Ty::Map);
    }

    #[test]
    fn ty_lowering_bare_builtin_bilingual() {
        // Cascade step 1: `from_bare_name` catches builtins in both languages.
        assert_eq!(ctx().lower_bare_name(&Name::new("Число")), Ty::Number);
        assert_eq!(ctx().lower_bare_name(&Name::new("NUMBER")), Ty::Number);
        assert_eq!(ctx().lower_bare_name(&Name::new("Массив")), Ty::Array);
        assert_eq!(ctx().lower_bare_name(&Name::new("Соответствие")), Ty::Map);
    }

    #[test]
    fn ty_lowering_manager_collection_plural() {
        // Cascade step 2: MDO plural → ManagerCollection.
        assert_eq!(
            ctx().lower_bare_name(&Name::new("Документы")),
            Ty::ManagerCollection(MdoType::Document)
        );
        assert_eq!(
            ctx().lower_bare_name(&Name::new("Справочники")),
            Ty::ManagerCollection(MdoType::Catalog)
        );
    }

    #[test]
    fn ty_lowering_bare_unknown_falls_to_platform_object() {
        // Cascade step 4: unknown bare name → PlatformObject(name). Matches
        // the legacy `infer::Expr::New` fallback that lets `Новый Запрос`
        // type the expression as a platform object even without verifying
        // against `bsl_platform`.
        let request = Name::new("Запрос");
        assert_eq!(ctx().lower_bare_name(&request), Ty::PlatformObject(request));

        // Case is preserved verbatim — the caller owns display casing.
        let mixed = Name::new("HTTPЗапрос");
        assert_eq!(ctx().lower_bare_name(&mixed), Ty::PlatformObject(mixed));
    }

    #[test]
    fn ty_lowering_bare_metadata_prefix_without_name_is_unknown() {
        // Guard against the `AnyObjectRef` mis-routing Codex flagged:
        // a stray `СправочникСсылка` or `CatalogRef` without an object name
        // must never become `Ty::PlatformObject("CatalogRef")`. Both
        // languages covered because `metadata_kind_from_prefix` is
        // case-insensitive bilingual.
        assert_eq!(ctx().lower_bare_name(&Name::new("СправочникСсылка")), Ty::Unknown);
        assert_eq!(ctx().lower_bare_name(&Name::new("CatalogRef")), Ty::Unknown);
        assert_eq!(ctx().lower_bare_name(&Name::new("documentobject")), Ty::Unknown);
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
            let qname = QualifiedName::from_segments([Name::new(prefix), Name::new("Х")]);
            assert_eq!(
                ctx().lower_qualified(&qname),
                Ty::Unknown,
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
            let qname = QualifiedName::from_segments([Name::new(prefix), Name::new("Х")]);
            assert_eq!(
                ctx().lower_qualified(&qname),
                Ty::MetadataRef { kind: expected, name: Name::new("Х") },
                "expected MetadataRef({expected:?}) for `{prefix}.Х`"
            );
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
            let qname = QualifiedName::from_segments([Name::new(prefix), Name::new("Х")]);
            assert_eq!(
                ctx().lower_qualified(&qname),
                Ty::MetadataRef { kind: expected, name: Name::new("Х") },
                "expected MetadataRef({expected:?}) for `{prefix}.Х`"
            );
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
            let qname = QualifiedName::from_segments([Name::new(prefix), Name::new("Х")]);
            assert_eq!(
                ctx().lower_qualified(&qname),
                Ty::MetadataRef { kind: expected, name: Name::new("Х") },
                "expected MetadataRef({expected:?}) for `{prefix}.Х`"
            );
        }
    }

    #[test]
    fn ty_lowering_qualified_metadata_ref_english() {
        let qname = QualifiedName::from_segments([Name::new("CatalogRef"), Name::new("Товары")]);
        assert_eq!(
            ctx().lower_qualified(&qname),
            Ty::MetadataRef { kind: MetadataKind::CatalogRef, name: Name::new("Товары") }
        );
    }

    #[test]
    fn ty_lowering_qualified_metadata_ref_russian() {
        let qname = QualifiedName::from_segments([Name::new("ДокументСсылка"), Name::new("ПКО")]);
        assert_eq!(
            ctx().lower_qualified(&qname),
            Ty::MetadataRef { kind: MetadataKind::DocumentRef, name: Name::new("ПКО") }
        );
    }

    #[test]
    fn ty_lowering_qualified_unknown_prefix_is_unknown() {
        // Not a MetadataKind prefix; resolver will produce the user-facing
        // diagnostic in Task 7.
        let qname = QualifiedName::from_segments([Name::new("ОбщийМодуль"), Name::new("Х")]);
        assert_eq!(ctx().lower_qualified(&qname), Ty::Unknown);
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
        assert_eq!(ctx().lower_qualified(&three), Ty::Unknown);
    }

    #[test]
    fn ty_lowering_union_flows_through_ty_union_constructor() {
        // Each member lowers through the same `lower_type_ref`, then the
        // smart constructor normalises the result. Sibling primitives stay
        // distinct; `Ty::union` imposes a stable order so two syntactically
        // different composites with the same member set compare equal.
        let tr = TypeRef::Union(vec![
            TypeRef::Builtin(BuiltinTypeRef::Number),
            TypeRef::Builtin(BuiltinTypeRef::String),
        ]);
        let ty = ctx().lower_type_ref(&tr);
        match ty {
            Ty::Union(ref parts) => assert_eq!(parts.len(), 2),
            _ => panic!("expected Ty::Union, got {ty:?}"),
        }

        // Flipping the order reaches the same semantic `Ty`.
        let flipped = TypeRef::Union(vec![
            TypeRef::Builtin(BuiltinTypeRef::String),
            TypeRef::Builtin(BuiltinTypeRef::Number),
        ]);
        assert_eq!(ctx().lower_type_ref(&flipped), ty);
    }

    #[test]
    fn ty_lowering_union_singleton_collapses() {
        // `TypeRef::Union([x])` goes through `Ty::union([lowered_x])` which
        // unwraps to `lowered_x` — callers never have to pattern-match on a
        // one-element union.
        let tr = TypeRef::Union(vec![TypeRef::Builtin(BuiltinTypeRef::Number)]);
        assert_eq!(ctx().lower_type_ref(&tr), Ty::Number);
    }

    #[test]
    fn ty_lowering_union_empty_becomes_unknown() {
        // Empty union has no type information — `Ty::union([])` returns
        // `Ty::Unknown`, keeping the "stated but empty" case distinguishable
        // from a truly absent type.
        assert_eq!(ctx().lower_type_ref(&TypeRef::Union(vec![])), Ty::Unknown);
    }

    #[test]
    fn ty_lowering_type_ref_routes_through_name_branches() {
        // `TypeRef::Name([single])` → bare-name cascade.
        let single = TypeRef::Name(QualifiedName::from_segments([Name::new("Массив")]));
        assert_eq!(ctx().lower_type_ref(&single), Ty::Array);

        // `TypeRef::Name([prefix, name])` → qualified cascade.
        let qualified = TypeRef::Name(QualifiedName::from_segments([
            Name::new("СправочникСсылка"),
            Name::new("Номенклатура"),
        ]));
        assert_eq!(
            ctx().lower_type_ref(&qualified),
            Ty::MetadataRef {
                kind: MetadataKind::CatalogRef, name: Name::new("Номенклатура")
            }
        );

        // AnyRef / Unknown remain Unknown until Ty::AnyRef lands.
        assert_eq!(ctx().lower_type_ref(&TypeRef::AnyRef), Ty::Unknown);
        assert_eq!(ctx().lower_type_ref(&TypeRef::Unknown), Ty::Unknown);
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
        assert_eq!(ctx().lower_qualified(&qname), Ty::Unknown);
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
        assert_eq!(lowering.lower_qualified(&qname), Ty::Number);
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
        assert_eq!(lowering.lower_qualified(&qname), Ty::String);
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
        assert_eq!(lowering.lower_qualified(&qname), Ty::Unknown);
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
        match lowering.lower_qualified(&qname) {
            Ty::Union(arms) => {
                assert!(arms.contains(&Ty::Number), "union must contain Number");
                assert!(arms.contains(&Ty::String), "union must contain String");
            }
            other => panic!("expected Ty::Union, got {other:?}"),
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
        assert_eq!(lowering.lower_type_ref(&tref), Ty::Number);
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
        // Self-reference inside the composite collapses to `Ty::Unknown`,
        // the other arm to `Ty::Number` — the smart constructor builds a
        // union of the two.
        match lowering.lower_qualified(&qname) {
            Ty::Union(arms) => {
                assert!(arms.contains(&Ty::Number));
                assert!(arms.contains(&Ty::Unknown));
            }
            other => panic!("expected Ty::Union, got {other:?}"),
        }
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
            assert_eq!(
                lowering.lower_qualified(&qname),
                Ty::Boolean,
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
        assert_eq!(lowering.lower_qualified(&qname), Ty::Boolean);
    }
}
