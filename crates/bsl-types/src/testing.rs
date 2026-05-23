//! Sandbox helpers — always compiled, NOT feature-gated.
//!
//! Hosts `InMemoryDb` (elsa-backed in-memory implementation of
//! [`crate::intern::TypeKernelDb`]) and `RootConfigCtx` (trivial
//! `ConfigCtx` that returns `ConfigId::Root` for any input).
//!
//! Production crates ignore this module; tests import it freely.
//!
//! Filled in by Phase 1.C / 1.E.
