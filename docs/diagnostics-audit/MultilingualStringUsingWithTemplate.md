# MultilingualStringUsingWithTemplate

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет `НСтр` / `NStr` с недостающими языками, когда строка используется как шаблон в `СтрШаблон` / `StrTemplate`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/multilingual_string_using_with_template.rs`
- `crates/ide-diagnostics/src/utils/nstr.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/MultilingualStringUsingWithTemplate.md`

## Как реализовано

Код почти зеркален `MultilingualStringHasAllDeclaredLanguages`, но с обратным фильтром: diagnostic создается только если `НСтр` находится внутри `СтрШаблон` или присвоен переменной, которая позже используется в `СтрШаблон`.

## Что покрыто

Покрыты inline `НСтр` в `СтрШаблон`, переменная с `НСтр`, использованная позже как шаблон, пустые аргументы и missing languages по `declaredLanguages`.

## Пробелы и ограничения

Есть дублирование с соседней диагностикой. Поиск использования переменной в шаблоне синтаксический и может не увидеть передачу через параметры, поля, коллекции или межпроцедурный поток.

## Может ли инфраструктура улучшить качество

Да. Общий NStr/template analyzer с dataflow usage classification уберет дублирование и даст точнее разделение severity.

## Возможное объединение

Главный кандидат на объединение - `MultilingualStringHasAllDeclaredLanguages`. Разделение по severity понятно, но реализацию лучше объединить уже сейчас; публичные коды можно оставить как два результата одного анализа.

## Вывод

Смысл правила оправдан, но текущая реализация явно просит общего движка для `НСтр` и `СтрШаблон`.
