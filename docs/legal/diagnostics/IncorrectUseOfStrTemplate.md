# Provenance: IncorrectUseOfStrTemplate

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is primarily an API-correctness rule, not a direct
implementation of a specific 1C coding standard.

The underlying idea is simple and generic:

- `StrTemplate` / `СтрШаблон` expects valid placeholder syntax;
- the number of passed arguments must match the placeholders in the template;
- malformed placeholders or malformed `NStr(...)` wrapping lead to runtime
  errors or incorrect strings.

There is related public 1C context in `#std763`, which shows common legitimate
usage patterns of `СтрШаблон` together with `НСтр`, but that standard does not
define this diagnostic one-to-one.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/incorrect_use_of_str_template.rs` is local
and substantial:

- it combines HIR-lowering diagnostics with a post-HIR dataflow-based pass;
- it resolves template variables through local reaching-definitions analysis;
- it validates placeholder numbering, parameter counts, and `%%` escaping using
  local parsing logic.

This strongly favors permissive treatment because the implementation is clearly
local and technically substantial.

### Documentation

Local RU/EN documentation was rewritten during this audit to describe the rule
as an API-correctness check and to avoid overstating `#std763` as a direct
normative source.

### Tests

Current tests are local and extensive. They cover:

- direct string literal errors;
- invalid placeholder numbers;
- mismatched parameter counts;
- escaped percent handling;
- variable resolution through local dataflow;
- malformed `NStr(...)` wrapping.

Some tests use `test_fixture::Fixture`, but they remain local inline scenarios
within the repository rather than borrowed external fixture files.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared HIR
  and dataflow infrastructure.

## Conclusion

`IncorrectUseOfStrTemplate` is a strong permissive candidate because it is an
API-correctness rule with a clearly local, nontrivial implementation and local
test coverage.
