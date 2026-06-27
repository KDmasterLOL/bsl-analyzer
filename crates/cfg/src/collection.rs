use crate::ControlFlowGraph;
use rustc_hash::FxHashMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleCfgs {
    cfgs: FxHashMap<u32, Arc<ControlFlowGraph>>,
}

impl ModuleCfgs {
    pub fn new(cfgs: FxHashMap<u32, Arc<ControlFlowGraph>>) -> Self {
        Self { cfgs }
    }

    pub fn get(&self, local_id: u32) -> Option<&Arc<ControlFlowGraph>> {
        self.cfgs.get(&local_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (u32, &Arc<ControlFlowGraph>)> + '_ {
        self.cfgs.iter().map(|(&id, cfg)| (id, cfg))
    }

    pub fn len(&self) -> usize {
        self.cfgs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cfgs.is_empty()
    }

    /// Approximate live heap bytes of all per-method CFGs for Salsa's
    /// `memory_usage` report. `ModuleCfgs` is the owning store of these graphs, so
    /// it recurses into each [`ControlFlowGraph::estimated_heap`]; the per-method
    /// `method_cfg` accessor query returns clones of these same `Arc`s and must
    /// therefore report zero to avoid double-counting the shared payload. Includes
    /// the hashbrown table backbone (`u32` key + `Arc` slot per bucket).
    pub fn estimated_heap(&self) -> usize {
        use std::mem::size_of;

        let len = self.cfgs.len();
        let cap =
            if len == 0 { 0 } else { (len * 8 / 7 + 1).checked_next_power_of_two().unwrap_or(len) };
        let mut bytes = cap * (size_of::<u32>() + size_of::<Arc<ControlFlowGraph>>() + 1);
        for cfg in self.cfgs.values() {
            bytes += cfg.estimated_heap();
        }
        bytes
    }
}
