//! `semantic_eq` / `type_eq` helpers on `TypeKind`.
//!
//! `semantic_eq` ignores provenance fields (`*_origin`, `*_source`);
//! `type_eq` is strict, distinguishing e.g. `Число(15,2)` from
//! `Число(20,4)`.
//!
//! Filled in by Phase 1.B (basic) and Phase 1.D (canonicalisation
//! integration).
