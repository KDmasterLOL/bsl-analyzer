## Why

`add-partitioned-diagnostics-baselines` разделяет хранение baseline по владельцам, но
по-прежнему требует применять baseline ко всем partition проекта. На практике команда
может сопровождать технический долг только основной конфигурации или нескольких
расширений, а диагностики остальных расширений должна видеть без подавления. Отсутствие
файла для такого расширения сейчас неотличимо от повреждения набора и переводит весь
baseline в ошибку.

Нельзя решать это отдельным анализом выбранных расширений: расширения должны видеть
основную конфигурацию, dependencies и полную нормализованную topology, иначе появляются
ложные семантические диагностики, прежде всего `UnresolvedMethodCall`. Выбор должен
ограничивать только применение и хранение baseline после одного полного анализа.

## What Changes

- В partitioned-режим добавляется необязательный allowlist `include` со стабильными
  selector-ами `main`, `extension:<name>` и `group:<name>`.
- Отсутствующий `include` сохраняет прежнее поведение: baseline включён для всех
  partition. Явный непустой список включает baseline только для перечисленных
  partition; остальные получают policy `unsuppressed`, сохраняя обычный
  `Full`/`Partial` coverage state.
- Partition planner и owner routing по-прежнему покрывают всю topology ровно один раз.
  `unsuppressed` не означает отсутствие владельца и не создаёт отдельный анализ.
- Диагностики `unsuppressed` partition остаются активными, не участвуют в
  `new`/`known`/`resolved` и учитываются отдельным счётчиком `unsuppressed`.
- Отсутствующий, повреждённый или несовместимый файл любой включённой partition
  инвалидирует весь baseline snapshot. Ошибка выключенной partition не читается и не
  превращает её в неявно включённую либо ошибочную.
- `create`, `check` и `update` без selector работают только с включёнными baseline
  partitions, сохраняя сводки всей topology. `--partition` разрешает read-only `check`
  для любой partition, но изменяющие операции только для включённой.
- Форматы manifest schema v1 и partition schema v2 не меняются. `include` определяет
  effective set: entries вне allowlist становятся dormant, не читаются и не
  наблюдаются; существующий полный набор начинает работать selective без переписи.
  В selective-режиме изменение только невключённой topology не инвалидирует чтение
  включённых objects; следующая полная запись согласует manifest и удалит dormant
  metadata без чтения objects.
- MCP, LSP и репортёры получают policy и счётчик `unsuppressed`; смена selection
  меняет epoch/result_id. Изменение config проходит существующий полный config reload;
  reload manifest/active objects по-прежнему не пересоздаёт Salsa.

## Capabilities

### New Capabilities

- `selective-diagnostics-baseline-partitions`: явный выбор partition, для которых
  применяется baseline, при намеренно видимых diagnostics остальных владельцев.

### Modified Capabilities

- `partitioned-diagnostics-baselines` условно специализируется требованиями SDBP-01,
  SDBP-04, SDBP-06, SDBP-08 и SDBP-09, включая PDB-11/PDB-13: при наличии `include`
  слова «все ожидаемые
  baseline partitions» означают enabled subset, тогда как полный owner/topology plan
  остаётся неизменным. Поскольку capability пока принадлежит активному зависимому
  change, дельта оформлена в новой capability; при последовательном архивировании она
  является более специальным нормативным контрактом, а не независимой альтернативой.

## Dependencies

- Change `add-diagnostics-baseline` задаёт fingerprint, coverage, защитные диагностики
  и legacy schema v1.
- Change `add-partitioned-diagnostics-baselines` задаёт partition plan, schema v2
  objects, manifest v1, атомарную публикацию, общий classifier и поверхности
  CLI/MCP/LSP/reporters.
- `add-selective-diagnostics-baseline-partitions` реализуется и архивируется только
  после этих changes либо вместе с доказанным эквивалентным базовым контрактом.

## Impact

- `project-model`: поле `include`, валидация selector-ов и policy полного partition
  plan без дублирования topology.
- `ide`/`ide-host-core`: effective selective snapshot, загрузка только включённых objects,
  `unsuppressed` classification/summary и selection epoch.
- CLI: all/selected semantics, миграция v1/full-v2 и машинные результаты.
- MCP/LSP: additive schema, bounded summaries, штатный config reload и fail-visible
  LSP при ошибке включённого baseline.
- Репортёры: видимые unsuppressed findings и совместимые additive summaries без смены
  Code Quality fingerprint.
- Тесты и документация: конфигурация, CLI, parity, reload, migration, reporters и
  масштабный selective-load gate.

## Non-Goals

- отдельный анализ main, расширения, группы или выбранного подграфа;
- разные diagnostic rules, suppression directives или fingerprint recipes по
  partition;
- denylist, glob/regex selector-ы либо автоматический выбор по размеру/автору;
- автоматическое принятие новых diagnostics или изменение baseline обычным analyze,
  MCP либо LSP;
- удалённое хранилище, UI или фоновая синхронизация;
- гарантия бессрочного архивного хранения dormant objects `unsuppressed` partition;
- отдельный быстрый путь reload только для `include`;
- новая телеметрия, внешний сервис или зависимость;
- изменение семантики legacy `[diagnostics.baseline].path`;
- автоматический перенос baseline при переименовании extension/group.
