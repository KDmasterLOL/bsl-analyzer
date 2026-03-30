# Layer Review: Frameworks & Drivers

## Граница слоя

Внешний слой, который подключает систему к миру:

- протоколы;
- процессы;
- наблюдение за файловой системой;
- запуск внешних инструментов;
- CLI и packaging.

## Кандидаты на слой

- `bsl-analyzer`
- `mcp-server`
- `vfs-notify`
- `onec-client`
- `bsl-launcher`
- `bsl-debug`
- `naparnik`
- `xtask`
- `extension`

## Вопросы ревью

- Насколько легко заменить или добавить новый delivery mechanism без изменения inner layers?
- Есть ли в entrypoints бизнес-логика или orchestration, которую надо вытолкнуть внутрь?
- Как outer layer управляет жизненным циклом, конкурентностью и ошибками?
- Нет ли прямых зависимостей outer компонентов на детали друг друга вместо use-case boundary?
- Можно ли тестировать delivery слой отдельно от предметного ядра?

## Что искать как проблему

- LSP/MCP handlers, содержащие domain decisions;
- прямой доступ к внутренним структурам в обход use-case API;
- сложные runtime concerns, протекающие в доменные сигнатуры;
- tight coupling между CLI/server/debug subsystems.

## Результаты

### Findings

- `H`: `bsl-analyzer` как LSP outer layer местами обходит application boundary и ходит прямо в adapter/query runtime.
  Подтверждение: [`crates/bsl-analyzer/src/handlers/notification.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/handlers/notification.rs#L20) планирует диагностики напрямую через `file_diagnostics_query`; [`crates/bsl-analyzer/src/handlers/notification.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/handlers/notification.rs#L52) конструирует `FileIdInput` и `DiagnosticsConfigId` в delivery code; [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs#L324) напрямую конфигурирует `set_all_config_paths`; [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs#L566) и [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs#L591) сами вызывают metadata query/runtime API.
  Для outer слоя это уже слишком глубокое знание о внутренних адаптерах и runtime-shaped контрактах.
  Вывод: LSP driver местами выступает не только delivery-механизмом, но и ручным orchestrator-ом adapter/query machinery.

- `H`: `GlobalState` разросся в runtime god-object и смешивает слишком много обязанностей внешнего слоя.
  Подтверждение: [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs#L51) держит в одном типе transport, request queue, mutable analysis host, VFS, loader, task pool, project config и diagnostics config; [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs#L118) добавляет сюда conversion project config -> diagnostics policy; [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs#L303) реализует workspace/project/bootstrap orchestration; [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs#L393) и [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs#L516) управляют sync VFS↔Salsa и SourceRoot lifecycle.
  Это повышает стоимость тестирования, усложняет замену delivery/runtime частей и затрудняет отделение outer concerns от application orchestration.
  Вывод: внешний слой сейчас централизован функционально, но архитектурно перегружен.

- `M`: основной LSP event loop и `vfs-notify` выглядят как удачные outer-layer компоненты.
  Подтверждение: [`crates/bsl-analyzer/src/server.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/server.rs#L32) и [`crates/bsl-analyzer/src/server.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/server.rs#L106) формулируют `main_loop` и event dispatch как composition root без предметной логики языка; [`crates/vfs-notify/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/vfs-notify/src/lib.rs#L1) прямо позиционируется как `loader::Handle` implementation; [`crates/vfs-notify/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/vfs-notify/src/lib.rs#L37) и [`crates/vfs-notify/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/vfs-notify/src/lib.rs#L97) действительно ограничиваются glue-кодом вокруг `notify`, `walkdir` и загрузчика.
  Это хороший ориентир: outer layer силён там, где остаётся composition/glue, а не тащит application policy внутрь.

- `M`: request handlers LSP в основном тонкие, но transport mapping дублируется, а местами protocol-state просачивается в application API.
  Подтверждение: [`crates/bsl-analyzer/src/handlers/request.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/handlers/request.rs#L22), [`crates/bsl-analyzer/src/handlers/request.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/handlers/request.rs#L149) и [`crates/bsl-analyzer/src/handlers/request.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/handlers/request.rs#L195) повторяют один и тот же сценарий `URI -> FileId -> LineIndex -> offset -> Analysis -> LSP DTO`; [`crates/bsl-analyzer/src/handlers/request.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/handlers/request.rs#L251) передаёт `workspace_root` прямо в completion use case.
  Само по себе такое дублирование типично для delivery слоя.
  Но оно показывает, что между transport mapping и use cases не хватает более узких входных контрактов.

- `M`: `mcp-server` в целом хорошо ведёт себя как outer driver, но его shared state уже стал очень широким composition object.
  Подтверждение: [`crates/mcp-server/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/mcp-server/src/lib.rs#L13) и [`crates/mcp-server/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/mcp-server/src/lib.rs#L183) показывают корректную роль tool router/server handler; [`crates/mcp-server/src/tools/query.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/mcp-server/src/tools/query.rs#L13) и [`crates/mcp-server/src/tools/query.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/mcp-server/src/tools/query.rs#L84) держат transport-level validation/formatting; [`crates/mcp-server/src/tools/debug.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/mcp-server/src/tools/debug.rs#L26) оборачивает blocking debug API в MCP-friendly форму.
  При этом [`crates/mcp-server/src/state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/mcp-server/src/state.rs#L20) объединяет metadata, extensions, workspace root, live 1C client, debug session и search engine в одном shared object; [`crates/mcp-server/src/state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/mcp-server/src/state.rs#L31) и [`crates/mcp-server/src/state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/mcp-server/src/state.rs#L99) добавляют туда ещё и режимы standalone/shared.
  Вывод: как драйвер слой выглядит хорошо, но composition state уже стоит дробить по capability-наборам.

- `M`: `onec-client` является хорошим примером чистого gateway-driver.
  Подтверждение: [`crates/onec-client/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/onec-client/src/lib.rs#L26) вводит изолированный `Client`; [`crates/onec-client/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/onec-client/src/lib.rs#L60) и [`crates/onec-client/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/onec-client/src/lib.rs#L91) ограничиваются HTTP endpoint-ами, сериализацией и error mapping.
  Это почти эталонный outer adapter: транспорт и DTO, без знания о внутренней архитектуре анализатора.

- `M`: `naparnik` оказался не “непонятным внешним хвостом”, а достаточно чисто оформленным внешним AI-subsystem с явным port-pattern.
  Подтверждение: [`crates/naparnik/src/client.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/naparnik/src/client.rs#L7) задаёт внешний контракт `NaparnikApi`; [`crates/naparnik/src/completion.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/naparnik/src/completion.rs#L9) оформляет `InlineCompletionUseCase`; [`crates/naparnik/src/http_client.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/naparnik/src/http_client.rs#L20) реализует HTTP-клиент отдельно от use case orchestration.
  Вывод: по внутренней структуре `naparnik` ближе к `bsl-launcher`, чем к проблемному `bsl-analyzer`: внешний порт, отдельный use case и отдельный transport adapter.

- `M`: `bsl-debug` тоже по сути внешний драйвер, но в нём raw protocol client и high-level session workflow живут в одном контуре.
  Подтверждение: [`crates/bsl-debug/src/session.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-debug/src/session.rs#L18) позиционирует `DebugSession` как high-level API для AI/CLI; [`crates/bsl-debug/src/session.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-debug/src/session.rs#L121) объединяет attach/init/indexing/auto-attach/event listener в одном lifecycle; [`crates/mcp-server/src/tools/debug.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/mcp-server/src/tools/debug.rs#L1) использует этот API как готовый driver boundary.
  Для outer layer это допустимо.
  Но если debug tooling будет расти, тут тоже появится смысл отделять protocol transport от session-level сценариев.

- `M`: `bsl-launcher` интересен как отдельный внешний subsystem, который уже сам внутри себя следует чистому разделению лучше, чем основной LSP runtime.
  Подтверждение: [`crates/bsl-launcher/src/main.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-launcher/src/main.rs#L14) разделяет `entities`, `provider`, `use_cases`; [`crates/bsl-launcher/src/use_cases.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-launcher/src/use_cases.rs#L68) и [`crates/bsl-launcher/src/use_cases.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-launcher/src/use_cases.rs#L146) концентрируют application logic загрузки/обновления; [`crates/bsl-launcher/src/provider.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-launcher/src/provider.rs#L8) задаёт внешний port `ReleaseProvider`; [`crates/bsl-launcher/src/entities.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-launcher/src/entities.rs#L5) держит DTO/manifest отдельно.
  Вывод: это локальный положительный контрпример, показывающий, что проектная команда уже умеет строить outer subsystem с более чистыми границами.

- `M`: `extension` действительно относится к outer layer, но его надо понимать не как часть ядра анализатора, а как platform-side companion service.
  Подтверждение: [`extension/src/HTTPServices/BSLAnalyzerService/Ext/Module.bsl`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/extension/src/HTTPServices/BSLAnalyzerService/Ext/Module.bsl#L127) реализует HTTP entrypoint для query execution; [`extension/src/HTTPServices/BSLAnalyzerService/Ext/Module.bsl`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/extension/src/HTTPServices/BSLAnalyzerService/Ext/Module.bsl#L221) и [`extension/src/HTTPServices/BSLAnalyzerService/Ext/Module.bsl`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/extension/src/HTTPServices/BSLAnalyzerService/Ext/Module.bsl#L258) реализуют platform-backed validation/execution endpoints.
  Здесь есть прикладные решения уровня “что можно выполнять и как сериализовать данные”, но они намеренно живут по другую сторону boundary, внутри 1С runtime.
  Вывод: `extension` не стоит смешивать с внутренней архитектурой анализатора; это внешний companion driver, который обслуживает `onec-client`/MCP integration.

### Срез по состоянию слоя

- Самые удачные драйверы:
  `vfs-notify`
  `onec-client`
  `naparnik`
  значительная часть `mcp-server`
  `bsl-launcher`
  `extension` как platform-side companion service

- Самый проблемный внешний компонент:
  `bsl-analyzer` LSP runtime из-за разросшегося `GlobalState` и прямого доступа к query/runtime API

- Главный риск слоя:
  outer entrypoints знают слишком много о `Salsa`, `FileIdInput`, metadata cache invalidation и runtime storage деталях.

### Decisions

- Для дальнейшего ревью считать целевыми composition roots внешнего слоя:
  `bsl-analyzer` для LSP;
  `mcp-server` для MCP;
  `bsl-launcher` как отдельный delivery subsystem.

- Внешнему слою нежелательно напрямую работать с:
  `file_diagnostics_query`
  `ConfigurationPathInput`
  concrete `RootDatabaseImpl` lifecycle methods
  низкоуровневыми `Salsa` input/interned типами

- При рефакторинге первым кандидатом на упрощение считать `GlobalState`:
  отделить transport/runtime state;
  отделить project/bootstrap orchestration;
  отделить diagnostics scheduling и metadata warmup в application/service boundary.

- `mcp-server` пока можно считать архитектурно приемлемым outer driver, но его `SharedState` стоит рассматривать как будущую точку декомпозиции по capability-группам:
  metadata/search/debug/live-DB.

- `bsl-launcher` использовать как локальный reference-example того, как outer subsystem может быть устроен с явными `entities/use_cases/providers`.

- После фиксации внешнего слоя следующий полезный шаг:
  собрать сводный список cross-layer violations и наметить приоритетный рефакторинг на границе `use cases <-> adapters <-> drivers`.
