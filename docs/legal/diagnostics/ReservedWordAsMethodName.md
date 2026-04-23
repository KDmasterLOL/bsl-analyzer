# ReservedWordAsMethodName provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule is based on public BSL language syntax. The fact that a procedure or function name cannot be a reserved keyword is a language constraint, not an original upstream idea.

## Public sources

- Public BSL syntax and reserved-keyword semantics.

## Implementation notes

The current implementation is local and HIR-based. It:

- consumes a lowering-time signal that a method name is a reserved word;
- reports both Russian and English keyword forms;
- emits a straightforward syntax-level diagnostic with no project-specific configuration.

## Audit notes

- Rule idea: clean and language-based.
- Docs were simplified to match the real implementation instead of enumerating a pseudo-authoritative keyword list inside the documentation.
- Existing tests are local and cover reserved Russian names, reserved English names, and valid procedure/function names.
