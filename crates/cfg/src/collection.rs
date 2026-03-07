//! Module-level CFG collections for batch processing.
//!
//! This module provides collections of Control Flow Graphs for all methods
//! in a module, enabling efficient batch processing through Salsa queries.

use crate::ControlFlowGraph;
use rustc_hash::FxHashMap;
use std::sync::Arc;

/// Collection of CFGs for all methods in a module.
///
/// Built once per module and cached by Salsa. This enables batch processing
/// where all CFGs are constructed in one pass, which is much more efficient
/// than building them individually through per-method queries.
///
/// # Usage
///
/// ```ignore
/// // In Salsa query:
/// let module_cfgs = db.module_cfgs(module_id);
/// let cfg = module_cfgs.get(local_method_id)?;
/// ```
///
/// # Performance
///
/// Building all CFGs at once eliminates Salsa overhead:
/// - Before: N × method_cfg_query calls = N × Salsa lookup cost
/// - After: 1 × module_cfgs_query call = 1 × Salsa lookup cost
///
/// On doc3 project (96,317 methods):
/// - Per-method: ~97s (Salsa overhead)
/// - Module-level: ~0.45s (direct construction)
/// - Speedup: ~215x for CFG construction
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleCfgs {
    cfgs: FxHashMap<u32, Arc<ControlFlowGraph>>,
}

impl ModuleCfgs {
    /// Create a new collection of CFGs.
    pub fn new(cfgs: FxHashMap<u32, Arc<ControlFlowGraph>>) -> Self {
        Self { cfgs }
    }

    /// Get CFG for a specific method.
    ///
    /// Returns `None` if the method doesn't have a CFG (e.g., malformed code).
    pub fn get(&self, local_id: u32) -> Option<&Arc<ControlFlowGraph>> {
        self.cfgs.get(&local_id)
    }

    /// Iterate over all (method_id, cfg) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &Arc<ControlFlowGraph>)> + '_ {
        self.cfgs.iter().map(|(&id, cfg)| (id, cfg))
    }

    /// Get the number of CFGs in this collection.
    pub fn len(&self) -> usize {
        self.cfgs.len()
    }

    /// Check if this collection is empty.
    pub fn is_empty(&self) -> bool {
        self.cfgs.is_empty()
    }
}
