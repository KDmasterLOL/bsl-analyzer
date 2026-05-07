# CommonModuleAssign

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Диагностика сообщает о присваивании в идентификатор, совпадающий с именем
общего модуля из метаданных конфигурации. Общий модуль не является записываемой
переменной, поэтому такое присваивание приводит к ошибке выполнения или к
конфликту имен, который нужно устранять переименованием локального
идентификатора.

Прямой привязки к стандарту в локальном `v8std` нет; правило основано на
платформенной семантике разрешения имен.

## Проверенные источники

- Handler:
  `crates/ide-diagnostics/src/handlers/common_module_assign.rs`.
- Эмиссия HIR candidate:
  `crates/hir-def/src/body/lower/stmt.rs`,
  `crates/hir-def/src/body.rs`.
- Dispatch:
  `crates/ide-diagnostics/src/hir_dispatch.rs`.
- Metadata/configuration:
  `crates/bsl-metadata/src/configuration.rs`,
  `crates/ide-db/src/salsa_provider.rs`,
  `crates/ide-diagnostics/src/context.rs`.
- Resolver/name-shadowing context:
  `crates/hir-def/src/resolver.rs`.
- Смежные diagnostics:
  `crates/ide-diagnostics/src/handlers/this_object_assign.rs`,
  `crates/ide-diagnostics/src/handlers/read_only_property.rs`,
  `crates/ide-diagnostics/src/handlers/unresolved_method_call.rs`,
  `crates/ide-diagnostics/src/handlers/common_module_missing_api.rs`.
- Rule-доки:
  `crates/ide-diagnostics/docs/ru/CommonModuleAssign.md`,
  `crates/ide-diagnostics/docs/en/CommonModuleAssign.md`.
- Provenance:
  `docs/legal/diagnostics/CommonModuleAssign.md`.
- Локальный `v8std`:
  `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/CommonModuleAssign.md`.
- Внешняя ссылка из rule-доков:
  `https://1c-syntax.github.io/bsl-language-server/diagnostics/CommonModuleAssign/`.

## Как реализовано

Во время lowering `ASSIGN_STMT` левая часть понижается в HIR expression. Если
target — простой `Expr::Path`, lowering:

- регистрирует implicit local variable, если имени еще нет среди локальных
  переменных и параметров;
- всегда эмитит `BodyDiagnostic::CommonModuleAssign { variable_name, range }`;
- затем отдельно эмитит другие assignment diagnostics, например
  `ThisObjectAssign` и `FunctionOutParameter`.

Handler `from_hir()`:

- проверяет, что `CommonModuleAssign` не отключен;
- вызывает `ctx.load_configuration()`;
- ищет `configuration.find_common_module(variable_name)` case-insensitive;
- если общий модуль найден, создает diagnostic на range идентификатора;
- quick-fix нет.

Field/index assignments не дают candidate, потому что target уже не
`Expr::Path`.

## Что покрыто

- без metadata diagnostic не появляется;
- присваивание в свойство `СвойМодуль.Свойство = ...` не диагностируется;
- index assignment `Массив[0] = ...` не диагностируется;
- простое assignment target проходит через candidate path;
- `Configuration::find_common_module()` покрыт отдельными unit-тестами и
  работает case-insensitive.

Покрыта только отрицательная часть handler'а и корректная фильтрация не-simple
targets. Позитивный end-to-end сценарий "есть CommonModule metadata, assignment
с тем же именем диагностируется" в тестах самого правила не закреплен.

## Пробелы покрытия

- Нет позитивного теста с реальной `Configuration`, где общий модуль загружен и
  `ОбщийМодуль = ...` дает `CommonModuleAssign`. Текущий
  `check_hir_diagnostic()` создает `SalsaProvider::new(..., None)`, поэтому
  `ctx.load_configuration()` возвращает `None`.
- Нет теста на case-insensitive срабатывание именно диагностики, хотя lower
  слой и metadata lookup должны это поддерживать.
- Нет тестов на параметр/локальную переменную с именем общего модуля:
  `Процедура Тест(ОбщийМодуль) ОбщийМодуль = 1;`. Resolver для чтения имен
  отдает приоритет locals перед common modules, а эта диагностика эмитит
  candidate независимо от `local_vars`/`param_names`. Нужно явно подтвердить
  платформенное expected behavior, иначе возможен рассинхрон с остальной
  семантической моделью.
- Candidate эмитится на каждое простое присваивание в методах и module-level
  коде, даже когда в workspace нет загруженной configuration. Это дешево, но
  создает лишний diagnostic traffic через HIR dispatch.
- Handler смотрит только `ctx.load_configuration()`, а не
  `visible_configurations()`. Для расширений и нескольких конфигураций это
  может пропустить common module, видимый из текущего файла, или выбрать не тот
  source of truth.
- Сообщение говорит про общий модуль из metadata, но не помогает понять, где он
  определен. При большом проекте полезно показывать canonical name/source.
- Нет quick-fix. Единственный безопасный fix обычно не автоматический: нужно
  переименовать локальную переменную/параметр и все ссылки.
- Rule-доки опираются только на v8std/BSLLS страницу без стандарта. Это
  нормально для provenance, но в пользовательской документации стоит явно
  назвать платформенную причину и ограничение "нужна загруженная metadata".

## Может ли инфраструктура улучшить качество

Да. Инфраструктурно все основные части уже есть: HIR знает simple assignment
targets, metadata умеет case-insensitive lookup common modules, resolver умеет
различать locals/common modules for reads.

Нужно не усложнять handler, а привести его к той же semantic model:

- использовать общий resolver/visible configuration layer для ответа "это имя
  действительно разрешается как CommonModule в этой точке?";
- добавить тестовый helper для diagnostics with configuration metadata;
- опционально эмитить candidate только когда есть видимые common modules или
  переносить проверку в metadata-aware pass;
- переиспользовать rename/code-action инфраструктуру, если она появится.

## Возможное объединение

Ближайшие правила:

- `ThisObjectAssign` — прямое присваивание read-only built-in имени;
- `ReadOnlyPropertyAssignment` — присваивание read-only platform property;
- `UnresolvedMethodCall` / deprecated `MissingCommonModuleMethod` — разрешение
  common-module receivers и методов;
- `CommonModuleName*`, `CommonModuleInvalidType`, `CommonModuleMissingAPI` —
  metadata rules вокруг общих модулей.

Сливать `CommonModuleAssign` с common-module naming/API правилами не стоит:
там проверяется metadata design, а здесь runtime error в коде. С
`ThisObjectAssign` и `ReadOnlyPropertyAssignment` объединение как
`DiagnosticCode` тоже не нужно, потому что разные объекты и сообщения. Но
внутренне полезен общий "assignment target semantic checks" слой:

- классификация target `Path`/`Field`/`Index`;
- resolution target: local, common module, read-only built-in, read-only
  platform property;
- общий range и rename/fix hints.

## Варианты снятия ограничений

1. Добавить fixture-based positive tests с `CommonModules/<Name>/Ext/Module.bsl`
   и provider configuration, чтобы реально проверить `ctx.load_configuration()`.
2. Добавить тесты на case-insensitive common-module name.
3. Явно проверить и задокументировать поведение для параметров и `Перем` с
   именем общего модуля; после этого синхронизировать диагностику с resolver.
4. Перейти с `load_configuration()` на `visible_configurations()` для
   extension-aware проектов.
5. Добавить source hint в сообщение, если metadata содержит URI общего модуля.
6. Снизить candidate шум: либо не эмитить `CommonModuleAssign` без metadata,
   либо батчить проверку common-module names на уровне handler/dispatch.
7. Рассматривать code action не как простое удаление, а как rename локального
   идентификатора с обновлением ссылок.

## Вывод

Идея диагностики проста и полезна, а текущая реализация минимальна: HIR находит
simple assignment target, metadata подтверждает совпадение с common module.
Главный долг — тесты и семантическая согласованность. Нужно закрепить
позитивный metadata-сценарий и разобраться с shadowing локальных имен, чтобы
правило не расходилось с resolver и cross-module семантикой.
