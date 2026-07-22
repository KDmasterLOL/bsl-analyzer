# Unlimited-length string field in a restricted query position (UnlimitedLengthStringUsageInQuery)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Fields declared as unlimited-length strings (`Строка(0)`, i.e. a string type
with the "unlimited length" qualifier) cannot participate in most query
operations. The 1C:Enterprise platform raises a runtime error such as:

```
Неверные параметры в операции сравнения.
Нельзя сравнивать поля неограниченной длины и поля несовместимых типов.
```

The diagnostic reports unlimited-length string fields used in:

- comparison operations (`=`, `<>`, `<`, `<=`, `>`, `>=`) in `WHERE` (`ГДЕ`),
  `HAVING` (`ИМЕЮЩИЕ`) and join conditions (`СОЕДИНЕНИЕ ... ПО`);
- the `IN` (`В`) and `BETWEEN` (`МЕЖДУ`) operators;
- `GROUP BY` (`СГРУППИРОВАТЬ ПО`);
- `ORDER BY` (`УПОРЯДОЧИТЬ ПО`);
- `SELECT DISTINCT` (`ВЫБРАТЬ РАЗЛИЧНЫЕ`);
- `TOTALS BY` (`ИТОГИ ... ПО`).

To use such a field in these positions, cast it to a bounded-length string
with `ВЫРАЗИТЬ(... КАК СТРОКА(N))`. Pattern matching with `ПОДОБНО` (`LIKE`)
and `NULL` checks (`ЕСТЬ NULL`) are allowed by the platform and are not
reported.

## Examples
Diagnostic:

```bsl
ВЫБРАТЬ Лог.Ссылка
ИЗ Справочник.ЖурналЗапросов КАК Лог
ГДЕ Лог.ПредложеноAI <> ""
```

Corrected:

```bsl
ВЫБРАТЬ Лог.Ссылка
ИЗ Справочник.ЖурналЗапросов КАК Лог
ГДЕ ВЫРАЗИТЬ(Лог.ПредложеноAI КАК СТРОКА(1000)) <> ""
```

## Sources
- [Standard: Using string attributes, Clause 3 (RU)](https://its.1c.ru/db/v8std/content/432/hdoc)
- [Public mirror: v8std.ru / #std432](https://v8std.ru/std/432/)
