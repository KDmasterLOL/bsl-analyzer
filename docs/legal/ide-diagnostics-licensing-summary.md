# IDE Diagnostics Licensing Summary

## Scope

This note summarizes the current licensing/provenance posture of
`crates/ide-diagnostics` after the diagnostic-by-diagnostic audit.

It is intentionally high-level. The detailed reasoning for each individual rule
lives in `docs/legal/diagnostics/*.md`.

## Current state

As of this audit snapshot:

- `DiagnosticCode` enum contains `185` live codes;
- `crates/ide-diagnostics/docs/{ru,en}` covers all `185` codes;
- `docs/legal/diagnostics/` now also covers all `185` codes.

That does **not** mean every rule can immediately be relicensed as
`MIT OR Apache-2.0`, but it does mean the project now has a complete
provenance ledger for the diagnostics layer.

## Working conclusion

For `crates/ide-diagnostics`, the default working assumption can now be:

- a diagnostic is a **strong permissive candidate by default**
- **unless** it appears in one of the explicit exception buckets below

This is a much better posture than the earlier repo-wide conservative
`LGPL-3.0-or-later` fallback, because the remaining risk is now much more
localized and easier to reason about.

## Exception bucket 1: SDBL implementation caveat

These diagnostics look clean at the rule level, but final implementation-level
confidence still depends on the broader audit of:

- `crates/parser`
- SDBL portions of `crates/lexer`
- `crates/sdbl-hir`

See also:

- `docs/legal/parser-sdbl-hir-audit.md`
- `docs/legal/parser-sdbl-inventory.md`
- `docs/legal/parser-sdbl-select-audit.md`

Current SDBL-caveat list:

- `AssignAliasFieldsInQuery`
- `FieldsFromJoinsWithoutIsNull`
- `FullOuterJoinQuery`
- `IncorrectUseLikeInQuery`
- `JoinWithSubQuery`
- `JoinWithVirtualTable`
- `LogicalOrInJoinQuerySection`
- `LogicalOrInTheWhereSectionOfQuery`
- `MultilineStringInQuery`
- `QueryNestedFieldsByDot`
- `QueryParseError`
- `QueryToMissingMetadata`
- `RefOveruse`
- `SelectTopWithoutOrderBy`
- `UnionAll`
- `UsingLikeInQuery`
- `VirtualTableCallWithoutParameters`

Practical meaning:

- the **idea** of these rules is usually public and clean;
- the **current implementation** should keep the parser/SDBL provenance caveat
  until the parser foundation is disentangled from `bsl-parser` risk.

## Exception bucket 2: functionality caveat, not licensing caveat

These diagnostics are still plausible permissive candidates, but they have an
important product-quality caveat that should be remembered separately from
licensing:

- `TypeMismatch`
  - currently an inactive placeholder; live emitter is still disabled
- `Typo`
  - not an adequate spell checker for real 1C code; disabled by default

These caveats do **not** force copyleft by themselves. They only mean the rule
should not be presented as more mature than it actually is.

## Exception bucket 3: optional security/review hotspots

Some diagnostics are best understood as local security-review policies rather
than hard language rules. They still look compatible with permissive licensing,
but their semantics are project-policy-driven:

- `UseSystemInformation`
- `SetPrivilegedMode`
- `UsingServiceTag`
- `OSUsersMethod`
- `PrivilegedModuleMethodCall`
- `ProtectedModule`
- `FileSystemAccess`
- `InternetAccess`

This bucket is not a legal blocker. It is only a reminder that these rules are
policy-heavy and may evolve independently from standards-based diagnostics.

## Crate-level conclusion

`crates/ide-diagnostics` now looks substantially cleaner than the parser stack.

The practical hierarchy of blockers is:

1. biggest relicensing blocker: `crates/parser` and SDBL lexer pieces
2. secondary caveat: SDBL-backed diagnostics in `crates/ide-diagnostics`
3. relatively low-risk area: non-SDBL diagnostics in `crates/ide-diagnostics`

In other words:

- the diagnostics crate is **not** the main reason the repository still needs a
  conservative licensing posture;
- the parser/SDBL foundation is currently the harder blocker.

## Recommended next step

If the goal is gradual transition toward `MIT OR Apache-2.0`, the next practical
step should be one of:

1. create a crate-level mixed-licensing map for the workspace, using this audit
   as the input for `crates/ide-diagnostics`;
2. continue clean-room work on SDBL parser/lexer support so that the SDBL-caveat
   bucket can shrink over time.

## Bottom line

Today, the best working position is:

- **most `ide-diagnostics` rules are already good permissive candidates;**
- **SDBL-backed rules still inherit parser provenance caution;**
- **repo-wide permissive relicensing is blocked more by parser/lexer history than
  by the diagnostics crate itself.**
