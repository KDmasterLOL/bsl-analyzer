# Cross-Layer Summary

## Зачем нужен этот срез

После послойного ревью стало видно, что основные проблемы проекта лежат не внутри отдельных crate-ов, а на стыках слоёв:

- где use cases выражены через query/runtime API;
- где adapter layer стал фактическим application facade;
- где outer drivers знают слишком много о конкретной реализации runtime;
- где domain model связана с VFS/filesystem identity.

Этот файл фиксирует уже подтверждённые cross-layer нарушения и порядок, в котором их имеет смысл разбирать.

## Подтверждённые границы

- Внутреннее синтаксическое ядро уже существует и достаточно чисто:
  `lexer` + `parser` + `syntax`.

- Use-case слой проекта реально существует, но распределён:
  `ide-diagnostics`
  feature-модули `ide`
  локально хорошо оформленный subtree `ide/src/completion/sdbl`.

- Adapter слой тоже существует, но главный boundary там пока не стабилизирован:
  `base-db`
  `ide-db`
  `project-model`
  `vfs`.

- Outer layer читается достаточно ясно:
  `bsl-analyzer`
  `mcp-server`
  `vfs-notify`
  `onec-client`
  `bsl-debug`
  `bsl-launcher`
  `naparnik`
  `extension`.

## Приложение A. Масштаб проблемы в цифрах

- [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs) сейчас имеет 748 строк.

- [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs) сейчас имеет 866 строк.

- `RootDatabase` добавляет 15 собственных методов поверх уже широкого набора supertrait-ов.
  Подтверждение: [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L62).

- В `bsl-analyzer` есть как минимум 8 исполняемых call site-ов, где delivery/runtime слой напрямую работает с `Salsa`-specific конструкциями:
  `DiagnosticsConfigId::new`
  `FileIdInput::new`
  `ConfigurationPathInput::new`
  `load_configuration`
  `file_diagnostics_query`.
  Подтверждение: [`crates/bsl-analyzer/src/handlers/notification.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/handlers/notification.rs#L52), [`crates/bsl-analyzer/src/handlers/notification.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/handlers/notification.rs#L53), [`crates/bsl-analyzer/src/handlers/notification.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/handlers/notification.rs#L54), [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs#L153), [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs#L584), [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs#L591), [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs#L603), [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs#L608).

- В проекте всего 5 реализаций `AnalysisProvider`, и только одна из них является тестовой.
  Подтверждение: [`crates/ide-db/src/salsa_provider.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/salsa_provider.rs#L44), [`crates/ide-diagnostics/src/test_utils.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/test_utils.rs#L318).

## Главные cross-layer нарушения

### Findings

- `H`: Основная публичная граница между use cases и infrastructure сегодня выражена через слишком широкий `db/query` контракт вместо узких портов.
  Подтверждение: [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L58) задаёт `RootDatabase` как общий supertrait над parsing/HIR/metadata/query API; [`crates/ide/src/goto_definition.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide/src/goto_definition.rs#L24) и [`crates/ide/src/hover.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide/src/hover.rs#L20) принимают этот широкий contract напрямую; [`crates/ide-diagnostics/src/query.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/query.rs#L21) выражает сценарий диагностики как tracked query.
  Последствие: application logic трудно отделить от runtime, а delivery слой получает соблазн работать с query API напрямую.

- `H`: `ide-db` стал главной точкой смешения сразу трёх слоёв: use cases, adapters и runtime host.
  Подтверждение: [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L85) и [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L135) держат application-facing аналитические операции; [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L323) одновременно содержит `RootDatabaseImpl` с `salsa::Storage`, `Files`, metadata version и config paths; [`crates/ide-db/src/metadata.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/metadata.rs#L68) рядом живёт concrete FS-backed metadata loading.
  Вывод: пока `ide-db` не разрезан, любая попытка “чистить архитектуру” будет упираться в этот crate.

- `H`: Outer layer `bsl-analyzer` обходит use-case boundary и управляет внутренним runtime почти вручную.
  Подтверждение: [`crates/bsl-analyzer/src/handlers/notification.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/handlers/notification.rs#L20) и [`crates/bsl-analyzer/src/handlers/notification.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/handlers/notification.rs#L52) напрямую работают с `file_diagnostics_query`, `FileIdInput` и `DiagnosticsConfigId`; [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs#L393) и [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs#L516) управляют sync VFS↔Salsa и `SourceRoot`; [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs#L566) прогревает metadata cache напрямую.
  Это уже не просто delivery code, а фактический orchestrator adapter/runtime уровня.

- `H`: Нарушение инверсии зависимости проявляется не только в ширине контрактов, но и в прямом downcast-е к concrete backend.
  Подтверждение: [`crates/ide-db/src/vfs_helpers.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/vfs_helpers.rs#L12) и [`crates/ide-db/src/vfs_helpers.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/vfs_helpers.rs#L17) downcast-ят `dyn RootDatabase` к `RootDatabaseImpl`.
  Это один из самых явных признаков того, что текущий boundary формально абстрактный, но фактически concrete.

- `H`: Часть domain identity и semantic model зависит от VFS/file storage вместо собственной предметной идентичности.
  Подтверждение: [`crates/hir-def/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/hir-def/src/lib.rs#L371) определяет `ModuleId` через `vfs::FileId`; [`crates/hir-def/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/hir-def/src/lib.rs#L447) переносит `file_id` в `ModuleData`.
  Это не обязательно надо срочно переделывать, но это долгосрочный structural coupling между entity layer и adapter layer.

- `M`: `DiagnosticsContext` и отдельные diagnostic handlers демонстрируют, как application boundary протекает в adapter/workspace concerns.
  Подтверждение: [`crates/ide-diagnostics/src/context.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/context.rs#L29) и [`crates/ide-diagnostics/src/context.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/context.rs#L85) тащат `workspace_root`, `configuration_path`, `file_set`; [`crates/ide-diagnostics/src/handlers/missing_event_subscription_handler.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/handlers/missing_event_subscription_handler.rs#L331) сам решает path/VFS resolution.
  Это важный сигнал: даже внутри наиболее зрелого use-case слоя граница ещё не очищена.

- `M`: `bsl-metadata` и `bsl-platform` создают вторичное смешение слоёв, потому что их model-части живут рядом с parser/loader/query facade.
  Подтверждение: [`crates/bsl-metadata/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-metadata/src/lib.rs#L93) и [`crates/bsl-metadata/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-metadata/src/lib.rs#L126) экспортируют и модель, и loader; [`crates/bsl-platform/src/db.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-platform/src/db.rs#L3) прямо описывает dual role как query facade и singleton.
  Это не первый приоритет, но без этого разделения entity layer останется “загрязнённым” даже после чистки boundary выше.

- `M`: В проекте уже есть локальные положительные контрпримеры целевого дизайна, и их стоит использовать как эталон, а не изобретать новый стиль.
  Подтверждение: [`crates/ide/src/completion/sdbl/mod.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide/src/completion/sdbl/mod.rs#L1) явно разделяет `domain / use_cases / infrastructure`; [`crates/ide-db/src/provider.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/provider.rs#L32) и [`crates/ide-db/src/salsa_provider.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/salsa_provider.rs#L44) показывают port + implementation pattern; [`crates/bsl-launcher/src/main.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-launcher/src/main.rs#L14) и [`crates/bsl-launcher/src/provider.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-launcher/src/provider.rs#L8) показывают чистое разделение outer subsystem.

## Приложение B. Ограничения `Salsa`

- `Salsa` уже определяет форму значительной части boundary API, и это не stylistic detail, а реальный refactoring constraint.
  Подтверждение: [`crates/base-db/src/input.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/base-db/src/input.rs#L98) прямо объясняет, что `FileId` приходится оборачивать в `FileIdInput`, потому что tracked functions требуют `Salsa`-типы; [`crates/ide-db/src/metadata.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/metadata.rs#L40) делает то же самое для `ConfigurationPathInput`.

- `RootDatabaseImpl` сейчас физически собирается как единая `#[salsa::db]` реализация поверх нескольких trait-слоёв.
  Подтверждение: [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L447), [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L450), [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L492), [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L513), [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L601), [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L614), [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs#L744).

- Из этого следует важное практическое ограничение:
  первый этап разделения `ide-db` почти наверняка должен быть логическим, а не физическим.
  То есть сначала:
  выделение узких портов;
  вынос helper/service boundary;
  отделение module-level API внутри crate.
  И только потом, если это позволит `Salsa`-shape, можно пытаться делать физический split по crate-ам.

- Поэтому рекомендацию “разрезать `ide-db`” надо читать не как “немедленно вынести всё в новые crate-ы”, а как “сначала перестать использовать монолитный `RootDatabase` как основную архитектурную границу”.

## Приложение C. Testability и реальные seams

- Текущая тестовая картина подтверждает вывод о слабой boundary-изоляции: большинство тестов идут через concrete `RootDatabaseImpl`.
  Подтверждение: [`crates/ide/tests/sdbl_completion_integration_test.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide/tests/sdbl_completion_integration_test.rs#L22), [`crates/hir-def/src/body/lower/tests.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/hir-def/src/body/lower/tests.rs#L15), [`crates/ide-diagnostics/src/handlers/assign_alias_fields_in_query.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/handlers/assign_alias_fields_in_query.rs#L151) и множество аналогичных handler tests создают `RootDatabaseImpl` напрямую.

- Реальный seam всё же существует, но он узкий: `DiagnosticsContext::with_provider(...)` плюс тестовый `MetadataTestProvider`.
  Подтверждение: [`crates/ide-diagnostics/src/context.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/context.rs#L67), [`crates/ide-diagnostics/src/test_utils.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/test_utils.rs#L313), [`crates/ide-diagnostics/src/test_utils.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/test_utils.rs#L496).

- Но этот seam пока неполный: даже тестовый provider опирается на внутренний `RootDatabaseImpl`, а не на независимую минимальную test double.
  Подтверждение: [`crates/ide-diagnostics/src/test_utils.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/test_utils.rs#L314) хранит `ide_db::RootDatabaseImpl`; [`crates/ide-diagnostics/src/test_utils.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/test_utils.rs#L378), [`crates/ide-diagnostics/src/test_utils.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/test_utils.rs#L388), [`crates/ide-diagnostics/src/test_utils.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/test_utils.rs#L449) продолжают собирать `FileIdInput` и вызывать `RootDatabase` API.

- Вывод по testability:
  boundary через provider уже доказала полезность, но ещё не стала основной формой тестирования.
  Это хороший аргумент в пользу pilot refactoring именно вокруг diagnostics или completion, где seam уже намечен.

## Карта приоритетов

### Priority 1

Стабилизировать boundary между use cases и adapters.

- Цель:
  перестать выражать use-case API через широкий `RootDatabase`.

- Что это означает practically:
  вычленять узкие provider/port интерфейсы;
  переводить `ide` и `ide-diagnostics` на них;
  перестать тащить `FileIdInput`, `DiagnosticsConfigId`, `ConfigurationPathInput` в delivery code.

- Основные кандидаты:
  [`crates/ide-db/src/lib.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/lib.rs)
  [`crates/ide-diagnostics/src/context.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/context.rs)
  [`crates/ide/src`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide/src)

### Priority 2

Разрезать `ide-db` на роли.

- Минимальное целевое разделение:
  application-facing ports;
  salsa-backed implementations;
  runtime host/state;
  metadata/path/VFS-specific helpers.

- Почему это вторым номером:
  пока `ide-db` монолитен, outer и inner слои будут продолжать связываться через него напрямую.

### Priority 3

Вынести orchestration из `bsl-analyzer::GlobalState`.

- Цель:
  сделать LSP runtime тонким composition root, а не точкой, где живут lifecycle policies анализа.

- Что выносить первым:
  diagnostics scheduling;
  metadata warmup;
  project/bootstrap orchestration;
  VFS↔database synchronization policy.

- Основной hotspot:
  [`crates/bsl-analyzer/src/global_state.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/bsl-analyzer/src/global_state.rs)

### Priority 4

Чистить use-case boundary внутри diagnostics.

- Цель:
  чтобы handlers знали о policy диагностики, а не о path resolution, VFS и source roots.

- Основные кандидаты:
  [`crates/ide-diagnostics/src/context.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/context.rs)
  [`crates/ide-diagnostics/src/handlers`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-diagnostics/src/handlers)

### Priority 5

Вернуться к очистке entity layer.

- Цель:
  отделить pure model от query/runtime/parser-loader concerns в:
  `hir-def`
  `hir-ty`
  `bsl-metadata`
  `bsl-platform`.

- Почему не раньше:
  сначала нужно стабилизировать boundary выше, иначе рефакторинг ядра будет сразу “загрязняться” обратно через текущие adapter contracts.

## Рекомендуемая последовательность рефакторинга

1. Выделить один узкий pilot-port для диагностик или completion и пройти полный цикл:
   use case contract -> adapter impl -> driver wiring.

2. На базе пилота разрезать `ide-db` на интерфейсы и реализации без массового изменения предметного ядра.

3. После появления новых портов убрать из `bsl-analyzer` прямые зависимости на `file_diagnostics_query`, `ConfigurationPathInput` и похожие runtime-specific типы.

4. Затем упростить `GlobalState`, оставив в нём только transport/runtime coordination, а orchestration перенести в отдельные application services.

5. Только после стабилизации этих границ возвращаться к более дорогой чистке entity layer.

## Decisions

- Главным архитектурным узлом проекта считать не “грязное ядро”, а нестабильную границу `use cases <-> adapters <-> drivers`.

- Основной локальный эталон для нового style guide:
  [`crates/ide/src/completion/sdbl`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide/src/completion/sdbl)
  плюс port-pattern из [`crates/ide-db/src/provider.rs`](/home/itrous/src/tools_migration/lsp/bsl-analyzer/crates/ide-db/src/provider.rs#L32).

- `RootDatabase` не считать целевой архитектурной границей проекта.
  Это временный transitional facade, который надо декомпозировать.
  С учётом `Salsa` constraints декомпозицию начинать логически, а не обязательно физически.

- `GlobalState` не считать “нормальным местом” для application orchestration.
  Его рост надо воспринимать как architectural debt, а не как естественную эволюцию runtime.

- Следующий практический шаг после этого файла:
  выбрать один pilot refactoring slice и разложить его в отдельный backlog по шагам и рискам.
