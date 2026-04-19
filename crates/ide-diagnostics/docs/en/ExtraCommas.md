# Commas without a parameter at the end of a method call (ExtraCommas)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Do not leave trailing commas at the end of a method call when no parameter follows them.

Such commas do not make the call clearer, can be confused with intentionally skipped optional parameters, and in practice only add syntax noise.

Bad:

```bsl
Result = Action(P1, P2,,);
```

Good:

```bsl
Result = Action(P1, P2);
```

## Sources

* [Code-writing conventions. Parameters of procedures and functions (RU)](https://its.1c.ru/db/v8std/content/640/hdoc)
* [v8std.ru: #std640](https://v8std.ru/std/640/)
