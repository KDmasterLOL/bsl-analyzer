//! Shared capacity-based arithmetic for the `heap_size` estimators wired into
//! Salsa tracked queries (item tree, symbol tree, method bodies). Each estimator
//! lives next to the struct it measures (so it can read private fields); this
//! module only holds the reusable byte math.

use std::mem::size_of;

use crate::Name;

/// Heap bytes of a `Vec`/`Box<[T]>`/`Arena<T>` backing store holding `len`
/// elements, counted at element granularity (ignores spare capacity).
pub(crate) fn vec_bytes<T>(len: usize) -> usize {
    len * size_of::<T>()
}

/// Approximate live bytes of an `FxHashMap`/hashbrown table with `len` entries
/// of `(K, V)`: one control byte plus the `(K, V)` slot per bucket, with bucket
/// count grown to the next power of two above `len / (7/8)`. Mirrors the
/// estimator in `hir-ty::infer::heap_estimate`.
pub(crate) fn map_table_bytes<K, V>(len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let cap = (len * 8 / 7 + 1).checked_next_power_of_two().unwrap_or(len);
    cap.saturating_mul(size_of::<K>() + size_of::<V>() + 1)
}

/// `smol_str::SmolStr` inlines names up to this length, touching no heap.
const SMOL_STR_INLINE_CAP: usize = 22;

/// Heap bytes owned by a string of `len` bytes stored in a `SmolStr` (the
/// backing of [`Name`]): zero while it fits inline, its full length otherwise.
pub(crate) fn smol_str_bytes(len: usize) -> usize {
    if len > SMOL_STR_INLINE_CAP {
        len
    } else {
        0
    }
}

/// Heap bytes owned by a [`Name`]'s `SmolStr` payload.
pub(crate) fn name_bytes(name: &Name) -> usize {
    smol_str_bytes(name.as_str().len())
}
