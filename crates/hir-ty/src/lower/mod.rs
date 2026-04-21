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

use bsl_metadata::MdoType;
use hir_def::path::QualifiedName;
use hir_def::ty::{MetadataKind, Ty};
use hir_def::type_ref::TypeRef;
use hir_def::Name;

/// Adapter that lowers a syntactic [`TypeRef`] into a semantic [`Ty`].
///
/// Stateless in M2. Gains a `Resolver` + database reference in Task 7 when
/// three-segment paths and cross-module resolution move through this layer.
#[derive(Debug, Default, Clone, Copy)]
pub struct TyLoweringContext;

impl TyLoweringContext {
    /// Build an empty lowering context.
    pub fn new() -> Self {
        Self
    }

    /// Lower a [`TypeRef`] into a [`Ty`].
    ///
    /// Dispatches over the `TypeRef` variants:
    /// - `Builtin(b)` → the fixed primitive from [`builtin_names::builtin_to_ty`].
    /// - `Array(_)` / `Map(_)` → `Ty::Array` / `Ty::Map` (element types are
    ///   dropped in M2 — `Ty` does not carry them yet).
    /// - `Name(qname)` → [`Self::lower_qualified`] for 2+ segments, else
    ///   [`Self::lower_bare_name`].
    /// - `AnyRef` / `Unknown` → `Ty::Unknown` (deliberately: we have no
    ///   `Ty::AnyRef` variant yet, and narrowing it to a concrete kind would
    ///   lie to callers).
    pub fn lower_type_ref(&self, type_ref: &TypeRef) -> Ty {
        match type_ref {
            TypeRef::Builtin(b) => builtin_names::builtin_to_ty(*b),
            TypeRef::Array(_) => Ty::Array,
            TypeRef::Map(_) => Ty::Map,
            TypeRef::Name(qname) => match qname.len() {
                0 => Ty::Unknown,
                1 => self.lower_bare_name(qname.first()),
                _ => self.lower_qualified(qname),
            },
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
    /// The only pattern M2 decodes is the 2-segment metadata reference
    /// (`СправочникСсылка.Товары`, `DocumentObject.ПКО`). Three-segment
    /// paths (`Документы.ПКО.СоздатьДокумент`) are delegated to the resolver
    /// in Task 7 — this method returns `Ty::Unknown` for anything beyond the
    /// 2-segment case so callers cannot silently observe a wrong tail.
    pub fn lower_qualified(&self, qname: &QualifiedName) -> Ty {
        if qname.len() != 2 {
            return Ty::Unknown;
        }

        match metadata_kind_from_prefix(qname.first().as_str()) {
            Some(kind) => Ty::MetadataRef { kind, name: qname.last().clone() },
            None => Ty::Unknown,
        }
    }
}

/// Prefix → [`MetadataKind`] table for the reference/object forms currently
/// modelled by [`Ty::MetadataRef`]. Both Russian and English variants are
/// accepted to keep the resolver case-insensitive and bilingual end-to-end.
///
/// # Narrower than `type_ref::mdo_ref_prefix`
///
/// `hir-def::type_ref::from_attribute_type` round-trips every XML prefix
/// in `REF_TYPE_MAP` (`InformationRegisterRef`, `EnumRef`, `TaskRef`,
/// `ExchangePlanRef`, `ConstantValueManager`, …). `MetadataKind` only has
/// six variants today, so prefixes outside that subset intentionally land
/// on `Ty::Unknown` here. Extending `Ty::MetadataKind` (and the surrounding
/// lookups in `infer` / `bsl-platform`) unlocks them in one go — track this
/// in `docs/architecture/TYPE_SYSTEM.md` when M3 adds more kinds.
fn metadata_kind_from_prefix(prefix: &str) -> Option<MetadataKind> {
    match prefix.to_lowercase().as_str() {
        "catalogref" | "справочникссылка" => Some(MetadataKind::CatalogRef),
        "catalogobject" | "справочникобъект" => Some(MetadataKind::CatalogObject),
        "documentref" | "документссылка" => Some(MetadataKind::DocumentRef),
        "documentobject" | "документобъект" => Some(MetadataKind::DocumentObject),
        "informationregisterrecordmanager" | "регистрсведенийменеджерзаписи" => {
            Some(MetadataKind::InformationRegisterRecordManager)
        }
        "accumulationregisterrecordset" | "регистрнакоплениянаборзаписей" => {
            Some(MetadataKind::AccumulationRegisterRecordSet)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hir_def::type_ref::BuiltinTypeRef;

    fn ctx() -> TyLoweringContext {
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
        // Narrow by design: `MetadataKind` has six variants, but the bridge
        // emits many more XML-valid prefixes (`EnumRef`, `TaskRef`,
        // `InformationRegisterRef`, `ConstantValueManager`, …). Until M3
        // widens `MetadataKind`, these must land on `Ty::Unknown` instead of
        // producing a misleading `MetadataRef` with a wrong kind.
        for prefix in [
            "EnumRef",
            "TaskRef",
            "BusinessProcessRef",
            "ExchangePlanRef",
            "InformationRegisterRef",
            "AccumulationRegisterRef",
            "ConstantValueManager",
        ] {
            let qname = QualifiedName::from_segments([Name::new(prefix), Name::new("Х")]);
            assert_eq!(
                ctx().lower_qualified(&qname),
                Ty::Unknown,
                "expected Unknown for `{prefix}.Х`"
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
}
