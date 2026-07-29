# MCP-сервер: установка и профили

Этот документ отвечает на два вопроса:

- какой MCP-профиль нужен в вашем сценарии;
- как установить его в AI-клиент без ручного редактирования конфигов.

Описание самих инструментов и подключение к базе 1С вынесены в
`docs/mcp/TOOLS_AND_EXTENSION.md`.

## Как устроены профили

`bsl-analyzer` публикует два отдельных MCP-профиля.

| Профиль | Для чего нужен | Что обычно требуется |
|--------|----------------|----------------------|
| `reference` | справка платформы, поиск по документации, `syntax_help`, `its_help` | при необходимости `EMBEDDING_URL` и `NAPARNIK_TOKEN` |
| `workspace` | поиск по коду проекта, metadata, SDBL, выполнение BSL-кода, debug | `--source-dir`; для live-инструментов ещё `--onec-url` и учётные данные |

Для нескольких ИБ используйте один `workspace`: скопируйте
`docs/mcp/onec-connections.example.json` за пределы репозитория, заполните и
передайте путь через `BSL_ONEC_CONNECTIONS_FILE`; живые инструменты выбирают
профиль параметром `connection`. Пароли задаются только переменными окружения,
указанными в реестре.

Расширение VS Code запускает языковой сервер отдельным процессом
`bsl-analyzer-app --stdio`. MCP-профиль `workspace` использует локальный
посредник и Unix-сокет, поэтому порты и сеансы с языковым сервером не делит.
Несколько MCP-клиентов одного проекта, напротив, переиспользуют один тяжёлый
сервер посредника.

Практическое правило:

- ставьте `reference`, если AI должен знать платформу и ИТС;
- ставьте `workspace`, если AI должен работать с конкретным репозиторием;
- чаще всего нужен комплект из обоих профилей.

> `reference` не использует `--source-dir` и не принимает параметры подключения
> к 1С. Live-доступ к базе относится только к `workspace`.

## Рекомендуемая установка

Самый удобный сценарий — установить оба профиля сразу:

```bash
bsl-analyzer mcp install \
  --target all \
  --preset recommended \
  --source-dir ./my-project
```

Этот preset делает следующее:

- `reference` ставится как user-scoped конфигурация;
- `workspace` ставится как project-scoped конфигурация для текущего репозитория.

Если нужны дополнительные возможности, добавьте переменные окружения и
параметры подключения:

```bash
bsl-analyzer mcp install \
  --target all \
  --preset recommended \
  --source-dir ./my-project \
  --env NAPARNIK_TOKEN=your_token \
  --env EMBEDDING_URL=http://localhost:8000/v1/embeddings \
  --env EMBEDDING_API_KEY=your_api_key \
  --onec-url http://localhost/base/hs/bsl-analyzer \
  --onec-user admin
```

## Установка по отдельности

Только глобальный профиль справки:

```bash
bsl-analyzer mcp install \
  --target all \
  --preset reference \
  --scope user \
  --env NAPARNIK_TOKEN=your_token \
  --env EMBEDDING_URL=http://localhost:8000/v1/embeddings \
  --env EMBEDDING_API_KEY=your_api_key
```

Только проектный профиль рабочего каталога:

```bash
bsl-analyzer mcp install \
  --target all \
  --preset workspace \
  --scope project \
  --source-dir ./my-project
```

Поддерживаемые scope'ы по целевым клиентам:

| Target | Поддерживаемые scope'ы | Как применяется |
|--------|-------------------------|-----------------|
| `codex` | `user`, `project` | user через CLI `codex mcp add`, project через `.codex/config.toml` |
| `gemini` | `user`, `project` | через CLI `gemini mcp add` |
| `claude` | `user`, `project`, `local` | через CLI `claude mcp add` |
| `cursor` | `user`, `project` | через merge в `mcp.json` |

## Полезные флаги `mcp install`

- `--dry-run` — показать итоговую команду или конфиг без записи на диск;
- `--force` — обновить существующую MCP-запись с тем же именем;
- `--name custom-bsl` — изменить базовое имя сервера;
- `--env KEY=value` — передать переменные окружения для MCP-процесса;
- `--onec-password` — сохранить пароль в конфиге целевого клиента.

Если пароль передаётся через `--onec-password`, он попадает в аргументы
запуска MCP-сервера. Для небезопасных контуров лучше использовать отдельные
тестовые учётные данные.

## Ручной запуск серверов

Если нужно сначала проверить профиль локально, можно запустить его вручную.
Для профиля `workspace` брокер включён по умолчанию на всех платформах, включая
Windows: транспорт — named pipe с security descriptor, ограниченным текущим
пользователем, с проверкой личности backend'а (защита от подмены имени pipe).
Принудительно вернуться на прямой `stdio` можно переменной `BSL_MCP_BROKER=0`.

Глобальный профиль справки:

```bash
bsl-analyzer mcp serve --profile reference
```

Профиль проекта:

```bash
bsl-analyzer mcp serve --profile workspace --source-dir ./my-project
```

Профиль проекта с live-доступом к базе 1С:

```bash
bsl-analyzer mcp serve \
  --profile workspace \
  --source-dir ./my-project \
  --onec-url http://localhost/base/hs/bsl-analyzer \
  --onec-user admin \
  --onec-password secret
```

### Запуск по HTTP

Чтобы запустить профиль `workspace` по HTTP с доступом из локальной сети:

```bash
bsl-analyzer mcp serve \
  --profile workspace \
  --source-dir ./my-project \
  --mode http \
  --host 0.0.0.0 \
  --port 8021 \
  --allowed-host mcp-server.local
```

- `--mode http` включает транспорт Streamable HTTP;
- `--port` задаёт обязательный порт от `1` до `65535`;
- `--host 0.0.0.0` открывает все сетевые интерфейсы; без параметра сервер
  доступен только на `127.0.0.1`;
- `--allowed-host` разрешает имя сервера из адреса подключения, а не имя
  клиента. Для нелокального `--host` нужен хотя бы один такой параметр; для
  IP-адресов и дополнительных имён повторите его. Пустое значение не считается
  разрешённым именем и отвергается: иначе сервер запустился бы и отказывал всем.
  Заданный список заменяет умолчание целиком, поэтому если нужен ещё и доступ
  с самой машины — перечислите `localhost` и `127.0.0.1` явно.
- `--host`, `--port` и `--allowed-host` используются только с `--mode http`.

В примере MCP доступен по адресу `http://mcp-server.local:8021/mcp`. Одинаковые
HTTP-процессы одновременно не запускаются. Без `--mode http` поведение команды
не меняется.

HTTP-сервер забирает у демона того же проекта владение производными кэшами
в `.build` — графом вызовов и поисковым индексом. Демон дослуживает своих
клиентов, перестаёт писать кэши и выходит по простою, поэтому держать оба режима
на одном проекте одновременно незачем.

Если нужна ручная интеграция в конкретный AI-клиент, обычно проще не писать
конфиг с нуля, а сначала выполнить `mcp install --dry-run` и использовать
показанную команду или сгенерированный фрагмент как образец.

## Эмбеддинги и семантический поиск

Семантическая составляющая поиска требует `EMBEDDING_URL`:

- `search(action=search_docs)` в профиле `reference`;
- семантическая ветвь `search(action=search_code)` в профиле `workspace`
  (лексическая ветвь `search_code` работает и без неё).

Если embedding-провайдер требует Bearer-авторизацию
(например, OpenRouter, OpenAI или совместимый сервис), дополнительно задайте
`EMBEDDING_API_KEY`.

Для задач семантического поиска по коду и документации разумно явно задавать
`EMBEDDING_MODEL`. Практический ориентир:

- `Qwen/Qwen3-Embedding-0.6B` — минимальная рекомендуемая модель и текущий
  дефолт в `bsl-analyzer`;
- `Qwen/Qwen3-Embedding-4B` — усиленный компромиссный вариант;
- `Qwen/Qwen3-Embedding-8B` — предпочтительный вариант, если позволяет ресурс.

Пример:

```bash
EMBEDDING_URL=https://openrouter.ai/api \
EMBEDDING_API_KEY=your_api_key \
EMBEDDING_MODEL=Qwen/Qwen3-Embedding-0.6B \
  bsl-analyzer mcp serve --profile reference
```

Если `EMBEDDING_URL` не задан, остаётся полнотекстовый `find_docs` в `reference`, а
`search_code` в `workspace` возвращает лексические результаты с пометкой
`-- semantic skipped: … --`.

Сейчас `bsl-analyzer` для embedding API поддерживает стандартный заголовок
`Authorization: Bearer ...` через `EMBEDDING_API_KEY`. Если конкретный
провайдер требует дополнительные нестандартные заголовки, это нужно учитывать
отдельно.

Если используется централизованный baseline в PostgreSQL, семантический поиск
комбинирует локальный runtime и shared baseline. Подробности — в
`docs/central-postgres-search/README.md`.

## Что читать дальше

- `docs/mcp/SETUP.md` — установка с нуля (бинарник, PATH, LSP-плагин, верификация)
- `docs/mcp/TOOLS_AND_EXTENSION.md` — доступные инструменты, prerequisites и расширение 1С
- `docs/mcp/CONTRACT.md` — машиночитаемая декларация контракта (`bsl-analyzer contract`,
  ресурс `bsl-analyzer://contract`) для проверки совместимости в чужом CI
- `docs/central-postgres-search/README.md` — shared baseline и overlay для поиска
