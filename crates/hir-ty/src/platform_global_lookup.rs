use bsl_types::builders::Builders;
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;
use hir_def::execution_env::EnvFlags;
use hir_def::Name;

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
pub(crate) fn manager_collection_env(mdo_type: bsl_metadata::MdoType) -> EnvFlags {
    EnvFlags::from_platform_context(
        mdo_type.hbk_global_property().and_then(|prop| prop.context.as_ref()),
    )
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
