# MissingTempStorageDeletion

Статус: `done`, `needs-code-work`
Track 1 closure: Q-α `c1330185`, Q-β-1 `2366e1fb` — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.
Дата разбора: 2026-05-07

## Суть правила

Проверяет, что после `ПолучитьИзВременногоХранилища` / `GetFromTempStorage` есть последующее удаление через `УдалитьИзВременногоХранилища` / `DeleteFromTempStorage`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/missing_temp_storage_deletion.rs`
- `crates/ide-diagnostics/src/runner.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/MissingTempStorageDeletion.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/487.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/642.md`

## Как реализовано

HIR-обход по всем телам и module-level body. Сначала собираются вызовы получения с первым аргументом, затем вызовы удаления. Для каждого получения ищется удаление позже по тексту с тем же аргументом через structural equality HIR expressions. В отличие от `MissingTemporaryFileDeletion`, CFG не используется — сравнение чисто текстовое по `range.start()`.

## Что покрыто

Покрыты простые переменные, member access вроде `Результат.АдресРезультата`, literals, method calls и index expressions при сравнении аргументов. Диагностика выключена по умолчанию.

## Пробелы и ограничения

Нет CFG: “удаление позже” не означает, что оно выполнится на всех путях, и наоборот условные ветки могут давать ложную уверенность. Нет tracking адреса через присваивания и параметры вспомогательных методов.

## Может ли инфраструктура улучшить качество

Да. Нужен path-sensitive resource lifetime analysis, похожий на временные файлы, но с alias tracking для адресов временного хранилища.

## Возможное объединение

Близко к `MissingTemporaryFileDeletion`: обе диагностики про resource cleanup. Можно объединить общий resource lifecycle framework, оставив разные источники/методы и severity.

## Вывод

Правило полезное, но сейчас менее строгое, чем нужно для resource lifecycle: оно проверяет порядок в теле, а не обязательность удаления на всех путях.
