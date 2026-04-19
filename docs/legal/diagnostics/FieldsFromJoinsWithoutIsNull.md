# FieldsFromJoinsWithoutIsNull provenance

## Assessment

`FieldsFromJoinsWithoutIsNull` is a plausible candidate for `MIT OR Apache-2.0` at the rule level, but it should remain in the `needs extra review` bucket because it depends on the SDBL parser and `sdbl-hir` layers that are still under provenance audit.

The rule concept itself is public and generic: fields coming from outer joins can contain `NULL`, so using them without `ISNULL()` or an explicit `IS NULL` / `IS NOT NULL` check is error-prone.

## Source basis

- 1C guidance on `ЕСТЬNULL()` / `ISNULL()`: <https://its.1c.ru/db/metod8dev/content/2653/hdoc>
- 1C guidance on empty values and `NULL`: <https://its.1c.ru/db/metod8dev/content/2614/hdoc/_top/%D0%B5%D1%81%D1%82%D1%8C%20null>
- 1C guidance on `Undefined` vs `Null`: <https://its.1c.ru/db/metod8dev/content/2516/hdoc>
- 1C standard note on sorting fields that can contain `NULL`: <https://its.1c.ru/db/v8std/content/412/hdoc/_top/%D0%B5%D1%81%D1%82%D1%8C%20null>

These materials are sufficient to justify the semantic rule and the recommended use of `ЕСТЬNULL()` / `IS NULL`.

## Implementation notes

The current implementation in `bsl-analyzer` is local at the diagnostic layer:

- `sdbl_hir` emits a diagnostic with join type and field references;
- the IDE layer maps those references back into BSL ranges and formats the message;
- the implementation recognizes several protection patterns, including `ЕСТЬNULL`, `IS NULL`, `IS NOT NULL`, and `NOT ( ... IS NULL )`.

## Residual risk

Residual risk is medium.

- the rule concept is public and defensible;
- however, the underlying SDBL parser and `sdbl-hir` provenance has not been fully cleared yet;
- this means the diagnostic logic should not be treated as fully clean just because the surrounding docs are.

## Conclusion

Keep this diagnostic in the `rule is clean, implementation depends on SDBL audit` bucket for now.
