# RefOveruse provenance

## Status

Promising candidate for `MIT OR Apache-2.0`, but final confidence still depends on the SDBL parser and `sdbl_hir` audit.

## Why this rule is probably clean

The core idea follows from public 1C query-performance guidance: dereferencing reference fields through dot-access may introduce extra implicit joins, and such patterns should be minimized. Using `.Ссылка` on a value that is already a reference is a straightforward specialization of that public guidance.

## Public sources

- `#std654` "Разыменование ссылочных полей составного типа в языке запросов"
- `v8std.ru/std/654/` as a public secondary reference

## Audit result

The current implementation is local Rust code that consumes `sdbl_hir::SdblDiagnostic::RefOveruse` and maps it back to source positions.

The effective rule is narrower than the broad public discussion in `#std654`: it is focused on redundant `.Ссылка` access on values that are already references.

## Important caveats

- This is an SDBL diagnostic, so final licensing confidence still depends on the broader audit of `parser` and `sdbl_hir`.
- Detection depends on query type resolution and metadata context. The current standalone tests explicitly show that many suspicious examples do not produce a diagnostic without metadata.
- The implementation scope is narrower than the whole standard: it does not try to detect every expensive dereference pattern in query text.

## Conclusion

`RefOveruse` looks like a good future permissive candidate at the rule level, but implementation-level confidence should remain tied to the remaining SDBL provenance audit.
