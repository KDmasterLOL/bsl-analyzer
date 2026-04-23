# Provenance: NestedFunctionInParameters

## Status

Candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a readability rule with a public standards-based rationale.

`#std640` ("Parameters of procedures and functions") supports the general idea
that argument passing should remain readable and easy to understand. The exact
heuristic used here - flagging nested calls and nested constructors in
parameters - is still a local policy layered on top of that public guidance.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/nested_function_in_parameters.rs`
is local and configurable:

- it walks local HIR expressions for calls, method calls, and constructors;
- it uses local syntax/token helpers to locate the reported name range;
- it supports project-level configuration such as `allowOneliner` and
  `allowedMethodNames`.

This favors permissive treatment because the concrete behavior is not a copied
standard text but a local implementation choice built on current HIR and syntax
infrastructure.

### Documentation

RU/EN documentation was rewritten during this audit to describe the current
behavior directly and to separate the public rationale from the local heuristic.

### Tests

The current test suite is local and extensive. It exercises:

- ordinary nested function and method calls;
- nested constructors;
- one-line exceptions;
- allowlisted method names;
- range selection and token lookup edge cases.

The coverage is implemented as local inline fixtures and Rust assertions.

## Remaining caveats

- the exact threshold of what is "too nested" is a project policy, not a direct
  normative requirement;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`NestedFunctionInParameters` is a good permissive candidate because it has a
public readability basis in `#std640`, but the current detection logic,
configuration model, tests, and docs are local to this project.
