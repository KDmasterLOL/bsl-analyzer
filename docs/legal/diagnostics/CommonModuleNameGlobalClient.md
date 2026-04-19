# Provenance: CommonModuleNameGlobalClient

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in the published 1C naming rules for common
modules.

Primary source:

- ITS / v8std `#std469`, section `3.2.1`

The rule is organizational and metadata-based: once a common module is marked
as global, the `Глобальный` / `Global` postfix is sufficient and the
`Клиент` / `Client` postfix becomes redundant.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/common_module_name_global_client.rs` is
defined through local metadata predicates:

- it checks that the module is both global and client-side;
- it validates that the module name does not contain client postfix variants;
- it reports only when the naming rule is violated.

This favors permissive treatment because the rule follows a published naming
standard and the implementation is a small local metadata check.

### Documentation

Public documentation was rewritten during this audit to explain the rule from
`#std469` and the redundancy of the extra client postfix rather than inherited
short-form wording.

### Tests

Current tests are local and synthetic:

- global client module with English client postfix;
- global client module without client postfix;
- global client module with Russian client postfix.

They do not rely on borrowed upstream fixture files.

## Remaining caveats

- repository history may still contain earlier upstream-aligned wording;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CommonModuleNameGlobalClient` is a good permissive candidate because:

- the rule directly follows from `#std469`;
- the current implementation is local and metadata-driven;
- the active docs and tests do not require retaining copyleft treatment.
