# FileSystemAccess

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Доступ к файловой системе требует security review. Основание - `#std542`,
связанный `#std774` и безопасный режим.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/file_system_access.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/hir-def/src/body/lower/diagnostics.rs`
- `crates/ide-diagnostics/docs/ru/FileSystemAccess.md`
- `docs/legal/diagnostics/FileSystemAccess.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/542.md`

## Как реализовано

HIR lowering ловит `New` expressions файловых типов и глобальные методы работы
с файлами/каталогами/расширениями. Diagnostic disabled by default как audit
tool.

## Что покрыто

Тесты покрывают constructor types, глобальные file methods, server annotations,
русские/английские варианты, case-insensitive matching и standard type negative.

## Пробелы и ограничения

- Список API зашит в коде, отдельно от `ExternalAppStarting` и
  `InternetAccess`.
- Перекрытие с `TempFilesDir`: `КаталогВременныхФайлов`/`TempFilesDir`
  попадает и в `is_file_system_method`, и в `is_temp_files_dir` — диагностика
  выдаётся дважды на одну точку.
- Нет анализа путей/источников данных/allowlist.
- Qualified object methods покрываются не полностью без type information.
- Message на английском в русской документации.

## Может ли инфраструктура улучшить качество

Общий security API registry и optional policy config: allowed directories,
temporary storage, client/server context, argument taint.

## Возможное объединение

Внутренне объединить с security audit diagnostics. Внешний код оставить
отдельным из-за отдельной категории риска.

## Вывод

Хороший audit detector, но не vulnerability proof. Нужны registry и
arg/context analysis.

