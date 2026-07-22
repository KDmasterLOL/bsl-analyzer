//! Byte arithmetic for the `heap_size` estimators wired into Salsa tracked
//! queries (item tree, symbol tree, method bodies). Each estimator lives next
//! to the struct it measures (so it can read private fields); the reusable
//! math is shared workspace-wide via [`stdx::heap`], re-exported here, plus a
//! [`Name`]-aware helper.

pub(crate) use stdx::heap::{map_table_bytes, smol_str_bytes, vec_bytes};

use crate::Name;

/// Heap bytes owned by a [`Name`]'s `SmolStr` payload.
pub(crate) fn name_bytes(name: &Name) -> usize {
    smol_str_bytes(name.as_str().len())
}
