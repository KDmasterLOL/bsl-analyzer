# SelfInsertion provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This is a generic bug-pattern rule about circular self-references in collections. The idea that `Array.Add(Array)` or `Structure.Insert(..., Structure)` is dangerous is not specific to any upstream project.

## Public sources

- General reasoning about circular references in collections.
- Public 1C guidance on searching for circular links.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and HIR-based. It:

- consumes lowering-time diagnostics for direct self-insertion cases;
- covers both Russian and English collection method names;
- intentionally does not treat arbitrary calls with the same object as self-insertion.

This is a narrow bug-pattern detector, not a general circular-reference analyzer.

## Audit notes

- Rule idea: clean and generic.
- Docs were expanded to describe the actual narrow scope of the detector.
- Existing tests are local and cover array/structure self-insertion, English aliases, non-self insertion, and irrelevant same-object method calls.
