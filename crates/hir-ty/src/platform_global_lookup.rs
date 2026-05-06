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
//! ## Narrowing rules (mirror the inline they replace)
//!
//! - The caller is responsible for confirming the workspace is NOT
//!   the authoritative resolver (i.e. `Resolver::user_common_module_exists`
//!   is `false` for the receiver name). Without this gate the platform
//!   would silently mask a real diagnostic on a user CommonModule that
//!   happens to share its name with a platform global (test
//!   `test_user_module_shadows_platform_global`).
//! - Returns `None` when the platform catalogue has no member with that
//!   `(receiver, method)` pair — the caller decides whether to emit
//!   `UnresolvedMethodCall { ReceiverNotResolved }` or another shape.
//! - Returns `Some(Ty)` with the lowered return type when resolved.
//!   Lowering uses an empty [`crate::lower::TyLoweringContext`] —
//!   platform return types are bare type names without form / mdo
//!   context, so the empty context is sufficient.

use hir_def::ty::Ty;
use hir_def::Name;

/// Look up a platform global-context method.
///
/// Returns the lowered return type if the platform's global catalogue
/// has a member matching `(receiver_name, method_name)`. Otherwise
/// returns `None`.
///
/// Pure function (no `db` dependency) — `PlatformDataInner` is a
/// singleton seeded at startup. Callers that need to gate on
/// workspace-vs-platform precedence (e.g. `infer_qualified_call`) must
/// perform the `module_index().resolve_common_module()` probe
/// themselves before calling this helper.
pub(crate) fn try_resolve_platform_global_member(
    receiver_name: &Name,
    method_name: &Name,
) -> Option<Ty> {
    let method = bsl_platform::PlatformDataInner::instance()
        .resolve_global_member(receiver_name.as_str(), method_name.as_str())?;

    let return_ty = method
        .return_type
        .as_ref()
        .map(|s| {
            let lowering = crate::lower::TyLoweringContext::new();
            lowering.lower_bare_name(&Name::new(s.as_str()))
        })
        .unwrap_or(Ty::Unknown);

    Some(return_ty)
}
