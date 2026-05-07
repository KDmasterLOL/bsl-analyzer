# ConsecutiveEmptyLines

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Ограничивает число подряд идущих пустых строк. Локальная документация
ссылается на v8-code-style `module-consecutive-blank-lines`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/consecutive_empty_lines.rs`
- `crates/ide-diagnostics/docs/ru/ConsecutiveEmptyLines.md`
- `docs/legal/diagnostics/ConsecutiveEmptyLines.md`
- `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/v8-code-style/module-consecutive-blank-lines.md`

## Как реализовано

Handler читает `allowedEmptyLinesCount` (default `1`), проходит файл по
`LineIndex` и создает diagnostic на группу пустых строк, если их больше
лимита.

## Что покрыто

Тесты проверяют пустой файл, одну/две/три пустые строки, строки из пробелов,
несколько групп и границу default threshold.

## Пробелы и ограничения

- Диапазон diagnostic заканчивается на start последней пустой строки, а не
  включает весь текст последней строки; для фикса удаления это может быть
  неудобно.
- Нет тестов custom config.
- Нет quick-fix "сжать до N пустых строк".
- Не учитывается, что в конце файла trailing empty lines могут попадать под
  другие formatter rules.

## Инфраструктурные улучшения

Добавить line-edit quick-fix и общий whitespace scanner для `MissingSpace`,
`IncorrectLineBreak`, `LineLength`, `OneStatementPerLine`.

## Возможное объединение

Сливать с другими whitespace diagnostics в один внешний код не нужно. Но общий
formatter/whitespace слой уменьшит дублирование и даст безопасные фиксы.

## Вывод

Правило простое и покрыто базово. Главный недочет - нет автоправки и тестов
на конфиг.

