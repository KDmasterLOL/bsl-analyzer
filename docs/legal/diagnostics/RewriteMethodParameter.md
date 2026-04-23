# RewriteMethodParameter provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This is a generic static-analysis rule about overwritten by-value parameters. The idea that a method parameter should not be immediately overwritten before use is not specific to any upstream project.

## Public sources

- General static-analysis practice for overwritten parameters.
- Public PVS-Studio note `V763` as a secondary supporting source.

## Implementation notes

The current implementation is local and materially nontrivial. It:

- consumes HIR lowering diagnostics for assignments to by-value parameters;
- resolves the real statement via `BodySourceMap`;
- uses module-level reaching definitions to see whether only the original parameter definition reaches the assignment;
- suppresses diagnostics when the parameter is used in the right-hand side or in an earlier meaningful statement;
- skips self-assignments such as `Param = Param`.

This is significantly more precise than a naive "first assignment wins" textual heuristic.

## Audit notes

- Rule idea: clean and generic.
- Docs were rewritten to match the actual scope: by-value parameters only, reaching-defs based, with explicit self-assign and prior-use exclusions.
- Existing tests are local and cover by-value vs by-ref, self-assign, RHS use, prior field access, and the large regression fixture.
