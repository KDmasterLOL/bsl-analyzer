# OneStatementPerLine

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Находит несколько операторов в одной строке.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/one_statement_per_line.rs`
- `<v8std mirror>/docs/diagnostics/bslls/OneStatementPerLine.md`
- `<v8std mirror>/docs/std/456.md`

## Как реализовано

Событие формируется в HIR при обнаружении второго и последующих statements на одной строке. Handler создает simple diagnostic.

## Что покрыто

Покрыты последовательные присваивания, statements внутри однострочного `Если`, конец файла и исключение для statements с препроцессором, пустых statements (`EMPTY_STMT`) и узлов с parse error.

## Пробелы и ограничения

Нет fix для переноса на отдельные строки. Диапазон не включает точку с запятой, что ограничивает UX при редактировании.

## Может ли инфраструктура улучшить качество

Да. Форматтер или line-splitting code action может сделать rule auto-fixable.

## Возможное объединение

Близко к `LineLength`, `MissingSpace`, `IncorrectLineBreak`, `SemicolonPresence`. Стоит объединять formatter infrastructure.

## Вывод

Правило хорошо ловит стиль, но должно стать частью форматтера или иметь безопасный fix.
