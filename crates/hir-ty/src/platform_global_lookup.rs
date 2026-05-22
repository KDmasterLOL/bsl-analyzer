//! Platform global-member fallback for `<Container>.<Method>()` shape.
//!
//! Resolves calls whose first segment is a platform global identifier
//! (e.g. `ОбработкаОшибок`, `ИспользоватьИмя`) declared in the
//! platform's global-context catalogue, when the workspace `Resolver`
//! has already declined to claim the receiver as a CommonModule.
//!
//! Single source of truth — both `infer_qualified_call` (existing
//! workspace-first → platform-fallback flow on `Module.Method()`) and
//! the bare-IDENT cascade gate (added in Phase 1 for `Field`-shaped
//! 2-segment calls in `infer_call`) share this helper instead of
//! duplicating the inline lookup.
//!
//! ## Narrowing rules
//!
//! - The caller is responsible for confirming the workspace is NOT
//!   the authoritative resolver (i.e. `Resolver::user_common_module_exists`
//!   is `false` for the receiver name). Without this gate the platform
//!   would silently mask a real diagnostic on a user CommonModule that
//!   happens to share its name with a platform global (test
//!   `test_user_module_shadows_platform_global`).
//! - The lookup is tri-state — see [`PlatformGlobalLookup`]. Phase 2
//!   needs to distinguish "method missing on a known global container"
//!   from "receiver isn't a known container at all" so the cascade gate
//!   can pick `MethodNotFound` for the first case and `ReceiverNotResolved`
//!   for the second; collapsing them (as the previous `Option<Ty>` did)
//!   would emit the wrong kind for `ОбработкаОшибок.НеизвестныйМетод()`.
//! - Lowering uses an empty [`crate::lower::TyLoweringContext`] —
//!   platform return types are bare type names without form / mdo
//!   context, so the empty context is sufficient.

use hir_def::ty::Ty;
use hir_def::Name;

/// Outcome of a platform global-member lookup.
///
/// Distinguishes the three states `(receiver, method)` can be in
/// against the platform global catalogue, so the caller can pick the
/// right diagnostic kind:
///
/// - [`Self::Resolved`] — `receiver` names a known global container
///   AND its declared type carries `method`. The lowered return type
///   is returned.
/// - [`Self::KnownContainerMissingMember`] — `receiver` IS a known
///   global container (e.g. `ОбработкаОшибок`) but its declared type
///   does not carry `method`. The dispatcher emits
///   `UnresolvedMethodCall { MethodNotFound }` — collapsing this with
///   `NotAContainer` would produce the misleading `ReceiverNotResolved`
///   for a real platform method typo.
/// - [`Self::NotAContainer`] — `receiver` is not a global container at
///   all. The cascade gate falls through to its terminal
///   `ReceiverNotResolved` arm.
pub(crate) enum PlatformGlobalLookup {
    Resolved(Ty),
    KnownContainerMissingMember,
    NotAContainer,
}

/// Look up a platform global-context method.
///
/// Returns a tri-state [`PlatformGlobalLookup`] so callers can
/// distinguish "method missing on a known global container" from
/// "receiver isn't a known container at all".
///
/// Pure function (no `db` dependency) — `PlatformDataInner` is a
/// singleton seeded at startup. Callers that need to gate on
/// workspace-vs-platform precedence (e.g. `infer_qualified_call`) must
/// perform the `module_index().resolve_common_module()` probe
/// themselves before calling this helper.
pub(crate) fn try_resolve_platform_global_member(
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
                lowering.lower_bare_name(&Name::new(s.as_str()))
            })
            .unwrap_or(Ty::Unknown);
        return PlatformGlobalLookup::Resolved(return_ty);
    }

    if platform.get_global_property(receiver_name.as_str()).is_some() {
        return PlatformGlobalLookup::KnownContainerMissingMember;
    }

    PlatformGlobalLookup::NotAContainer
}

/// Resolve a bare platform-global *property* name to its declared `Ty`.
///
/// Companion to [`try_resolve_platform_global_member`]:
/// - That one resolves the `<Container>.<Method>()` call shape and
///   returns the method's return type.
/// - This one resolves the bare-identifier shape (`Метаданные`,
///   `Справочники`, `ОбработкаОшибок`, …) and returns the property's
///   declared type — the value the user sees when they write the name on
///   its own and expect dot-access to surface members of that type.
///
/// Returns `None` when:
/// - `name` is not a platform global property, OR
/// - the property has no declared type (defensive — platform data
///   normally guarantees at least one entry).
///
/// Single source of truth for both `infer.rs::infer_path_name` step 6
/// (bare-identifier inference) and `ide::completion::platform_completion`
/// (fallback when `Semantics::type_of_expr` returns `Unknown`). Without
/// this helper both call sites lowered through identical inline code
/// against the same singleton, easy to drift.
///
/// Pure function — no `db` dependency, same `PlatformDataInner` singleton
/// the rest of this module reads.
pub fn resolve_platform_global_property_type(name: &Name) -> Option<Ty> {
    let prop = bsl_platform::PlatformDataInner::instance().get_global_property(name.as_str())?;
    let declared = prop.property_types.first()?;
    let lowering = crate::lower::TyLoweringContext::new();
    Some(lowering.lower_bare_name(&Name::new(declared.as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_platform_global_property_type_returns_declared_ty_for_known_global() {
        // `Метаданные` is an always-present platform global (declared
        // type: `КонфигурацияМетаданныеОбъект`). The helper must surface
        // its declared type so completion / inference can route
        // dot-access through the proper platform type.
        //
        // No skip-when-empty branch: platform data is shipped with the
        // analyzer (`crates/bsl-platform/data/platform_data.json`). If
        // it's missing the whole inference layer is non-functional, so
        // this test should fail loudly rather than silently pass.
        let ty = resolve_platform_global_property_type(&Name::new("Метаданные"))
            .expect("`Метаданные` must resolve via platform data");
        assert!(!matches!(ty, Ty::Unknown), "expected non-Unknown Ty, got {ty:?}");
    }

    #[test]
    fn resolve_platform_global_property_type_returns_none_for_unknown_name() {
        // Names that aren't platform globals must return `None` so the
        // caller's cascade falls through to the next gate (e.g. the
        // `PlatformObject(name)` fallback in completion).
        let result = resolve_platform_global_property_type(&Name::new(
            "ЗаведомоНеСуществуетГлобалПлатформы",
        ));
        assert!(result.is_none());
    }
}
