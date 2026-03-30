# Архитектурное ревью по слоям Clean Architecture

Цель: пройти проект последовательно по слоям Martin Clean Architecture, не смешивая предметное ядро, сценарии анализа, адаптеры и внешние драйверы.

Этот разрез не заменяет текущую crate-структуру из `docs/architecture/ARCHITECTURE.md`, а накладывается поверх неё как рамка для ревью.

## Предлагаемая последовательность

1. `00-context`
   - Зафиксировать границы системы, основные потоки данных и спорные места.
2. `01-entities`
   - Проверить предметное ядро: языковая модель, семантика, типы, CFG/dataflow, модель metadata.
3. `02-use-cases`
   - Проверить сценарии: диагностики, assists, high-level IDE API, orchestration анализа.
4. `03-interface-adapters`
   - Проверить адаптеры между use cases и инфраструктурой: Salsa DB, project model, VFS-facing abstractions, преобразование данных.
5. `04-frameworks-drivers`
   - Проверить внешние входы и рантайм: LSP, MCP, debug, launcher, watcher, extension, build tooling.
6. `05-cross-layer`
   - Свести нарушения между слоями, подтвердить целевые границы и зафиксировать приоритетный порядок рефакторинга.

## Базовый принцип ревью

Для каждого слоя смотрим одно и то же:

- какова его ответственность;
- от кого он зависит;
- кто зависит от него;
- где нарушено направление зависимостей;
- где в слой протекли детали соседнего слоя;
- какие инварианты и контракты не выражены явно.

## Рабочая гипотеза по проекту

### 1. Entities

Предметное ядро статического анализа BSL:

- `lexer`, `parser`, `syntax`
- `hir-def`, `hir-ty`, `hir`
- `sdbl-hir`
- `cfg-types`, `cfg`, `dataflow`
- `bsl-platform`
- существенная часть `bsl-metadata`

### 2. Use Cases

Сценарии, ради которых существует система:

- `ide-diagnostics`
- `ide-assists`
- `ide`
- части `hir` и `ide-db`, если они уже не описывают сущности, а оркестрируют сценарии

### 3. Interface Adapters

Адаптеры между ядром/сценариями и конкретным хранением/представлением:

- `ide-db`
- `base-db`
- `project-model`
- `vfs`
- `line-index`
- части `bsl-search`

### 4. Frameworks & Drivers

Наружные механизмы запуска и интеграции:

- `bsl-analyzer`
- `mcp-server`
- `vfs-notify`
- `onec-client`
- `bsl-launcher`
- `bsl-debug`
- `naparnik`
- `xtask`
- `extension`

## На что я бы обратил особое внимание

- `Salsa` и query-модель могут протекать слишком глубоко во внутренние слои.
- `ide-db` выглядит как смешанный слой: и adapter, и application service.
- `bsl-metadata` частично похоже на entity layer, частично на infrastructure parser/loader.
- `hir` и `cfg/dataflow` надо развести: где чистая модель анализа, а где уже сервисы для use cases.
- внешний API (`LSP`, `MCP`, debug/execution) стоит ревьюить только после фиксации внутренних контрактов.

## Следующий практический шаг

Начать с файла `00-context/system-map.md`, затем идти по папкам сверху вниз и фиксировать выводы прямо в соответствующих `review.md`.
