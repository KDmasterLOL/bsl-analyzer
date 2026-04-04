# Архитектура BSL Analyzer

Этот документ описывает текущее устройство проекта на уровне слоёв, крейтов и
основных вычислительных пайплайнов. Он не дублирует внутренние детали каждой
подсистемы, а фиксирует рабочую картину проекта в актуальном состоянии.

## Краткая схема

```text
bsl-analyzer
  ├─ CLI: analyze / format / rules / search / mcp
  ├─ LSP server
  └─ MCP server
       │
       ▼
      ide
  ├─ ide-diagnostics
  ├─ ide-assists
  ├─ ide-db
  └─ formatting / hover / completion / goto / references
       │
       ▼
 hir / hir-def / hir-ty / sdbl-hir
       │
       ▼
 syntax / parser / lexer
```

## Слои системы

### 1. Интерфейсный слой

Верхний бинарник `bsl-analyzer` объединяет несколько сценариев работы:

- LSP-сервер для IDE;
- CLI-анализатор для CI и локального запуска;
- MCP-сервер для AI-инструментов;
- вспомогательные команды форматирования, экспорта правил и управления search baseline.

Этот слой отвечает за:

- разбор CLI-аргументов;
- запуск нужного runtime-профиля;
- инициализацию логирования;
- загрузку конфигурации проекта;
- сборку и публикацию результатов наружу.

### 2. IDE-слой

Крейт `ide` даёт высокоуровневый API над остальными подсистемами. Через него
реализованы hover, completion, goto definition, references, formatting и
запуск диагностик.

Сопутствующие крейты:

- `ide-diagnostics` — 180 диагностик;
- `ide-assists` — code actions и quick-fix сценарии;
- `ide-db` — основная база запросов поверх Salsa.

### 3. Семантический слой

Семантика разделена на несколько крейтов:

- `hir-def` — декларации, item tree, module data, symbol tree;
- `hir-ty` — вывод типов;
- `hir` — HIR-представление и семантические операции;
- `sdbl-hir` — отдельный HIR для языка запросов SDBL.

Именно здесь строятся структуры, на которых работают большинство
диагностик, переходов по коду и подсказок.

### 4. Синтаксический слой

Нижний слой состоит из:

- `lexer` — токенизация BSL/SDBL;
- `parser` — построение синтаксического дерева;
- `syntax` — typed AST wrappers и CST на базе Rowan.

Проект использует full-fidelity CST: дерево сохраняет пробелы, комментарии и
прочие синтаксические детали, поэтому его можно использовать не только для
анализа, но и для форматирования и точных правок.

## Основные инфраструктурные крейты

| Крейты | Назначение |
|--------|------------|
| `base-db` | базовые Salsa-input'ы, source roots, file text |
| `vfs`, `vfs-notify` | виртуальная файловая система и слежение за файлами |
| `project-model` | загрузка конфигурации проекта, legacy JSON-конфигов и search-настроек |
| `bsl-metadata` | чтение `Configuration.xml` и метаданных 1С |
| `bsl-platform` | модели платформенных типов и API |
| `cfg`, `cfg-types` | control-flow graph |
| `dataflow` | reaching definitions и liveness analysis |
| `bsl-search` | индексирование и поиск по коду/документации |
| `mcp-server` | MCP runtime и интеграция с AI-клиентами |

## Salsa и модель вычислений

Проект построен вокруг инкрементальных вычислений на базе Salsa `0.25.2`.

Ключевая идея:

- исходные тексты и source roots задаются как входы;
- парсинг, HIR, metadata, CFG и dataflow вычисляются как кэшируемые запросы;
- при изменении файла пересчитываются только зависимые части графа.

На практике это позволяет:

- не перепарсивать весь проект при каждом изменении;
- кэшировать дорогостоящие этапы анализа;
- переиспользовать результаты между LSP, CLI и диагностическими проходами.

## Пайплайны данных

### Разбор исходника

```text
Текст файла
  → lexer
  → parser
  → syntax::SyntaxNode
  → AST wrappers
  → HIR / SDBL HIR
```

### Выпуск диагностик

Диагностики в проекте не сводятся к одному общему обходу дерева. Сейчас есть
несколько независимых источников:

- line-based проверки;
- syntax diagnostics;
- item tree / module bodies diagnostics;
- configuration-based diagnostics;
- SDBL HIR diagnostics;
- HIR diagnostics;
- dataflow diagnostics;
- metadata diagnostics.

Общий entrypoint — `ide::compute_diagnostics`, который собирает результаты из
этих слоёв и затем устраняет известные дубликаты.

### Dataflow-диагностики

Flow-sensitive анализ вынесен в отдельную подсистему.

Сейчас:

- `IncorrectUseOfStrTemplate` использует reaching definitions;
- `RewriteMethodParameter` использует module-level reaching definitions;
- `UnusedLocalVariable` использует module-level liveness;
- ещё несколько диагностик выполняются через flow-sensitive collector в `ide-diagnostics`.

Подробности — в `docs/architecture/DATAFLOW.md`.

## Конфигурация проекта

Основной формат конфигурации — `bsl-analyzer.toml`. Его загружает
`project-model`, а затем нормализованные настройки передаются в диагностики,
форматирование, code lens, поиск и разрешение source roots / extensions.

Если TOML-файл отсутствует, проект всё ещё умеет читать legacy-файлы
`.bsl-analyzer.json` и `.bsl-language-server.json`, но в новых проектах они
рассматриваются только как слой совместимости.

Подробности по структуре файла и diagnostic-параметрам вынесены в:

- `docs/configuration/PROJECT_CONFIGURATION.md`
- `docs/configuration/DIAGNOSTICS.md`

## Поиск и MCP

Подсистема поиска обслуживает CLI и MCP-профили `workspace` / `reference`.
На архитектурном уровне важно только следующее:

- есть локальный runtime;
- есть shared baseline для PostgreSQL;
- для рабочей копии поверх baseline может накладываться локальный overlay;
- MCP-профиль определяет, с какими источниками и tools работает сервер.

Детали runtime, branch policy, merge baseline + overlay и deployment-вопросы
вынесены в отдельные документы:

- `docs/mcp/README.md`
- `docs/mcp/TOOLS_AND_EXTENSION.md`
- `docs/architecture/SEARCH_BASELINE_OVERLAY.md`
- `docs/central-postgres-search/README.md`

## Почему архитектура разделена именно так

Такое разделение даёт несколько практических преимуществ:

- синтаксис, семантика и IDE-функции можно развивать независимо;
- диагностики могут переиспользовать HIR, metadata и dataflow вместо прямого обхода AST;
- один и тот же набор вычислений обслуживает и LSP, и CLI, и MCP;
- экспериментальные search/backend-решения не требуют ломать основной анализатор.

## Полезные документы рядом

- `docs/architecture/DATAFLOW.md` — flow-sensitive анализ и текущее покрытие диагностик
- `docs/architecture/SEARCH_BASELINE_OVERLAY.md` — модель baseline + overlay
- `docs/configuration/PROJECT_CONFIGURATION.md` — структура конфигурации проекта
- `docs/configuration/DIAGNOSTICS.md` — параметры диагностических правил
- `docs/contributing/SALSA_GUIDE.md` — практические рекомендации по работе с Salsa
