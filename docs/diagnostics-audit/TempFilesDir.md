# TempFilesDir

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Предупреждает о прямом вызове `КаталогВременныхФайлов()` / `TempFilesDir()`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/temp_files_dir.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/TempFilesDir.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/542.md`

## Как реализовано

HIR находит неквалифицированный глобальный вызов; handler различает русское/английское имя для сообщения.

## Что покрыто

Покрыты русское/английское имя и case-insensitive вызовы. Квалифицированный `Модуль.КаталогВременныхФайлов()` не срабатывает.

## Пробелы и ограничения

Английское сообщение не локализовано. Нет анализа, используется ли результат безопасно.

## Может ли инфраструктура улучшить качество

Да. Связать с `MissingTemporaryFileDeletion` и file-resource lifecycle, чтобы проверять полный сценарий работы с временными файлами.

## Возможное объединение

Близко к `MissingTemporaryFileDeletion`, `FileSystemAccess`, `UsingHardcodePath`. Можно объединить file-system/resource diagnostics.

## Вывод

Правило ловит прямой вызов, но само по себе не оценивает весь риск работы с временным файлом.
