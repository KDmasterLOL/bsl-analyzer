//! `TypeKernelDb` trait — the interning gateway.
//!
//! Production interning lives behind this trait. Implementations choose
//! their own storage: sandbox uses `elsa::FrozenVec<Box<TypeKind>>`
//! (see [`crate::testing::InMemoryDb`]); production crates may use a
//! Salsa input + manual table, or `RwLock<FrozenVec>`, etc.
//!
//! Filled in by Phase 1.C. Canonicalisation rules live in
//! [`crate::canonicalise`] (Phase 1.D).
