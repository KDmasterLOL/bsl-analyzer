# Space at the beginning of the comment (SpaceAtStartComment)

## Description

There should be a space between `//` and the comment text.

The diagnostic does not report separator lines made only of slashes and spaces, and it also skips configured annotation prefixes such as `//@`, `//(c)`, and `//©`.

## Examples

Incorrect

```bsl
//Load counterparties
Counterparties = GetCounterparties();
```

Correct

```bsl
// Load counterparties
Counterparties = GetCounterparties();
```

## Sources

* [Standard: Module texts (RU)](https://its.1c.ru/db/v8std#content:456:hdoc)
* [v8std: SpaceAtStartComment](https://v8std.ru/diagnostics/bslls/SpaceAtStartComment/)
