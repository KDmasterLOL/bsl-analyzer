# Глобальный модуль с постфиксом "Клиент" (CommonModuleNameGlobalClient)

## Описание диагностики

Имена глобальных общих модулей формируются с постфиксом "Глобальный" (англ. "Global"). При наличии этого постфикса дополнительно указывать постфикс "Клиент" не требуется, поскольку он является избыточным.

## Примеры

Неправильно:

```
УправлениеПечатьюГлобальныйКлиент
ConfigurationUpdateGlobalClient
```

Правильно:

```
УправлениеПечатьюГлобальный
ConfigurationUpdateGlobal
```

## Источники

- [Стандарт: Правила создания общих модулей, раздел 3.2.1](https://its.1c.ru/db/v8std#content:469:hdoc:3.2.1)
- [v8std: #std469 Правила создания общих модулей](https://v8std.ru/std/469/)
- [v8std: common-module-name-global-client](https://v8std.ru/diagnostics/v8-code-style/common-module-name-global-client/)
