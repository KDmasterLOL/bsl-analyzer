# Layer Review: Interface Adapters

## Граница слоя

Этот слой адаптирует внутренние модели и use cases к конкретным способам хранения, индексирования, конфигурирования и представления данных.

## Кандидаты на слой

- `ide-db`
- `base-db`
- `project-model`
- `vfs`
- `line-index`
- adapter-части `bsl-metadata`
- части `bsl-search`

## Вопросы ревью

- Какие интерфейсы здесь считаются boundary между inner и outer слоями?
- Не протекают ли детали `Salsa` в use cases и entities?
- Насколько `base-db` и `ide-db` являются адаптерами, а не носителями бизнес-правил?
- Где граница между in-memory model и filesystem/project config?
- Есть ли лишняя двусторонняя связанность между adapters и domain layer?

## Что искать как проблему

- query objects, которые становятся единственной формой доменного API;
- смешение read model, cache orchestration и бизнес-решений;
- adapters, которые знают слишком много о delivery protocols;
- отсутствие явных trait/boundary интерфейсов между use cases и infra.

## Результаты

### Findings

- `H`: `ide-db` сейчас является центральным adapter layer, но он слишком широк и смешивает несколько разных ролей в одной публичной поверхности.
  Подтверждение: [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L58) определяет `RootDatabase` как общий supertrait над `SourceDatabase`, `RootQueryDb`, `DefDatabase`, `HirDatabase`, `MetadataDb`; [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L74) и [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L115) держат в том же контракте и metadata loading, и SDBL/HIR surface, и CFG/dataflow batch queries.
  Это уже не просто adapter boundary, а одновременно runtime host, query facade и application-facing API.
  Вывод: `ide-db` стоит считать главным смешанным слоем между use cases и infra, а не “одним адаптером”.

- `H`: граница адаптера нарушена прямым downcast-ом из trait object в конкретную реализацию.
  Подтверждение: [`crates/ide-db/src/vfs_helpers.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/vfs_helpers.rs#L1) прямо документирует downcast; [`crates/ide-db/src/vfs_helpers.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/vfs_helpers.rs#L12) и [`crates/ide-db/src/vfs_helpers.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/vfs_helpers.rs#L17) требуют `RootDatabaseImpl`; при этом concrete FS/VFS logic живёт в [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L398) и [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L419).
  Это архитектурный smell сильнее обычного helper-а: интерфейс формально абстрактный, но реальный use-case-support код зависит от конкретного backend-а.
  Вывод: если нужен `get_file_path/find_configuration_root`, это должен быть явный port в contract, а не скрытый downcast.

- `M`: `SalsaProvider` и `AnalysisProvider` показывают правильное направление для adapter boundary, но этот boundary пока не стал главным.
  Подтверждение: [`crates/ide-db/src/provider.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/provider.rs#L20) формулирует явную abstraction над источником данных; [`crates/ide-db/src/salsa_provider.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/salsa_provider.rs#L20) вводит отдельную adapter implementation; [`crates/ide-db/src/salsa_provider.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/salsa_provider.rs#L44) делегирует вызовы в `RootDatabase`.
  Это хороший паттерн: use cases могут опираться на provider-level contract, а конкретный `Salsa` остаётся снаружи.
  Ограничение: сам `AnalysisProvider` пока всё ещё очень широкий и тащит почти весь query surface проекта.

- `M`: `base-db` в целом соответствует adapter/support layer, но его контракты уже глубоко “отформованы” под `Salsa`.
  Подтверждение: [`crates/base-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/base-db/src/lib.rs#L31) и [`crates/base-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/base-db/src/lib.rs#L95) формулируют database traits как `#[salsa::db]`; [`crates/base-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/base-db/src/lib.rs#L173) выносит mutable state helper `Files` за пределы `Salsa`; [`crates/base-db/src/input.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/base-db/src/input.rs#L16) и [`crates/base-db/src/input.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/base-db/src/input.rs#L56) смешивают logical source root и durability policy.
  Для adapter слоя это допустимо: здесь как раз и живут решения про file inputs, durability и runtime invalidation.
  Но такой API неудобно использовать как долгоживущую границу приложения, потому что outer runtime shape начинает диктовать форму внутренних контрактов.

- `M`: `DiagnosticsConfigInput` и raw JSON в `project-model` выглядят как зрелое adapter-решение против циклических зависимостей.
  Подтверждение: [`crates/base-db/src/input.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/base-db/src/input.rs#L123) прямо объясняет анти-cycle design для `DiagnosticsConfigInput`; [`crates/base-db/src/input.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/base-db/src/input.rs#L153) задаёт deterministic DTO для кэша; [`crates/project-model/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/project-model/src/lib.rs#L190) и [`crates/project-model/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/project-model/src/lib.rs#L203) хранят diagnostics config как raw JSON, не связывая crate напрямую с `ide-diagnostics`.
  Вывод: это хороший локальный пример того, как adapter layer может разрывать циклы и не тянуть use-case типы внутрь discovery/config кода.

- `M`: `project-model` в целом хорошо попадает в interface adapters, но внутри него смешаны DTO-конфиг и filesystem discovery heuristics.
  Подтверждение: [`crates/project-model/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/project-model/src/lib.rs#L31) строит `Project` как результат конфигурации и discovery; [`crates/project-model/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/project-model/src/lib.rs#L55) и [`crates/project-model/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/project-model/src/lib.rs#L127) содержат concrete path-search strategy; [`crates/project-model/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/project-model/src/lib.rs#L229) рядом держит и загрузку JSON-конфига.
  Для текущего масштаба проекта это практично.
  Но при дальнейшем росте стоит отделять “модель проекта/настройки” от “механизма поиска конфигурации на диске”.

- `M`: `vfs` выглядит как хороший adapter crate, но у `FileSet` есть явная проблема консистентности.
  Подтверждение: [`crates/vfs/src/file_set.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/vfs/src/file_set.rs#L40) обновляет обе стороны маппинга через простой `insert`; [`crates/vfs/src/file_set.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/vfs/src/file_set.rs#L143) и [`crates/vfs/src/file_set.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/vfs/src/file_set.rs#L160) сами фиксируют кейс, где `len() == 1`, но `iter().count() == 2`.
  Это уже не абстрактный стиль-вопрос, а риск для boundary-контракта: FileId↔Path mapping перестаёт быть однозначным.
  Вывод: `vfs` концептуально стоит на месте, но `FileSet` требует стабилизации инвариантов.

- `M`: `line-index` полезен для adapter/use-case boundary, но его лучше считать support utility, а не самостоятельным ключевым адаптером.
  Подтверждение: [`crates/line-index/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/line-index/src/lib.rs#L43) задаёт чистую value-transform структуру; [`crates/line-index/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/line-index/src/lib.rs#L57) и [`crates/line-index/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/line-index/src/lib.rs#L190) решают задачи конвертации offset/line/utf16 без знаний о проекте, файлах или runtime.
  Вывод: в архитектурной карте его лучше учитывать как инфраструктурную утилиту слоя, а не как часть основного boundary дизайна.

- `M`: metadata loading внутри `ide-db` остаётся concrete filesystem adapter и хорошо показывает, где должен заканчиваться inner layer.
  Подтверждение: [`crates/ide-db/src/metadata.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/metadata.rs#L34) вводит path-based input; [`crates/ide-db/src/metadata.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/metadata.rs#L68) реализует tracked query, которая вызывает [`bsl_metadata::load_from_directory`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/metadata.rs#L80); [`crates/ide-db/src/metadata.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/metadata.rs#L126) рядом держит path-based URI parsing для определения module type.
  Это не проблема само по себе: именно здесь и должны жить filesystem-specific metadata adapters.
  Проблема начинается тогда, когда этот concrete adapter становится напрямую видим use-case или delivery слоям.

### Срез по состоянию слоя

- Наиболее удачные adapter-компоненты:
  `vfs`
  `project-model`
  `SalsaProvider` + `AnalysisProvider`
  metadata/path-конур в `ide-db`

- Наиболее смешанные участки:
  `ide-db::RootDatabase`
  `base-db` как runtime-shaped boundary
  helper-ы с downcast к `RootDatabaseImpl`

- Главный structural risk слоя:
  use cases и delivery начинают опираться не на узкие порты, а на широкий `db/query` интерфейс.

### Decisions

- Для дальнейшего ревью считать целевым boundary слоя не `RootDatabase`, а более узкие provider/port интерфейсы наподобие [`AnalysisProvider`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/provider.rs#L32).

- `ide-db` трактовать как основной кандидат на переразделение:
  отдельно application-facing ports;
  отдельно `Salsa`/FS-backed implementations;
  отдельно runtime host/state.

- Downcast из `dyn RootDatabase` в `RootDatabaseImpl` считать запрещённым architectural smell, который надо устранять в первую очередь при рефакторинге границ.

- `project-model` и `vfs` оставить в adapter layer, но дальше смотреть их как два разных типа адаптеров:
  `project-model` как config/discovery adapter;
  `vfs` как identity/change-tracking adapter.

- `line-index` не использовать как аргумент в споре о слоях; это support utility, а не носитель boundary-политики.

- Следующий шаг ревью:
  перейти к `04-frameworks-drivers` и проверить, насколько outer entrypoints (`bsl-analyzer`, `mcp-server`, debug tooling, launcher) действительно используют adapter boundary, а не обходят его напрямую.
