use bsl_types::builders::Builders;
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;
use cfg_types::IdConversion;
use hir_def::execution_env::EnvFlags;
use hir_def::hir::{Expr, Stmt};
use hir_def::resolver::{Resolution, Resolver};
use hir_def::{Body, BodySourceMap, ExprId, Name};
use intern::NormName;
use syntax::TextSize;

use crate::db::HirDatabase;

pub(crate) enum PlatformGlobalLookup {
    Resolved { ty: TypeId, env: EnvFlags },
    KnownContainerMissingMember,
    NotAContainer,
}

pub(crate) fn try_resolve_platform_global_member(
    db: &dyn TypeKernelDb,
    receiver_name: &Name,
    method_name: &Name,
) -> PlatformGlobalLookup {
    let platform = bsl_platform::PlatformDataInner::instance();

    if let Some(method) =
        platform.resolve_global_member(receiver_name.as_str(), method_name.as_str())
    {
        let return_ty = method
            .return_type
            .as_ref()
            .map(|s| {
                let lowering = crate::lower::TyLoweringContext::new();
                lowering.lower_bare_name_id(db, &Name::new(s.as_str()))
            })
            .unwrap_or(db.unknown());
        return PlatformGlobalLookup::Resolved {
            ty: return_ty,
            env: EnvFlags::from_platform_context(method.context.as_ref()),
        };
    }

    if platform.get_global_property(receiver_name.as_str()).is_some() {
        return PlatformGlobalLookup::KnownContainerMissingMember;
    }

    PlatformGlobalLookup::NotAContainer
}

pub fn resolve_platform_global_property_type(db: &dyn TypeKernelDb, name: &Name) -> Option<TypeId> {
    resolve_platform_global_property(db, name).map(|(ty, _)| ty)
}

/// Like [`resolve_platform_global_property_type`], additionally returning the
/// property's execution-environment availability.
pub(crate) fn resolve_platform_global_property(
    db: &dyn TypeKernelDb,
    name: &Name,
) -> Option<(TypeId, EnvFlags)> {
    let prop = bsl_platform::PlatformDataInner::instance().get_global_property(name.as_str())?;
    let declared = prop.property_types.first()?;
    let lowering = crate::lower::TyLoweringContext::new();
    Some((
        lowering.lower_bare_name_id(db, &Name::new(declared.as_str())),
        EnvFlags::from_platform_context(prop.context.as_ref()),
    ))
}

/// Environment availability of the global manager-collection property
/// (`Справочники`, `Перечисления`, …) backing `mdo_type`. Manager collections
/// resolve through the metadata layer, but their availability is owned by the
/// platform's Global-context property record; a type without such a record
/// carries no restriction.
///
/// Public because completion must judge the same table the diagnostic does:
/// two hand-written copies of this bridge would drift apart silently.
pub fn manager_collection_env(mdo_type: bsl_metadata::MdoType) -> EnvFlags {
    EnvFlags::from_platform_context(
        mdo_type.hbk_global_property().and_then(|prop| prop.context.as_ref()),
    )
}

/// Body scope for judging a claim at a specific read position.
pub struct BodyShadowScope<'a> {
    pub body: &'a Body,
    pub source_map: &'a BodySourceMap,
    /// Reads are judged positionally, matching sequential inference: an
    /// assignment claims the name only for reads after it has COMPLETED —
    /// inside its own right-hand side the name still denotes the previous
    /// owner (`Справочники = Справочники.Х`). A declared binding claims the
    /// whole body.
    pub read_offset: TextSize,
}

/// A user symbol's claim on a bare global name at a read.
pub struct BareGlobalClaim {
    /// The value of the textually-last assignment completed before the read,
    /// when the claiming symbol is a body local written in this body — its
    /// inferred type is the local's type at the read. The type of the READ
    /// itself cannot serve: inference's name cascade re-types an
    /// unknown-valued local as the same-named global, so the read's type
    /// proves nothing about the owner. `None` when the claim has no reaching
    /// write (declared-but-unwritten locals, module items, form members,
    /// common modules).
    pub reaching_value: Option<ExprId>,
}

/// The user symbol claiming a bare global name, if any: a body local (written
/// or declared), a module-level variable or method, a form attribute or
/// form-self property, an implicit `ЭтотОбъект`/record-set member, or a
/// workspace common module. `None` means the name denotes the platform
/// global.
///
/// The shared predicate behind both the availability diagnostic and
/// completion's availability gate — the two must judge shadowing identically.
/// Inference passes `scope: None` and layers its own cached, flow-aware body
/// facts on top; callers without inference state pass the enclosing body.
pub fn bare_global_name_claim(
    db: &dyn HirDatabase,
    resolver: &Resolver,
    scope: Option<&BodyShadowScope<'_>>,
    name: &Name,
) -> Option<BareGlobalClaim> {
    // Data members own a write: assigning over a module variable or a form /
    // object member stores into that member — a value inference deliberately
    // does not flow-type, so no reaching write is reported for it. Resolution
    // order decides ownership: a method winning the name blocks the member
    // lookups, so an assignment over it creates an implicit body local even
    // when a same-named implicit member exists.
    let write_owner_claim = || match resolver.resolve_name(db, name) {
        Some(Resolution::Variable(_)) => true,
        Some(Resolution::Method(_)) => false,
        _ => {
            crate::form_self::resolve_form_self_property(db, resolver, name).is_some()
                || crate::form_attr::resolve_form_attribute(db, resolver, name).is_some()
                || crate::this_object_attr::resolve_this_object_member(db, resolver, name).is_some()
                || crate::this_object_attr::resolve_this_record_set_member(db, resolver, name)
                    .is_some()
        }
    };
    // Methods and common modules shadow a read but cannot be assigned to —
    // an assignment over them creates an implicit body local instead.
    let read_only_claim = || {
        matches!(resolver.resolve_name(db, name), Some(Resolution::Method(_)))
            || resolver.user_common_module_exists(db, name)
    };
    let Some(scope) = scope else {
        return (write_owner_claim() || read_only_claim())
            .then_some(BareGlobalClaim { reaching_value: None });
    };

    let key = NormName::intern(name.as_str());
    let body_declares =
        scope.body.bindings_iter().any(|(_, b)| NormName::intern(b.name.as_str()) == key);
    if !body_declares && write_owner_claim() {
        return Some(BareGlobalClaim { reaching_value: None });
    }
    let mut reaching: Option<(TextSize, ExprId)> = None;
    for (_, stmt) in scope.body.stmts_iter() {
        let Stmt::Assign { target, value } = stmt else { continue };
        let target_id = ExprId::from_idx(*target);
        if !matches!(scope.body.expr(target_id), Expr::Path(n)
            if NormName::intern(n.as_str()) == key)
        {
            continue;
        }
        let value_id = ExprId::from_idx(*value);
        let completed_at = scope
            .source_map
            .expr_range(value_id)
            .or_else(|| scope.source_map.expr_range(target_id))
            .map(|range| range.end());
        let Some(end) = completed_at else { continue };
        if end <= scope.read_offset && reaching.is_none_or(|(best, _)| end >= best) {
            reaching = Some((end, value_id));
        }
    }
    if let Some((_, value_id)) = reaching {
        return Some(BareGlobalClaim { reaching_value: Some(value_id) });
    }
    (body_declares || read_only_claim()).then_some(BareGlobalClaim { reaching_value: None })
}

/// Resolve a bare identifier that names a platform **system enumeration**
/// (`ВидДвиженияБухгалтерии`, `ВидСравненияКомпоновкиДанных`, …) to its platform
/// object type, so member access such as `ВидДвиженияБухгалтерии.Дебет` resolves
/// against the enum's members.
///
/// System enums are the platform types that may be referenced directly by name
/// as a value: they have no constructor and no methods, and their members are
/// enum values — modelled as properties that carry **no declared type** (a value
/// like `Дебет` simply *is* the enum). Constructible value types
/// (`ТаблицаЗначений`, `Массив`, …) carry a constructor and instance methods, and
/// property-bearing objects (`ПолеНастройки`, `ПараметрыОбменаДанными`) have
/// members with concrete types; both are excluded, since a bare type name is not
/// a value for them.
pub fn resolve_platform_system_enum_type(db: &dyn TypeKernelDb, name: &Name) -> Option<TypeId> {
    let platform = bsl_platform::PlatformDataInner::instance();
    let ty = platform.get_type(name.as_str())?;
    let english = ty.english_name.as_str();

    if !platform.get_constructors(english).is_empty() {
        return None;
    }
    if !platform.get_type_methods(english).is_empty() {
        return None;
    }

    let members = platform.get_type_properties(english);
    if members.is_empty() {
        return None;
    }
    if members.iter().any(|m| !m.property_types.is_empty()) {
        return None;
    }

    Some(db.platform_object(ty.name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object_resolver::ConfigsObjectResolver;

    #[test]
    fn resolve_platform_global_property_type_returns_declared_ty_for_known_global() {
        let db = bsl_types::testing::InMemoryDb::default();
        let id = resolve_platform_global_property_type(&db, &Name::new("Метаданные"))
            .expect("`Метаданные` must resolve via platform data");
        assert_ne!(id, db.unknown(), "expected non-Unknown type id");
    }

    #[test]
    fn resolve_platform_global_property_type_returns_none_for_unknown_name() {
        let db = bsl_types::testing::InMemoryDb::default();
        let result = resolve_platform_global_property_type(
            &db,
            &Name::new("ЗаведомоНеСуществуетГлобалПлатформы"),
        );
        assert!(result.is_none());
    }

    #[test]
    fn system_enum_resolves_to_platform_object() {
        let db = bsl_types::testing::InMemoryDb::default();
        let id = resolve_platform_system_enum_type(&db, &Name::new("ВидДвиженияБухгалтерии"))
            .expect("system enum must resolve to its platform object");
        assert_eq!(id, db.platform_object("ВидДвиженияБухгалтерии".to_string()));
    }

    #[test]
    fn constructible_value_type_is_not_a_bare_enum_value() {
        let db = bsl_types::testing::InMemoryDb::default();
        // ТаблицаЗначений has a constructor and methods — not referenceable as a bare value.
        assert!(resolve_platform_system_enum_type(&db, &Name::new("ТаблицаЗначений")).is_none());
    }

    #[test]
    fn property_bearing_object_is_not_a_bare_enum_value() {
        let db = bsl_types::testing::InMemoryDb::default();
        // No ctor/methods, but its members carry concrete types — an object, not an enum.
        assert!(resolve_platform_system_enum_type(&db, &Name::new("CustomField")).is_none());
        assert!(
            resolve_platform_system_enum_type(&db, &Name::new("DataExchangeParameters")).is_none()
        );
    }

    #[test]
    fn unknown_name_is_not_a_system_enum() {
        let db = bsl_types::testing::InMemoryDb::default();
        assert!(resolve_platform_system_enum_type(&db, &Name::new("ЗаведомоНеТип")).is_none());
    }

    #[test]
    fn system_enum_member_resolves_end_to_end() {
        let db = bsl_types::testing::InMemoryDb::default();
        let receiver = resolve_platform_system_enum_type(&db, &Name::new("ВидДвиженияБухгалтерии"))
            .expect("bare enum name must type as a platform object");

        let member = crate::field_lookup::lookup_field(
            &db,
            &ConfigsObjectResolver(&[]),
            receiver,
            &Name::new("Дебет"),
        );
        assert!(
            member.is_some(),
            "`ВидДвиженияБухгалтерии.Дебет` must resolve as an enum member via platform properties"
        );

        assert!(
            crate::field_lookup::lookup_field(
                &db,
                &ConfigsObjectResolver(&[]),
                receiver,
                &Name::new("НетТакогоЧлена")
            )
            .is_none(),
            "a non-existent enum member must not resolve"
        );
    }
}
