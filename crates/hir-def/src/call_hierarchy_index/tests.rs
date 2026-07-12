use std::time::{Duration, Instant};

use super::{CallHierarchyReverseIndex, MethodCallPair};
use crate::{MethodId, ModuleId};

fn method(file_id: u32, local_id: u32) -> MethodId {
    MethodId { module: ModuleId::new(vfs::FileId(file_id)), local_id }
}

fn pair(caller: MethodId, target: MethodId) -> MethodCallPair {
    MethodCallPair { caller, target }
}

#[test]
fn call_hierarchy_reverse_index_collapses_duplicate_pairs() {
    // Given: one module contributes the same caller-target pair twice.
    let module = ModuleId::new(vfs::FileId(1));
    let caller = method(1, 1);
    let target = method(2, 1);
    let mut index = CallHierarchyReverseIndex::default();

    // When: its pairs replace the module's previous contribution.
    index.replace_module(module, [pair(caller, target), pair(caller, target)], 11);

    // Then: the reverse callers and index cardinality are deduplicated.
    assert_eq!(index.callers(target), &[caller]);
    assert_eq!(index.len(), 1);
}

#[test]
fn call_hierarchy_reverse_index_replacement_preserves_other_modules() {
    // Given: two modules call the same target.
    let first_module = ModuleId::new(vfs::FileId(1));
    let second_module = ModuleId::new(vfs::FileId(2));
    let first_caller = method(1, 1);
    let second_caller = method(2, 1);
    let old_target = method(3, 1);
    let new_target = method(4, 1);
    let mut index = CallHierarchyReverseIndex::default();
    index.replace_module(first_module, [pair(first_caller, old_target)], 11);
    index.replace_module(second_module, [pair(second_caller, old_target)], 22);

    // When: the first module is replaced with a different target.
    index.replace_module(first_module, [pair(first_caller, new_target)], 33);

    // Then: only its old reverse contribution is removed.
    assert_eq!(index.callers(old_target), &[second_caller]);
    assert_eq!(index.callers(new_target), &[first_caller]);
    assert_eq!(index.layout_hash(first_module), Some(33));
    assert_eq!(index.len(), 2);
}

#[test]
fn call_hierarchy_reverse_index_removal_clears_module_contributions() {
    // Given: a module with an indexed call and layout hash.
    let module = ModuleId::new(vfs::FileId(1));
    let caller = method(1, 1);
    let target = method(2, 1);
    let mut index = CallHierarchyReverseIndex::default();
    index.replace_module(module, [pair(caller, target)], 11);

    // When: the module is removed.
    index.remove_module(module);

    // Then: both its call and layout hash are gone.
    assert!(index.callers(target).is_empty());
    assert_eq!(index.layout_hash(module), None);
    assert!(index.is_empty());
}

#[test]
fn call_hierarchy_reverse_index_orders_callers_deterministically() {
    // Given: callers arrive in non-canonical order.
    let module = ModuleId::new(vfs::FileId(1));
    let target = method(2, 1);
    let callers = [method(1, 3), method(1, 1), method(1, 2)];
    let mut index = CallHierarchyReverseIndex::default();

    // When: the module is indexed.
    index.replace_module(module, callers.map(|caller| pair(caller, target)), 11);

    // Then: callers are returned in stable method-id order.
    assert_eq!(index.callers(target), &[method(1, 1), method(1, 2), method(1, 3)]);
}

#[test]
fn call_hierarchy_reverse_index_insert_pair_hot_target() {
    // Given: many modules contribute callers to the same target in reverse order.
    let target = method(0, 1);
    let caller_count = 50_000;
    let modules = (1..=caller_count).rev().map(|file_id| {
        let caller = method(file_id, 1);
        (caller.module, vec![pair(caller, target), pair(caller, target)], u64::from(file_id))
    });

    // When: the reverse index is constructed.
    let started = Instant::now();
    let index = CallHierarchyReverseIndex::from_modules(modules);
    let elapsed = started.elapsed();

    // Then: construction remains bounded and query output is sorted and deduplicated.
    assert!(
        elapsed < Duration::from_millis(100),
        "constructing {caller_count} callers took {elapsed:?}"
    );
    assert_eq!(index.callers(target).len(), caller_count as usize);
    assert_eq!(index.callers(target).first(), Some(&method(1, 1)));
    assert_eq!(index.callers(target).last(), Some(&method(caller_count, 1)));
    assert!(index.callers(target).windows(2).all(|callers| callers[0] != callers[1]
        && super::compare_methods(&callers[0], &callers[1]).is_lt()));
}

#[test]
fn call_hierarchy_reverse_index_size_estimate_grows_with_capacity() {
    // Given: an empty index and one small module contribution.
    let module = ModuleId::new(vfs::FileId(1));
    let target = method(2, 1);
    let mut index = CallHierarchyReverseIndex::default();
    let empty_size = index.estimated_heap_bytes();
    index.replace_module(module, [pair(method(1, 1), target)], 11);
    let small_size = index.estimated_heap_bytes();

    // When: the module is replaced with enough pairs to grow storage capacity.
    let pairs = (0..32).map(|local_id| pair(method(1, local_id), target));
    index.replace_module(module, pairs, 22);
    let grown_size = index.estimated_heap_bytes();

    // Then: capacity-backed estimates are nonzero and never decrease.
    assert!(small_size > empty_size);
    assert!(grown_size >= small_size);
}
