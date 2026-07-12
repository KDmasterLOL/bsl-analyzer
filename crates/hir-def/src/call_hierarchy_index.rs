use std::cmp::Ordering;
use std::mem::size_of;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{MethodId, ModuleId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MethodCallPair {
    pub caller: MethodId,
    pub target: MethodId,
}

impl MethodCallPair {
    pub const fn new(caller: MethodId, target: MethodId) -> Self {
        Self { caller, target }
    }
}

#[derive(Debug, Default)]
pub struct CallHierarchyReverseIndex {
    reverse_callers: FxHashMap<MethodId, Vec<MethodId>>,
    module_pairs: FxHashMap<ModuleId, Vec<MethodCallPair>>,
    module_layout_hashes: FxHashMap<ModuleId, u64>,
}

impl CallHierarchyReverseIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_modules<I, P>(modules: I) -> Self
    where
        I: IntoIterator<Item = (ModuleId, P, u64)>,
        P: IntoIterator<Item = MethodCallPair>,
    {
        let mut index = Self::new();
        for (module, pairs, layout_hash) in modules {
            let pairs = canonical_pairs(pairs);
            for pair in &pairs {
                debug_assert_eq!(pair.caller.module, module);
            }
            index.module_layout_hashes.insert(module, layout_hash);
            index.module_pairs.insert(module, pairs);
        }
        index.rebuild_reverse_callers();
        index
    }

    pub fn callers(&self, target: MethodId) -> &[MethodId] {
        self.reverse_callers.get(&target).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn layout_hash(&self, module: ModuleId) -> Option<u64> {
        self.module_layout_hashes.get(&module).copied()
    }

    pub fn replace_module<P>(&mut self, module: ModuleId, new_pairs: P, new_layout_hash: u64)
    where
        P: IntoIterator<Item = MethodCallPair>,
    {
        self.remove_module(module);

        let pairs = canonical_pairs(new_pairs);
        let mut added_targets = FxHashSet::default();
        for &pair in &pairs {
            debug_assert_eq!(pair.caller.module, module);
            self.insert_pair(pair);
            added_targets.insert(pair.target);
        }
        self.canonicalize_reverse_callers(added_targets);
        self.module_layout_hashes.insert(module, new_layout_hash);
        self.module_pairs.insert(module, pairs);
    }

    pub fn remove_module(&mut self, module: ModuleId) {
        if let Some(pairs) = self.module_pairs.remove(&module) {
            for pair in pairs {
                let remove_target =
                    self.reverse_callers.get_mut(&pair.target).is_some_and(|callers| {
                        callers.retain(|caller| *caller != pair.caller);
                        callers.is_empty()
                    });
                if remove_target {
                    self.reverse_callers.remove(&pair.target);
                }
            }
        }
        self.module_layout_hashes.remove(&module);
    }

    pub fn len(&self) -> usize {
        self.module_pairs.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.module_pairs.values().all(Vec::is_empty)
    }

    /// Returns a capacity-based estimate of this index's heap storage, not exact allocated bytes.
    ///
    /// It sums map capacities and owned vector capacities, while omitting allocator and hash-table
    /// overhead and `Arc` header overhead.
    pub fn estimated_heap_bytes(&self) -> usize {
        map_heap_bytes(&self.reverse_callers)
            + self
                .reverse_callers
                .values()
                .map(|callers| callers.capacity().saturating_mul(size_of::<MethodId>()))
                .sum::<usize>()
            + map_heap_bytes(&self.module_pairs)
            + self
                .module_pairs
                .values()
                .map(|pairs| pairs.capacity().saturating_mul(size_of::<MethodCallPair>()))
                .sum::<usize>()
            + map_heap_bytes(&self.module_layout_hashes)
    }

    fn insert_pair(&mut self, pair: MethodCallPair) {
        self.reverse_callers.entry(pair.target).or_default().push(pair.caller);
    }

    fn rebuild_reverse_callers(&mut self) {
        self.reverse_callers.clear();
        for pairs in self.module_pairs.values() {
            for &pair in pairs {
                self.reverse_callers.entry(pair.target).or_default().push(pair.caller);
            }
        }
        for callers in self.reverse_callers.values_mut() {
            canonicalize_callers(callers);
        }
    }

    fn canonicalize_reverse_callers(&mut self, targets: impl IntoIterator<Item = MethodId>) {
        for target in targets {
            let Some(callers) = self.reverse_callers.get_mut(&target) else {
                continue;
            };
            canonicalize_callers(callers);
        }
    }
}

fn canonical_pairs(pairs: impl IntoIterator<Item = MethodCallPair>) -> Vec<MethodCallPair> {
    let mut pairs: Vec<_> = pairs.into_iter().collect();
    pairs.sort_unstable_by(compare_pairs);
    pairs.dedup();
    pairs
}

fn canonicalize_callers(callers: &mut Vec<MethodId>) {
    callers.sort_unstable_by(compare_methods);
    callers.dedup();
}

fn compare_pairs(left: &MethodCallPair, right: &MethodCallPair) -> Ordering {
    compare_methods(&left.caller, &right.caller)
        .then_with(|| compare_methods(&left.target, &right.target))
}

fn compare_methods(left: &MethodId, right: &MethodId) -> Ordering {
    left.module
        .file_id
        .0
        .cmp(&right.module.file_id.0)
        .then_with(|| left.local_id.cmp(&right.local_id))
}

fn map_heap_bytes<K, V>(map: &FxHashMap<K, V>) -> usize {
    map.capacity().saturating_mul(size_of::<(K, V)>())
}

#[cfg(test)]
mod tests;
