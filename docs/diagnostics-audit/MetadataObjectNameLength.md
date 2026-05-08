# MetadataObjectNameLength

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Контролирует максимальную длину имен объектов метаданных. По умолчанию лимит равен 80 символам.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/metadata_object_name_length.rs`
- `crates/ide-diagnostics/src/runner.rs`
- `<v8std mirror>/docs/diagnostics/bslls/index.md`

## Как реализовано

Диагностика читает metadata model и проверяет имена common modules, MDO и регистров. Конфиг `maxMetadataObjectNameLength`. Есть `check_session_module` для проверки объектов без модулей через всю Configuration, но в `metadata_dispatch` / `lib.rs` он не подключён — фактически dead code.

## Что покрыто

Покрыты длинные имена объектов метаданных и конфигурируемый лимит. Тесты проверяют включение/отключение и изменение порога.

## Пробелы и ограничения

Диапазон обычно привязан к модулю или синтетической позиции, а не к точному XML-атрибуту имени. Нет rename fix и нет проверки последствий переименования.

## Может ли инфраструктура улучшить качество

Да. Нужны source ranges для metadata XML и общий rename-capable слой для метаданных. Без этого диагностика остается проектным предупреждением с грубым местом.

## Возможное объединение

Близко к `ForbiddenMetadataName`, `SameMetadataObjectAndChildNames`, `CommonModuleName*`. Можно объединять name policy infrastructure, но публичные коды лучше оставить отдельными.

## Вывод

Правило полезно как project-level signal, но для хорошего UX нужны точные диапазоны в метаданных и безопасные rename-инструменты.
