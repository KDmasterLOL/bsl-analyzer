//! `MetadataResolver` adapter for `&[VisibleConfig]`.
//!
//! `hir-ty` consumes the trait through a thin wrapper that mirrors the
//! "extension wins" iteration order used everywhere else in field
//! enumeration: extensions are visited reverse-first, so a `DefinedType`
//! redeclared in an extension shadows the same name in the main
//! configuration.
//!
//! The trait itself lives in `bsl-metadata` so `sdbl-hir` can reuse the
//! shared `resolve_defined_type_terminal` walk without taking a dependency
//! on `hir-ty`.

use bsl_metadata::{AttributeType, MetadataResolver};
use hir_def::configs::VisibleConfig;

/// Adapter that exposes `&[VisibleConfig]` as a `MetadataResolver`.
///
/// Iteration follows the same "extensions override main on collisions"
/// convention as `enumerate_mdo_fields` in `field_enum.rs` — `iter().rev()`
/// so the latest-registered extension wins.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ConfigsResolver<'a>(pub &'a [VisibleConfig]);

impl<'a> MetadataResolver for ConfigsResolver<'a> {
    fn resolve_defined_type(&self, name: &str) -> Option<&AttributeType> {
        self.0.iter().rev().find_map(|cfg| cfg.configuration.resolve_defined_type(name))
    }
}
