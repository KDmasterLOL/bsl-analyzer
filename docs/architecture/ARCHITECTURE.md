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

- `ide-diagnostics` — набор диагностик качества кода;
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

#### Норма привязки тривии

**Тривия принадлежит общему предку соседних значимых токенов.** Отсюда
свойство, на которое можно опираться: ни один узел, кроме корня, не начинается
и не кончается пробелом, переводом строки или комментарием, а `node.text()` и
`node.text_range()` покрывают ровно значимые токены узла. Корень исключён по
необходимости — хвост файла держать больше негде.

Норма объявлена в `Sink` (`crates/parser/src/sink.rs`) и держится его
устройством, а не дисциплиной авторов правил: тривия копится в буфере и уходит
билдеру только перед значимым токеном, а открытие узла откладывается до его
первого значимого токена. Правило грамматики, съевшее пробел ради заглядывания
вперёд, на дерево не влияет — а таких вызовов `skip_trivia` в грамматике
подавляющее большинство, и `Marker::complete` закрывает узел по текущей позиции
парсера независимо от того, зачем тривия была съедена.

Что из этого следует потребителям:

- точка с запятой отделена от своего оператора тривией и НЕ является его
  непосредственным соседом; шаг к ней — `syntax::trailing_semicolon`;
- подрезать хвостовой пробел у диапазона узла не нужно;
- узел без единого значимого токена пуст, и такие узлы (обычно `ERROR`
  пропущенного элемента) стоят на краях узлов постоянно. Обход токенов rowan
  об пустой узел спотыкается — для шага по токенам есть
  `syntax::prev_token_past_empty` и `syntax::next_token_past_empty`.

Гейт нормы — `crates/parser/tests/trivia_attachment.rs`. Строчная
чувствительность (может ли конструкция пересечь перевод строки) нормой
привязки не закрывается и живёт отдельно.

## Основные инфраструктурные крейты

| Крейты | Назначение |
|--------|------------|
| `base-db` | базовые Salsa-input'ы, source roots, file text |
| `vfs`, `vfs-notify` | виртуальная файловая система и слежение за файлами |
| `project-model` | загрузка конфигурации проекта, legacy JSON-конфигов и search-настроек |
| `bsl-metadata` | чтение `Configuration.xml` и метаданных 1С |
| `bsl-platform` | модели платформенных типов, API и курируемые оверлеи |
| `cfg`, `cfg-types` | control-flow graph |
| `dataflow` | reaching definitions и liveness analysis |
| `bsl-search` | индексирование и поиск по коду/документации |
| `mcp-server` | MCP runtime и интеграция с AI-клиентами |

### Владение данными и диагностиками

В проекте соблюдается строгое разделение ответственности между слоями:
- **Факты и метаданные**: Знания о платформе (включая [курируемые оверлеи](TYPE_SYSTEM.md#платформенные-оверлеи-platform-overlays)) и метаданных проекта принадлежат нижним слоям (`bsl-platform`, `bsl-metadata`).
- **Семантический анализ**: Логика вывода типов и разрешения имен сосредоточена в `hir-ty` и `hir-def`.
- **Диагностики**: Обнаружение семантических несоответствий происходит в `hir-ty`; оформление и выдача сообщений пользователю — в `ide-diagnostics`.

Это гарантирует, что семантический слой остается "чистым" вычислителем, а диагностический слой — тонким потребителем этих вычислений.

## Salsa и модель вычислений

Проект построен вокруг инкрементальных вычислений на базе Salsa `0.26`.

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

### LSP-диспетчинг запросов

`RequestDispatcher` (`crates/bsl-analyzer/src/handlers/dispatch.rs`)
раздаёт LSP-запросы по трём каналам:

- `on_sync_mut` — mutable-хендлеры на main thread (shutdown, reload).
- `on_sync` — read-хендлеры на main thread. Сейчас здесь живут только
  `Formatting` / `RangeFormatting` / `OnTypeFormatting`: клиенты делают
  save-and-format синхронно, а форматтер быстрый.
- `on_latency` — read-хендлеры на task pool. Все latency-чувствительные
  запросы (`textDocument/definition`, `hover`, `completion`, `references`,
  `documentSymbol`, `semanticTokens/full`, `codeAction`, `signatureHelp`)
  уходят сюда и не блокируют main loop.

`on_latency` на main thread:

1. Клонирует owned Salsa DB (snapshot) и берёт у него
   `salsa::CancellationToken`.
2. Регистрирует токен в `GlobalState.request_tokens` по `RequestId`;
   при повторе id предыдущий токен отменяется.
3. Замораживает `MemDocs` и пути VFS в `LatencyRequestContext` (см.
   `crates/bsl-analyzer/src/frozen_context.rs`). Worker читает только
   этот снимок и не видит последующих `didChange`.

На task-pool потоке worker обернут в два слоя защиты —
`salsa::Cancelled::catch` конвертирует кооперативную отмену в
`ErrorCode::RequestCanceled`, а внешний `std::panic::catch_unwind`
логирует любой панический payload + backtrace и возвращает
`ErrorCode::InternalError`. Инвариант: **на каждый dispatch ровно один
`Task::RequestResult`**.

`$/cancelRequest` на main thread удаляет токен из `request_tokens` и
вызывает `cancel()`. Worker унывает на следующей границе salsa-запроса
— так же, как диагностика после `didChange`.

Cancellation-карты (`diagnostics_tokens`, `preload_tokens`,
`preload_external_tokens`, `request_tokens`) живут на `GlobalState`;
каждая описывает свой owner и lifecycle в doc-комментариях.

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
