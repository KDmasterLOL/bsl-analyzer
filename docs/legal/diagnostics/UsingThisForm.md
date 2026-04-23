# UsingThisForm provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule follows directly from public platform evolution: `ЭтаФорма` / `ThisForm` was deprecated starting from the relevant 1C platform version, and `ЭтотОбъект` / `ThisObject` should be used instead.

This is a public migration and API-correctness concern, not a unique analyzer-specific idea.

## Public sources

- official 1C migration material for transition away from `ЭтаФорма`
- `v8std.ru/diagnostics/bslls/UsingThisForm/` as a secondary public reference

## Audit result

The current implementation is local Rust code built on top of HIR diagnostics. It distinguishes actual use of the deprecated form property from unrelated cases such as:

- local parameters named `ЭтаФорма`
- function calls named `ЭтаФорма`
- member names on unrelated receivers

## Important caveats

- The exact set of exclusions is a local implementation detail of this project.
- The compatibility mode threshold is encoded in project metadata and tied to platform evolution rather than copied implementation text.

## Conclusion

`UsingThisForm` looks like a strong permissive candidate. The rule is public and migration-driven, and the current implementation is local HIR-based logic.
