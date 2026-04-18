# Provenance: BadWords

## Status

Candidate for `MIT OR Apache-2.0` after targeted cleanup of docs and tests.

## Why this rule exists

`BadWords` is not tied to a specific 1C language rule or mandatory `v8std`
requirement.

It is a configurable project-policy diagnostic: a team supplies a regular
expression with words or phrases that should not appear in code and, optionally,
in comments.

That makes the rule itself straightforward:

- the idea is generic and not unique to `bsl-language-server`;
- each project decides its own forbidden vocabulary and scope.

## Audit result

### Production code

Current implementation in `crates/ide-diagnostics/src/handlers/bad_words.rs`
is a simple local regex-based scan over file text.

This strongly favors independent expression:

- no specialized upstream algorithm is required for the rule;
- current code is short, direct, and shaped around local diagnostics/config
  helpers;
- the rule logic is generic enough that similar implementations are expected.

### Documentation

Local English and Russian documentation were rewritten during this audit to
describe the rule as a project-policy diagnostic rather than rely on upstream
phrasing.

### Tests

The previous local tests reused the upstream `BadWordsDiagnostic.bsl` fixture and
the characteristic `лотус|шмотус` scenario from `bsl-language-server`.

During this audit, those tests were replaced with new local examples based on a
different pattern and independently authored fixture text.

## Remaining caveats

- upstream history still exists in earlier commits;
- this provenance note applies to the current tree, not retroactively to all
  historical revisions.

## Conclusion

`BadWords` is a good permissive candidate because:

- the rule is generic and configurable by project policy;
- the current implementation is simple local code;
- the most obvious borrowed docs and test fixture were replaced.
