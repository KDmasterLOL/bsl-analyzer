# Provenance: CommonModuleNameFullAccess

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in the published 1C naming rules for common
modules.

Primary source:

- ITS / v8std `#std469`

The rule is organizational and metadata-based: a privileged common module
should expose that elevated execution mode in the module name through the
`ПолныеПрава` / `FullAccess` postfix.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/common_module_name_full_access.rs`
is defined through a local metadata predicate:

- it checks the local `privileged` flag;
- it validates the module name against local keyword variants;
- it reports only when the privileged-module naming convention is violated.

This favors permissive treatment because the rule follows a published naming
standard and the implementation is a small local metadata check.

### Documentation

Public documentation was rewritten during this audit to explain the rule from
`#std469` and privileged-module naming semantics rather than inherited terse
wording.

### Tests

Current tests are local and synthetic:

- privileged module without postfix;
- privileged module with English postfix;
- privileged module with Russian postfix.

They do not rely on borrowed upstream fixture files.

## Remaining caveats

- repository history may still contain earlier upstream-aligned wording;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CommonModuleNameFullAccess` is a good permissive candidate because:

- the rule directly follows from `#std469`;
- the current implementation is local and metadata-driven;
- the active docs and tests do not require retaining copyleft treatment.
