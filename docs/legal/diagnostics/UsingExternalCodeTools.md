# UsingExternalCodeTools provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule follows from public 1C security guidance: loading or creating external code artifacts is risky and restricted, especially in unsafe server-side execution scenarios. This is a public security concern, not a unique analyzer-specific idea.

## Public sources

- `#std669` "Ограничение на выполнение "внешнего" кода"
- `v8std.ru/diagnostics/bslls/UsingExternalCodeTools/` as a secondary public reference

## Audit result

The current implementation is local Rust code built on top of HIR diagnostics. It recognizes direct calls to a fixed set of known APIs:

- external data processors
- external reports
- configuration extensions

The detection is semantic enough to avoid some obviously unrelated qualified accesses such as metadata paths or local variables with the same names.

## Important caveats

- The current implementation does not distinguish server and client context, even though the public guidance is primarily about unsafe server-side execution.
- The exact API list is a local implementation choice of this project.

## Conclusion

`UsingExternalCodeTools` looks like a strong permissive candidate. The rule is standards-based, and the current implementation is local HIR-based security logic with a clearly documented scope limitation.
