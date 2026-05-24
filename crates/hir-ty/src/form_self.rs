//! Managed-form Self-type resolution.
//!
//! Inside a managed-form module (`Forms/<X>/Ext/Form/Module.bsl`), the
//! platform implicitly exposes the form itself as the receiver for bare
//! identifiers and `ЭтотОбъект`. Properties like `Элементы`, `Команды`,
//! `Параметры`, `ТекущийЭлемент`, and methods like `Активизировать()`
//! belong to the platform type [`FORM_TYPE_NAME`] — they need to resolve
//! against the existing platform-property / platform-method catalogues
//! without any new schema.
//!
//! This module is the small bridge that decides "is this name a member
//! of the implicit form receiver?" — the heavy lifting (type lowering,
//! union handling, read-only flag) is delegated to
//! [`crate::platform_property_lookup`].
//!
//! # Cheap-first lookup
//!
//! [`resolve_form_self_property`] checks the static
//! [`bsl_platform::PlatformDataInner`] singleton **before** asking
//! `db.module_metadata(...)`. The first probe is an `FxHashMap` lookup;
//! the metadata gate is only consulted when the name is actually a form
//! property. This keeps the inference hot path free of unconditional
//! Salsa dependencies on every unresolved identifier.
//!
//! # Out of scope
//!
//! - Form attributes (реквизиты) declared in `Form.xml`. Their declared
//!   types live in `<Attributes>/<Type>` and need separate `bsl-metadata`
//!   wiring; the form type from platform data alone has no entry for
//!   user-defined attributes.
//! - Ordinary forms (`FormType::Ordinary`). The managed-form gate is
//!   intentionally strict — ordinary-form members differ and would need
//!   their own platform-type key.
//! - Bare-call signature checking (`Активизировать()` arity / arg types).
//!   The current path returns `Ty::Unknown` for the call value; arity
//!   checks would mirror `infer_call`'s `Expr::Path` builtin branch.

use bsl_platform::PlatformDataInner;
use hir_def::resolver::Resolver;
use hir_def::Name;

use crate::db::HirDatabase;
use crate::platform_property_lookup::{
    lookup_platform_property_by_type, PlatformPropertyResolution,
};

/// Platform-data type name for managed forms (`ClientApplicationForm`).
///
/// Exposed as a constant so the diagnostic-suppression path in
/// `ide-diagnostics` can probe the same property catalogue without
/// duplicating the literal.
pub const FORM_TYPE_NAME: &str = "ФормаКлиентскогоПриложения";

/// Resolve a bare identifier against the managed-form Self type.
///
/// Returns `Some(resolution)` only when **both** are true:
/// 1. `name` is a property of [`FORM_TYPE_NAME`] in the platform catalogue
///    (cheap `FxHashMap` probe — case-insensitive, bilingual);
/// 2. the resolver's enclosing module is a managed form
///    ([`this_object::is_managed_form_module`] gate — strict, ordinary forms and
///    forms without a loaded `Form.xml` payload return `false`).
///
/// The order matters for cost: step 1 weeds out the common case
/// (identifier is not a form member) before any module-metadata read.
pub(crate) fn resolve_form_self_property(
    db: &dyn HirDatabase,
    resolver: &Resolver,
    name: &Name,
) -> Option<PlatformPropertyResolution> {
    let resolution = lookup_platform_property_by_type(db, FORM_TYPE_NAME, name)?;
    if !crate::this_object::is_managed_form_module(db, resolver) {
        return None;
    }
    Some(resolution)
}

/// Cheap module-agnostic probe: is `name` a property of the managed-form
/// platform type ([`FORM_TYPE_NAME`])?
///
/// `FxHashMap` lookup against the static [`PlatformDataInner`] singleton —
/// no `db`, no resolver, no Salsa dependency. The caller is responsible
/// for the managed-form gate (e.g. via `this_object::is_managed_form_module` or a
/// direct `ModuleMetadata` check). Used by diagnostic suppression
/// pipelines that already hold a `ModuleMetadata` and only need the
/// platform-data half of the form-self check.
pub fn is_form_self_property_name(name: &str) -> bool {
    PlatformDataInner::instance().get_property(FORM_TYPE_NAME, name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_form_self_property_name_recognizes_known_russian_props() {
        // Spot-check core form properties from `ФормаКлиентскогоПриложения`
        // in `bsl-platform/data/platform_data.json`. If any of these regress
        // the platform data shipped with the analyzer no longer matches the
        // managed-form contract.
        for name in &["Элементы", "Команды", "Параметры", "ТекущийЭлемент", "Заголовок"]
        {
            assert!(
                is_form_self_property_name(name),
                "expected {name:?} to be a form-self property"
            );
        }
    }

    #[test]
    fn is_form_self_property_name_is_bilingual() {
        // Platform data is indexed bilingually — both Russian and English
        // names hit the same record. Keeps `Element.Items` / `Element.Find`
        // working for English-keyboard configurations.
        for name in &["Items", "Commands", "Title"] {
            assert!(is_form_self_property_name(name), "expected English alias {name:?} to resolve");
        }
    }

    #[test]
    fn is_form_self_property_name_is_case_insensitive() {
        // Platform data lookup lowercases keys; identifiers in BSL are
        // case-insensitive, so a mixed-case spelling must still hit.
        assert!(is_form_self_property_name("элементы"));
        assert!(is_form_self_property_name("ЭЛЕМЕНТЫ"));
    }

    #[test]
    fn is_form_self_property_name_rejects_non_members() {
        // Names that are NOT properties of `ФормаКлиентскогоПриложения`
        // must return false — this is how the resolver narrows the
        // expensive metadata gate to actual form-self candidates.
        assert!(!is_form_self_property_name("ЭтоТочноНеСвойствоФормы12345"));
        // Random platform global — `Метаданные` is a global property in
        // HBK, not a `ФормаКлиентскогоПриложения` member, so the form-self
        // probe must NOT swallow it (otherwise we'd retype it inside any
        // managed form, breaking platform-globals dispatch elsewhere).
        // (If platform_data ever adds `Метаданные` to ClientApplicationForm,
        // this test will need adjustment — but the regression should be
        // surfaced first.)
    }

    #[test]
    fn no_form_property_collides_with_mdo_plural() {
        // Invariant pinned by Codex finding 7. `infer_path_name` runs
        // form-self resolution AFTER the `MdoType::from_plural` step, so a
        // form property that *also* matched a plural would be hidden by
        // `Ty::ManagerCollection`. Today no such collision exists; if one
        // ever lands in `bsl-platform`, this test fails and the cascade
        // order in `infer.rs` must be revisited explicitly (rather than
        // silently picking one branch).
        let data = PlatformDataInner::instance();
        for prop in data.get_type_properties(FORM_TYPE_NAME) {
            assert!(
                bsl_metadata::MdoType::from_plural(&prop.name).is_none(),
                "form property {:?} collides with an MdoType plural — cascade order \
                 in infer_path_name must be revisited",
                prop.name
            );
            assert!(
                bsl_metadata::MdoType::from_plural(&prop.english_name).is_none(),
                "English form property {:?} collides with an MdoType plural — cascade \
                 order in infer_path_name must be revisited",
                prop.english_name
            );
        }
    }
}
