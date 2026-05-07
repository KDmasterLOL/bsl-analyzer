# DisableSafeMode

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Отключение или ослабление безопасного режима создает security risk.
Документация связывает правило со стандартами `#std669`, `#std678`, `#std770`
и safe-mode API.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/disable_safe_mode.rs`
- `crates/hir-def/src/body/lower/expr.rs`
- `crates/ide-diagnostics/docs/ru/DisableSafeMode.md`
- `docs/legal/diagnostics/DisableSafeMode.md`

## Как реализовано

HIR lowering запоминает safe-mode method name, понижает args и считает
безопасными только `УстановитьБезопасныйРежим(Истина)` /
`SetSafeMode(True)` и `УстановитьОтключениеБезопасногоРежима(Ложь)` /
`SetSafeModeDisabled(False)`. Все остальные literals/variables/no args дают
diagnostic.

## Что покрыто

Тесты покрывают safe/unsafe literals, variables, оба метода, object method
exclusion, bilingual names, case-insensitive variants и all-patterns fixture.

## Пробелы и ограничения

- Variables всегда считаются unsafe, даже если в пределах метода очевидно
  присвоено безопасное значение.
- Нет context analysis: временное отключение с последующим включением все равно
  diagnostic, и это правильно как hotspot, но message не различает severity.
- Нет quick-fix, кроме очевидных literal replacements.
- Не объединено с `SetPrivilegedModeCall` и другими security-hotspot rules.

## Инфраструктурные улучшения

Добавить constant propagation для bool literals и общий security-sensitive API
registry с polarity правилами.

## Возможное объединение

Внутренне объединить с `SetPrivilegedModeCall`, `ExecuteExternalCode`,
`UsingExternalCodeTools` через security API registry. Внешний код оставить
отдельным.

## Вывод

Покрытие хорошее. Улучшения - constant propagation и общий registry
безопасностных API.

