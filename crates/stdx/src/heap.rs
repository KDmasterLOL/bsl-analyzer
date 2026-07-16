//! Capacity-based byte arithmetic shared by the `heap_size` estimators wired
//! into Salsa ingredients across the workspace. Each estimator lives next to
//! the type it measures (so it can read private fields); this module only
//! holds the reusable math. Estimators run only while a memory report is
//! being built, never in a hot path.

use std::mem::size_of;

/// Heap bytes of a `Vec`/`Box<[T]>`/`Arena<T>` backing store holding `len`
/// elements, counted at element granularity (ignores spare capacity).
pub fn vec_bytes<T>(len: usize) -> usize {
    len * size_of::<T>()
}

/// Approximate live bytes of an `FxHashMap`/hashbrown table with `len` entries
/// of `(K, V)`: one control byte plus the `(K, V)` slot per bucket, with bucket
/// count grown to the next power of two above `len / (7/8)`.
pub fn map_table_bytes<K, V>(len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    // `checked_*`/`saturating_*` keep the estimator total for (theoretically)
    // unbounded `len`: `next_power_of_two` panics in debug and wraps to 0 in
    // release near `usize::MAX`.
    let cap = (len * 8 / 7 + 1).checked_next_power_of_two().unwrap_or(len);
    cap.saturating_mul(size_of::<K>() + size_of::<V>() + 1)
}

/// `smol_str::SmolStr` inlines strings up to this length, touching no heap
/// (`INLINE_CAP` of the pinned smol_str 0.3.x layout).
const SMOL_STR_INLINE_CAP: usize = 23;

/// Heap bytes owned by a string of `len` bytes stored in a `SmolStr`: zero
/// while it fits inline, its full length otherwise.
pub fn smol_str_bytes(len: usize) -> usize {
    if len > SMOL_STR_INLINE_CAP {
        len
    } else {
        0
    }
}

/// Estimator for memoised values that own no heap (`Copy` ids, small enums):
/// reports a measured zero so the memory report prints `0` for the ingredient
/// instead of an unmeasured dash.
pub fn zero<T>(_: &T) -> usize {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_table_bytes_is_zero_for_empty_and_grows_with_len() {
        assert_eq!(map_table_bytes::<u64, u64>(0), 0);
        let small = map_table_bytes::<u64, u64>(10);
        let big = map_table_bytes::<u64, u64>(10_000);
        // Order of magnitude: at least the raw entries, at most 4x the
        // perfectly-packed table (power-of-two growth + 7/8 load factor).
        assert!((10 * 16..=4 * 10 * 17).contains(&small));
        assert!((10_000 * 16..=4 * 10_000 * 17).contains(&big));
    }

    #[test]
    fn smol_str_bytes_counts_only_spilled_strings() {
        assert_eq!(smol_str_bytes(23), 0);
        assert_eq!(smol_str_bytes(24), 24);
    }
}
