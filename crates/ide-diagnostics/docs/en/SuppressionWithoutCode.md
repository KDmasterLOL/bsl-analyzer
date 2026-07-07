# Suppression directive without a code (SuppressionWithoutCode)

## Description

A comment suppression directive (`// bsl-analyzer:off`, `disable-next-line`, `disable-line`) lists no diagnostic codes, so it mutes **every** finding in its scope. That hides not only the finding the author meant to exclude but any future ones too, including genuinely important diagnostics.

Always list the specific codes that should be suppressed.

## Examples

Incorrect

```bsl
// bsl-analyzer:off
A = A;
```

Correct

```bsl
// bsl-analyzer:off SelfAssign
A = A;
```

## Sources

* [Standard: Module texts (RU)](https://its.1c.ru/db/v8std#content:456:hdoc)
