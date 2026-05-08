# Typo

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Ищет опечатки в идентификаторах и строковых литералах с помощью Hunspell-словарей.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/typo.rs`
- `<v8std mirror>/docs/diagnostics/bslls/Typo.md`

## Как реализовано

Использует встроенные `en_US`/`ru_RU` словари, большие списки исключений, конфиг `minWordLength`, `userWordsToIgnore`, `caseInsensitive`. Диагностика выключена по умолчанию из-за false positives.

## Что покрыто

Покрыты слова из identifiers/strings, пользовательские исключения, минимальная длина и регистрозависимость.

## Пробелы и ограничения

Hunspell на коде шумит: технические термины, сокращения, доменные слова. Нет project dictionary discovery.

## Может ли инфраструктура улучшить качество

Да. Нужны workspace/domain dictionaries, suppression comments и раздельные правила для comments/strings/identifiers.

## Возможное объединение

Близко к `LatinAndCyrillicSymbolInWord`, `YoLetterUsage`, `BadWords`. Общий spelling/lexical hygiene layer нужен.

## Вывод

Правило потенциально полезно, но правильно выключено по умолчанию до улучшения словарей и suppression UX.
