# UsingSynchronousCalls provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule follows from public 1C guidance for web-client compatible configurations: synchronous calls should be replaced with asynchronous alternatives. This is a public platform and UX restriction, not a unique analyzer-specific idea.

## Public sources

- `#std703` "Ограничение на использование модальных окон и синхронных вызовов"
- public documentation mapping synchronous methods to asynchronous analogs
- `v8std.ru/diagnostics/bslls/UsingSynchronousCalls/` as a secondary public reference

## Audit result

The current implementation is local Rust code built on top of HIR diagnostics. It reports a fixed set of known synchronous global-context methods and includes a suggested replacement in the final message.

## Important caveats

- The current implementation explicitly covers only global-context methods, not every possible synchronous pattern in the platform.
- The mapping from synchronous method to recommended replacement is a local implementation detail of this project.

## Conclusion

`UsingSynchronousCalls` looks like a strong permissive candidate. The rule is standards-based, and the current implementation path is local and HIR-based.
