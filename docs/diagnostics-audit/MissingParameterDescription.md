# MissingParameterDescription

Статус: `done`, `needs-code-work`
Дата разбора: 2026-05-07

## Суть правила

Проверяет соответствие описания параметров сигнатуре метода: отсутствующие, лишние, дублирующиеся и неправильно упорядоченные описания.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/missing_parameter_description.rs`
- `crates/ide-diagnostics/src/runner.rs`
- `<v8std mirror>/docs/diagnostics/bslls/MissingParameterDescription.md`
- `<v8std mirror>/docs/std/453.md`

## Как реализовано

Для процедур и функций берутся `module_data`, `item_tree` и `ctx.method_docs(method_id)`. Hyperlink docs пропускаются. Если у экспортного метода есть параметры, но нет param docs, создается общий diagnostic. Если docs есть, строится map по именам, проверяются отсутствующие, лишние, дубликаты и порядок.

## Что покрыто

Покрыты экспортные методы без описания параметров, конкретные пропущенные параметры, лишние описания, дубли и порядок описаний. Есть исключение `is_single_parameter_legacy_type_only_doc`: одиночный параметр с описанием в виде dotted type reference (`СправочникСсылка.X`) пропускается.

## Пробелы и ограничения

Метод без комментария целиком не дублируется здесь, потому что это зона `PublicMethodsDescription`. У неэкспортного метода без param docs diagnostic не создается, но некорректные существующие docs проверяются. Нет quick fix для генерации секции параметров.

## Может ли инфраструктура улучшить качество

Да. Общий docs model уже есть, но нужен генератор документации из сигнатуры и более устойчивое связывание `MethodId` с item tree.

## Возможное объединение

Близко к `MissingReturnedValueDescription`, `MissingVariablesDescription`, `PublicMethodsDescription`. Логично иметь общий documentation diagnostics engine, но публичные коды оставить раздельными.

## Вывод

Правило покрывает больше, чем следует из названия: не только missing, но и extra/order/duplicate. Это стоит отразить в пользовательской документации.

## Закрыто Track 2

**Phase B §5.3 (commit `bbaf2bde`, 2026-05):** добавлен strict-mode
content-quality knob — handler теперь проверяет не только presence
описания, но и пустоту/whitespace-only. Phase B §5.1 (Slice A,
`55ce9dc0`) — общий `MethodDocs`/`VariableDocs` parser, source
которого этот handler потребляет.
