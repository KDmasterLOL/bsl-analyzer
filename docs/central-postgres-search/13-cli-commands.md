# PostgreSQL Baseline CLI Operations

Для проектов, использующих централизованный search baseline в PostgreSQL, BSL Analyzer предоставляет набор CLI-команд для публикации и управления индексами.

Эта модель позволяет всем разработчикам переиспользовать единый проиндексированный корпус кода и справки (включая lexical и semantic embeddings), вместо того чтобы индексировать проект локально на каждой машине.

## 1. Конфигурация проекта

Централизованный backend задаётся в файле `bsl-analyzer.toml`. В конфигурации определяются политики выбора веток (например, если нет ветки `feature/*`, использовать `develop`), а параметры подключения (`url`, `schema`) могут быть переопределены через переменные окружения.

Пример:

```toml
[search.baseline]
backend = "postgres"

[search.baseline.postgres]
schema = "bsl_search"

[search.baseline.workspaceCode.policy]
publishBranches = ["vendor", "develop"]

[[search.baseline.workspaceCode.policy.branches]]
match = "vendor"
selectBranch = "vendor"

[[search.baseline.workspaceCode.policy.branches]]
match = "develop"
selectBranch = "develop"
fallbackBranch = "vendor"

[[search.baseline.workspaceCode.policy.branches]]
match = "feature/*"
selectBranch = "develop"
fallbackBranch = "vendor"

[[search.baseline.workspaceCode.policy.branches]]
match = "fix/*"
selectBranch = "develop"
fallbackBranch = "vendor"

[[search.baseline.workspaceCode.policy.branches]]
match = "bug/*"
selectBranch = "develop"
fallbackBranch = "vendor"

[[search.baseline.workspaceCode.policy.branches]]
match = "*"
selectBranch = "develop"
fallbackBranch = "vendor"

[search.baseline.reference]
snapshotId = "reference:0.1.104"
```

Проверить, как конфиг будет разрешён на runtime, можно командой:
```bash
bsl-analyzer check-config bsl-analyzer.toml
```
Команда покажет итоговый `snapshot`, `branch`, `commit` и проблемы конфигурации (например, отсутствие `search.baseline.postgres.url`).

## 2. Публикация Baseline (Publishing)

Типовой workflow для CI/CD:

```bash
# 1. Опубликовать baseline develop в PostgreSQL
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline sync-pg \
  --source-dir ./my-project \
  --branch develop \
  --commit "$CI_COMMIT_SHA"

# 2. Обновить baseline vendor, когда приходит новая поставка
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline sync-pg \
  --source-dir ./my-project \
  --branch vendor \
  --commit "$CI_COMMIT_SHA"

# 3. Опубликовать shared reference baseline (глобальная справка)
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline sync-pg \
  --corpus reference
```

`sync-pg` выводит итоговые параметры публикации:
- `Corpus`, `Mode` (`root` или `delta`), `Snapshot`, `Schema`, `Parent`, `Branch`, `Commit`
- Статистика: `Reused files` / `Written files` / `Deleted files`, `Reused chunks` / `Written chunks`

Если `--parent-snapshot-id` не передан, `sync-pg` сам выберет последний опубликованный snapshot в том же `corpus/branch`.

> **Эмбеддинги:** Если при `sync-pg` заданы `EMBEDDING_URL` и `EMBEDDING_MODEL`, команда также публикует shared embeddings в PostgreSQL.

## 3. Чтение и аудит (Read-only commands)

Команды для проверки содержимого shared PostgreSQL storage:

```bash
# Показать последние snapshot'ы
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline list-pg --limit 20

# Отфильтровать по corpus/branch
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline list-pg --corpus workspace-code --branch develop

# Посмотреть один snapshot подробно
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline show-pg --snapshot-id workspace-code:develop@abcdef

# Показать shared file objects
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline list-file-objects-pg --limit 20

# Посмотреть один file object и его ссылки из snapshot'ов
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline show-file-object-pg --file-object-id abcdef

# Показать инвентарь shared embeddings
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline list-embeddings-pg

# Показать покрытие active semantic payload'ов embeddings
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline show-embedding-coverage-pg

# Read-only анализ retention policy для snapshot-ов
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline retention-pg --source-dir ./my-project
```

`list-pg` показывает effective состояние snapshot после применения parent lineage (Количество файлов, чанков, fingerprint и т.д.).

## 4. Очистка и Garbage Collection

Publish использует shared file-object storage. Удалённые файлы фиксируются в `snapshot_deletions`. Старые данные удаляются через сборку мусора:

```bash
# Dry-run очистки мусора (покажет orphan file objects, items, semantic rows)
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline gc-pg

# Реальное удаление orphan-объектов
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline gc-pg --execute
```

## Как это работает на клиенте (MCP Runtime)

После установки MCP проверьте состояние через инструмент `search(action=status)`:
- В `workspace` покажет `Configured baseline`, `Action` (если ветка стала `stale` или `expired`), источники лексического и семантического кэша.
- В `reference` покажет свежесть глобального baseline.

Для централизованного **reference**:
- Lexical (`find_docs`) читается из shared PostgreSQL baseline.
- Semantic (`search_docs`) использует локальный кэш этого snapshot, который синхронизируется при старте без переэмбеддинга (если есть shared embeddings).

Для централизованного **workspace**:
- Lexical `find_code` читает baseline из shared snapshot и накладывает local overlay (изменённые локально файлы).
- Semantic `search_code` использует локальный SQLite кэш поверх shared baseline. Shared embeddings скачиваются как первый источник, локальный embedder работает только как fallback.
- Если политика ветки помечает shared baseline как `expired`, поиск возвращает ошибку `expired_branch`, требуя обновить ветку из `develop` (через `git pull` или `sync-pg`).
