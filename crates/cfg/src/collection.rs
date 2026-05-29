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
}
