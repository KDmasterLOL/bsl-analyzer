# Line Length limit (LineLength)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

BSL code lines should normally stay within 120 characters.

Long lines are harder to read, review, and compare in version control. When a
statement becomes too long, split it across several lines.

The current implementation supports configuration:

- `maxLineLength` sets the threshold, `120` by default;
- `checkMethodDescription` controls whether method-description comments are
  included in the check;
- `excludeTrailingComments` can ignore trailing comments on code lines.

Some long lines may still be acceptable in practice when splitting is
technically awkward, for example for specific message text scenarios.

## Examples

Invalid:

```bsl
СообщениеДляПользователя = "Операция обработки документа " + ИмяДокумента + " завершена с ошибкой. Обратитесь к администратору системы для получения дополнительной информации по данной проблеме.";
```

Preferred formatting:

```bsl
СообщениеДляПользователя = "Операция обработки документа " + ИмяДокумента
    + " завершена с ошибкой."
    + " Обратитесь к администратору системы для получения дополнительной информации.";
```

## Sources

* Source: [Standard: Modules (RU)](https://its.1c.ru/db/v8std#content:456:hdoc)
* Public mirror: [v8std.ru / #std456](https://v8std.ru/std/456/)
