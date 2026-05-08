# CommonModuleInvalidType

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Общий модуль должен соответствовать одному из четырех стандартных сочетаний
флагов контекста выполнения: серверный, вызов сервера, клиентский или
клиент-серверный. Правило следует из `#std469`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/common_module_invalid_type.rs`
- `crates/ide-diagnostics/src/common_module_helpers.rs`
- `crates/ide-diagnostics/docs/ru/CommonModuleInvalidType.md`,
  `crates/ide-diagnostics/docs/en/CommonModuleInvalidType.md`
- `docs/legal/diagnostics/CommonModuleInvalidType.md`
- `<v8std mirror>/docs/std/469.md`,
  `<v8std mirror>/docs/diagnostics/bslls/CommonModuleInvalidType.md`

## Как реализовано

Handler работает по `ModuleMetadata`, берет `metadata.common_module` и
проверяет флаги через `is_server`, `is_server_call`, `is_client`,
`is_client_server`. Diagnostic ставится на весь модуль.

## Что покрыто

Есть синтетические metadata-тесты для недопустимой комбинации, валидного
серверного модуля и non-common module. Проверки не требуют AST.

## Пробелы и ограничения

- Матрица валидности завязана на `ordinary_app_support`; нужно больше тестов
  на обе конфигурации этого флага.
- Серверный тип сейчас требует `external_connection = true`. Это совпадает с
  таблицей `#std469`, но стандарт допускает исключения в отдельных случаях для
  клиентских признаков; для серверных исключения явно не моделируются.
- Нет end-to-end теста с реальной выгрузкой metadata и URI модуля.
- Сообщение не показывает, какая именно комбинация флагов найдена и какие
  флаги нужно изменить.

## Инфраструктурные улучшения

Сделать табличное описание типов общего модуля: имя типа, predicate, ожидаемые
флаги, текст подсказки и тестовые кейсы. Тогда `CommonModuleInvalidType` и все
`CommonModuleName*` будут использовать один источник правды.

## Возможное объединение

Сливать внешний код с name-диагностиками не стоит: здесь ошибка metadata-типа,
а там naming convention. Но внутренне это один `common_module_kind` layer,
который должен вычислять тип модуля один раз.

## Вывод

Правило полезное и уже опирается на metadata. Главный риск - расхождение
локальных predicate'ов с реальной платформенной матрицей и недостаток
end-to-end metadata tests.

