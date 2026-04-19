# Provenance: DeprecatedMethods8317

## Status

Candidate for `MIT OR Apache-2.0`, with one caveat.

## Why this rule exists

This diagnostic is grounded primarily in public platform change documentation
for 1C:Enterprise `8.3.17`.

Primary sources:

- official 8.3.17 platform changelog
- v8std `#std404` for the `ПолучитьФорму` / `GetForm` recommendation

The rule is API-based: deprecated global error-handling methods should be
replaced with members of `МенеджерОбработкиОшибок` /
`ErrorProcessingManager`. The current implementation also groups
`ПолучитьФорму` / `GetForm` under the same diagnostic family.

## Audit result

### Production code

Current implementation is local and HIR-based:

- deprecated names are matched through local replacement tables;
- replacement text is maintained locally;
- the handler emits diagnostics from local HIR findings.

This favors permissive treatment because the active implementation is local and
the main deprecated names come from public platform changes.

### Documentation

Public documentation was rewritten during this audit to avoid inherited English
wording and to align the documented replacements with the current local
implementation.

### Tests

Current tests are local and inline, covering:

- deprecated Russian error-handling methods;
- exclusion of object method calls.

They do not rely on borrowed upstream fixture files.

## Remaining caveats

- the handler currently mixes two concerns:
  1. 8.3.17 deprecated global error-handling methods;
  2. `ПолучитьФорму` / `GetForm`, which conceptually overlaps with the separate
      `GetFormMethod` diagnostic.
- because of that overlap, future cleanup may want to split `GetForm` handling
  out of this family for conceptual clarity.
- the catalog of deprecated names naturally overlaps with public `bsl-ls`
  material because both tools reflect the same public rules.

## Conclusion

`DeprecatedMethods8317` is a reasonable permissive candidate because:

- the rule is rooted in public platform changes;
- the current implementation is local and HIR-based;
- the active docs and tests do not require retaining copyleft treatment.
