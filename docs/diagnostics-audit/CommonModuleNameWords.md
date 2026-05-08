# CommonModuleNameWords

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Имя общего модуля не должно содержать неинформативные слова вроде
`Процедуры`, `Функции`, `Обработчики`, `Модуль`, `Функциональность`.
Основание - `#std469`, раздел 3.1.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/common_module_name_words.rs`
- `crates/ide-diagnostics/docs/ru/CommonModuleNameWords.md`,
  `crates/ide-diagnostics/docs/en/CommonModuleNameWords.md`
- `docs/legal/diagnostics/CommonModuleNameWords.md`
- `<v8std mirror>/docs/std/469.md`,
  `<v8std mirror>/docs/diagnostics/bslls/CommonModuleNameWords.md`

## Как реализовано

Берется `metadata.common_module.name()`. Список слов читается из настройки
`words` или из `DEFAULT_WORDS`, разделенного `|`. Проверка - case-insensitive
`contains`.

## Что покрыто

Есть тесты на русское запрещенное слово, английское слово, отсутствие слова и
`Процедуры`.

## Пробелы и ограничения

- Настройка называется `words`, но формат фактически не regex, а split by `|`.
- `contains` дает false positives внутри более длинных слов.
- Нет нормализации CamelCase/token boundaries.
- Нет тестов пользовательской конфигурации `words`.
- Нет связки с `ForbiddenMetadataName`, хотя обе диагностики проверяют naming
  vocabulary.

## Инфраструктурные улучшения

Нужен общий tokenizer имен metadata: слова в CamelCase, русские/английские
части, postfix tokens. Он улучшит и `CommonModuleName*`, и metadata naming
rules.

## Возможное объединение

Можно объединять внутренне с common-module name engine. Внешне правило лучше
оставить отдельным: это не missing postfix, а качество доменного имени.

## Вывод

Правило полезное, но текущий substring-подход слишком грубый. Следующий шаг -
token-based matching и тесты кастомной конфигурации.

