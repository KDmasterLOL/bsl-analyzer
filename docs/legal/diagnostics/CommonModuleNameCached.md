# Provenance: CommonModuleNameCached

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in the published 1C naming rules for common
modules.

Primary source:

- ITS / v8std `#std469`

The rule is organizational and metadata-based: a common module with repeated
use of return values should declare that behavior in its name via the standard
`ПовтИсп` / `Cached` postfix family.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/common_module_name_cached.rs`
is defined through a local metadata predicate:

- it checks the local `return_values_reuse` flag;
- it validates the module name against local keyword variants;
- it reports only when the cached-module naming convention is violated.

This favors permissive treatment because the rule follows a published naming
standard and the implementation is a small local metadata check.

### Documentation

Public documentation was rewritten during this audit to describe the rule from
`#std469` and cached-module naming semantics rather than inherited terse wording.

### Tests

Current tests are local and synthetic:

- cached module without postfix;
- cached module with postfix;
- non-cached module.

They do not rely on borrowed upstream fixture files.

## Remaining caveats

- repository history may still contain earlier upstream-aligned wording;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CommonModuleNameCached` is a good permissive candidate because:

- the rule directly follows from `#std469`;
- the current implementation is local and metadata-driven;
- the active docs and tests do not require retaining copyleft treatment.
