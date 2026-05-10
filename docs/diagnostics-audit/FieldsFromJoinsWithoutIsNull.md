# FieldsFromJoinsWithoutIsNull

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Поля из `LEFT`/`RIGHT`/`FULL JOIN` могут быть `NULL`; их нужно защищать через
`ЕСТЬNULL`, `ЕСТЬ NULL`/`ЕСТЬ НЕ NULL` или менять join semantics.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/fields_from_joins_without_is_null.rs`
- `crates/sdbl-hir` diagnostics
- `crates/ide-diagnostics/docs/ru/FieldsFromJoinsWithoutIsNull.md`
- `docs/legal/diagnostics/FieldsFromJoinsWithoutIsNull.md`

## Как реализовано

SDBL HIR генерирует `SdblDiagnostic::FieldsFromJoinWithoutNullCheck` с join
type и unprotected fields. Handler мапит ranges query -> BSL string range и
создает diagnostic на каждое поле.

## Что покрыто

Тесты покрывают left/right/full joins, `ISNULL`, WHERE protection,
`IS NOT NULL` exemption, inner join negative, несколько полей, поля в ON
других joins (не триггерят), nested LEFT через INNER, и проверку highlight
на поле, а не на `СОЕДИНЕНИЕ`.

## Пробелы и ограничения

- `activated_by_default = false`; нужно сверить ожидания, потому что severity
  critical.
- Правило полагается на точность SDBL HIR alias/nullability analyzer.
- Global WHERE `IS NOT NULL` может быть слишком широким exemption для сложных
  условий.
- Нет quick-fix, потому что выбор default value/domain semantics неочевиден.

## Может ли инфраструктура улучшить качество

Улучшать нужно в SDBL HIR: nullability propagation, alias scopes, join graph,
condition implication. Handler уже тонкий.

## Возможное объединение

Внутренне объединять с query analyzer diagnostics по nullability/performance.
Внешне оставить отдельным, так как это точная semantic query проблема.

## Вывод

Сильная диагностика, но качество полностью зависит от SDBL nullability
анализа. Нужно осторожно с exemptions.


## Закрыто Track 2

**Phase C §4 delta-audit (2026-05):** `IS NULL` / LEFT/FULL OUTER JOIN
семантика покрыта существующей классификацией в `sdbl-hir`; работ Track 2
не требуется. Closed без implementation slice.

## Закрыто Track 3

**Phase C sub-slice C3 (commit `<pending>`, 2026-05):** добавлены
snapshot-fixtures для LEFT/FULL/RIGHT OUTER JOIN classification gaps:

- `track3_full_outer_join_classification_snapshot` — `FULL OUTER JOIN`
  классифицируется как полное соединение и эмитит diagnostics для обеих сторон.
- `track3_left_outer_join_isnull_wrapped_field_snapshot` — поле из
  `ЛЕВОЕ ВНЕШНЕЕ СОЕДИНЕНИЕ`, обернутое в `ЕСТЬNULL`, не диагностируется.
- `track3_right_outer_join_classification_snapshot` — `ПРАВОЕ ВНЕШНЕЕ
  СОЕДИНЕНИЕ` поддерживается и диагностирует nullable-поле левой стороны.

Все fixtures используют `check_diagnostics_snapshot_for`; production changes не
потребовались.
