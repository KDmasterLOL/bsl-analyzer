# Unknown code in a suppression directive (UnknownSuppressionCode)

## Description

A comment suppression directive (`// bsl-analyzer:off …`, `disable-next-line`, `disable-line`) references a diagnostic code that does not exist — usually a typo in the code name. Such a directive silently suppresses nothing, so the author may believe a finding is muted while it keeps firing.

The diagnostic points at the exact unknown token so the typo can be fixed.

## Examples

Incorrect

```bsl
// bsl-analyzer:off NoSuchRule
A = A;
```

Correct

```bsl
// bsl-analyzer:off SelfAssign
A = A;
```

## Sources

* [Standard: Module texts (RU)](https://its.1c.ru/db/v8std#content:456:hdoc)
