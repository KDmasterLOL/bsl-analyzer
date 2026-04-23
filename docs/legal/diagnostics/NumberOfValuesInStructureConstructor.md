# Provenance: NumberOfValuesInStructureConstructor

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic has a public standards-based rationale.

`#std693` ("Using objects of type Structure") supports the general idea that
large inline structure-constructor calls reduce readability and should often be
replaced with explicit `Insert` calls. The exact numeric threshold is a local
configurable heuristic built on top of that guidance.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/number_of_values_in_structure_constructor.rs`
is local and straightforward:

- it walks local HIR expressions and identifies `Structure` /
  `FixedStructure` constructors;
- it counts constructor values after the first key-string argument;
- it compares the count with the configurable `maxValuesCount`.

This strongly favors permissive treatment because the implementation is a local
configurable check over a public design recommendation.

### Documentation

RU/EN documentation was rewritten during this audit to separate the public
rationale from the local configurable threshold.

### Tests

Current tests are local inline fixtures covering:

- empty structures;
- key-only constructors;
- constructors at and above the default threshold;
- Russian and English keywords;
- fixed structures and unrelated constructors;
- nested constructors.

The test suite is embedded directly in the Rust module.

## Remaining caveats

- the exact numeric threshold is a local policy choice rather than a literal
  value mandated by the standard;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`NumberOfValuesInStructureConstructor` is a strong permissive candidate because
it is a local configurable implementation of a public 1C design recommendation,
with local tests and now-local documentation.
