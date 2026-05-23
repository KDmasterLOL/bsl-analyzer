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

// Phase 2.A is scaffolding only. `VisibleConfig` arrives in 2.B,
// `ConfigId` migrates from `bsl-types::kind` in 2.B as well.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        // Sanity smoke — establishes the crate is reachable from the
        // workspace and dev-test harness runs. Real type-level tests
        // arrive with the `VisibleConfig` / `ConfigId` moves in 2.B.
    }
}
