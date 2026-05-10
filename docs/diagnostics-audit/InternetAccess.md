# InternetAccess

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Создание HTTP/FTP/WebService/Mail объектов означает доступ к внешним ресурсам и
требует security review. Основание - `#std794` и `#std678`.

## Проверенные источники

- `crates/ide-diagnostics/src/handlers/internet_access.rs`
- `crates/ide-diagnostics/docs/ru/InternetAccess.md`
- `docs/legal/diagnostics/InternetAccess.md`
- `<v8std mirror>/docs/std/794.md`
- `<v8std mirror>/docs/std/678.md`

## Как реализовано

HIR pass через `for_each_body()` ищет `Expr::New` с type name из списка
internet access patterns или dynamic constructor `Новый("HTTPСоединение")`.
Diagnostic disabled by default.

## Что покрыто

Тесты покрывают HTTP/FTP/WS/Mail/Proxy constructors, dynamic `Новый("...")`,
module-level code, русские/английские names и negative cases.

## Пробелы и ограничения

- Ловятся только constructors, но не все методы, которые реально делают
  network I/O на уже созданном объекте.
- Нет анализа адресов, протокола HTTP vs HTTPS, allowlist и server/client
  context.
- Message на английском.
- Dynamic constructor покрыт только string literal, без constant propagation.

## Может ли инфраструктура улучшить качество

Security API registry с constructor/method categories, endpoint extraction,
allowlist config и context severity.

## Возможное объединение

Внутренне с `FileSystemAccess`, `ExternalAppStarting`, `ExecuteExternalCode` как
security audit family. Внешний код оставить отдельным для категории
`external resources`.

## Вывод

Хороший disabled-by-default audit rule, но для полноценной проверки внешних
ресурсов нужен endpoint/method analysis.


## Закрыто Track 2

**Phase A §1.6 Group A (commit `4a9a9290`, 2026-05):** локальный whitelist
internet-API заменён на `bsl_platform::security::registry` lookup
(`Category::Internet`). Endpoint/method-analysis (см. «Вывод») — Track 6.
