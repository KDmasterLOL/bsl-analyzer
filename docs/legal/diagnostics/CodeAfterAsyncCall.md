# Provenance: CodeAfterAsyncCall

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic follows from the execution model of asynchronous client-side
methods in 1C.

If an async UI/API call returns immediately, code placed after the call
continues to execute without waiting for the user's action or for the async
operation to complete.

That behavior is part of the platform's async model, not a project-specific
invention.

Primary source:

- Developer Guide, built-in language, sync/async methods

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/code_after_async_call.rs`
is integrated into the local HIR/body-diagnostics pipeline.

This favors permissive treatment:

- the rule comes from platform semantics;
- the current Rust file uses local diagnostic conversion infrastructure;
- the explicit `ported from` residue was removed during this audit.

### Documentation

Local English documentation was rewritten during this audit to explain the rule
from the 1C async model rather than from upstream wording.

Russian documentation already described the rule in local wording and was kept as
the main user-facing explanation.

### Tests

Current local tests are inline Rust scenarios built around several control-flow
cases: top-level, branch-local, nested blocks, loops, and English syntax.

No single large copied upstream fixture is required for the current coverage.

## Remaining caveats

- earlier repository history may still contain wording close to upstream docs;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CodeAfterAsyncCall` is a good permissive candidate because:

- the rule is grounded in documented async behavior of the platform;
- the current implementation is expressed through local analysis infrastructure;
- the obvious upstream-specific residue in docs/comments was removed.
