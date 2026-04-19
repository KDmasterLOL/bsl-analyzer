# Provenance: DeprecatedMethods8310

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in public platform change documentation for
1C:Enterprise `8.3.10`.

Primary source:

- official 8.3.10 platform changelog

The rule is API-based: several global client-application methods were replaced
with members of the `КлиентскоеПриложение` / `ClientApplication` object.

## Audit result

### Production code

Current implementation is local and HIR-based:

- deprecated names are matched through local replacement tables;
- replacement text is maintained locally;
- the handler emits diagnostics from local HIR findings.

This favors permissive treatment because the rule follows public platform API
changes and the implementation is tied to local HIR infrastructure.

### Documentation

Public documentation was rewritten during this audit to avoid inherited English
wording and to describe the replacement object model in project-local language.

### Tests

Current tests are local and inline, covering:

- deprecated Russian methods;
- deprecated English methods;
- exclusion of object method calls.

They do not rely on borrowed upstream fixture files.

## Remaining caveats

- the catalog of deprecated names naturally overlaps with public `bsl-ls`
  material because both tools reflect the same 8.3.10 platform changes;
- deeper provenance of the underlying HIR detection still depends on the
  broader audit of `hir` and lowering logic.

## Conclusion

`DeprecatedMethods8310` is a good permissive candidate because:

- the rule directly follows from public 8.3.10 platform changes;
- the current implementation is local and HIR-based;
- the active docs and tests do not require retaining copyleft treatment.
