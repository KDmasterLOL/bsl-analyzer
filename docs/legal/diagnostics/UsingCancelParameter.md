# UsingCancelParameter provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule follows directly from public 1C guidance for the `Отказ` event-handler parameter: once cancellation is set, later checks should not accidentally reset it. This is a public event-handling convention rather than a unique analyzer-specific idea.

## Public sources

- `#std686` "Работа с параметром "Отказ" в обработчиках событий"
- `v8std.ru/diagnostics/bslls/UsingCancelParameter/` as a secondary public reference

## Audit result

The current implementation is local Rust code built on top of HIR diagnostics. The handler itself only emits the final message for unsafe assignments that were already recognized by local HIR logic.

The accepted forms reflected by the current tests are:

- assignment to `Истина`
- `Отказ = Отказ ИЛИ ...`
- `Отказ = ... ИЛИ Отказ`

## Important caveats

- The exact detection coverage comes from the local HIR layer, not from this handler file.
- The rule is standards-based, but the concrete implementation choices for which boolean expressions are allowed are local project logic.

## Conclusion

`UsingCancelParameter` looks like a strong permissive candidate. The rule is directly standards-based, and the current implementation path is local and HIR-based.
