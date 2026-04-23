# UnreachableCode provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This is a generic static-analysis rule about dead code and control-flow semantics. The idea that code after `return`, `raise`, `break`, or `continue` is unreachable is not specific to any upstream project.

## Public sources

- Public control-flow semantics of the BSL language.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and CFG-based. It:

- computes reachable vertices from the CFG entry while skipping dead-code edges;
- for method bodies, computes a narrower set of locally unreachable vertices connected to reachable code when traversed backwards;
- merges adjacent unreachable statement ranges into larger diagnostics.

This is a materially nontrivial local implementation detail rather than a thin textual port.

## Audit notes

- Rule idea: clean and generic.
- The old handler comment was rewritten to remove an unnecessary `ported from` provenance trail.
- Existing tests are local and cover ordinary unreachable code, labels, parse-error exclusions, and omitted semicolons before closing constructs that still affect reachability.
