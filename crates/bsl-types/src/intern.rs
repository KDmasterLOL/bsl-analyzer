//! `TypeKernelDb` trait — the interning gateway.
//!
//! Production interning lives behind this trait. Implementations choose
//! their own storage: sandbox uses `elsa::FrozenVec<Box<TypeKind>>`
//! (see [`crate::testing::InMemoryDb`]); production crates may use a
//! Salsa input + manual table, or `RwLock<FrozenVec>`, etc.
//!
//! Canonicalisation rules ([`canonicalise`]) are applied inside
//! `intern_type` before hashing — they're spec'd in
//! `.omc/plans/type-kernel-phase-1-sandbox.md` §1.D. Phase 1.C ships
//! `canonicalise` as the identity function (no transformations yet);
//! Phase 1.D fills the Union algebra, provenance stripping, etc.

use crate::kind::{TypeId, TypeKind};

/// The interning gateway. Implementations canonicalise on input so two
/// callers that build semantically equal `TypeKind` literals via
/// different paths get the same `TypeId`.
///
/// **Cross-db caveat:** `TypeId` carries no db-identity tag. Callers
/// must not mix `TypeId`s obtained from one db with `lookup_type` on a
/// different db — IDs are opaque indices into a particular db's intern
/// table.
pub trait TypeKernelDb {
    /// Intern a `TypeKind`, returning the canonical `TypeId`.
    fn intern_type(&self, kind: TypeKind) -> TypeId;

    /// Look up the `TypeKind` interned at the given handle. Returned
    /// reference borrows from `&self` of the db; it cannot outlive the
    /// db borrow.
    fn lookup_type(&self, id: TypeId) -> &TypeKind;
}

/// Canonicalise a `TypeKind` before interning.
///
/// Phase 1.C: identity. Phase 1.D fills the rules:
/// - `Union` algebra (flatten nested, absorb `Unknown`, dominate by
///   `Any`, drop `Never`, dedupe, sort, single-member unwrap, empty
///   → `Unknown`).
/// - Provenance stripping (`*_origin`, `*_source` fields zeroed for
///   hash/equality).
/// - Projection field-order preserved (NOT dedup'd).
pub(crate) fn canonicalise(kind: TypeKind) -> TypeKind {
    kind
}
