//! UNION clause lowering.
//!
//! NOTE: This module is deprecated in favor of flat list approach.
//! UNION queries are now processed in the main loop in `mod.rs`,
//! not as nested structures.

use super::context::LoweringContext;

impl<'a> LoweringContext<'a> {
    // DEPRECATED: This method is no longer used.
    // UNION queries are now processed in flat list in lower_sdbl_to_hir().
    //
    // Old approach (nested):
    //   - lower_select_query() called lower_union_clauses()
    //   - Each UNION query was nested in parent query
    //
    // New approach (flat list):
    //   - subquery.queries() returns iterator over all queries (main + UNION)
    //   - Each query is processed independently in main loop
    //   - Each query gets its own scope (cleared at start of lower_query())
    //
    // This allows proper handling of temporary tables across UNION queries.
}
