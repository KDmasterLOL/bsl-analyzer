# MissingCodeTryCatchEx provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule directly follows public 1C guidance on exception handling. Empty `Попытка ... Исключение` handlers are discouraged by `#std499`, so the diagnostic idea is standards-based rather than an original upstream invention.

## Public sources

- `#std499` Catching exceptions in code.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and primarily HIR-based:

- it scans lowered `Try` statements and classifies each `Exception` body
  via `hir::catch_class::classify_catch_body` (Track 2 Phase D §2.1) into
  one of `Empty`, `RaisesOnly`, `LogsOnly`, `Mixed`, `RollbackOnly`,
  `Silent`;
- recognition of logging calls is delegated to the platform-wide security
  registry (`Category::Logging`, Track 2 Phase A §1.1), no per-handler
  whitelists;
- it emits diagnostics for `Empty`, `Silent`, and `RollbackOnly`, with a
  rollback-specific message for the latter; `RaisesOnly`, `LogsOnly`, and
  `Mixed` are recognised as proper recovery paths and skipped;
- it uses a small AST fallback to place the diagnostic on the `Исключение`
  keyword;
- it also uses AST fallback for the `commentAsCode` option, which only
  affects the `Empty` branch (HIR drops trivia).

By default, a comment-only `Exception` block is still considered empty. When `commentAsCode = true`, such a block is accepted.

## Audit notes

- Rule idea: clean and standards-based.
- Docs were rewritten to capture the real `commentAsCode` behavior.
- Existing tests are local and cover normal empty handlers, nested handlers, module-level code, and the comment-only configuration toggle.
