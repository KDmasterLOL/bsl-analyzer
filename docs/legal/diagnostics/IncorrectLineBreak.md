# Provenance: IncorrectLineBreak

## Status

Good candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is grounded in public 1C formatting guidance.

Primary source:

- ITS / v8std `#std444`: wrapping expressions

The standard explicitly covers:

- operators at the start of the continued line;
- placement of `)` and `;` with the last parameter;
- wrapping of long logical conditions;
- multiline string handling.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/incorrect_line_break.rs` is local and
token-based:

- it inspects line starts and line ends over the local parsed syntax tree;
- it flags a specific subset of style violations;
- it includes a local exception for multiline string continuation after `+`.

This supports permissive treatment because the implementation is local and the
rule basis comes from public style guidance.

### Important scope caveat

Current implementation is narrower and more concrete than the full text of
`#std444`.

It does **not** implement every formatting option and exception described in the
standard. In particular, it currently checks a fixed set of forbidden
line-start/line-end tokens and does not expose the configuration options that
older documentation text implied.

### Documentation

Local RU/EN documentation was rewritten during this audit to match the actual
behavior of the current implementation instead of describing unsupported
configuration knobs.

### Tests

Current tests are local inline Rust scenarios covering:

- operators at line end;
- logical operators at line end;
- comma and closing parenthesis at line start;
- exceptions for multiline string continuation;
- correctly formatted wrapped expressions.

The tests are local and do not depend on an external fixture file.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared
  parser / syntax infrastructure.

## Conclusion

`IncorrectLineBreak` is a good permissive candidate because it is based on a
public 1C formatting standard and the current implementation/docs/tests are
local, with the remaining risk mainly in historical wording rather than code.
