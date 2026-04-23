# UsingFindElementByString provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

This is a generic portability and maintainability rule: hardcoded lookups by description, code, or document number couple the source code to specific database contents. That idea is standard static-analysis guidance and not a unique analyzer-specific invention.

## Public sources

- `v8std.ru/diagnostics/bslls/UsingFindElementByString/` as a secondary public reference

There is no direct `v8std` standard mapping for the rule itself.

## Audit result

The current implementation is local Rust code built on top of HIR diagnostics. It detects a specific set of direct patterns:

- `FindByDescription` / `НайтиПоНаименованию` with string literals
- `FindByCode` / `НайтиПоКоду` with string or numeric literals
- `FindByNumber` / `НайтиПоНомеру` with string or numeric literals
- empty calls such as `НайтиПоНаименованию()`

## Important caveats

- The current implementation does not try to follow variable values or broader dataflow; it mainly reports direct literal-based calls.
- The exact method set and literal patterns are local implementation choices of this project.

## Conclusion

`UsingFindElementByString` looks like a strong permissive candidate. The rule is generic, and the current implementation is local HIR-based logic with a clearly bounded scope.
