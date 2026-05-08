# UsingHardcodeNetworkAddress

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит захардкоженные IPv4/IPv6 адреса в строковых литералах.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/using_hardcode_network_address.rs`
- `<v8std mirror>/docs/diagnostics/bslls/UsingHardcodeNetworkAddress.md`

## Как реализовано

HIR проходит по строковым литералам, ищет IPv4/IPv6 regex, исключает URL, localhost, популярные версии и контексты по regex-настройкам `searchWordsExclusion` / `searchPopularVersionExclusion`.

## Что покрыто

Покрыты IPv4, IPv6, адреса внутри длинных строк, параметры функций и настраиваемые исключения. Есть защита от версий, namespace/driver контекстов, localhost и части URL.

## Пробелы и ограничения

Regex-подход хрупок: версии, OID, XPath, конфигурационные строки и адреса внутри путей могут давать шум или пропуски. Диагностика не понимает назначение строки и не умеет отличать тестовые данные от production endpoint.

## Может ли инфраструктура улучшить качество

Да. Нужны общий literal-classifier, распознавание URL/URI/UNC/OID, настройка тестовых модулей и возможно taint-контекст для сетевых endpoint.

## Возможное объединение

Сильный кандидат на общий hardcode-анализ вместе с `UsingHardcodePath` и `UsingHardcodeSecretInformation`: один проход по строковым литералам, разные классификаторы и исключения. Пользовательские diagnostics лучше оставить отдельными.

## Вывод

Текущее покрытие практичное, но качество зависит от исключающих regex. Лучше развивать общий классификатор литералов.
