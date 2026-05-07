# MultilingualStringHasAllDeclaredLanguages

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет, что вызовы `НСтр` / `NStr` содержат строки для всех языков из `declaredLanguages`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/multilingual_string_has_all_declared_languages.rs`
- `crates/ide-diagnostics/src/utils/nstr.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/MultilingualStringHasAllDeclaredLanguages.md`

## Как реализовано

AST-обход по `IDENT` с именем `НСтр` / `NStr`. Первый аргумент извлекается как literal, затем `extract_language_keys` ищет паттерны `lang='...'` или `lang="..."`. Если `НСтр` находится в `СтрШаблон` или присвоен переменной, которая позже используется в `СтрШаблон`, это правило пропускает случай.

## Что покрыто

Покрыты пустой `НСтр`, отсутствующие языки, многострочные строки и конфиг `declaredLanguages`. Шаблонные случаи отданы отдельной диагностике.

## Пробелы и ограничения

Определение использования переменной в `СтрШаблон` синтаксическое и локальное. Извлечение языков - простой parser, не полноценный grammar `НСтр`. Нет fix для добавления недостающих языков.

## Может ли инфраструктура улучшить качество

Да. Нужен полноценный parser NStr-content и dataflow использования переменных в шаблонах. Для fix нужен генератор недостающих языковых сегментов.

## Возможное объединение

Очень близко к `MultilingualStringUsingWithTemplate`: сейчас это две стороны одного правила, разделенные по контексту использования. Можно рассмотреть один internal analyzer с двумя public diagnostics или даже единый public code с разной severity по контексту.

## Вывод

Правило покрывает базовый localization контроль, но разделение с template-версией создает дублирование кода и требует общего NStr analyzer.
