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
