# Provenance: CommonModuleNameGlobal

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in the published 1C naming rules for common
modules.

Primary source:

- ITS / v8std `#std469`, section `3.2.1`

The rule is organizational and metadata-based: a global common module should
expose that execution role in its name through the `Глобальный` / `Global`
postfix.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/common_module_name_global.rs` is defined
through a local metadata predicate:

- it checks the local `global` flag;
- it validates the module name against local keyword variants;
- it reports only when the global-module naming convention is violated.

This favors permissive treatment because the rule follows a published naming
standard and the implementation is a small local metadata check.

### Documentation

Public documentation was rewritten during this audit to explain the rule from
`#std469` and global-module naming semantics rather than inherited terse
wording.

### Tests

Current tests are local and synthetic:

- global module without postfix;
- global module with Russian postfix;
- global module with English postfix;
- non-global module.

They do not rely on borrowed upstream fixture files.

## Remaining caveats

- repository history may still contain earlier upstream-aligned wording;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CommonModuleNameGlobal` is a good permissive candidate because:

- the rule directly follows from `#std469`;
- the current implementation is local and metadata-driven;
- the active docs and tests do not require retaining copyleft treatment.
