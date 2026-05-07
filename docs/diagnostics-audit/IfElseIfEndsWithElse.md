# IfElseIfEndsWithElse

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Цепочка `Если`/`ИначеЕсли` должна завершаться `Иначе`, чтобы явно обработать
оставшиеся случаи.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/if_else_if_ends_with_else.rs`
- `crates/hir-def/src/body/lower/stmt.rs`
- `crates/ide-diagnostics/docs/ru/IfElseIfEndsWithElse.md`
- `docs/legal/diagnostics/IfElseIfEndsWithElse.md`

## Как реализовано

HIR lowering эмитит diagnostic для if-chain, где есть хотя бы один `elsif`, но
нет `else`. Handler создает простой message.

## Что покрыто

Тесты проверяют цепочку без else, с else, простой if без elsif, if/else без
elsif и несколько elsif.

## Пробелы и ограничения

- Правило спорное: не всякая цепочка `ИначеЕсли` обязана иметь catch-all ветку.
- Нет настройки исключений или severity по проекту.
- Нет quick-fix добавления `Иначе`, потому что содержимое нельзя угадать.

## Может ли инфраструктура улучшить качество

Добавить config/suppression и умный message "рассмотрите явную обработку
остальных случаев", чтобы не звучать как абсолютная ошибка.

## Возможное объединение

Внутренне с if-chain analyzer. Внешне оставить, но возможно стоит default-off
или lower severity policy.

## Вывод

Правило полезно как style/safety hint, но может быть шумным. Нужна гибкая
политика включения.

