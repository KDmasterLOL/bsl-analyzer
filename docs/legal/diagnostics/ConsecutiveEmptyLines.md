# Provenance: ConsecutiveEmptyLines

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic reflects a general code-style convention rather than a
platform-specific algorithm: multiple empty lines in a row usually do not add
clarity and instead reduce readability.

Public supporting source:

- v8-code-style `module-consecutive-blank-lines`

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/consecutive_empty_lines.rs` is local:

- it scans plain text through the local line index;
- it uses a configurable `allowedEmptyLinesCount`;
- it reports only groups of empty lines longer than the configured limit.

This favors permissive treatment because the implementation is a straightforward
text scan and does not rely on upstream parser or visitor structure.

### Documentation

Public documentation was rewritten during this audit to avoid inherited
wording and to describe the rule in project-local language.

### Tests

Current tests are local and inline:

- empty file;
- single empty line;
- multiple empty lines;
- whitespace-only lines;
- trailing newline normalization;
- multiple offending groups.

They do not rely on borrowed upstream fixture files.

## Remaining caveats

- the rule itself is common and may naturally resemble other linters;
- repository history may still contain earlier upstream-aligned wording;
- repository-wide relicensing still depends on the broader audit.

## Conclusion

`ConsecutiveEmptyLines` is a good permissive candidate because:

- the rule is a generic formatting convention;
- the current implementation is local and simple;
- the active docs and tests do not require retaining copyleft treatment.
