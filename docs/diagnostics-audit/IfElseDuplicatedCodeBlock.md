# IfElseDuplicatedCodeBlock

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Если разные ветки `Если` выполняют одинаковый код, условие теряет смысл или
содержит copy-paste ошибку. Связанный стандартный контекст - `#std440`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/if_else_duplicated_code_block.rs`
- `crates/hir-def/src/body/lower/diagnostics.rs`
- `crates/ide-diagnostics/docs/ru/IfElseDuplicatedCodeBlock.md`
- `docs/legal/diagnostics/IfElseDuplicatedCodeBlock.md`
- `<v8std mirror>/docs/std/440.md`

## Как реализовано

HIR lowering/diagnostics сравнивает statement blocks веток `if`/`elsif`/`else`
по нормализованному тексту (whitespace удаляется, всё приводится к lowercase) и
эмитит diagnostic на конструкцию при дубликате. Empty blocks игнорируются.

## Что покрыто

Тесты проверяют if/else duplicates, different blocks, duplicated elsif,
empty-block negative и multi-statement duplicates.

## Пробелы и ограничения

- Сравнение текстовое (после whitespace+lowercase), без semantic equivalence,
  AST/HIR-структуры и alias/value normalization.
- Нет partial duplicate detection внутри больших блоков.
- Нет quick-fix, потому что нужно безопасно вынести общий код.

## Может ли инфраструктура улучшить качество

Общий block canonicalizer/diff для duplicate code diagnostics, с будущей
поддержкой extract common code.

## Возможное объединение

Внутренне с `IfElseDuplicatedCondition`, `IdenticalExpressions`, duplicate-code
rules. Внешне оставить отдельным.

## Вывод

Покрывает явные copy-paste ветки. Улучшения - partial clone detection и
canonicalization.

