# Diagnostics

Used for code analysis to meet coding standards and search for possible errors.

Some diagnostics are disabled by default. Use the
[configuration guide](../../../../docs/configuration/DIAGNOSTICS.md) to enable them.

To suppress diagnostics for individual lines, ranges, or whole files, use in-code
comment directives:

- `// bsl-analyzer:off Code1, Code2` … `// bsl-analyzer:on Code1` — suppress a range
  (an `off` with no matching `on` reaches the end of the file; put it on the first line
  for a whole-file mute);
- `// bsl-analyzer:disable-next-line Code1` — suppress the next line;
- `// bsl-analyzer:disable-line Code1` — suppress the directive's own line (trailing use).

Listing no codes suppresses every diagnostic in scope. A typo'd code raises
`UnknownSuppressionCode`; a code-less directive raises `SuppressionWithoutCode`.

bsl-language-server directives (`// BSLLS:DiagnosticKey-off`/`-on`, `// BSLLS-off`/`-on`)
are recognised as aliases by default, so an existing project's suppression comments keep
working; set `bsllsSuppressionCompat = false` in the project config to turn them off.
## Implemented diagnostics
