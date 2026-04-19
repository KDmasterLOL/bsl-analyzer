# Provenance: DeletingCollectionItem

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic reflects a general collection-safety rule rather than a
1C-specific proprietary idea: mutating the collection currently being iterated
can invalidate enumeration semantics and lead to skipped elements or errors.

Public supporting source:

- ITS beginner programming guidance for 1C collections

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/deleting_collection_item.rs` is local and
HIR-based:

- the user-facing diagnostic is created from a local HIR body diagnostic;
- the active path is integrated with local `hir` infrastructure;
- the handler itself only converts that local finding into an IDE diagnostic.

This favors permissive treatment because the active implementation is local and
the rule is a common safety constraint on mutable iteration.

### Documentation

Public documentation was rewritten during this audit to avoid inherited English
wording and to explain the rule in project-local language.

### Tests

Current tests are local and inline. During this audit, a comment that referred
to a specific external provenance trail was removed while preserving the same
behavioral coverage.

Covered scenarios include:

- deleting from the iterated collection;
- deleting from a different collection;
- global `Удалить()` / `Delete()` calls;
- English keywords;
- chained collection expressions;
- safe `Delete + Break` and `Delete + Return` patterns.

## Remaining caveats

- the diagnostic idea overlaps with public `bsl-ls` material because the rule is
  generic and unsurprising;
- deeper provenance of the underlying HIR body-diagnostic origin still depends
  on the broader audit of `hir-def` lowering logic;
- repository history may still contain earlier upstream-aligned wording.

## Conclusion

`DeletingCollectionItem` is a reasonable permissive candidate because:

- the rule is a general safe-iteration constraint;
- the active implementation is local and HIR-based;
- the current docs and tests do not require retaining copyleft treatment on
  their face.
