# Cached public methods (CachedPublic)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Common modules with return-value reuse enabled should not expose their
programming interface directly.

The compatibility problem is not caching by itself, but coupling consumers to a
specialized implementation module such as `...ПовтИсп`. If the library later
needs to move the exported function to an ordinary common module, every call
site must be updated.

The safer pattern is:

- keep the public API in an ordinary common module;
- use the cached module as an internal helper;
- leave consumers dependent on a stable interface rather than on the caching
  strategy.

## Examples

### Incorrect

```bsl
// Cached common module
#Область ПрограммныйИнтерфейс

Функция ПараметрыСеанса() Экспорт
    Возврат СобратьПараметры();
КонецФункции

#КонецОбласти
```

### Correct

```bsl
// Ordinary common module
#Область ПрограммныйИнтерфейс

Функция ПараметрыСеанса() Экспорт
    Возврат ПараметрыСеансаПовтИсп.ВнутренниеПараметрыСеанса();
КонецФункции

#КонецОбласти
```

## Sources

Primary source: [Standard: Ensuring Library Compatibility (RU)](https://its.1c.ru/db/v8std#content:644:hdoc:3.6)

Secondary source: [v8std.ru: #std644 Ensuring Library Compatibility](https://v8std.ru/std/644/)
