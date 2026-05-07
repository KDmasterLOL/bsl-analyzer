# MissingTemporaryFileDeletion

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет, что временный файл, полученный через `ПолучитьИмяВременногоФайла` / `GetTempFileName`, затем удаляется или перемещается.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/missing_temporary_file_deletion.rs`
- `crates/ide-diagnostics/src/runner.rs`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/MissingTemporaryFileDeletion.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/std/542.md`

## Как реализовано

HIR + CFG. Обработчик проходит как тела методов, так и module-level code (CFG для него строится отдельно). Находит прямое присваивание результата `GetTempFileName` в переменную и ищет reachable вызов удаления/перемещения для этой переменной; regex `searchDeleteFileMethod` якорится `^...$` и матчится на полный путь callee (включая module-qualified пути вида `Module.Метод`). Дефолт включает `УдалитьФайлы`, `НачатьУдалениеФайлов`, `ПереместитьФайл` и английские варианты. Inline usage без присваивания считается ошибкой.

## Что покрыто

Покрыты assigned temp filenames, inline calls, configurable delete/move methods и reachability через CFG.

## Пробелы и ограничения

Alias tracking ограничен: если имя файла передано в другую переменную, коллекцию или вспомогательный метод, проверка может ошибиться. CFG reachability не обязательно означает cleanup на всех exit paths, если нет post-dominator анализа.

## Может ли инфраструктура улучшить качество

Да. Нужен общий resource lifecycle analysis с alias tracking, post-dominators и моделями helper-методов.

## Возможное объединение

Ближайшее правило - `MissingTempStorageDeletion`. Их стоит объединять на уровне framework: source operation, cleanup operation, identity expression, path requirement.

## Вывод

Это более продвинутая resource-cleanup диагностика, чем temp storage, но ей все еще не хватает строгого анализа всех путей и aliases.
