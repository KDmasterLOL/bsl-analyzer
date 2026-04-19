# Provenance: GetFormMethod

## Status

Candidate for `MIT OR Apache-2.0`, with an important scope caveat.

## Why this rule exists

This diagnostic is rooted in public 1C guidance about opening forms.

Primary sources:

- ITS / v8std `#std404`: opening forms through `ОткрытьФорму`
- public `v8std.ru` diagnostic mapping for `GetFormMethod`

The official recommendation is specific: opening a form through
`ПолучитьФорму()` followed by `Открыть()` / `ОткрытьМодально()` is discouraged.

## Audit result

### Production code

Current implementation is local and HIR-based:

- `crates/hir-def/src/body/lower/expr.rs` emits `GetFormMethod` for both global
  and object `ПолучитьФорму` / `GetForm` calls;
- `crates/ide-diagnostics/src/handlers/get_form_method.rs` only formats and
  emits the local message;
- tests are local inline Rust scenarios.

This supports permissive treatment for the code itself.

### Documentation

Local RU/EN documentation was rewritten during this audit to align with the
actual current behavior and public 1C sources.

### Tests

Current tests are local and inline. During this audit the last remaining
upstream-looking provenance trail in the test name was removed.

## Important caveat

Current rule scope is broader than the wording of `#std404`.

The standard explicitly criticizes the pattern “get form object, then open it”.
The current implementation flags any direct `ПолучитьФорму` / `GetForm` call,
including object method calls where the replacement is not always a trivial
one-line `ОткрытьФорму(...)`.

That means:

- the public rationale is strong;
- the exact current detection policy is partly a local project decision layered
  on top of the standard.

## Residual risk

Residual legal risk is moderate but acceptable for a permissive-candidate
bucket:

- low for code structure and tests, which are local;
- higher only in the sense that rule scope is stricter than the public source
  text and should therefore be described honestly.

## Conclusion

`GetFormMethod` is a reasonable permissive candidate if documented as a local
policy built from public 1C form-opening recommendations, not as a literal
one-to-one implementation of `#std404`.
