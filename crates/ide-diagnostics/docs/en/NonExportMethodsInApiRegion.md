# Non-export methods in API regions (NonExportMethodsInApiRegion)

## Description

This diagnostic reports non-export procedures and functions placed inside API
regions such as `ПрограммныйИнтерфейс`, `СлужебныйПрограммныйИнтерфейс`,
`Public`, and `Internal`.

According to the standard module structure, these regions are intended for the
module interface. Methods placed there are expected to participate in that
interface, which means non-export methods do not belong in such regions.

The diagnostic also supports an optional `skipAnnotatedMethods` setting for
projects that intentionally keep certain built-in annotated methods in API
regions.

## Examples

Incorrect:

```bsl
#Область ПрограммныйИнтерфейс

Процедура ВнутренняяОбработка()
КонецПроцедуры

#КонецОбласти
```

Correct:

```bsl
#Область ПрограммныйИнтерфейс

Процедура ОбработатьДанные() Экспорт
КонецПроцедуры

#КонецОбласти

#Область СлужебныеПроцедурыИФункции

Процедура ВнутренняяОбработка()
КонецПроцедуры

#КонецОбласти
```

## Sources

- Source: [1C standard: Module structure (#std455)](https://its.1c.ru/db/v8std#content:455:hdoc)
- Secondary reference: [v8std.ru: NonExportMethodsInApiRegion](https://v8std.ru/diagnostics/bslls/NonExportMethodsInApiRegion/)
