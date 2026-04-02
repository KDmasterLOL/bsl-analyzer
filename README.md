# BSL Analyzer

Высокопроизводительный Language Server для языка BSL (1С:Предприятие), написанный на Rust.

## Возможности

- **180+ диагностик** качества кода BSL
- **LSP** — поддержка Language Server Protocol для IDE
- **MCP** — встроенный сервер Model Context Protocol для AI-агентов
- **SonarQube** — отчёты SARIF, потоковый режим для крупных проектов
- **Совместимость** с форматом конфигурации `.bsl-language-server.json`
- **Кроссплатформенность** — Linux, Windows, macOS (Apple Silicon)

## Установка

### Linux / macOS

<!-- INSTALL_URL:gitlab -->
```bash
curl -fsSL https://dev.runsystems.ru/releases/static/install.sh | bash
```

Или с указанием версии:

```bash
curl -fsSL https://dev.runsystems.ru/releases/static/install.sh | bash -s -- --version 0.1.38
```
<!-- /INSTALL_URL -->

### Windows (PowerShell)

```powershell
Invoke-WebRequest "https://github.com/itrous/bsl-analyzer/releases/latest/download/bsl-analyzer-windows-amd64.exe" -OutFile bsl-analyzer.exe
```

## Использование

### LSP-сервер

```bash
bsl-analyzer lsp
```

### Анализ (SonarQube)

```bash
# Консольный вывод
bsl-analyzer analyze -s ./my-project

# SARIF-отчёт
bsl-analyzer analyze -s ./my-project -r sarif -o ./reports

# JSONL-вывод (для SonarQube)
bsl-analyzer analyze -s ./my-project --format=jsonl > report.jsonl
```

### MCP-сервер

BSL Analyzer включает встроенный [MCP-сервер](https://modelcontextprotocol.io/) (Model Context Protocol). Теперь он разделён на два явных профиля:

- `reference` — глобальная справка платформы и ИТС, устанавливается один раз в `user scope`
- `workspace` — работа с конкретным проектом и конкретной базой, устанавливается отдельно в каждый проект

> **Внимание:** MCP-сервер с подключением к 1С предоставляет полный доступ к базе данных, включая выполнение произвольного кода и запросов. Используйте только в контуре разработки или тестирования. Подключение к продуктивной базе крайне не рекомендуется.

```bash
bsl-analyzer mcp serve --profile reference
```

```bash
bsl-analyzer mcp serve --profile workspace --source-dir ./my-project
```

Автоустановка MCP-конфига в AI-инструменты:

```bash
# Рекомендуемый сценарий: одной командой установить
# global reference + project workspace
bsl-analyzer mcp install \
  --target all \
  --preset recommended \
  --source-dir ./my-project \
  --env NAPARNIK_TOKEN=your_token \
  --env EMBEDDING_URL=http://localhost:8000/v1/embeddings

# Установить глобальный MCP справки в Codex / Gemini / Claude / Cursor
bsl-analyzer mcp install \
  --target all \
  --preset reference \
  --scope user \
  --env NAPARNIK_TOKEN=your_token \
  --env EMBEDDING_URL=http://localhost:8000/v1/embeddings

# Установить project-scoped MCP workspace для конкретного проекта
bsl-analyzer mcp install \
  --target all \
  --preset workspace \
  --scope project \
  --source-dir ./my-project

# Посмотреть, что будет записано, без изменений на диске
bsl-analyzer mcp install --target codex --preset reference --scope user --dry-run

# Обновить существующую запись MCP с тем же именем
bsl-analyzer mcp install \
  --target cursor \
  --preset workspace \
  --scope project \
  --source-dir ./my-project \
  --force
```

Профили MCP:

| Preset | Scope | Инструменты |
|--------|-------|-------------|
| `reference` | `user` | `search(find_docs/search_docs/status)`, `syntax_help`, `its_help` |
| `workspace` | `project` | `metadata`, `search(find_code/search_code/status)`, `query`, `execute`, `debug` |

`recommended` в `mcp install` разворачивается в две установки:

- `reference` в `user scope`
- `workspace` в `project scope`

Если передать `--name custom-bsl`, будут созданы серверы:

- `custom-bsl-reference`
- `custom-bsl-workspace`

Поддерживаемые targets:

| Target | Способ установки |
|--------|------------------|
| `codex` | `user` через `codex mcp add`, `project` через merge в `.codex/config.toml` |
| `gemini` | через `gemini mcp add` |
| `claude` | через `claude mcp add` |
| `cursor` | через merge в `~/.cursor/mcp.json` или `.cursor/mcp.json` |

> **Примечание:** если передать `--onec-password`, пароль будет сохранён в конфиге целевого инструмента как аргумент запуска MCP. `--dry-run` показывает итоговый CLI-вызов или файл конфигурации до записи.

Типовые ошибки установки:

- `server '...' already exists ...`
  Перезапустите команду с `--force`, если хотите обновить существующую запись.
- `failed to run 'codex': binary not found in PATH`
  Установите соответствующий CLI (`codex`, `gemini` или `claude`) и проверьте, что он доступен в `PATH`.
- `failed to parse ... config file`
  Целевой конфиг уже существует, но содержит некорректный JSON/TOML. Исправьте файл и повторите установку.
- `preset 'workspace' does not support 'user' scope`
  Используйте `workspace + project` или `reference + user`.

Если в конфиге уже есть старая single-server запись с именем `bsl-analyzer`, установка с `--force` автоматически мигрирует её в новую split-модель с отдельными `reference` и `workspace` серверами.

Если для проекта используется централизованный search baseline в PostgreSQL, backend задаётся в `.bsl-analyzer.json`, а env остаётся только для секретов и override:

```json
{
  "search": {
    "baseline": {
      "backend": "postgres",
      "postgres": {
        "schema": "bsl_search"
      },
      "workspaceCode": {
        "policy": {
          "publishBranches": ["vendor", "develop"],
          "branches": [
            { "match": "vendor", "selectBranch": "vendor" },
            { "match": "develop", "selectBranch": "develop", "fallbackBranch": "vendor" },
            { "match": "feature/*", "selectBranch": "develop", "fallbackBranch": "vendor" },
            { "match": "fix/*", "selectBranch": "develop", "fallbackBranch": "vendor" },
            { "match": "bug/*", "selectBranch": "develop", "fallbackBranch": "vendor" },
            { "match": "*", "selectBranch": "develop", "fallbackBranch": "vendor" }
          ]
        }
      },
      "reference": {
        "snapshotId": "reference:0.1.104"
      }
    }
  }
}
```

Проверить, как этот конфиг будет разрешён на runtime, можно без запуска MCP:

```bash
bsl-analyzer check-config .bsl-analyzer.json
```

Команда покажет отдельно для `workspace` и `reference`:

- выбранный backend;
- итоговый `snapshot` / `branch` / `commit`;
- проблемы конфигурации, например отсутствие `search.baseline.postgres.url`.

Порядок разрешения такой:

1. `search.baseline.backend` в `.bsl-analyzer.json` выбирает `sqlite` или `postgres`.
2. Для `postgres` `url/schema` берутся из env, если они заданы, иначе из конфига.
3. `snapshotId/branch/commit` тоже могут быть переопределены через env.
4. Если explicit selection не задан, `workspaceCode.policy` выбирает published baseline по текущей git-ветке проекта.

Для workspace-профиля env больше не переключает backend сам по себе: если в конфиге не выбран `postgres`, будет использован локальный SQLite. Для `reference` user-scope без project root по-прежнему допускается env-only режим.

Типовой workflow для centralized baseline:

```bash
# 1. Проверить, что проектный конфиг разрешается в нужный backend/snapshot
bsl-analyzer check-config .bsl-analyzer.json

# 2. Опубликовать baseline develop в PostgreSQL
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline sync-pg \
  --source-dir ./my-project \
  --branch develop \
  --commit "$CI_COMMIT_SHA"

# 2a. Обновить baseline vendor, когда приходит новая поставка
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline sync-pg \
  --source-dir ./my-project \
  --branch vendor \
  --commit "$CI_COMMIT_SHA"

# 3. Опубликовать shared reference baseline
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline sync-pg \
  --corpus reference
```

`sync-pg` печатает итоговые параметры публикации:

- `Corpus`
- `Snapshot`
- `Schema`
- `Parent`
- `Branch`
- `Commit`
- `Reused files` / `Written files`
- `Reused chunks` / `Written chunks`
- количество файлов и чанков

Если `--parent-snapshot-id` не передан, `sync-pg` сам выберет последний опубликованный snapshot
в том же `corpus/branch`. Для `reference` без branch будет выбран последний snapshot того же corpus.

Publish теперь использует shared file-object storage в PostgreSQL:
- каждый logical file получает общий `file_object`;
- если такой объект уже есть, snapshot только ссылается на него;
- `snapshot_items` остаётся как legacy fallback для старых snapshot'ов.

Если при `sync-pg` заданы `EMBEDDING_URL` и `EMBEDDING_MODEL`, команда также публикует
shared embeddings в PostgreSQL. Тогда на старте MCP local semantic cache сначала забирает
готовые embeddings из shared storage, а локально считает только missing часть.

Для проверки содержимого shared PostgreSQL storage доступны read-only команды:

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

# Dry-run очистки мусора
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline gc-pg

# Реальное удаление orphan-объектов
BSL_SEARCH_BASELINE_PG_URL=postgres://shared-search \
bsl-analyzer search baseline gc-pg --execute
```

`list-pg` показывает:

- `Snapshot`
- `Corpus`
- `Created`
- `Parent`
- `Branch`
- `Commit`
- `Files`
- `Chunks`
- `Fingerprint`

`show-pg` дополнительно показывает breakdown по `collection`.

Дополнительные операторские команды:

- `list-file-objects-pg` показывает shared file objects, число snapshot-ссылок и fingerprint.
- `show-file-object-pg` показывает, из каких snapshot/path используется конкретный file object.
- `list-embeddings-pg` показывает объём stored embeddings по `model + dimension`.
- `show-embedding-coverage-pg` показывает, сколько active semantic payload'ов уже покрыто shared embeddings.
- `gc-pg` по умолчанию только считает orphan file objects / items / semantic rows. Реальное удаление выполняется только с `--execute`.

После установки или запуска MCP проверьте runtime-состояние через инструмент `search` с `action=status`:

- в `workspace` профиле статус покажет `Configured baseline`, `Code lexical source`, `Code semantic source` и `External baseline`;
- в `reference` профиле статус покажет `Configured baseline`, `Docs lexical source`, `Docs semantic source`, `External baseline` и `Freshness`.

Для centralized `reference` semantic search работает так:

- lexical (`find_docs`) читается из shared PostgreSQL baseline;
- semantic (`search_docs`) использует локальный user-scope cache этого snapshot;
- cache синхронизируется при старте reference MCP и не переэмбеддится, если fingerprint/snapshot не изменился;
- если в shared baseline уже есть embeddings для текущего `EMBEDDING_MODEL`, они переиспользуются напрямую.

Для centralized `workspace` semantic search используется та же hybrid-модель:

- lexical `find_code` читает baseline из shared snapshot и накладывает local overlay;
- semantic `search_code` использует локальный SQLite cache shared baseline;
- локальные изменённые файлы и удаления по-прежнему накладываются overlay-поверх этого cache;
- cache baseline не переиндексируется заново, если snapshot не изменился;
- shared embeddings из PostgreSQL используются как первый источник, локальный embedder остаётся fallback.

С подключением к 1С (для выполнения запросов и кода):

```bash
bsl-analyzer mcp serve --profile workspace \
  --source-dir ./my-project \
  --onec-url http://localhost/base/hs/bsl-analyzer \
  --onec-user admin \
  --onec-password secret
```

#### Инструменты

| Инструмент | Описание |
|------------|----------|
| **metadata** | Структура конфигурации: объекты, реквизиты, табличные части, формы |
| **search** | Полнотекстовый и семантический поиск по коду и документации платформы |
| **syntax_help** | Справка по API платформы: типы, методы, глобальные функции |
| **query** | Валидация и выполнение SDBL-запросов |
| **execute** | Проверка синтаксиса, выполнение кода и вычисление выражений |
| **debug** | Интеграция с отладчиком 1С: точки останова, стек, шаги |
| **its_help** | Вопрос эксперту по ИТС: стандарты разработки, паттерны БСП, методические рекомендации |

#### Расширение 1С (для query, execute, debug)

Инструменты `query`, `execute` и `debug` требуют подключения к работающей базе 1С через HTTP-сервис. Расширение конфигурации встроено в бинарник.

Установка:

1. Экспортируйте расширение из бинарника:
   ```bash
   bsl-analyzer extension export -o ./bsl-extension
   ```
2. В конфигураторе 1С откройте **Конфигурация → Расширения конфигурации**
3. Нажмите **Добавить** и загрузите каталог `bsl-extension` как расширение
4. Опубликуйте HTTP-сервис: **Администрирование → Публикация на веб-сервере**, включите сервис `BSLAnalyzerService` по пути `/hs/bsl-analyzer`
5. Убедитесь, что пользователю назначена роль `BSL_ОсновнаяРоль`

После публикации проверьте доступность:

```bash
curl http://localhost/base/hs/bsl-analyzer/version
# {"version":"1.0.0"}
```

> **Примечание:** инструменты `reference`-профиля и локальные инструменты `workspace` (`metadata`, `search` по коду) работают без расширения. Для `query`, `execute` и `debug` требуется подключение к 1С.

#### Справка ИТС (its_help)

Инструмент `its_help` обращается к [1С:Напарник](https://code.1c.ai/) для поиска по стандартам ИТС, методическим рекомендациям и документации платформы. Требуется токен доступа:

1. Получите токен на [code.1c.ai/tokens](https://code.1c.ai/tokens)
2. Передайте через переменную окружения `NAPARNIK_TOKEN`

> **Примечание:** без `NAPARNIK_TOKEN` все остальные инструменты работают. `its_help` просто возвращает ошибку при вызове.

#### Настройка в Claude Desktop / Claude Code

Добавьте в конфигурацию MCP (`claude_desktop_config.json` или `.mcp.json`) оба сервера:

```json
{
  "mcpServers": {
    "bsl-analyzer-reference": {
      "command": "bsl-analyzer",
      "args": ["mcp", "serve", "--profile", "reference"]
    },
    "bsl-analyzer-workspace": {
      "command": "bsl-analyzer",
      "args": ["mcp", "serve", "--profile", "workspace", "--source-dir", "/path/to/project"]
    }
  }
}
```

С подключением к 1С:

```json
{
  "mcpServers": {
    "bsl-analyzer-workspace": {
      "command": "bsl-analyzer",
      "args": [
        "mcp",
        "serve",
        "--profile", "workspace",
        "--source-dir", "/path/to/project",
        "--onec-url", "http://localhost/base/hs/bsl-analyzer",
        "--onec-user", "admin",
        "--onec-password", "secret"
      ]
    }
  }
}
```

Полная конфигурация со всеми возможностями (1С, ИТС, семантический поиск):

```json
{
  "mcpServers": {
    "bsl-analyzer-reference": {
      "command": "bsl-analyzer",
      "args": ["mcp", "serve", "--profile", "reference"],
      "env": {
        "NAPARNIK_TOKEN": "ваш_токен_с_code.1c.ai",
        "EMBEDDING_URL": "http://localhost:8000/v1/embeddings",
        "OPENROUTER_API_KEY": "ваш_ключ_openrouter"
      }
    },
    "bsl-analyzer-workspace": {
      "command": "bsl-analyzer",
      "args": [
        "mcp",
        "serve",
        "--profile", "workspace",
        "--source-dir", "/path/to/project",
        "--onec-url", "http://localhost/base/hs/bsl-analyzer",
        "--onec-user", "admin",
        "--onec-password", "secret"
      ]
    }
  }
}
```

#### Настройка в VS Code (Copilot / Continue / Cline)

Добавьте в `.vscode/mcp.json` в корне проекта:

```json
{
  "servers": {
    "bsl-analyzer-workspace": {
      "command": "bsl-analyzer",
      "args": ["mcp", "serve", "--profile", "workspace", "--source-dir", "${workspaceFolder}"]
    }
  }
}
```

#### Семантический поиск

Для работы семантического поиска (`search_code`, `search_docs`) необходим эмбеддинг-сервер, совместимый с OpenAI API:

```bash
EMBEDDING_URL=http://localhost:8000/v1/embeddings bsl-analyzer mcp serve --profile reference
```

### Фиксация версии (CI/CD)

```bash
BSL_ANALYZER_VERSION=0.1.33 bsl-analyzer analyze -s ./src
```

## Конфигурация

Файл `.bsl-analyzer.json` (или `.bsl-language-server.json`):

```json
{
    "diagnostics": {
        "skip": ["CommentedCode"],
        "parameters": {
            "CyclomaticComplexity": {
                "complexityThreshold": 20
            }
        }
    }
}
```

Пример с централизованным search baseline:

```json
{
    "configurationRoot": "src/cf",
    "search": {
        "baseline": {
            "backend": "postgres",
            "postgres": {
                "url": "postgres://shared-search",
                "schema": "bsl_search"
            },
            "workspaceCode": {
                "policy": {
                    "publishBranches": ["vendor", "develop"],
                    "branches": [
                        { "match": "vendor", "selectBranch": "vendor" },
                        {
                            "match": "develop",
                            "selectBranch": "develop",
                            "fallbackBranch": "vendor"
                        },
                        {
                            "match": "feature/*",
                            "selectBranch": "develop",
                            "fallbackBranch": "vendor"
                        },
                        {
                            "match": "*",
                            "selectBranch": "develop",
                            "fallbackBranch": "vendor"
                        }
                    ]
                }
            },
            "reference": {
                "snapshotId": "reference:0.1.104"
            }
        }
    }
}
```

Если `search.baseline` отсутствует или `backend` равен `sqlite`, используется локальный SQLite search index.

## Сборка из исходников

**Требования:** Rust 1.91+

```bash
git clone https://github.com/itrous/bsl-analyzer.git
cd bsl-analyzer
cargo build --release
```

## Производительность

Сравнительный бенчмарк с [bsl-language-server](https://github.com/1c-syntax/bsl-language-server) на реальном проекте.

**Тестовый проект:** Управление торговлей 11.5.22.134 (12 578 BSL-файлов, 500 MB кода)

**Конфигурация:** настройки по умолчанию, отключена только Typo. Без skipSupport.

**Система:** AMD Ryzen 5 5600X (6 ядер / 12 потоков), 32 GB RAM, Linux 6.19, NVMe SSD

**Методика:** `/usr/bin/time -v` (GNU time), холодный запуск без кэша. Оба инструмента используют все доступные ядра.

| Метрика | bsl-language-server 0.28.5 | bsl-analyzer 0.1.36 | Разница |
|---------|---------------------------|---------------------|---------|
| **Wall time** | 132.6s | 52.8s | 2.5x быстрее |
| **CPU time** | 1388.7s | 499.0s | 2.8x меньше CPU |
| **Peak RSS** | 4 566 MB | 1 474 MB | 3.1x меньше памяти |
| **System time** | 9.24s | 3.93s | 2.4x меньше I/O |
| **Files/sec** | 94 | 238 | 2.5x пропускная способность |
| **Диагностик** | 552 111 | 724 961 | — |

**О разнице в количестве диагностик.** bsl-analyzer нашёл на 31% больше срабатываний. Это объясняется несколькими факторами:

- Ряд SDBL-диагностик (запросы к базе данных) в bsl-analyzer работают точечно — выделяют каждое проблемное поле или выражение в запросе отдельной диагностикой, тогда как bsl-language-server отмечает запрос целиком одним предупреждением.
- Возможны ложные срабатывания — проект молодой, качество детекции активно улучшается.
- Некоторые диагностики реализованы с отличиями в логике — это штатная ситуация для независимой реализации.

Мы работаем над повышением точности диагностик и уменьшением числа ложных срабатываний.

## Архитектура

```
bsl-analyzer (LSP-сервер)
    └── ide (API верхнего уровня)
        ├── ide-diagnostics (180+ диагностик)
        ├── ide-assists (Quick-fix действия)
        └── ide-db (Salsa — инкрементальные вычисления)
            └── hir (семантический анализ)
                └── syntax (CST, Rowan)
                    └── parser → lexer
```

Подробнее: [docs/architecture/ARCHITECTURE.md](docs/architecture/ARCHITECTURE.md)

## Участие в разработке

См. [CONTRIBUTING.md](CONTRIBUTING.md).

## Благодарности

Проект вдохновлён [BSL Language Server](https://github.com/1c-syntax/bsl-language-server) — инструментом статического анализа BSL на Java. Спасибо авторам за огромную работу по формализации диагностик и стандартов качества кода 1С.

- [bsl-language-server](https://github.com/1c-syntax/bsl-language-server) — статический анализатор BSL (Java, LGPL-3.0)
- [1c-syntax](https://github.com/1c-syntax) — сообщество разработчиков инструментов для 1С
- [RDT1C](https://github.com/Segate-ekb/1c_RDT) — «Инструменты разработчика» для 1С. Реализация интеграции с API 1С:Напарник (FIM completion, context/update) использована как референс для модуля `naparnik`

## Лицензия

MIT или Apache-2.0, на выбор. См. [LICENSE-MIT](LICENSE-MIT) и [LICENSE-APACHE](LICENSE-APACHE).
