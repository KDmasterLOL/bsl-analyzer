# Provenance: JoinWithVirtualTable

## Status

Rule is a good permissive candidate, but the current implementation still
depends on the broader SDBL parser and `sdbl-hir` audit.

## Why this rule exists

This diagnostic is directly grounded in public 1C guidance.

The primary source is `#std655`, which advises against joins with virtual
tables and recommends rewriting such queries through temporary tables when
performance matters.

So the rule idea and its general remediation strategy are public.

## Audit result

### Production code

Current handler in
`crates/ide-diagnostics/src/handlers/join_with_virtual_table.rs` is minimal and
local:

- it reacts to `sdbl_hir::SdblDiagnostic::JoinWithVirtualTable`;
- it maps the SDBL range back into the BSL source;
- it reports a local diagnostic message.

However, actual detection is performed below this layer in the SDBL parser /
lowering stack, which is still being audited separately.

### Documentation

Local RU/EN documentation was rewritten during this audit to describe the rule
through `#std655` and the temporary-table rewrite pattern, without relying on
upstream wording.

### Tests

Current tests are local inline Rust scenarios covering:

- left, right, and multiple joins with virtual tables;
- virtual tables used as joined sources or as the main `FROM` source together
  with joins;
- negative cases without joins or with ordinary tables only.

## Important caveat

Even though the rule itself has a strong public standards basis, the current
implementation still depends on SDBL infrastructure whose provenance is not yet
fully closed:

- `crates/parser`
- SDBL-related lexer logic
- `crates/sdbl-hir`

So this diagnostic should currently be treated as:

`rule is clean, implementation depends on SDBL audit`

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader parser / SDBL audit.

## Conclusion

`JoinWithVirtualTable` has a strong public standards basis and locally rewritten
documentation, but it should not be considered fully cleared independently from
the ongoing SDBL parser audit.
