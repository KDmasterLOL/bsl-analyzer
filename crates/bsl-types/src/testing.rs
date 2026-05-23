//! Sandbox helpers — always compiled, NOT feature-gated.
//!
//! Hosts [`InMemoryDb`] (elsa-backed in-memory implementation of
//! [`crate::intern::TypeKernelDb`]) and [`RootConfigCtx`] (trivial
//! `ConfigCtx` that returns `ConfigId::Root` for any input).
//!
//! Production crates ignore this module; tests import it freely.
//!
//! See `.omc/plans/type-kernel-phase-1-sandbox.md` §1.C for the
//! contract.

use std::cell::RefCell;

use elsa::FrozenVec;
use rustc_hash::FxHashMap;

use crate::intern::{canonicalise, TypeKernelDb};
use crate::kind::{TypeId, TypeKind};

/// Pre-seeded `TypeId`s for hot-path sentinels.
///
/// The **only** contract is that [`InMemoryDb::new`] populates these
/// slots and they're available via accessor methods on `InMemoryDb`
/// (`db.unknown()`, `db.never()`, …). The numeric layout of the
/// underlying `u64` is an implementation detail — callers must
/// always go through the accessors, never fabricate a `TypeId(0)`
/// and assume it's `Unknown`. Canonicalisation rules in Phase 1.D
/// will consult sentinels by id internally without exposing the
/// layout.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Sentinels {
    pub unknown: u64,
    pub never: u64,
    pub any: u64,
    pub null: u64,
    pub undefined: u64,
    pub boolean: u64,
}

/// In-memory implementation of [`TypeKernelDb`].
///
/// Storage:
/// - `kinds: elsa::FrozenVec<Box<TypeKind>>` — append-only with stable
///   `&TypeKind` references across pushes. This is the property that
///   makes `lookup_type(&self, id) -> &TypeKind` sound; a plain
///   `Vec<TypeKind>` would invalidate references on realloc.
/// - `intern: RefCell<FxHashMap<TypeKind, u64>>` — reverse index for
///   the deduplication fast-path.
/// - `sentinel: Sentinels` — fixed slots pre-seeded by `new()`.
///
/// **`intern_type` operation order** (push BEFORE map insert):
///
/// 1. Canonicalise the input.
/// 2. Fast path: if the canonical form is already interned, return
///    its `TypeId` without growing storage.
/// 3. Push a `Box<TypeKind>` into `kinds` (FrozenVec, stable ref).
/// 4. Insert the reverse-map entry pointing at the newly allocated
///    slot.
///
/// Panic safety: if (3) panics, no map dirt; intern of the same kind
/// retries cleanly. If (4) panics (unlikely with `HashMap::insert`),
/// the slot exists but is unfindable through the reverse map — leaks
/// a slot but stays sound.
///
/// Re-entrancy: forbidden during intern. `RefCell::borrow_mut` panics
/// at runtime if a re-entrant `intern_type` is attempted.
pub struct InMemoryDb {
    kinds: FrozenVec<Box<TypeKind>>,
    intern: RefCell<FxHashMap<TypeKind, u64>>,
    sentinel: Sentinels,
}

impl Default for InMemoryDb {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryDb {
    /// Build a fresh db with sentinel `TypeId`s pre-seeded.
    pub fn new() -> Self {
        let kinds = FrozenVec::new();
        let mut intern = FxHashMap::default();

        // Helper: push + record in the reverse map, return slot id.
        let mut seed = |kind: TypeKind| -> u64 {
            let id = kinds.len() as u64;
            kinds.push(Box::new(kind.clone()));
            intern.insert(kind, id);
            id
        };

        let sentinel = Sentinels {
            unknown: seed(TypeKind::Unknown),
            never: seed(TypeKind::Never),
            any: seed(TypeKind::Any),
            null: seed(TypeKind::Null),
            undefined: seed(TypeKind::Undefined),
            boolean: seed(TypeKind::Boolean),
        };

        Self { kinds, intern: RefCell::new(intern), sentinel }
    }

    /// Pre-seeded `Unknown` sentinel.
    pub fn unknown(&self) -> TypeId {
        TypeId(self.sentinel.unknown)
    }

    /// Pre-seeded `Never` sentinel.
    pub fn never(&self) -> TypeId {
        TypeId(self.sentinel.never)
    }

    /// Pre-seeded `Any` sentinel.
    pub fn any(&self) -> TypeId {
        TypeId(self.sentinel.any)
    }

    /// Pre-seeded `Null` sentinel.
    pub fn null(&self) -> TypeId {
        TypeId(self.sentinel.null)
    }

    /// Pre-seeded `Undefined` sentinel.
    pub fn undefined(&self) -> TypeId {
        TypeId(self.sentinel.undefined)
    }

    /// Pre-seeded `Boolean` sentinel.
    pub fn boolean(&self) -> TypeId {
        TypeId(self.sentinel.boolean)
    }

    /// Total interned-type count. For debug / hit-rate testing.
    pub fn len(&self) -> usize {
        self.kinds.len()
    }

    /// `true` iff no non-sentinel types have been interned.
    pub fn is_empty(&self) -> bool {
        self.kinds.len() == self.sentinel_count()
    }

    /// How many slots were preseeded by `new()`. Constant.
    pub(crate) fn sentinel_count(&self) -> usize {
        // Match the seeded count in `new()`. If you add a sentinel,
        // bump this.
        6
    }
}

impl TypeKernelDb for InMemoryDb {
    fn intern_type(&self, kind: TypeKind) -> TypeId {
        let canon = canonicalise(kind);

        // Fast path: already interned.
        if let Some(&id) = self.intern.borrow().get(&canon) {
            return TypeId(id);
        }

        // Slow path: push first, then record. `FrozenVec` gives us a
        // stable slot pointer; the reverse map is just an index.
        let id = self.kinds.len() as u64;
        self.kinds.push(Box::new(canon.clone()));
        self.intern.borrow_mut().insert(canon, id);
        TypeId(id)
    }

    fn lookup_type(&self, id: TypeId) -> &TypeKind {
        // `FrozenVec::get` returns `Option<&T>`. The id is opaque so
        // out-of-range is a programming error — panic with a clear
        // message rather than silently returning a wrong type.
        self.kinds
            .get(id.raw() as usize)
            .expect("TypeId out of range: caller mixed IDs across db instances or fabricated a TypeId directly")
    }
}

/// Trivial `ConfigCtx` that returns `ConfigId::Root` for any input.
///
/// Sandbox-only. Production `bsl-config::VisibleConfig` implements the
/// same trait (Phase 2) but returns `Resolved(u32)` for known names and
/// `Unknown(name)` for unresolvable ones.
pub struct RootConfigCtx;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facet::NumberFacet;

    #[test]
    fn sentinels_are_distinct_and_preseeded() {
        // Contract: `db.new()` preseeds six distinct sentinel slots and
        // their `TypeId`s are exposed via accessor methods. The numeric
        // values of the underlying handle are NOT contract — only the
        // accessor identity matters. Callers must always use
        // `db.unknown()` etc. rather than fabricating a `TypeId(0)`.
        let db = InMemoryDb::new();
        let ids = [db.unknown(), db.never(), db.any(), db.null(), db.undefined(), db.boolean()];
        for (i, &a) in ids.iter().enumerate() {
            for (j, &b) in ids.iter().enumerate().skip(i + 1) {
                assert_ne!(a, b, "sentinel {} and sentinel {} aliased to the same id", i, j);
            }
        }
        assert_eq!(db.len(), 6, "exactly six sentinels preseeded");
        assert!(db.is_empty(), "no non-sentinel types yet");
    }

    #[test]
    fn lookup_sentinels_round_trip() {
        let db = InMemoryDb::new();
        assert_eq!(db.lookup_type(db.unknown()), &TypeKind::Unknown);
        assert_eq!(db.lookup_type(db.never()), &TypeKind::Never);
        assert_eq!(db.lookup_type(db.any()), &TypeKind::Any);
        assert_eq!(db.lookup_type(db.null()), &TypeKind::Null);
        assert_eq!(db.lookup_type(db.undefined()), &TypeKind::Undefined);
        assert_eq!(db.lookup_type(db.boolean()), &TypeKind::Boolean);
    }

    #[test]
    fn intern_dedupes_equal_kinds() {
        let db = InMemoryDb::new();
        let a = db.intern_type(TypeKind::Number(NumberFacet::with_scale(15, 2)));
        let b = db.intern_type(TypeKind::Number(NumberFacet::with_scale(15, 2)));
        assert_eq!(a, b);
    }

    #[test]
    fn intern_returns_preseeded_sentinel_for_unknown() {
        let db = InMemoryDb::new();
        let from_intern = db.intern_type(TypeKind::Unknown);
        assert_eq!(from_intern, db.unknown());
        assert_eq!(db.len(), 6, "no new slot allocated");
    }

    #[test]
    fn distinct_kinds_get_distinct_ids() {
        let db = InMemoryDb::new();
        let a = db.intern_type(TypeKind::Number(NumberFacet::with_scale(15, 2)));
        let b = db.intern_type(TypeKind::Number(NumberFacet::with_scale(20, 4)));
        assert_ne!(a, b);
    }

    #[test]
    fn lookup_borrow_stays_valid_across_pushes() {
        // The whole point of `FrozenVec<Box<T>>`: a `&TypeKind` returned
        // by `lookup_type` must NOT be invalidated by subsequent
        // `intern_type` calls that grow storage. Without `FrozenVec`,
        // a `Vec<TypeKind>` push could realloc and move the data.
        let db = InMemoryDb::new();
        let id_a = db.intern_type(TypeKind::Number(NumberFacet::with_scale(15, 2)));
        let ref_a = db.lookup_type(id_a);

        // Force several more inserts — would re-allocate `Vec` storage.
        for p in 0..16 {
            db.intern_type(TypeKind::Number(NumberFacet::with_precision(p)));
        }

        // The borrow obtained before the inserts is still valid and
        // still points at the same payload.
        assert_eq!(ref_a, &TypeKind::Number(NumberFacet::with_scale(15, 2)));
    }
}
