# MissedRequiredParameter

Статус: `done`, `needs-code-work`
Track 1 closure: G1 `27fb95ec`, G2 `1e5230fd` — см. `docs/diagnostics-audit/TRACK_1_CLOSURE.md`.
Дата разбора: 2026-05-07

## Суть правила

Находит вызовы, где обязательный параметр метода пропущен, например через пустой аргумент между запятыми.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/missed_required_parameter.rs`
- `crates/ide-diagnostics/src/hir_dispatch.rs`
- `crates/ide-diagnostics/src/hir_inference_dispatch.rs` (CommonModule вариант через `MissedRequiredParameterCommonModule`)
- `<v8std mirror>/docs/diagnostics/bslls/MissedRequiredParameter.md`
- `<v8std mirror>/docs/std/640.md`

## Как реализовано

Handler получает из HIR имя callee, опциональный модуль, MDO type/name и массив `args: &[bool]`, где видно, какие позиции заполнены. Дальше он резолвит локальный метод, общий модуль или manager module и сравнивает пропуски с обязательностью параметров.

## Что покрыто

Покрыты локальные вызовы, `CommonModule.Method()`, трехуровневые вызовы менеджеров метаданных и `ЭтотОбъект.Method()` как локальный вызов. Метод в общем модуле должен быть экспортным.

## Пробелы и ограничения

Если callee не резолвится, диагностика молчит. Выбор сигнатуры для расширений детерминированный, но не моделирует все конфликтные случаи. Нет quick fix для вставки имени/значения параметра.

## Может ли инфраструктура улучшить качество

Да. Чем лучше project-wide symbol index и extension semantics, тем меньше пропусков. Для фикса нужен доступ к expected parameter names и syntax edit для пустой позиции.

## Возможное объединение

Близко к `MismatchedArgCount`: обе диагностики проверяют call arguments. Можно объединить internal validator, который сначала проверяет пропущенные обязательные параметры, затем общее количество.

## Вывод

Правило покрывает важный класс ошибок точнее, чем общий mismatch count, но полностью зависит от разрешения метода и корректной сигнатуры.
