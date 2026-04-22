# Финальный план: Vault helper + PostgreSQL baseline

## Статус

Этот документ фиксирует целевой контракт и порядок внедрения интеграции
`bsl-analyzer` с `rtools vault setup-db` / `rtools vault credential-helper`.

Документ предназначен как рабочая точка входа для реализации после очистки
контекста.

## Границы решения

- Обратная совместимость не требуется.
- Целевой конфиг для helper-based PostgreSQL integration: только `bsl-analyzer.toml`.
- Identity для Vault credentials задается явно, а не выводится из endpoint.
- Helper возвращает готовый PostgreSQL URL.
- Обычный publish path не делает DDL.
- DDL/bootstrap/migrations живут только в admin-контуре.
- CLI и MCP используют один и тот же credential resolver.
- Helper запускается без shell, только как `program + args`.

## Почему нужен redesign

Текущее состояние имеет четыре системные проблемы:

1. helper contract привязан к `host/port/dbname`, а analyzer сам собирает URL из
   `username/password`: `crates/project-model/src/lib.rs:993-1017`,
   `crates/project-model/src/lib.rs:1104-1182`.
2. обычный write path тянет за собой `ensure_storage()` и, следовательно, DDL:
   `crates/bsl-analyzer/src/bin/main.rs:914-916`,
   `crates/bsl-search/src/publish.rs:273-297`,
   `crates/bsl-search/src/external_baseline/postgres.rs:1190-1215`.
3. MCP/runtime держит долгоживущий source/pool и не перевыпускает credentials до
   пересоздания runtime/process: `crates/mcp-server/src/state.rs:214-260`,
   `crates/bsl-search/src/external_baseline/postgres.rs:101-123`.
4. helper запускается через `sh -c` / `cmd /c`, что оставляет лишний shell-boundary
   в чувствительной credential path: `crates/project-model/src/lib.rs:1054-1068`.

## Зафиксированные решения

### 1. Identity только через `vault_role_base`

`vault_role_base` является единственным source of truth для Vault identity.

`host`, `port` и `dbname` остаются в конфиге как ожидаемые target-параметры и
используются только для:

- проверки helper output;
- операторской диагностики;
- локальной валидации конфигурации.

`host`, `port` и `dbname` не участвуют в выборе Vault role.

### 2. Helper является source of truth для DSN

Analyzer больше не собирает PostgreSQL URL из `username/password`.

Helper возвращает уже готовый URL, а analyzer:

- парсит URL;
- проверяет схему `postgres`;
- проверяет совпадение `host`, `port`, `dbname` с ожидаемыми значениями из TOML;
- передает URL в postgres adapter как готовый runtime credential.

Mismatch между helper URL и ожидаемым target считается terminal
security/config error.

### 3. Жесткое разделение ролей

Используются три логические роли:

- `reader`
- `writer`
- `migrator`

Concrete Vault role name фиксируется межрепозиторно по правилу:

```text
<vault_role_base>:reader
<vault_role_base>:writer
<vault_role_base>:migrator
```

`rtools vault setup-db` обязан создавать именно эти три роли.

`rtools vault credential-helper` обязан резолвить именно по этой схеме, без
локальной магии и без fallback на endpoint-derived identity.

### 4. Publish строго DML-only

Обычный publish path:

- не вызывает `ensure_storage()`;
- не делает `CREATE SCHEMA`, `CREATE TABLE`, `ALTER TABLE`, `CREATE EXTENSION`;
- не чинит пустой storage;
- не лечит schema mismatch.

Если storage не инициализирован, publish падает с явной ошибкой
`storage_not_initialized`.

Если schema несовместима, publish падает с явной ошибкой
`schema_version_mismatch`.

### 5. DDL и maintenance только в admin-контуре

Отдельный admin path отвечает за:

- schema bootstrap;
- migrations;
- extension setup / verification;
- GC;
- будущие repair/admin операции.

Для этого используется только `migrator` role.

### 6. Один resolver для CLI и MCP

Цепочка резолва credentials должна быть одной и той же для всех интерфейсов.

Недопустимо сохранять отдельные flow вида:

- CLI path с одной логикой resolve;
- MCP path с другой логикой resolve и другими fallback.

Один use case должен использоваться из:

- CLI publish;
- CLI inspect;
- CLI admin;
- MCP/runtime.

### 7. Helper запускается без shell

Целевой helper contract использует безопасный запуск процесса:

- `program`
- `args`

Исполнение через shell-string запрещено.

Это решение заменяет текущую модель `sh -c` / `cmd /c`:
`crates/project-model/src/lib.rs:1054-1068`.

## Целевой TOML contract

Предпочтительный контракт:

```toml
[search.baseline]
backend = "postgres"

[search.baseline.postgres]
host = "pg-central.company.com"
port = 5432
dbname = "bsl_search"
schema = "bsl_search"
vault_role_base = "prod/search/bsl-analyzer"

[search.baseline.postgres.credential_helper]
program = "rtools"
args = ["vault", "credential-helper"]
```

### Обязательные поля

Обязательны:

- `host`
- `dbname`
- `schema`
- `vault_role_base`
- `credential_helper.program`

Опциональны:

- `port` с default `5432`
- `credential_helper.args`, если helper допускает пустой список аргументов

### Что не входит в target contract

В целевую helper-based модель не входят:

- `url`
- `url_env`
- `url_file`
- `url_command`
- `credential_helper_windows`
- shell-string `credential_helper`
- `connection_name`

Отдельный config-флаг для migrate path не нужен. Роль выбирается use case-ом,
а не конфигом.

## Целевой helper protocol

### Запрос analyzer -> helper

```json
{
  "protocol": "bsl-analyzer.postgres-helper.v1",
  "action": "resolve-url",
  "mode": "reader",
  "vault_role_base": "prod/search/bsl-analyzer"
}
```

`mode` принимает только значения:

- `reader`
- `writer`
- `migrator`

Analyzer не передает helper-у `host`, `port`, `dbname`.

### Успешный ответ helper -> analyzer

```json
{
  "protocol": "bsl-analyzer.postgres-helper.v1",
  "ok": true,
  "url": "postgres://reader:secret@pg-central.company.com:5432/bsl_search",
  "lease_id": "vault/database/creds/prod-search-reader/abc123",
  "expires_at": "2026-04-06T15:30:00Z",
  "renewable": false
}
```

### Ошибка helper -> analyzer

```json
{
  "protocol": "bsl-analyzer.postgres-helper.v1",
  "ok": false,
  "error": {
    "code": "vault_access_denied",
    "message": "role prod/search/bsl-analyzer:reader is not allowed",
    "retryable": false
  }
}
```

### Правила исполнения

- успех: exit code `0` и `ok = true`;
- отказ helper-а: non-zero exit code и `ok = false`;
- timeout helper-а: typed retryable error;
- malformed JSON: terminal protocol error;
- wrong `protocol`: terminal protocol error.

Analyzer сохраняет в rich error model:

- `mode`
- `vault_role_base`
- `exit_code`
- `stderr`
- `helper_error.code`
- `helper_error.message`
- `retryable`
- `lease_id`
- `expires_at`

При этом URL и секреты не логируются.

## Обязательная analyzer-side validation helper URL

После получения `url` analyzer обязан:

1. распарсить URL;
2. проверить `scheme == postgres`;
3. сравнить `host` с config `host`;
4. сравнить `port` с config `port` или default `5432`;
5. сравнить database name с config `dbname`.

Если хотя бы один параметр не совпадает, возвращается terminal ошибка
`resolved_target_mismatch`.

Это не warning и не soft fallback.

## Семантика ролей

### `reader`

Используется для:

- MCP/runtime reads;
- CLI inspect-команд;
- retention analysis.

`reader` не должен иметь DDL-прав.

### `writer`

Используется только для steady-state публикации:

- publish snapshot metadata;
- publish delta rows;
- publish embeddings;
- rebuild serving rows;
- update heads.

`writer` не должен иметь DDL-прав.

### `migrator`

Используется для:

- schema bootstrap;
- migrations;
- GC;
- repair/admin операций.

`migrator` является единственной ролью, которой разрешены DDL и maintenance.

## Целевой command split

```text
bsl-analyzer search baseline publish ...
bsl-analyzer search baseline inspect ...
bsl-analyzer search baseline admin migrate ...
bsl-analyzer search baseline admin gc ...
```

### `publish`

Новый обычный publish path.

Замещает текущий `sync-pg` и выполняет только DML.

### `inspect`

Под одним префиксом живут read-only команды:

- list snapshots
- show snapshot
- list file objects
- show file object
- list embeddings
- show embedding coverage
- retention

Все они используют `reader`.

### `admin migrate`

Единственная команда, которая:

- создает schema;
- накатывает migrations;
- проверяет schema version;
- проверяет критические prerequisites.

Использует `migrator`.

### `admin gc`

Запускает garbage collection только через `migrator`.

## Контракт для empty storage и schema mismatch

### Empty storage

Если storage не подготовлен:

- `publish` падает с `storage_not_initialized`;
- `inspect` падает с `storage_not_initialized` или показывает явный admin-required state;
- `admin migrate` исправляет ситуацию.

Автопочинка запрещена.

### Schema version mismatch

Если схема несовместима с текущей версией analyzer:

- `publish` падает с `schema_version_mismatch`;
- `inspect` и MCP/runtime считают внешний baseline недоступным и показывают явную
  ошибку `schema_version_mismatch`;
- `admin migrate` является единственной командой, которая лечит mismatch.

## Runtime strategy для MCP

Предпочтительная стратегия MVP: lazy reconnect с re-resolve helper-а.

### Правило

При retryable auth/connectivity failure MCP:

1. инвалидирует текущий reader adapter;
2. заново вызывает helper;
3. пересоздает adapter/pool;
4. повторяет исходную операцию один раз.

### Что считается retryable

Типично retryable:

- timeout helper-а;
- helper error с `retryable = true`;
- PostgreSQL auth/connectivity failure, совместимый с истекшим lease.

### Что считается terminal

Terminal:

- malformed helper response;
- wrong protocol version;
- `resolved_target_mismatch`;
- `vault_access_denied`;
- `storage_not_initialized`;
- `schema_version_mismatch`;
- missing config.

Для terminal ошибок MCP не делает refresh loop.

## Error model

Нужна typed модель ошибок credential resolution и storage access.

Минимальные категории:

- `helper_spawn_failed`
- `helper_timeout`
- `helper_protocol_error`
- `helper_rejected`
- `resolved_target_mismatch`
- `storage_not_initialized`
- `schema_version_mismatch`
- `postgres_connect_failed`
- `postgres_auth_failed`

### Что видит CLI

CLI должен показывать короткую причинную ошибку, например:

```text
failed to resolve PostgreSQL writer credentials: vault_access_denied: role prod/search/bsl-analyzer:writer is not allowed
```

или:

```text
failed to publish baseline: storage_not_initialized; run `bsl-analyzer search baseline admin migrate`
```

### Что уходит в MCP/runtime logs

В logs должны оставаться:

- `mode`
- `vault_role_base`
- `helper_error_code`
- `retryable`
- `lease_id`
- `expires_at`
- `postgres_error_class`

Секреты и полный URL в logs не попадают.

## Чистая архитектура

### Слои

1. config parsing / DTO
2. credential resolution use case
3. helper adapter
4. postgres runtime adapters
5. migration/admin use cases
6. CLI/MCP interface adapters

### Предлагаемая декомпозиция

#### `project-model`

Отвечает только за parsing и validation TOML.

Логично завести:

- `crates/project-model/src/search_baseline_postgres.rs`

Типы:

- `SearchBaselinePostgresConfig`
- `CredentialHelperProgramConfig`

`project-model` не запускает процессы и не резолвит credentials.

#### shared resolver layer

Новый общий слой, например отдельный crate:

- `crates/baseline-pg-access/`

Модули:

- `src/config.rs`
- `src/mode.rs`
- `src/error.rs`
- `src/helper_protocol.rs`
- `src/helper_command.rs`
- `src/resolver.rs`

Типы и интерфейсы:

- `BaselineAccessMode`
- `ResolvedPgConnection`
- `CredentialResolveError`
- `CredentialResolver`
- `HelperCredentialResolver`
- `BaselineConnectionResolver`

Этот слой знает про helper protocol, timeout и target validation.

#### `bsl-search`

Отвечает только за postgres storage operations после получения готового URL.

Логично разрезать:

- `crates/bsl-search/src/external_baseline/postgres/read.rs`
- `crates/bsl-search/src/external_baseline/postgres/write.rs`
- `crates/bsl-search/src/external_baseline/postgres/migrate.rs`
- `crates/bsl-search/src/external_baseline/postgres/schema_version.rs`

Новые интерфейсы:

- `SnapshotPublisher` без `ensure_storage()`;
- `BaselineStorageMigrator`;
- `BaselineMaintenanceAdmin`.

Read path не зависит от migrate path.

#### `bsl-analyzer`

CLI layer.

Логично выделить:

- `crates/bsl-analyzer/src/bin/baseline_cli.rs`
- `crates/bsl-analyzer/src/bin/baseline_resolver.rs`
- `crates/bsl-analyzer/src/bin/baseline_admin.rs`

CLI выбирает mode:

- `reader`
- `writer`
- `migrator`

и конвертирует typed errors в human-readable UX.

#### `mcp-server`

Interface adapter для long-lived runtime.

Логично выделить:

- `crates/mcp-server/src/baseline_runtime.rs`
- `crates/mcp-server/src/baseline_refresh.rs`

Новые типы:

- `RefreshableExternalBaselineSource`
- `ReaderAdapterFactory`
- `BaselineRefreshDecision`

### Допустимые зависимости

Допустимо:

- `project-model` -> DTO only
- `CLI/MCP` -> shared resolver layer
- `CLI/MCP` -> `bsl-search`

Недопустимо:

- `project-model` -> helper process execution
- `bsl-search` -> TOML parsing
- `bsl-search` -> helper protocol
- `mcp-server` -> ad-hoc resolver chain, отличный от CLI

## Порядок внедрения

### Этап 1. Новый config contract в `project-model`

Сделать:

- новый TOML DTO для `[search.baseline.postgres]`;
- nested `credential_helper { program, args }`;
- обязательную валидацию `vault_role_base`, `host`, `dbname`, `schema`;
- удаление старой helper-specific split-модели.

Результат этапа:

- единый конфиг parsed без shell-string helper contract.

### Этап 2. Общий credential resolver

Сделать:

- общий resolver layer;
- JSON helper protocol v1;
- typed error model;
- target validation returned URL;
- единый API `resolve(mode)` для CLI и MCP.

Результат этапа:

- одна точка truth для credential resolution.

### Этап 3. Разделение publish и migrate в `bsl-search`

Сделать:

- убрать `ensure_storage()` из steady-state publish path;
- выделить explicit migrator/admin API;
- ввести `storage_not_initialized`;
- ввести `schema_version_mismatch`;
- отделить read/write/migrate responsibilities.

Результат этапа:

- writer path становится DML-only.

### Этап 4. Новый CLI split в `bsl-analyzer`

Сделать:

- `publish` вместо `sync-pg`;
- `inspect` для read-only команд;
- `admin migrate`;
- `admin gc`;
- перевод всех baseline CLI paths на единый resolver.

Результат этапа:

- роли и команды становятся эксплуатационно очевидными.

### Этап 5. Refreshable runtime в `mcp-server`

Сделать:

- refreshable reader source;
- invalidate + re-resolve + recreate pool;
- single retry для retryable auth/connectivity failure;
- terminal handling для schema/config/protocol errors.

Результат этапа:

- runtime переживает истечение Vault lease без обязательного restart.

## Definition of done

Решение считается внедренным, когда одновременно выполняются условия:

- helper запускается только через `program + args`;
- analyzer не собирает URL из `username/password`;
- `vault_role_base` является единственным identity input;
- publish path не делает DDL;
- существует отдельный `admin migrate`;
- GC идет через `migrator`;
- CLI и MCP используют один resolver;
- analyzer валидирует helper URL against expected `host/port/dbname`;
- empty storage дает `storage_not_initialized`, без автопочинки;
- schema mismatch дает явную ошибку и лечится только через migrate;
- MCP умеет сделать один lazy reconnect при retryable lease-related failure.

## Короткий implementation checklist по репозиториям

### `project-model`

- новый TOML contract
- nested helper config
- обязательная config validation
- удаление shell-string helper contract

### `baseline-pg-access` или эквивалентный shared layer

- mode enum
- helper protocol types
- helper command runner
- typed resolver errors
- target validation

### `bsl-search`

- migrator/admin interfaces
- publish без `ensure_storage()`
- schema version checks
- `storage_not_initialized`
- `schema_version_mismatch`

### `bsl-analyzer`

- новый command split
- publish uses writer
- inspect uses reader
- admin migrate/gc use migrator
- CLI error UX

### `mcp-server`

- refreshable reader source
- reconnect-on-retryable-failure
- structured runtime logging
- explicit surfacing of terminal schema/config errors
