use bsl_types::builders::Builders;
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::TypeId;
use hir_def::Name;

pub(crate) enum PlatformGlobalLookup {
    Resolved(TypeId),
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
        return PlatformGlobalLookup::Resolved(return_ty);
    }

    if platform.get_global_property(receiver_name.as_str()).is_some() {
        return PlatformGlobalLookup::KnownContainerMissingMember;
    }

    PlatformGlobalLookup::NotAContainer
}

pub fn resolve_platform_global_property_type(db: &dyn TypeKernelDb, name: &Name) -> Option<TypeId> {
    let prop = bsl_platform::PlatformDataInner::instance().get_global_property(name.as_str())?;
    let declared = prop.property_types.first()?;
    let lowering = crate::lower::TyLoweringContext::new();
    Some(lowering.lower_bare_name_id(db, &Name::new(declared.as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
