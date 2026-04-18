# Provenance: AllFunctionPathMustHaveReturn

## Status

Candidate for `MIT OR Apache-2.0` after per-diagnostic audit.

## Why this rule exists

This diagnostic follows directly from BSL language semantics: if execution reaches
`EndFunction` without an explicit `Return`, the function returns `Undefined`.

For everyday 1C development this is a language rule, not a project-specific idea.
The diagnostic therefore protects against an implicit `Undefined` on at least one
execution path.

## Source of the rule

- Primary source: BSL language semantics and common 1C development practice.
- Practical rationale: every function should return an explicit value on every
  reachable path, unless `Undefined` is returned intentionally and explicitly.

At the time of this audit, no mandatory `v8std` link was attached to the upstream
diagnostic. For this rule, the semantic behavior of the language is sufficient to
justify an independent implementation.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/all_function_path_must_have_return.rs`
is based on the local HIR/CFG pipeline and materially differs from the Java
implementation in `bsl-language-server`.

Evidence in favor of independent expression:

- the diagnostic is validated on local HIR bodies and local CFG nodes;
- the architecture differs from the upstream AST visitor approach;
- later history contains substantial rewrites and false-positive fixes specific to
  this codebase;
- the remaining implementation expresses the rule in terms of local IR and control
  flow primitives, not as a line-by-line Java translation.

### Documentation

Russian documentation was already rewritten before this audit.

English documentation was rewritten during this audit to remove upstream wording
and examples.

### Tests

Some earlier test scenarios reused upstream examples such as
`ОпределитьСтавкуНДС` and `СуммаСкидки`.

During this audit, the inline tests for this diagnostic were rewritten with new
fixtures and names so the current local tests no longer depend on those borrowed
examples.

## Remaining caveats

- Earlier history still contains references to the upstream implementation.
- This status applies to the current file set, not automatically to every past
  commit.
- Repository-wide relicensing still requires a broader crate/file audit.

## Conclusion

`AllFunctionPathMustHaveReturn` is a good pilot candidate for permissive
licensing because:

- the rule itself follows from BSL semantics;
- the current handler is expressed through local HIR/CFG architecture;
- the most obvious borrowed documentation and test fixtures have been replaced.

Recommended classification for future mixed licensing:

- diagnostic logic and docs in the current tree: `MIT OR Apache-2.0` candidate;
- repository-wide status: unchanged until the rest of the audit is completed.
