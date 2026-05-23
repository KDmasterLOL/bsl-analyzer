//! Type kernel for bsl-analyzer.
//!
//! Single source of truth for runtime BSL value types. BSL, SDBL, form,
//! and doc-comment elaborators all construct values in the same
//! `TypeKind` universe; no parallel hierarchies, no bridges.
//!
//! ## Design contract
//!
//! See `.omc/plans/clean-slate-type-architecture.md` v5 and
//! `.omc/plans/type-kernel-phase-1-sandbox.md` v7 for the locked
//! design. Key invariants:
//!
//! - `TypeKind` is fully `pub` + `#[non_exhaustive]`; external crates
//!   `match` freely.
//! - `TypeId(u64)` is the only canonical identity, obtained exclusively
//!   via [`intern::TypeKernelDb::intern_type`], which canonicalises on
//!   input.
//! - Hand-built `TypeKind` literals are inert *as identity*: they may
//!   be inspected via [`equality::semantic_eq`] / [`equality::type_eq`]
//!   for debug or structural comparison, but they must not be used as
//!   cache keys or dispatch input — for that, intern first.
//!
//! ## Module map
//!
//! - [`kind`] — `TypeKind` enum + nested helpers (`TypeId`, `ConfigId`).
//! - [`facet`] — payload structs (`NumberFacet`, `StringFacet`, …).
//! - [`intern`] — `TypeKernelDb` trait + interning gateway contract.
//! - [`builders`] — `Builders` trait, the recommended construction API.
//! - [`equality`] — `semantic_eq` / `type_eq` helpers.
//! - [`display`] — locale-aware rendering via `display_name`.
//! - [`testing`] — sandbox `InMemoryDb` + helpers; always compiled, not
//!   feature-gated.

pub mod builders;
pub mod display;
pub mod equality;
pub mod facet;
pub mod intern;
pub mod kind;
pub mod testing;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_builds() {
        // Phase 1.A smoke test: just prove the crate compiles and
        // exposes its module skeleton. Real tests land in 1.B → 1.F.
    }
}
