# CLI-операции для baseline в PostgreSQL

Для централизованного поиска BSL Analyzer предоставляет набор команд под префиксом:

```bash
bsl-analyzer search baseline ...
```

## Конфигурация проекта

Минимальный TOML-пример для PostgreSQL-бэкенда:

```toml
[search.baseline]
backend = "postgres"

[search.baseline.postgres]
schema = "bsl_search"
url_env = "BSL_SEARCH_BASELINE_PG_URL"

[search.baseline.workspace_code.policy]
publish_branches = ["vendor", "develop"]

[[search.baseline.workspace_code.policy.branches]]
match = "vendor"
select_branch = "vendor"

[[search.baseline.workspace_code.policy.branches]]
match = "develop"
select_branch = "develop"
fallback_branch = "vendor"

[[search.baseline.workspace_code.policy.branches]]
match = "feature/*"
select_branch = "develop"
fallback_branch = "vendor"

[[search.baseline.workspace_code.policy.branches]]
match = "*"
select_branch = "develop"
fallback_branch = "vendor"

[search.baseline.reference]
snapshot_id = "reference:0.1.104"
```

Общая структура конфигурации проекта описана в
`docs/configuration/PROJECT_CONFIGURATION.md`. Здесь остаются только примеры,
связанные с search baseline.

## Публикация снимков

### `sync-pg`

Публикует snapshot в PostgreSQL.

```bash
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline sync-pg \
  --source-dir ./my-project \
  --branch develop \
  --commit "$CI_COMMIT_SHA"
```

Для справки платформы:

```bash
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline sync-pg \
  --corpus reference
```

Поддерживаемые полезные флаги:

- `--snapshot-id`
- `--branch`
- `--commit`
- `--parent-snapshot-id`
- `--allow-non-policy-branch`
- `--pg-url`
- `--pg-schema`

## Просмотр и аудит

### `list-pg`

Показать опубликованные snapshot'ы:

```bash
bsl-analyzer search baseline list-pg --limit 20
```

### `show-pg`

Показать один snapshot:

```bash
bsl-analyzer search baseline show-pg --snapshot-id workspace-code:develop@abcdef
```

### `list-file-objects-pg`

Показать общие file objects:

```bash
bsl-analyzer search baseline list-file-objects-pg --limit 20
```

### `show-file-object-pg`

Показать один file object:

```bash
bsl-analyzer search baseline show-file-object-pg --file-object-id abcdef
```

### `list-embeddings-pg`

Показать список эмбеддингов:

```bash
bsl-analyzer search baseline list-embeddings-pg
```

### `show-embedding-coverage-pg`

Показать покрытие embeddings:

```bash
bsl-analyzer search baseline show-embedding-coverage-pg
```

## Retention и GC

### `retention-pg`

Проверить retention policy:

```bash
bsl-analyzer search baseline retention-pg --source-dir ./my-project
```

### `gc-pg`

Сначала dry-run:

```bash
bsl-analyzer search baseline gc-pg
```

Затем реальное удаление:

```bash
bsl-analyzer search baseline gc-pg --execute
```

## Проверка конфигурации

Проверить, как конфиг разбирается и какой baseline будет выбран, можно так:

```bash
bsl-analyzer check-config --config ./bsl-analyzer.toml
```

Команда работает и для legacy JSON-конфига, если он ещё используется в проекте.
В выводе будет краткая сводка по:

- `source.root` и подключённым extensions;
- diagnostics / code lens / formatting;
- отключённым, явно включённым и параметризованным диагностическим правилам;
- workspace/reference baseline selection.

## Что смотреть в MCP-контуре

После настройки baseline полезно проверить `search(action=status)`:

- какой backend выбран;
- какой snapshot разрешился;
- не находится ли ветка в состоянии `stale` или `expired`;
- какие полнотекстовые и семантические источники реально используются;
- насколько готов прогрев семантического индекса и overlay.
