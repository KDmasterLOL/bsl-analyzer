# Layer Review: Use Cases

## Граница слоя

Здесь должны жить сценарии системы:

- вычисление диагностик;
- assists;
- high-level IDE operations;
- orchestration между semantic ядром, metadata и analysis services.

## Кандидаты на слой

- `ide-diagnostics`
- `ide-assists`
- `ide`
- orchestration-часть `ide-db`

## Вопросы ревью

- Какие пользовательские сценарии считаются основными и где они реализованы?
- Не зависят ли use cases напрямую от transport/runtime деталей?
- Не размазана ли orchestration между `ide`, `ide-db`, `hir` и entrypoints?
- Есть ли явные input/output контракты use case-ов?
- Как устроена конфигурация диагностик: это policy уровня use cases или infrastructural detail?

## Что искать как проблему

- сценарии, захардкоженные в LSP handlers;
- доменная логика, уехавшая в adapters или entrypoints;
- разрастание `ide-db` в god-object;
- трудность тестирования use case-ов без полного рантайма.

## Результаты

### Findings

- `H`: `ide-diagnostics` сейчас является самым явным use-case layer в проекте.
  Подтверждение: [`ide-diagnostics/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/lib.rs#L74) задаёт единый entry point `diagnostics(ctx)` и явный порядок исполнения сценария; [`ide-diagnostics/src/runner.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/runner.rs#L8) раскладывает диагностики по application-срезам данных; [`ide-diagnostics/src/runner.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/runner.rs#L200) показывает orchestration по категориям, а не по transport/UI.
  Вывод: диагностики реально оформлены как сценарии системы, а не как случайный набор helper-ов.

- `M`: внутри `ide-diagnostics` слой use cases уже начинает смешиваться с adapter concerns, но команда это частично осознаёт и уже строит boundary через provider/context.
  Подтверждение: [`ide-diagnostics/src/context.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/context.rs#L10) поддерживает два режима исполнения; [`ide-diagnostics/src/context.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/context.rs#L140) вводит dispatch через `AnalysisProvider`; [`ide-db/src/provider.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/provider.rs#L20) формулирует abstraction над источником данных.
  Но тот же `DiagnosticsContext` всё ещё тащит `workspace_root`, `configuration_path`, `file_set` и knowledge о `ConfigurationPathInput` в application API: [`context.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/context.rs#L29), [`context.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/context.rs#L85).
  Вывод: use-case boundary уже намечен, но ещё не очищен.

- `H`: отдельные diagnostic handlers всё ещё знают слишком много про filesystem/VFS, то есть use-case слой местами протекает во внешний адаптерный контур.
  Подтверждение: [`missing_event_subscription_handler.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/handlers/missing_event_subscription_handler.rs#L331) сам собирает absolute path из `workspace_root`, URI и `VfsPath`, а затем ветвится между `file_set` и provider/db resolution.
  Это уже не policy уровня “проверить существование обработчика”, а техническая логика резолва ресурса.
  Вывод: часть handler-ов надо упрощать до чистых use cases, выталкивая resolution logic в adapter/service layer.

- `H`: `ide-db` нельзя считать только adapter crate; он уже содержит use-case surface проекта.
  Подтверждение: [`ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L58) определяет `RootDatabase` как главный интерфейс IDE-операций; [`ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L85) включает сценарии уровня приложения (`sdbl_hir_in_file`, `module_cfgs`, `module_reaching_definitions`, `module_liveness_analysis`), а не только низкоуровневый доступ к данным.
  Вывод: `ide-db` сейчас логически смешивает application query surface и infrastructure host.

- `H`: одновременно `ide-db` остаётся и infrastructure host, поэтому граница между use cases и adapters размыта прямо в одном crate.
  Подтверждение: [`ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L323) держит `RootDatabaseImpl` с `salsa::Storage`, `Files`, `metadata_version` и registered config paths; [`ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L398) и [`ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L411) реализуют filesystem-конкретное разрешение путей; [`ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L616) и [`ide-db/src/metadata.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/metadata.rs#L68) грузят configuration с диска.
  Вывод: application API и adapter implementation нужно разделять хотя бы концептуально, а лучше и по модулям/крейтам.

- `M`: `ide` выступает как внешний application facade над сценариями и в этом смысле полезен, но сам по себе почти не выражает отдельную policy.
  Подтверждение: [`ide/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide/src/lib.rs#L35) даёт единый `Analysis`; [`ide/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide/src/lib.rs#L60) и далее в основном маршрутизирует вызовы в diagnostics/goto/completion/hover/formatting.
  Это хороший “application entry facade”, но orchestration почти целиком живёт ниже, а не в самом `Analysis`.
  Вывод: `ide` сейчас скорее composition layer над use cases, чем самостоятельный слой сценариев.

- `M`: use cases для базовых IDE-фич реализованы прямо в feature-модулях `ide`, и это в целом нормально, но у них мало явных boundary-интерфейсов.
  Подтверждение: [`ide/src/goto_definition.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide/src/goto_definition.rs#L24) и [`ide/src/hover.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide/src/hover.rs#L20) принимают `RootDatabase` напрямую и сразу работают с parse/tree/definition API.
  Вывод: сценарии оформлены как функции, но не отделены от конкретного db contract. Для небольших сценариев это допустимо, однако зависимость от `RootDatabase` остаётся широкой.

- `M`: в проекте уже есть локальный пример более чистого use-case дизайна, и он показывает целевое направление.
  Подтверждение: [`ide/src/completion/sdbl/mod.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide/src/completion/sdbl/mod.rs#L1) прямо декларирует `domain / use_cases / infrastructure`; [`ide/src/completion/sdbl/mod.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide/src/completion/sdbl/mod.rs#L49) создаёт providers и передаёт их в use cases; [`complete_mdo.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide/src/completion/sdbl/use_cases/complete_mdo.rs#L64) работает через `MetadataProvider`, а не через `RootDatabase`.
  Вывод: это лучший локальный эталон use-case слоя в кодовой базе.

- `M`: `file_diagnostics_query` находится в `ide-diagnostics`, но по сути смешивает application сценарий с cache/runtime contract.
  Подтверждение: [`ide-diagnostics/src/query.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/query.rs#L21) делает use case tracked-query, а [`ide-diagnostics/src/query.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/query.rs#L33) конструирует context под нужды runtime.
  Это не обязательно ошибка, но важный сигнал: application contract сейчас выражается через `Salsa`-форму, а не через независимый port.

- `H`: слой assists как application area фактически отсутствует.
  Подтверждение: [`ide-assists/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-assists/src/lib.rs#L1) содержит только DTO-структуры, а [`ide-assists/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-assists/src/lib.rs#L40) оставляет `TODO`; при этом [`ide/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide/src/lib.rs#L131) `code_actions()` тоже пока пустой.
  Вывод: use-case слой проекта пока сильно перекошен в diagnostics и read-only IDE features.

### Срез по состоянию слоя

- Самые зрелые use cases:
  diagnostics orchestration в `ide-diagnostics`
  read-only IDE scenarios в `ide` (`goto_definition`, `hover`, `references`, `signature_help`)
  SDBL completion как локально вычищенный пример

- Самые смешанные участки:
  `ide-db` как одновременно query surface и infra host
  `DiagnosticsContext` как boundary, в который протекли runtime/workspace детали
  отдельные handlers, знающие про путь/файлы/VFS

- Самая явная функциональная дыра слоя:
  `ide-assists` / `code_actions`

### Decisions

- Для дальнейшего ревью считать use-case layer не crate-равным `ide`, а composed layer из:
  `ide-diagnostics`
  feature-модулей `ide`
  application-facing части `ide-db`

- Локальный эталон для проектного style guide use-case слоя:
  SDBL completion subtree в [`crates/ide/src/completion/sdbl`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide/src/completion/sdbl)
  Там уже есть осмысленное разделение на `domain`, `use_cases`, `infrastructure`.

- Рабочая гипотеза на рефакторинг:
  `RootDatabase` и `file_diagnostics_query` должны стать адаптером исполнения use cases, а не их основной формой представления.

- Следующий шаг в ревью:
  перейти к `03-interface-adapters` и отдельно разобрать `ide-db`, `base-db`, `project-model`, `vfs` и metadata loading как boundary/infrastructure слой.
