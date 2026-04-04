# MCP-сервер и интеграция с AI

BSL Analyzer включает встроенный [MCP-сервер](https://modelcontextprotocol.io/) (Model Context Protocol). Он позволяет AI-агентам (Claude, Cursor, Gemini, Codex и др.) взаимодействовать с кодовой базой 1С, документацией платформы, выполнять SDBL-запросы и запускать отладку.

## Профили MCP

Сервер разделён на два явных профиля:

- `reference` — глобальная справка платформы и ИТС, устанавливается один раз в `user scope`. Не требует привязки к проекту.
- `workspace` — работа с конкретным проектом и конкретной базой, устанавливается отдельно в каждый проект (`project scope`).

> **Внимание:** MCP-сервер с подключением к 1С (через профиль `workspace`) предоставляет полный доступ к базе данных, включая выполнение произвольного кода и запросов. Используйте только в контуре разработки или тестирования. Подключение к продуктивной базе крайне не рекомендуется.

### Ручной запуск серверов

```bash
# Глобальная справка
bsl-analyzer mcp serve --profile reference

# Проектный сервер
bsl-analyzer mcp serve --profile workspace --source-dir ./my-project
```

## Установка в AI-инструменты (`mcp install`)

Команда `mcp install` автоматически настраивает конфигурацию AI-инструментов.

### Рекомендуемый сценарий

Одной командой устанавливает **global reference** (для пользователя) и **project workspace** (для текущего проекта):

```bash
bsl-analyzer mcp install \
  --target all \
  --preset recommended \
  --source-dir ./my-project \
  --env NAPARNIK_TOKEN=your_token \
  --env EMBEDDING_URL=http://localhost:8000/v1/embeddings
```

### Раздельная установка

Установить только глобальный MCP справки:
```bash
bsl-analyzer mcp install \
  --target all \
  --preset reference \
  --scope user \
  --env NAPARNIK_TOKEN=your_token \
  --env EMBEDDING_URL=http://localhost:8000/v1/embeddings
```

Установить project-scoped MCP workspace для конкретного проекта:
```bash
bsl-analyzer mcp install \
  --target all \
  --preset workspace \
  --scope project \
  --source-dir ./my-project
```

### Дополнительные опции

- `--dry-run` — посмотреть, что будет записано, без изменений на диске. Показывает итоговый CLI-вызов или файл конфигурации.
- `--force` — обновить существующую запись MCP с тем же именем.
- `--name custom-bsl` — изменить базовое имя (будут созданы серверы `custom-bsl-reference` и `custom-bsl-workspace`).
- `--onec-password` — если передать пароль, он будет сохранён в конфиге целевого инструмента.

### Поддерживаемые Targets

| Target | Способ установки |
|--------|------------------|
| `codex` | `user` через `codex mcp add`, `project` через merge в `.codex/config.toml` |
| `gemini` | через `gemini mcp add` |
| `claude` | через `claude mcp add` |
| `cursor` | через merge в `~/.cursor/mcp.json` или `.cursor/mcp.json` |

## Настройка вручную

Если вы не используете `mcp install`, вы можете настроить инструменты вручную.

### Claude Desktop / Claude Code

Добавьте в конфигурацию MCP (`claude_desktop_config.json` или `.mcp.json`) оба сервера:

```json
{
  "mcpServers": {
    "bsl-analyzer-reference": {
      "command": "bsl-analyzer",
      "args": ["mcp", "serve", "--profile", "reference"],
      "env": {
        "NAPARNIK_TOKEN": "ваш_токен_с_code.1c.ai",
        "EMBEDDING_URL": "http://localhost:8000/v1/embeddings"
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

### VS Code (Copilot / Continue / Cline)

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

## Семантический поиск

Для работы инструментов семантического поиска (`search_code`, `search_docs`) необходим эмбеддинг-сервер, совместимый с OpenAI API:

```bash
EMBEDDING_URL=http://localhost:8000/v1/embeddings bsl-analyzer mcp serve --profile reference
```

Если вы используете централизованный baseline в PostgreSQL (подробнее в `docs/central-postgres-search/`), семантический поиск комбинирует локальный кэш и данные из PostgreSQL.
