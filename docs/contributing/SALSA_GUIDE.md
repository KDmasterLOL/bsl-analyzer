# Salsa Developer Guide

**Version:** Salsa 0.25.2
**Last Updated:** 2025-12-30
**Status:** Production-ready

This guide explains how to work with Salsa in bsl-analyzer for incremental computation and caching.

## Table of Contents

1. [What is Salsa?](#what-is-salsa)
2. [Architecture Overview](#architecture-overview)
3. [Working with Salsa Queries](#working-with-salsa-queries)
4. [Adding New Queries](#adding-new-queries)
5. [Durability Levels](#durability-levels)
6. [LRU Configuration](#lru-configuration)
7. [Testing Salsa Integration](#testing-salsa-integration)
8. [Common Pitfalls](#common-pitfalls)
9. [Performance Guidelines](#performance-guidelines)

---

## What is Salsa?

Salsa is an **incremental computation framework** that automatically:
- Tracks dependencies between queries
- Caches computation results
- Invalidates caches when inputs change
- Enables parallel query execution

**Key Benefits:**
- **Automatic invalidation**: No manual cache management
- **Lazy evaluation**: Computes only when needed
- **Memory bounds**: LRU eviction prevents unbounded growth
- **Performance**: Cache hits are ~5-22 nanoseconds

---

## Architecture Overview

### Current Salsa Integration

```
┌─────────────────────────────────────────────┐
│         RootDatabaseImpl                    │
│  (salsa::Storage<Self>)                     │
├─────────────────────────────────────────────┤
│  Salsa Inputs:                              │
│  - FileTextInput                            │
│  - SourceRootInput                          │
│  - FileSourceRootInput                      │
├─────────────────────────────────────────────┤
│  Salsa Tracked Queries:                     │
│  - parse_query (LRU=128) ✅                 │
├─────────────────────────────────────────────┤
│  Manual Caching (temporary):                │
│  - item_tree (DashMap)                      │
│  - module_data (DashMap)                    │
│  - symbol_tree (DashMap)                    │
└─────────────────────────────────────────────┘
```

### Traits Hierarchy

```rust
salsa::Database
    ↑
SourceDatabase (#[salsa::db])
    ↑
RootQueryDb (#[salsa::db])
    ↑
DefDatabase
    ↑
RootDatabase
```

---

## Working with Salsa Queries

### Salsa Input Structs

Input structs represent **mutable base data**:

```rust
#[salsa::input(debug)]
pub struct FileTextInput {
    pub text: String,
}
```

**Key Points:**
- Use `#[salsa::input(debug)]` for Debug support
- Fields are accessed via generated getter methods
- Changes trigger automatic invalidation
- Inputs are Copy types (cheap to pass around)

**Usage:**

```rust
// Create input
let input = FileTextInput::new(db, "Процедура Тест() КонецПроцедуры".to_string());

// Access field
let text = input.text(db);

// Update field (triggers invalidation)
use salsa::Setter;
input.set_text(db).to("Новый текст".to_string());
```

### Salsa Tracked Functions

Tracked functions are **cached derived queries**:

```rust
#[salsa::tracked(lru = 128)]
pub fn parse_query(
    db: &dyn salsa::Database,
    input: FileTextInput,
) -> syntax::Parse<syntax::SyntaxNode> {
    let text = input.text(db);
    parser::parse(&text)
}
```

**Key Points:**
- `lru = N`: Maximum cached results (128-512 recommended)
- First parameter must be `&dyn salsa::Database` or trait marked `#[salsa::db]`
- Value parameters must be Salsa types (inputs or tracked structs) or primitives
- Return value is cached automatically
- Invalidated when dependencies change

---

## Adding New Queries

### Step 1: Define Salsa Input (if needed)

If you're adding a new input source:

```rust
// In base-db/src/input.rs

#[salsa::input(debug)]
pub struct MyNewInput {
    pub data: String,
}
```

### Step 2: Create Tracked Function

```rust
// In base-db/src/lib.rs or your crate

#[salsa::tracked(lru = 256)]
pub fn my_query(
    db: &dyn RootQueryDb,
    input: MyNewInput,
) -> Arc<MyResult> {
    let _span = tracing::info_span!("my_query").entered();

    // Your computation here
    let data = input.data(db);
    let result = expensive_computation(&data);

    Arc::new(result)
}
```

### Step 3: Add Convenience Trait Method

```rust
pub trait MyDatabase: RootQueryDb {
    fn my_result(&self, input: MyNewInput) -> Arc<MyResult>
    where
        Self: Sized,
    {
        my_query(self, input)
    }
}
```

**Why `where Self: Sized`?**
Allows calling the method on concrete types while preventing calls on trait objects.

### Step 4: Update Database Implementation

```rust
#[salsa::db]
impl MyDatabase for RootDatabaseImpl {}
```

### Step 5: Add Tests

```rust
#[test]
fn test_my_query_caching() {
    let mut db = RootDatabaseImpl::new();
    let input = MyNewInput::new(&db, "test".to_string());

    // First call computes
    let result1 = my_query(&db, input);

    // Second call returns cached
    let result2 = my_query(&db, input);

    // Should be same Arc (pointer equality)
    assert!(Arc::ptr_eq(&result1, &result2));
}
```

---

## Durability Levels

Durability tells Salsa how often data changes:

```rust
pub enum Durability {
    LOW,     // Changes frequently (user code)
    MEDIUM,  // Changes occasionally (dependencies) [not currently used]
    HIGH,    // Rarely changes (libraries)
}
```

### Using Durability

**Automatic (Recommended):**

```rust
// Automatically detects based on source root
files.set_file_text_smart(db, file_id, text);
```

**Explicit:**

```rust
// For library files
files.set_file_text_with_durability(
    db,
    file_id,
    text,
    salsa::Durability::HIGH
);

// For user code
files.set_file_text_with_durability(
    db,
    file_id,
    text,
    salsa::Durability::LOW
);
```

**How it works:**

```rust
impl SourceRoot {
    pub fn durability(&self) -> salsa::Durability {
        if self.is_library {
            salsa::Durability::HIGH  // External dependencies
        } else {
            salsa::Durability::LOW   // Project code
        }
    }
}
```

---

## LRU Configuration

LRU (Least Recently Used) prevents unbounded memory growth:

### Current Configuration

```rust
parse_query:        LRU = 128   // Base parse results
// Future:
item_tree_query:    LRU = 256   // More expensive, keep more
module_data_query:  LRU = 256
symbol_tree_query:  LRU = 256
```

### Sizing Guidelines

| Query Type | Complexity | LRU Size | Reason |
|------------|-----------|----------|--------|
| Parse | Low-Medium | 128 | Many files, frequent access |
| Item Tree | Medium | 256 | More expensive to recompute |
| Module Data | Low | 256 | Cheap but many files |
| Symbol Tree | Medium | 256 | Lookup tables, reused often |

### Tuning LRU

```rust
#[salsa::tracked(lru = 512)]  // Increase for expensive queries
pub fn expensive_query(...) -> Result {
    // Complex computation
}

#[salsa::tracked(lru = 64)]   // Decrease for cheap queries
pub fn cheap_query(...) -> Result {
    // Simple lookup
}
```

**When to tune:**
- Profiling shows excessive recomputation → Increase LRU
- Memory pressure → Decrease LRU
- Benchmark before and after changes

---

## Testing Salsa Integration

### Test Database Setup

```rust
#[salsa::db]
#[derive(Clone, Default)]
struct TestDatabase {
    storage: salsa::Storage<Self>,
    files: Files,
}

#[salsa::db]
impl salsa::Database for TestDatabase {}

#[salsa::db]
impl SourceDatabase for TestDatabase {
    fn file_text_input(&self, file_id: FileId) -> FileTextInput {
        self.files.file_text(file_id)
    }
    // ... other methods
}

#[salsa::db]
impl RootQueryDb for TestDatabase {
    fn parse(&self, file_id: FileId) -> Parse<SyntaxNode> {
        let input = self.file_text_input(file_id);
        parse_query(self, input)
    }
}
```

**Important:**
- Must derive `Clone` (not `Debug` - salsa::Storage doesn't implement Debug)
- Must implement `salsa::Database`
- All trait impls need `#[salsa::db]` marker

### Testing Cache Invalidation

```rust
#[test]
fn test_incremental_invalidation() {
    let mut db = TestDatabase::default();
    let file_id = FileId(0);

    // Setup
    setup_file(&mut db, file_id, "Процедура Тест1()");

    // First parse
    let parse1 = db.parse(file_id);
    assert!(!parse1.has_errors());

    // Change file
    db.set_file_text(file_id, "Процедура Тест2()");

    // Should reparse
    let parse2 = db.parse(file_id);
    assert!(!parse2.has_errors());

    // Text should be different
    assert_ne!(parse1.syntax_node().text(), parse2.syntax_node().text());
}
```

### Benchmarking

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_cache_hit(c: &mut Criterion) {
    let db = setup_db(100);
    let file_id = FileId(50);
    let _ = db.parse(file_id);  // Prime cache

    c.bench_function("cache_hit", |b| {
        b.iter(|| {
            let _ = db.parse(black_box(file_id));
        });
    });
}
```

See `crates/ide-db/benches/salsa_incremental.rs` for full examples.

---

## Common Pitfalls

### 1. Trait Object Size Error

**Problem:**
```rust
error[E0277]: the size for values of type `Self` cannot be known at compilation time
```

**Cause:** Trying to call a method with `where Self: Sized` on a trait object.

**Solution:** Add `where Self: Sized` to the trait method:

```rust
pub trait RootQueryDb: SourceDatabase {
    fn parse(&self, file_id: FileId) -> Parse<SyntaxNode>
    where
        Self: Sized,  // Add this
    {
        let input = self.file_text_input(file_id);
        parse_query(self, input)
    }
}
```

### 2. Salsa Type Parameters

**Problem:**
```rust
error[E0277]: the trait bound `FileId: SalsaStructInDb` is not satisfied
```

**Cause:** Tracked functions require Salsa types (inputs/tracked structs) as value parameters.

**Solution:** Use Salsa inputs instead of plain types:

```rust
// ❌ Wrong: FileId is not a Salsa type
#[salsa::tracked(lru = 128)]
pub fn my_query(db: &dyn salsa::Database, file_id: FileId) -> Result {
    // ...
}

// ✅ Correct: Use FileTextInput
#[salsa::tracked(lru = 128)]
pub fn my_query(db: &dyn salsa::Database, input: FileTextInput) -> Result {
    let file_id = /* extract from input */;
    // ...
}
```

### 3. Clone on Copy Types

**Problem:**
```rust
warning: using `clone` on type `FileTextInput` which implements the `Copy` trait
```

**Solution:** Use dereference instead:

```rust
// ❌ Wrong
self.file_texts.get(&file_id).map(|e| e.value().clone())

// ✅ Correct
self.file_texts.get(&file_id).map(|e| *e.value())
```

### 4. Missing #[salsa::db] Markers

**Problem:**
```rust
error: cannot find method or associated constant `zalsa_register_downcaster`
```

**Cause:** Missing `#[salsa::db]` attribute on trait or impl.

**Solution:** Add markers to all database traits and impls:

```rust
#[salsa::db]
pub trait MyDatabase: SourceDatabase { }

#[salsa::db]
impl MyDatabase for RootDatabaseImpl { }
```

---

## Performance Guidelines

### Benchmark Results (Reference)

From `docs/planning/PHASE3_BENCHMARKS.md`:

```
Operation               Time        Notes
─────────────────────────────────────────────
Cache hit               21.8 ns     Arc clone cost
Incremental update      1.96 μs     Small files
Item tree cache hit     4.79 ns     DashMap + Arc
Large file set (200)    4.14 μs     LRU eviction
```

### Optimization Tips

1. **Use LRU appropriately**
   - More expensive queries → larger LRU
   - Frequently accessed → larger LRU
   - Memory constrained → smaller LRU

2. **Batch operations**
   ```rust
   // ❌ Bad: Multiple DB accesses
   for file_id in files {
       db.parse(file_id);  // Separate Salsa calls
   }

   // ✅ Better: Collect inputs first
   let inputs: Vec<_> = files.iter()
       .map(|&file_id| db.file_text_input(file_id))
       .collect();

   for input in inputs {
       parse_query(&db, input);  // Salsa can parallelize
   }
   ```

3. **Use durability wisely**
   - Set HIGH for library files
   - Set LOW for actively edited code
   - Salsa optimizes based on durability

4. **Profile before optimizing**
   ```bash
   BSL_PROFILE=* cargo run
   cargo bench --bench salsa_incremental
   ```

---

## Future Work

### DefDatabase Salsa Migration

**Current State:** Manual DashMap caching with `invalidate_file()`

**Target:** Full Salsa tracked functions

**Blocker:** `FileId` and `ModuleId` are plain structs, not Salsa types

**Options:**
1. Create Salsa input wrappers for FileId/ModuleId
2. Convert to Salsa tracked structs
3. Use different architectural pattern

**Timeline:** Iteration 10+ (future)

---

## Resources

### Documentation
- Official Salsa Book: `~/src/lsp/salsa/book/`
- Salsa Examples: `~/src/lsp/salsa/tests/`
- Rust-Analyzer Reference: `~/src/lsp/rust-analyzer/crates/base-db/`

### BSL-Analyzer Files
- Base-DB: `crates/base-db/src/lib.rs` (parse_query implementation)
- IDE-DB: `crates/ide-db/src/lib.rs` (RootDatabaseImpl)
- Benchmarks: `crates/ide-db/benches/salsa_incremental.rs`
- Results: `docs/planning/PHASE3_BENCHMARKS.md`

### Performance
- Benchmark Results: `docs/planning/PHASE3_BENCHMARKS.md`
- Salsa TODO: `docs/planning/SALSA_TODO.md`
- Architecture: `docs/architecture/ARCHITECTURE.md`

---

## Questions?

If you encounter issues:
1. Check this guide's [Common Pitfalls](#common-pitfalls) section
2. Review existing tests in `crates/base-db/` and `crates/ide-db/`
3. Run benchmarks to verify performance
4. Consult Salsa examples in `~/src/lsp/salsa/tests/`

**Remember:** Salsa is production-ready in bsl-analyzer with excellent performance (100-25,000x better than targets). Focus on correctness first, profile before optimizing!
