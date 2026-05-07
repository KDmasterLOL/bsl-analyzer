# EmptyCodeBlock

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Пустые блоки `Если`, `ИначеЕсли`, `Иначе`, циклов и похожих конструкций обычно
означают незавершенный или ошибочный код.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/empty_code_block.rs`
- `crates/hir-def/src/body/lower/stmt.rs`
- `crates/ide-diagnostics/docs/ru/EmptyCodeBlock.md`
- `docs/legal/diagnostics/EmptyCodeBlock.md`

## Как реализовано

Диагностика эмитится во время lowering statement blocks. Handler только
преобразует `BodyDiagnostic::EmptyCodeBlock` в diagnostic с сообщением
`Пустой блок кода`.

## Что покрыто

Тесты проверяют empty `if`, `else`, `elseif`, `while`, а также что пустые тела
процедур/функций и пустой `except` этим правилом не ловятся.

## Пробелы и ограничения

- Комментарии внутри блока не делают блок непустым; это правильно для текущей
  политики, но нет настройки для `TODO`/`// intentionally empty`.
- Разные причины пустоты получают один message.
- Нет quick-fix удаления ветки/блока.

## Может ли инфраструктура улучшить качество

Можно добавить общий block-content classifier: code/comment/todo/directive, и
единый механизм intentional-empty suppression.

## Возможное объединение

Внутренне близко к `EmptyRegion` и `EmptyStatement`; пустые `try`/`except`
сейчас никем не ловятся, это смежный пробел. Внешний код стоит оставить
отдельным из-за разного fix-плана.

## Вывод

Базовое покрытие хорошее. Следующий шаг - intentional-empty policy и более
точные сообщения по типу блока.

