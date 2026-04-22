//! Adapter for platform-defined constructors (`Новый Массив(...)`).
//!
//! Multi-overload types (e.g. `Массив` has `По количеству элементов` and
//! `На основании фиксированного массива`) are **not** fully surfaced in phase 1 —
//! this adapter picks the first overload and leaves the multi-signature work
//! for phase 2. See `docs/.../plan` for the split rationale.

use bsl_platform::{platform_constructors_query, PlatformDataInner, TypeNameInput};
use ide_db::RootDatabase;
use smol_str::SmolStr;

use crate::adapters::params::build_constructor_params;
use crate::domain::{MethodKind, SignatureSource, SymbolSignature, TypeRef};

/// Builds a signature for `Новый <type_name>(...)`.
///
/// `type_name` is the original user-typed text from the call site (preserves
/// case). Lookup is bilingual/case-insensitive via `platform_constructors_query`;
/// returns `None` when no platform constructor is registered for the type
/// (including cases where the type is unknown, e.g. `Новый НесуществующийТип(`).
pub(super) fn build(db: &dyn RootDatabase, type_name: &str) -> Option<SymbolSignature> {
    let input = TypeNameInput::new(db, type_name.to_string());
    let ctors = platform_constructors_query(db, input);
    // TODO (phase 2): surface all overloads via LSP `signatures[]`.
    // Phase 1 deliberately picks the first overload; the active-overload
    // selection rule needs arity metadata we don't carry yet.
    let ctor = ctors.first()?;

    let docs = PlatformDataInner::instance().get_constructor_docs(ctor.id);

    // English type name lives on the ctor entry; russian is what the user
    // typed in source. Returns carry both so presenters can render a
    // bilingual signature label.
    Some(SymbolSignature {
        kind: MethodKind::Function,
        name_russian: SmolStr::new(type_name),
        name_english: Some(ctor.type_name.clone()),
        qualifier: None,
        prefix: Some("Новый ".into()),
        params: build_constructor_params(&ctor.parameters, docs.as_ref()),
        returns: vec![TypeRef {
            russian: SmolStr::new(type_name),
            english: Some(ctor.type_name.clone()),
            description: None,
            is_hyperlink: false,
        }],
        purpose: docs.as_ref().map(|d| d.description.clone()).filter(|s| !s.is_empty()),
        description: docs.as_ref().map(|d| d.description.clone()).filter(|s| !s.is_empty()),
        examples: docs
            .as_ref()
            .map(|d| {
                d.examples
                    .iter()
                    .map(|e| crate::domain::CodeExample {
                        code: e.code.clone(),
                        description: e.description.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        notes: docs.as_ref().and_then(|d| d.notes.clone()),
        deprecation: None,
        is_export: true,
        source: SignatureSource::PlatformConstructor,
        method_id: None,
    })
}
