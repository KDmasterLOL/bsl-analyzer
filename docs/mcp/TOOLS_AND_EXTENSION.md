# MCP: инструменты и подключение к 1С

Этот документ описывает сами MCP-инструменты и их зависимости. Установка
профилей в AI-клиенты вынесена в `docs/mcp/README.md`.

## Матрица инструментов

Набор доступных tools зависит от профиля сервера.

| Профиль | Tool | Что делает | Что нужно дополнительно |
|--------|------|------------|-------------------------|
| `reference` | `search(action=find_docs\|search_docs\|status)` | поиск по справке платформы | `EMBEDDING_URL` только для `search_docs` |
| `reference` | `syntax_help` | точечная справка по типу, методу или глобальной функции | ничего |
| `reference` | `its_help` | вопросы к ИТС / 1С:Напарник | `NAPARNIK_TOKEN` |
| `workspace` | `metadata` | обзор объектов конфигурации, реквизитов, форм и дерева метаданных | `--source-dir` |
| `workspace` | `search(action=find_code\|search_code\|status)` | поиск по коду проекта | `EMBEDDING_URL` только для `search_code` |
| `workspace` | `query` | `validate` для SDBL, а также `execute` для `SELECT` | `--onec-url` нужен для live-валидации через платформу и для `execute` |
| `workspace` | `execute` | `check`, `run`, `eval` для BSL-кода | `--onec-url` нужен для `run` и `eval` |
| `workspace` | `debug` | attach, breakpoints, step, stack trace, locals, eval | `--onec-url` и доступ к отладочному контуру |

## Что работает без подключения к базе 1С

Без HTTP-сервиса 1С доступны:

- весь профиль `reference`;
- в `workspace`: `metadata`, поиск по коду и `query(action=validate)` с локальным
  парсером.

Для `query(action=execute)`, `execute(action=run|eval)` и для отладки нужно
живое подключение к базе через `--onec-url`.

## Переменные окружения и prerequisites

### `EMBEDDING_URL`

Включает семантический поиск:

- `search_docs` в `reference`;
- `search_code` в `workspace`.

Без этой переменной остаётся доступен полнотекстовый поиск (`find_docs`,
`find_code`).

### `NAPARNIK_TOKEN`

Нужен только для `its_help`.

Токен можно получить на `https://code.1c.ai/tokens` и передать в MCP-процесс
через `--env NAPARNIK_TOKEN=...` при `mcp install` или обычную переменную
окружения при `mcp serve`.

Если токен не задан, остальные инструменты продолжают работать.

### `--onec-url`, `--onec-user`, `--onec-password`

Нужны для инструментов, которые работают с живой базой 1С:

- `query(action=execute)`;
- `execute(action=run|eval)`;
- `debug`.

Для `execute(action=check)` live-подключение не требуется.

## `its_help`: когда использовать

`its_help` нужен для вопросов по:

- стандартам разработки ИТС;
- паттернам БСП;
- методическим рекомендациям;
- типовым ошибкам и практикам 1С.

Для сигнатур методов платформы и API используйте `syntax_help`, а для поиска по
локальному проекту — `search` в `workspace`.

## Подключение live-инструментов к базе 1С

Для `query`, `execute` и `debug` нужен HTTP-сервис, который предоставляет
встроенное расширение `BSL_Analyzer`.

### 1. Экспортируйте расширение

```bash
bsl-analyzer extension export -o ./bsl-extension
```

### 2. Загрузите расширение в конфигуратор

В конфигураторе откройте:

`Конфигурация -> Расширения конфигурации`

Затем добавьте каталог `./bsl-extension` как расширение.

### 3. Опубликуйте HTTP-сервис

В конфигураторе откройте:

`Администрирование -> Публикация на веб-сервере`

Проверьте, что включена публикация HTTP-сервисов и сервис
`BSLAnalyzerService` доступен по пути `/hs/bsl-analyzer`.

### 4. Настройте права

Пользователю 1С, под которым MCP будет обращаться к сервису, должна быть
назначена роль `BSL_ОсновнаяРоль`.

### 5. Проверьте доступность сервиса

```bash
curl http://localhost/base/hs/bsl-analyzer/version
```

Ожидаемый ответ:

```json
{"version":"1.0.0"}
```

### 6. Запустите `workspace`-профиль с подключением

```bash
bsl-analyzer mcp serve \
  --profile workspace \
  --source-dir ./my-project \
  --onec-url http://localhost/base/hs/bsl-analyzer \
  --onec-user admin \
  --onec-password secret
```

## Практический маршрут

- нужен AI-справочник по платформе и ИТС — ставьте `reference`;
- нужен анализ конкретной конфигурации и поиск по коду — ставьте `workspace`;
- нужен SDBL `execute`, запуск BSL или debug — дополнительно публикуйте
  расширение и настраивайте `--onec-url`.

## Что читать дальше

- `docs/mcp/README.md` — установка профилей, scope'ы и `mcp install`
- `docs/central-postgres-search/README.md` — centralized baseline для поиска
