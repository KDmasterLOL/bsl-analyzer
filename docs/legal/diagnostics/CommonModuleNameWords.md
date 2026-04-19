# Provenance: CommonModuleNameWords

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in the published 1C naming rules for common
modules.

Primary source:

- ITS / v8std `#std469`, section `3.1`

The rule is organizational and naming-based: common module names should
describe the subsystem or mechanism they implement and should avoid generic
words such as `Процедуры`, `Функции`, `Обработчики`, `Модуль`,
`Функциональность`.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/common_module_name_words.rs` is local and
configuration-driven:

- it operates only on common-module metadata;
- it reads a configurable list of forbidden generic words;
- it reports when the module name contains one of those words.

This favors permissive treatment because the rule follows a published naming
standard and the implementation is a small local check over metadata.

### Documentation

Public documentation was rewritten during this audit to explain the rule from
`#std469` and the rationale behind generic-word avoidance rather than inherited
brief wording.

### Tests

Current tests are local and synthetic:

- module name with a forbidden Russian word;
- module name with a forbidden English word;
- module name without forbidden words;
- module name with `Процедуры`.

They do not rely on borrowed upstream fixture files.

## Remaining caveats

- the default generic-word list overlaps with the public wording of `#std469`
  and may also overlap with upstream implementations that follow the same
  standard;
- repository history may still contain earlier upstream-aligned wording;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`CommonModuleNameWords` is a good permissive candidate because:

- the rule directly follows from `#std469`;
- the current implementation is local and config-driven;
- the active docs and tests do not require retaining copyleft treatment.
