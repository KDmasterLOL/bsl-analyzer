# Layer Review: Entities

## Граница слоя

Здесь должны жить устойчивые правила предметной области анализа BSL:

- синтаксическая модель;
- семантическая модель;
- типы платформы;
- модель metadata;
- базовые модели CFG/dataflow;
- инварианты языка и анализа, не зависящие от LSP, файловой системы, JSON-RPC, watcher-ов и конкретного кеша.

## Кандидаты на слой

- `lexer`, `parser`, `syntax`
- `hir-def`, `hir-ty`, `hir`
- `sdbl-hir`
- `cfg-types`, `cfg`, `dataflow`
- `bsl-platform`
- domain-часть `bsl-metadata`

## Вопросы ревью

- Какие типы и инварианты являются действительно предметными и должны быть максимально стабильны?
- Есть ли здесь зависимости на `Salsa`, `VFS`, `LSP`, `serde`, `notify`, `tokio` или другие outer concerns?
- Не смешана ли модель с механизмом вычисления или кеширования?
- Можно ли выразить контракты без привязки к конкретному storage/query runtime?
- Где модель domain level зависит от формата designer XML вместо абстракции metadata?

## Что искать как проблему

- доменные типы, знающие про файловую систему или transport protocol;
- query/runtime-specific типы в сигнатурах ядра;
- низкоуровневые парсеры и loader-ы внутри чистых моделей;
- дублирование инвариантов между `syntax`, `hir-*`, `cfg`, `dataflow`.

## Результаты

### Findings

- `H`: Синтаксическое ядро уже выглядит как хороший inner core.
  Подтверждение: [`lexer/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/lexer/src/lib.rs#L1), [`parser/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/parser/src/lib.rs#L23), [`syntax/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/syntax/src/lib.rs#L39).
  Здесь API строится вокруг текста, токенов, CST/AST и ошибок парсинга, без знаний о `Salsa`, `VFS`, `LSP` или файловой системе.
  Вывод: `lexer`, `parser`, `syntax` можно считать эталонной частью слоя Entities.

- `H`: `hir-def` сейчас не является чистым entity-layer, а смешивает модель предметной области, query runtime и workspace/storage concerns.
  Подтверждение: [`hir-def/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/hir-def/src/lib.rs#L73) re-export-ит query functions; [`hir-def/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/hir-def/src/lib.rs#L130) объявляет `#[salsa::db] pub trait DefDatabase`; [`hir-def/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/hir-def/src/lib.rs#L220) держит placeholder `module_metadata`, завязанный на реализацию в `ide-db`; [`hir-def/src/queries.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/hir-def/src/queries.rs#L66) содержит tracked queries как часть публичного слоя.
  Дополнительно: [`hir-def/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/hir-def/src/lib.rs#L321) включает workspace-level API (`workspace_index`, `file_dependencies`), что относится скорее к adapter/application boundary, а не к сущностям языка.
  Вывод: предметная модель HIR здесь хорошая, но crate логически смешанный.

- `H`: Идентичность доменных сущностей привязана к механизму хранения файлов.
  Подтверждение: [`hir-def/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/hir-def/src/lib.rs#L371) определяет `ModuleId` через `vfs::FileId`, а [`hir-def/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/hir-def/src/lib.rs#L447) тащит `file_id` в `ModuleData`.
  Это удобный компромисс для инкрементального анализа, но с точки зрения Clean Architecture доменная идентичность модуля становится зависимой от адаптера хранения.
  Последствие: любое расширение на другой source backend будет тянуть `VFS` в центр модели.

- `M`: `hir-ty` продолжает тот же паттерн смешения, что и `hir-def`.
  Подтверждение: [`hir-ty/src/db.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/hir-ty/src/db.rs#L26) объявляет `#[salsa::db] pub trait HirDatabase`, а API типизации работает через `FileId` и query-ориентированные методы.
  Вывод: алгоритмы inference относятся к inner core, но текущий публичный слой `hir-ty` описывает скорее adapter-facing API, чем чистую модель типов.

- `H`: `hir` не стоит считать entity crate, хотя по названию он выглядит именно так.
  Подтверждение: [`hir/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/hir/src/lib.rs#L30) re-export-ит `DefDatabase`; [`hir/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/hir/src/lib.rs#L76) re-export-ит Salsa queries; [`hir/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/hir/src/lib.rs#L91) строит `Module<'db, DB>` как оболочку вокруг database dependency.
  Это уже façade над runtime/query layer, а не “чистое HIR”.
  Вывод: в терминах ревью `hir` надо относить ближе к use-case-support или adapter facade, а не к Entities.

- `M`: `cfg` и `dataflow` в целом ближе к entities, но с важной оговоркой: “generic framework” уже жёстко связан с BSL-specific HIR/CFG representation.
  Подтверждение: [`dataflow/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/dataflow/src/lib.rs#L43) зависит от `cfg::ControlFlowGraph`, [`dataflow/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/dataflow/src/lib.rs#L44) зависит от `hir_def::body::Body`; [`cfg/src/builder.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/cfg/src/builder.rs#L54) строит граф из HIR Body.
  Это не outer-layer violation, но означает, что “generic” здесь надо понимать как generic внутри домена анализа BSL, а не как независимое ядро анализа потока данных.
  Вывод: оставлять в Entities допустимо, но полезно явно признать зависимость `dataflow -> cfg -> hir-def`.

- `M`: `sdbl-hir` выглядит как ещё один удачный кусок entity-layer, который в текущем ревью был недоразобран.
  Подтверждение: [`sdbl-hir/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/sdbl-hir/src/lib.rs#L1) позиционирует crate как semantic representation SDBL; [`sdbl-hir/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/sdbl-hir/src/lib.rs#L43) и [`sdbl-hir/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/sdbl-hir/src/lib.rs#L54) экспортируют чистые model/lowering/source-map компоненты; [`sdbl-hir/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/sdbl-hir/src/lib.rs#L82) и [`sdbl-hir/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/sdbl-hir/src/lib.rs#L209) выражают completion context через typed domain enums, а не через runtime-specific API.
  Дополнительно важно, что в source tree crate не видно зависимостей на `Salsa`, `VFS`, `tokio`, `notify` или файловую систему.
  Вывод: `sdbl-hir` можно считать сильным кандидатом в semantic inner core. Его основная зависимость не на outer runtime, а на соседний domain reference data слой (`bsl_metadata`) и `syntax`/`text_size`, что для этой области допустимо.

- `M`: `cfg` имеет документарную неоднозначность, которая мешает архитектурному чтению слоя.
  Подтверждение: [`cfg/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/cfg/src/lib.rs#L16) всё ещё говорит “Constructs CFG from Rowan AST”, тогда как [`cfg/src/builder.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/cfg/src/builder.rs#L3) и код builder-а уже явно HIR-based.
  Это не дефект слоя как такового, но риск неверной архитектурной коммуникации.

- `H`: `bsl-metadata` объединяет в одном crate и предметную модель metadata, и designer-format adapter.
  Подтверждение: [`bsl-metadata/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-metadata/src/lib.rs#L93) экспортирует одновременно модель, `loader` и `xml_parser`; [`bsl-metadata/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-metadata/src/lib.rs#L126) re-export-ит `load_from_directory`; [`bsl-metadata/src/loader.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-metadata/src/loader.rs#L60) читает директорию конфигурации; [`bsl-metadata/src/loader.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-metadata/src/loader.rs#L109) содержит filesystem+parallel loading orchestration.
  Вывод: доменная модель metadata существует, но она не отделена от инфраструктурного способа её загрузки.

- `M`: часть модели `bsl-metadata` тоже содержит I/O/parser concern прямо внутри сущности `Configuration`.
  Подтверждение: [`bsl-metadata/src/configuration.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-metadata/src/configuration.rs#L164) реализует `from_xml_file`, а [`bsl-metadata/src/configuration.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-metadata/src/configuration.rs#L170) парсит XML внутри самой модели.
  Для inner entity логичнее иметь отдельно pure model и отдельно factory/mapper/parser.

- `M`: `bsl-platform` тоже смешанный, но менее проблемный, чем `bsl-metadata`.
  Подтверждение: [`bsl-platform/src/db.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-platform/src/db.rs#L3) прямо заявляет dual role: Salsa queries и singleton; [`bsl-platform/src/db.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-platform/src/db.rs#L20) содержит чистую индексированную модель `PlatformDataInner`; одновременно `Cargo.toml` и codebase добавляют tracked/interned query API.
  Вывод: platform catalog сам по себе ближе к Entities, но query facade стоит отделить концептуально, а лучше и физически.

### Срез по состоянию слоя

- Уверенно относятся к Entities:
  `lexer`, `parser`, `syntax`
  `sdbl-hir`
  большая часть `cfg`, `dataflow`
  модельные типы из `hir-def`, `hir-ty`
  модельные типы из `bsl-platform`
  модельные типы из `bsl-metadata`

- Смешанные crate, которые нельзя безоговорочно считать Entities:
  `hir-def`
  `hir-ty`
  `hir`
  `bsl-metadata`
  `bsl-platform`

### Decisions

- Для дальнейшего ревью считать “entity layer проекта” не по crate boundaries, а по подмножествам внутри crate-ов.

- Рабочее разделение на внутренние подслои:
  `syntax core`: `lexer` + `parser` + `syntax`
  `semantic core`: model-часть `hir-def` + `hir-ty`
  `analysis core`: `cfg` + `dataflow` + domain-часть `sdbl-hir`
  `reference data core`: model-части `bsl-platform` и `bsl-metadata`

- При последующих ревью использовать следующую гипотезу на вынос из Entities:
  из `hir-def` и `hir-ty` надо выдавливать `Salsa db/query` API в adapter layer;
  из `bsl-metadata` надо выдавливать `loader/xml_parser/fs` в adapter layer;
  из `bsl-platform` надо отделять pure catalog от query/runtime facade.

- Следующий архитектурный шаг после фиксации слоя:
  в `02-use-cases` отдельно проверить, не компенсирует ли `ide`/`ide-db` текущее смешение слоёв ещё большим смешением orchestration и adapter logic.
