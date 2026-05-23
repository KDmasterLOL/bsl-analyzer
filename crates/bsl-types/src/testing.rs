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
        let canon = canonicalise(self, kind);

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

// ── Phase 1.D canonicalisation tests ─────────────────────────────

#[cfg(test)]
mod canon_tests {
    use std::sync::Arc;

    use super::*;
    use crate::facet::NumberFacet;
    use crate::kind::TypeOrigin;

    /// Provenance must not leak into canonical identity: two `Number`
    /// values differing only in `origin` intern to the same `TypeId`.
    #[test]
    fn provenance_stripped_on_number() {
        let db = InMemoryDb::new();
        let a = db.intern_type(TypeKind::Number(NumberFacet {
            precision: Some(15),
            scale: Some(2),
            origin: Some(TypeOrigin::SdblCast),
        }));
        let b = db.intern_type(TypeKind::Number(NumberFacet {
            precision: Some(15),
            scale: Some(2),
            origin: Some(TypeOrigin::BslLiteral),
        }));
        assert_eq!(a, b);
        let c = db.intern_type(TypeKind::Number(NumberFacet::with_scale(15, 2)));
        assert_eq!(a, c);
    }

    /// Re-interning the same canonical form is a no-op.
    #[test]
    fn intern_is_idempotent() {
        let db = InMemoryDb::new();
        let kind = TypeKind::Number(NumberFacet::with_scale(15, 2));
        let a = db.intern_type(kind.clone());
        let b = db.intern_type(kind.clone());
        let c = db.intern_type(kind);
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    /// `Union([X, X])` → `X` (single-member unwrap after dedupe).
    #[test]
    fn union_with_one_distinct_member_unwraps() {
        let db = InMemoryDb::new();
        let n = db.intern_type(TypeKind::Number(NumberFacet::unsized_()));
        let u = db.intern_type(TypeKind::Union(Arc::from([n, n])));
        assert_eq!(u, n, "Union([X, X]) must collapse to X");
    }

    /// `Union([])` → `Unknown`.
    #[test]
    fn empty_union_collapses_to_unknown() {
        let db = InMemoryDb::new();
        let u = db.intern_type(TypeKind::Union(Arc::from([])));
        assert_eq!(u, db.unknown());
    }

    /// `Union([Unknown, X])` → `X` (Unknown absorbed).
    #[test]
    fn union_absorbs_unknown() {
        let db = InMemoryDb::new();
        let n = db.intern_type(TypeKind::Number(NumberFacet::unsized_()));
        let u = db.intern_type(TypeKind::Union(Arc::from([db.unknown(), n])));
        assert_eq!(u, n);
    }

    /// `Union([Never, X])` → `X` (Never dropped).
    #[test]
    fn union_drops_never() {
        let db = InMemoryDb::new();
        let n = db.intern_type(TypeKind::Number(NumberFacet::unsized_()));
        let u = db.intern_type(TypeKind::Union(Arc::from([db.never(), n])));
        assert_eq!(u, n);
    }

    /// `Union([Any, X])` → `Any` (Any dominates).
    #[test]
    fn union_dominated_by_any() {
        let db = InMemoryDb::new();
        let n = db.intern_type(TypeKind::Number(NumberFacet::unsized_()));
        let u = db.intern_type(TypeKind::Union(Arc::from([db.any(), n])));
        assert_eq!(u, db.any());
    }

    /// `Union([Y, X])` and `Union([X, Y])` intern equal (sort
    /// canonicalises member order).
    #[test]
    fn union_sort_canonicalises_order() {
        let db = InMemoryDb::new();
        let a = db.intern_type(TypeKind::Number(NumberFacet::with_scale(15, 2)));
        let b = db.intern_type(TypeKind::String(crate::facet::StringFacet::unsized_()));
        let u1 = db.intern_type(TypeKind::Union(Arc::from([a, b])));
        let u2 = db.intern_type(TypeKind::Union(Arc::from([b, a])));
        assert_eq!(u1, u2);
    }

    /// `Union([Union([A, B]), C])` → `Union([A, B, C])` (flatten).
    #[test]
    fn union_flatten_nested() {
        let db = InMemoryDb::new();
        let a = db.intern_type(TypeKind::Number(NumberFacet::with_scale(15, 2)));
        let b = db.intern_type(TypeKind::String(crate::facet::StringFacet::unsized_()));
        let c = db.intern_type(TypeKind::Date(crate::facet::DateFacet::datetime()));
        let inner = db.intern_type(TypeKind::Union(Arc::from([a, b])));
        let outer = db.intern_type(TypeKind::Union(Arc::from([inner, c])));
        let direct = db.intern_type(TypeKind::Union(Arc::from([a, b, c])));
        assert_eq!(outer, direct, "Union([Union([A, B]), C]) must equal Union([A, B, C])");
    }

    /// Union of multiple `Unknown` members → `Unknown` (single-member
    /// unwrap after dedupe, since absorption keeps the empty-Union
    /// fallback in scope).
    #[test]
    fn union_of_only_unknown_collapses_to_unknown() {
        let db = InMemoryDb::new();
        let u = db.intern_type(TypeKind::Union(Arc::from([db.unknown(), db.unknown()])));
        assert_eq!(u, db.unknown());
    }

    /// `Union([Never])` MUST stay `Never` — proven-unreachable single
    /// arm is meaningful (plan §1.D rule 4). Earlier draft dropped
    /// the last Never and collapsed to Unknown — Codex NO-GO.
    #[test]
    fn union_of_only_never_stays_never() {
        let db = InMemoryDb::new();
        let u = db.intern_type(TypeKind::Union(Arc::from([db.never(), db.never()])));
        assert_eq!(u, db.never());
    }

    /// `Union([Any, Any])` → `Any` (dominance + dedupe both apply).
    #[test]
    fn union_of_only_any_stays_any() {
        let db = InMemoryDb::new();
        let u = db.intern_type(TypeKind::Union(Arc::from([db.any(), db.any()])));
        assert_eq!(u, db.any());
    }

    /// `Union([Unknown, Never, X])` → `X` (both Unknown and Never
    /// absorbed when a concrete arm exists).
    #[test]
    fn union_absorbs_both_unknown_and_never_with_concrete_arm() {
        let db = InMemoryDb::new();
        let n = db.intern_type(TypeKind::Number(NumberFacet::unsized_()));
        let u = db.intern_type(TypeKind::Union(Arc::from([db.unknown(), db.never(), n])));
        assert_eq!(u, n);
    }

    /// `Union([Unknown, Never])` — Never-drop runs first (Unknown
    /// counts as a non-Never arm, so Never is dropped). Then absorb
    /// step has nothing to absorb (Unknown is the only arm left).
    /// Single-member unwrap yields `Unknown`. Rationale: "analysis
    /// incomplete" wins over "proven unreachable" because Never's
    /// drop rule fires first by design (rule 2 vs rule 4 order).
    #[test]
    fn union_unknown_plus_never_collapses_to_unknown() {
        let db = InMemoryDb::new();
        let u = db.intern_type(TypeKind::Union(Arc::from([db.unknown(), db.never()])));
        assert_eq!(u, db.unknown());
    }

    /// Provenance stripped on `Query` (the projection-slice path,
    /// different from the single-projection `QueryResult` path).
    #[test]
    fn provenance_stripped_on_query_variant() {
        use crate::kind::{Projection, ProjectionField, ProjectionFieldSource, ProjectionOrigin};

        let db = InMemoryDb::new();
        let n = db.intern_type(TypeKind::Number(NumberFacet::with_scale(15, 2)));

        let make = |field_src, proj_origin| {
            Some(Arc::new(Projection {
                fields: Arc::from([ProjectionField {
                    name: "Цена".to_string(),
                    ty: n,
                    source: field_src,
                }]),
                origin: proj_origin,
            }))
        };

        let a = db.intern_type(TypeKind::Query {
            projections: Arc::from([make(
                ProjectionFieldSource::Cast,
                ProjectionOrigin::SdblQuery,
            )]),
        });
        let b = db.intern_type(TypeKind::Query {
            projections: Arc::from([make(
                ProjectionFieldSource::Column,
                ProjectionOrigin::Unknown,
            )]),
        });
        assert_eq!(a, b);
    }

    /// Same as above but for `QueryBatchResult.per_query`.
    #[test]
    fn provenance_stripped_on_query_batch() {
        use crate::kind::{Projection, ProjectionField, ProjectionFieldSource, ProjectionOrigin};

        let db = InMemoryDb::new();
        let n = db.intern_type(TypeKind::Number(NumberFacet::with_scale(15, 2)));

        let make = |field_src, proj_origin| {
            Some(Arc::new(Projection {
                fields: Arc::from([ProjectionField {
                    name: "Цена".to_string(),
                    ty: n,
                    source: field_src,
                }]),
                origin: proj_origin,
            }))
        };

        let a = db.intern_type(TypeKind::QueryBatchResult {
            per_query: Arc::from([make(ProjectionFieldSource::Cast, ProjectionOrigin::SdblQuery)]),
        });
        let b = db.intern_type(TypeKind::QueryBatchResult {
            per_query: Arc::from([make(ProjectionFieldSource::Column, ProjectionOrigin::Unknown)]),
        });
        assert_eq!(a, b);
    }

    /// Idempotence sweep over a representative set of kinds — each
    /// interned three times in a row must produce the same `TypeId`.
    /// Random property tests (proptest) would be a stronger gate but
    /// add a dep; this fixed-set sweep is the Phase 1 floor.
    #[test]
    fn intern_is_idempotent_across_kind_variety() {
        use crate::facet::{ArrayFacet, DateFacet, MapFacet, StringFacet};

        let db = InMemoryDb::new();
        let n = db.intern_type(TypeKind::Number(NumberFacet::with_scale(15, 2)));

        let kinds = [
            TypeKind::Unknown,
            TypeKind::Never,
            TypeKind::Any,
            TypeKind::Boolean,
            TypeKind::Null,
            TypeKind::Undefined,
            TypeKind::Number(NumberFacet::with_scale(15, 2)),
            TypeKind::Number(NumberFacet::with_precision(10)),
            TypeKind::Number(NumberFacet::unsized_()),
            TypeKind::String(StringFacet::with_length(50)),
            TypeKind::String(StringFacet::unsized_()),
            TypeKind::Date(DateFacet::datetime()),
            TypeKind::Array(ArrayFacet { element: None }),
            TypeKind::Array(ArrayFacet { element: Some(n) }),
            TypeKind::Map(MapFacet { key: None, value: Some(n) }),
            TypeKind::Union(Arc::from([n, db.boolean()])),
        ];

        for kind in &kinds {
            let a = db.intern_type(kind.clone());
            let b = db.intern_type(kind.clone());
            let c = db.intern_type(kind.clone());
            assert_eq!(a, b, "intern(x) != intern(x) for {:?}", kind);
            assert_eq!(b, c, "intern(x) != intern(x) for {:?}", kind);
        }
    }

    /// Projection-bearing variants strip both `Projection.origin` and
    /// `ProjectionField.source`, so two callers with identical field
    /// shape but different provenance intern equally.
    #[test]
    fn provenance_stripped_on_query_result() {
        use crate::facet::{ProjectionFacet, ProjectionSource};
        use crate::kind::{Projection, ProjectionField, ProjectionFieldSource, ProjectionOrigin};

        let db = InMemoryDb::new();
        let n = db.intern_type(TypeKind::Number(NumberFacet::with_scale(15, 2)));

        let proj_a = Arc::new(Projection {
            fields: Arc::from([ProjectionField {
                name: "Цена".to_string(),
                ty: n,
                source: ProjectionFieldSource::Cast,
            }]),
            origin: ProjectionOrigin::SdblQuery,
        });
        let proj_b = Arc::new(Projection {
            fields: Arc::from([ProjectionField {
                name: "Цена".to_string(),
                ty: n,
                source: ProjectionFieldSource::Column,
            }]),
            origin: ProjectionOrigin::Unknown,
        });

        let a = db.intern_type(TypeKind::QueryResult(ProjectionFacet {
            projection: Some(proj_a),
            source: ProjectionSource::Sdbl,
        }));
        let b = db.intern_type(TypeKind::QueryResult(ProjectionFacet {
            projection: Some(proj_b),
            source: ProjectionSource::Unknown,
        }));
        assert_eq!(a, b, "provenance differences must not produce distinct TypeIds");
    }
}
