# SpaceAtStartComment

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет наличие пробела после `//` в комментарии.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/space_at_start_comment.rs`
- `<v8std mirror>/docs/diagnostics/bslls/SpaceAtStartComment.md`
- `<v8std mirror>/docs/std/456.md`

## Как реализовано

Token-based обход `COMMENT`. Хорошими считаются `// text`, пустой `//`, separator lines и annotation prefixes `//@`, `//(c)`, `//©`. Есть fix, вставляющий пробел после слешей.

## Что покрыто

Покрыты inline/comments, separator lines, annotations и strict mode для `//// text`.

## Пробелы и ограничения

Конфиги из BSL LS фактически захардкожены (`use_strict=true`, дефолтные annotations). TODO про распознавание закомментированного кода не реализован.

## Может ли инфраструктура улучшить качество

Да. Надо подключить реальные параметры диагностики и общий comment/code recognizer.

## Возможное объединение

Близко к `MissingSpace`, `LineLength`, `OneStatementPerLine`, `CommentedCode`. Форматирующие rules можно объединить вокруг formatter/comment helpers.

## Вывод

Auto-fix уже есть, но конфигурируемость и распознавание закомментированного кода неполные.
