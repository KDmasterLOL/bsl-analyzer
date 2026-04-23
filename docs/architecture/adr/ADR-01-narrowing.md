# ADR-01 — Сужение типов (narrowing)

**Статус:** принято, реализовано.
**Связано с:** [`TYPE_SYSTEM.md`](../TYPE_SYSTEM.md), [`DATAFLOW.md`](../DATAFLOW.md).

## Контекст

BSL позволяет переменной содержать значения одного из нескольких типов
(`ОписаниеТипов`, XML `Composite`, JSDoc-списки через запятую — лоуверятся
в `Ty::Union`). Без сужения hover внутри `Если ТипЗнч(Х) = Тип("Массив")`
показывает исходный `Union`, а `method_lookup` / `field_lookup` на
union-receiver'e возвращают `None` — union'ы остаются трубой без
полезного сигнала.

## Принятое решение

Сужение — отдельный flow-sensitive анализ поверх CFG, **не** расширение
линейного `InferenceContext`.

### Архитектура

- **Solver:** `hir_ty::narrow::NarrowingAnalysis`. Латтис `Name → Ty`
  поверх инфраструктуры `cfg` + `dataflow`.
- **Branch-awareness:** `dataflow::Transfer::transfer_edge` со знанием
  `cfg::EdgeKind` (True/False на рёбрах условного перехода) — общий
  хук, не narrowing-специфичный, reaching-defs и liveness берут его
  default-impl.
- **Salsa:** отдельный запрос `narrow_query(file_id, owner)`; базовый
  `infer_query` от него не зависит и не пересчитывается при изменении
  overlay'я.
- **Чтение:** `Semantics::type_of_expr` мерджит overlay с базовым
  типом выражения на запросе от IDE.
- **Feature flag:** `type_narrowing` в `bsl-analyzer.toml` (по
  умолчанию включён) — off-switch для регрессий.

### Поддерживаемые формы гардов

- `ТипЗнч(Х) = Тип("…")` — сужение до указанного типа.
- `Х = Неопределено` / `Х <> Неопределено` — сужение через
  `Ty::Undefined`.
- `ЗначениеЗаполнено(Х)` — снятие `Undefined` / `Null` из union'а.

### Поведение

- **Then-ветка:** `Х` получает сужённый тип.
- **Else-ветка / fall-through:** точное `Union \ Narrowed` через
  smart-constructor `ty_difference`; non-Union receiver даёт `Unknown`.
- **Присваивание внутри сужённого блока:** сужение действует до
  точки первого переприсваивания; после неё тип берётся из RHS. Не
  протекает за границы блока (merge-point join с pre-state).
- **Hover на receiver'е гарда** (`Х` в `ТипЗнч(Х)`): **pre-narrow**
  тип. Внутри then / else — post-narrow.
- **`hir::Type::is_assignable_to`:** чистая на `Ty`; narrowing
  попадает в неё через callers, строящих `hir::Type` от
  `Semantics::type_of_expr`.

## Текущие ограничения

- **`ИЛИ`-composition в гардах** (`ТипЗнч(X) = Тип(A) ИЛИ ТипЗнч(X) = Тип(B)`)
  — не сужает.
- **Cross-call narrowing** — сужение не пересекает границы вызовов;
  параметры callee'и всегда видны pre-narrowed.
- **`Попытка` / `Исключение`** — не сужает (требует escape-анализа).
- **`Х Есть Справочник`** — не сужает.
- **Cast-via-assignment** (`Х = Х КАК Строка`) — не сужает.
- **Refinement types** (dependent / index-based) — вне скоупа.
