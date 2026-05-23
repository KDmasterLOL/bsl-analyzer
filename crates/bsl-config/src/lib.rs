//! Layer 0.5 of the bsl-analyzer architecture: visible configurations
//! and `ConfigId` identity.
//!
//! This crate sits between `bsl-metadata` (Layer 0 — raw configuration
//! schema) and `bsl-types` (Layer 1 — type kernel). It owns the
//! semantic notion of "which configurations are visible from a BSL
//! file" and the opaque `ConfigId` handle that type-bearing references
//! (e.g. `MetaRefFacet`, `MetaObjFacet`) carry.
//!
//! The Salsa-tracked `ConfigsDatabase` trait stays in `hir-def`, where
//! it can extend `DefDatabase`. This crate provides only the plain
//! value types — no Salsa, no LSP, no I/O dependencies.

#![deny(rust_2018_idioms)]

use std::sync::Arc;

use bsl_metadata::{Configuration, Name};

/// A configuration visible from a given file: main or extension.
///
/// Carries only the metadata — never a filesystem URI — because name
/// resolution and type inference only consume the declarative
/// `Configuration` description. Path-aware callers in `ide-db` wrap
/// this in `VisibleConfigWithRoot` (see §1.5 of the Phase 2 plan).
#[derive(Clone, Debug)]
pub struct VisibleConfig {
    /// Extension name; `None` for the main configuration.
    pub name: Option<String>,
    /// Loaded configuration metadata.
    pub configuration: Arc<Configuration>,
}

/// Opaque identity of a configuration inside the type kernel.
///
/// `MetaRefFacet` / `MetaObjFacet` use this handle to disambiguate
/// type-equal MDO names that come from different visible configurations
/// (main vs CFE extension). Identity is by value, not by interning —
/// equality compares variant tags and inner data directly.
///
/// Documented limitation: two distinct configurations that both fail
/// to resolve the **same** name collide at the kernel layer (both
/// produce `ConfigId::Unknown(name)`). Diagnostics differentiate by
/// source location, not by kernel identity.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum ConfigId {
    /// Single-config workspaces or sandbox tests.
    Root,
    /// Multi-config workspace; index into the interned configuration
    /// table maintained by callers that materialise a stable u32 id
    /// per `VisibleConfig`.
    Resolved(u32),
    /// MDO name couldn't be resolved against any known configuration.
    /// Carries the name itself so different unresolved names produce
    /// different `ConfigId` values.
    Unknown(Name),
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::Configuration;

    #[test]
    fn config_id_round_trip() {
        let root = ConfigId::Root;
        let res = ConfigId::Resolved(7);
        let unk = ConfigId::Unknown("Контрагенты".to_string());

        assert_eq!(root, ConfigId::Root);
        assert_eq!(res, ConfigId::Resolved(7));
        assert_ne!(res, ConfigId::Resolved(8));
        assert_eq!(unk, ConfigId::Unknown("Контрагенты".to_string()));
        assert_ne!(unk, ConfigId::Unknown("Номенклатура".to_string()));
    }

    #[test]
    fn visible_config_round_trip() {
        let cfg = Arc::new(Configuration::new("test"));
        let main = VisibleConfig { name: None, configuration: cfg.clone() };
        let ext = VisibleConfig { name: Some("ExtA".into()), configuration: cfg };

        assert!(main.name.is_none());
        assert_eq!(ext.name.as_deref(), Some("ExtA"));
        assert!(Arc::ptr_eq(&main.configuration, &ext.configuration));
    }
}
