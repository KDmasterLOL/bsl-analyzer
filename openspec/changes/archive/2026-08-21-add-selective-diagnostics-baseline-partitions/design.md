## Context

`add-partitioned-diagnostics-baselines` строит полный непересекающийся partition plan
из main, extensions и настроенных groups. Все ожидаемые partition входят в один
baseline snapshot; отсутствие или повреждение любого object является ошибкой всего
набора. Это правильно для полного baseline, но не выражает намерение сопровождать
baseline только части проекта и оставлять diagnostics остальных владельцев видимыми.

Выбор baseline нельзя переносить в source loading или Salsa. Extension по-прежнему
должно видеть main, declared dependencies и остальную нормализованную topology.
Selective policy применяется только после общего вычисления diagnostics, штатных
suppression rules и назначения единственного owner partition.

## Locked Decisions

- Один `Project`, одна `ExtensionTopology`, одна Salsa-база и один semantic run для
  всей topology сохраняются независимо от selection.
- `main`, `extension:<name>` и `group:<name>` остаются единственными стабильными
  selector/id; groups продолжают менять владельца, но не dependency graph.
- Каждый source root и каждая diagnostic имеют ровно одного owner partition, включая
  intentional `unsuppressed`.
- Fingerprint, порядок suppression и защита `UnknownSuppressionCode`/
  `SuppressionWithoutCode` общие для CLI, MCP, LSP и reporters.
- Ошибка включённой partition не может молча превратить её в `unsuppressed` и не
  разрешает применять частичный baseline остальных включённых partition.
- Legacy `[diagnostics.baseline].path` и partitioned-конфигурация без нового поля
  сохраняют прежнее поведение.
- Обычный analyze, MCP и LSP не создают и не изменяют baseline.
- Production-код, commit, push и archive не входят в этот design change.

## Goals / Non-Goals

**Goals:**

- минимальный allowlist без повторения topology, paths, dependencies и diagnostic rules;
- явное различие `baseline` и intentional `unsuppressed` для каждого plan partition;
- немедленно видимые diagnostics выключенных partition;
- fail-closed snapshot для отсутствующего/повреждённого включённого baseline;
- единая all/selected CLI-семантика;
- additive MCP/LSP/reporter contract и детерминированный selection epoch;
- безопасная миграция v1 и полного partitioned manifest;
- загрузка памяти пропорционально включённым baseline entries, а не всей topology.

**Non-Goals:**

- отдельный semantic run либо отдельная Salsa-база для выбранного partition;
- denylist и динамические selector-ы;
- разные suppression/diagnostic настройки по partition;
- хранение active object для `unsuppressed` partition;
- удалённое хранилище, UI, автообновление или автоматическое принятие diagnostics;
- эвристика rename и перенос между owners;
- изменение Code Quality fingerprint либо существующих finding containers.

## Configuration Alternatives

### Вариант A — allowlist стабильных partition id

```toml
[diagnostics.baseline]
directory = ".bsl-analyzer/diagnostics-baselines"
include = ["main", "extension:Sales", "group:Vendor"]

[[diagnostics.baseline.groups]]
name = "Vendor"
extensions = ["VendorCore", "VendorReports"]
```

Отсутствующий `include` означает прежний режим «все partition». Явный непустой список
означает selective mode; все plan partition вне списка получают policy
`unsuppressed`.

Плюсы: один additive field, стабильные selector-ы уже существуют в CLI, topology не
дублируется. Добавленное расширение автоматически становится `unsuppressed`, если
allowlist явный. Минусы: rename выбранного extension требует правки списка.

### Вариант B — явная таблица policy для каждого partition

```toml
[[diagnostics.baseline.partitions]]
id = "main"
policy = "baseline"

[[diagnostics.baseline.partitions]]
id = "extension:Sales"
policy = "unsuppressed"
```

Плюсы: policy каждой записи видна рядом с id. Минусы: нужно перечислять всю topology,
новый extension не имеет безопасного очевидного default, а конфигурация превращается
во второй реестр partition и чаще расходится с project model.

### Выбор

Выбран вариант A. Он переиспользует существующий namespace и добавляет только одно
намерение: какие owners имеют baseline. `include` является allowlist, а не шаблоном;
точное сопоставление делает изменения topology проверяемыми.

`include` допустим только вместе с `directory`. Пустой список отклоняется: если
baseline не нужен ни одному owner, следует удалить `[diagnostics.baseline]`; это
сохраняет однозначное различие `disabled` и selective baseline. Дубликаты, неизвестные
id, case-fold collision и selector участника group, у которого нет индивидуального
partition, являются ошибкой `check-config` до анализа.

## Decisions

### 1. Partition plan остаётся полным, policy накладывается отдельно

`DiagnosticsBaselinePartitionPlan` по-прежнему содержит main и всех extension/group
owners. Чтобы не вводить второй реестр topology, он же хранит упорядоченное множество
enabled id и выводит policy:

```text
PartitionId -> Baseline | Unsuppressed
```

Owner routing ничего не знает о policy. Diagnostic сначала получает owner из полного
root table и только затем классифицируется по policy. Неизвестный root остаётся
`unowned_source`; он не становится `unsuppressed`.

Если `include` отсутствует, все plan partition имеют policy `Baseline`. Если список
задан, ровно его элементы имеют policy `Baseline`, остальные — `Unsuppressed`.
Канонический selection fingerprint — BLAKE3 версии рецепта и упорядоченного списка
`{partition_id, policy, identity}` для полного plan. Он не входит в diagnostic
fingerprint.

Изменения topology:

- новый extension при отсутствующем `include` включён и требует baseline как раньше;
- новый extension при явном `include`, не содержащем его id, становится
  `unsuppressed` без ошибки;
- удалённый либо переименованный id, оставшийся в `include`, является
  `unknown_partition_selector` до baseline I/O;
- изменение identity выбранной group/extension требует совместимого нового object;
- изменение identity `unsuppressed` partition не требует baseline object, но меняет
  selection epoch и summary identity.

### 2. Classification получает отдельный результат `unsuppressed`

Общий classifier выполняет один fingerprint calculation и возвращает одно из
состояний:

- `known`: owner включён, fingerprint есть в baseline; finding скрывается;
- `new`: owner включён, fingerprint отсутствует; finding активен;
- `unsuppressed`: owner намеренно выключен; baseline lookup не выполняется, finding
  активен;
- protected diagnostic остаётся активной независимо от policy и не записывается в
  baseline.

`unsuppressed` не считается `new`: иначе `check` selective baseline всегда падал бы из-за
намеренно видимых diagnostics. Общая и per-partition summary получают additive field
`unsuppressed`. Policy и coverage state ортогональны: `unsuppressed` не добавляется в
существующий enum `Disabled|Full|Partial|Error`. Для policy `Unsuppressed`:

- `state = "full" | "partial"` по coverage;
- для незащитных diagnostics `new = known = resolved = 0`;
- `unsuppressed` равно числу текущих незащитных diagnostics owner-а в покрытой области;
- `complete` отражает coverage owner-а, а не наличие baseline object;
- object path/schema/hash отсутствуют (`null`/omitted согласно существующему serde
  contract). Protected diagnostics не входят в `unsuppressed`: они остаются `new`,
  чтобы защитные ошибки продолжали делать `check` неуспешным.

Для включённой partition `unsuppressed = 0`, а `new`/`known`/`resolved` сохраняют
действующую семантику. Общие `known`/`resolved` суммируют только включённые partition;
общий `new` дополнительно включает protected diagnostics любого owner, а общий
`unsuppressed` — только незащитные diagnostics выключенных owners. Поле `selection` равно `all` либо
`selective`; `partitions_enabled` и `partitions_unsuppressed` содержат числа owners
каждой policy.

### 3. Fail-closed относится ко всему effective baseline snapshot

Loader открывает и индексирует только objects включённых partition. Для них
отсутствующий manifest entry/object, hash/JSON/schema/fingerprint/identity mismatch или
небезопасный путь переводит весь effective baseline snapshot в `error`; ни одна
включённая partition не применяется частично.

Objects `unsuppressed` partition не являются активными и не читаются, не хешируются и
не наблюдаются. Их отсутствие, повреждение и identity не проверяются до повторного
включения. Дубликат id в manifest отклоняется по metadata. Cross-object дубликаты
fingerprint/source проверяются только среди enabled objects; dormant object проходит
полную проверку при повторном включении.

При явном `include` глобальный `project_scope_fingerprint` manifest не является
условием готовности чтения: loader проверяет существование и текущую identity каждого
enabled id и игнорирует non-enabled/orphan entries без object I/O. Поэтому изменение
только невключённой topology не выключает enabled baseline. Без `include` сохраняется
строгое полное сравнение predecessor. Любая изменяющая selected-команда при scope
mismatch блокируется; существующий manifest согласует только full `update`.

CLI analyze/reporters и MCP при baseline error используют существующую fail-closed
ветвь и не выдают частично отфильтрованный результат. LSP сохраняет безопасную для
редактора fail-visible адаптацию: публикует все текущие diagnostics и одно уведомление
на `{partition_id, error_epoch}`. Это не downgrade в `unsuppressed`: state остаётся
`error`, а после восстановления общий classifier снова применяется атомарно.

### 4. Effective selection не меняет файловые схемы

Manifest остаётся schema v1, partition object — schema v2. Selection является
свойством конфигурации и `MUST NOT` дублироваться в manifest. Effective snapshot
содержит только пересечение current plan policy `Baseline` и manifest entries.
Entries owners с policy `Unsuppressed` являются dormant: их metadata может оставаться
в manifest для обратимого переключения, но object не открывается, не хешируется и не
наблюдается.

Для каждого enabled id manifest обязан иметь ровно одну совместимую entry. Остальные
структурно безопасные entries dormant и не участвуют в active set, даже если owner уже
исчез из текущей topology. Duplicate id и небезопасный manifest path отклоняются без
открытия object; orphan metadata удаляется следующей full publication.

Публикация сохраняет существующий commit point: immutable content-addressed objects,
writer lock, sync и одна атомарная замена manifest v1. Full operation создаёт/обновляет
objects только включённых partition. Если полный scope не изменился, structurally safe
dormant entries текущих plan owners переносятся как metadata без открытия objects, а
orphan metadata отбрасывается. При scope change full update консервативно удаляет все
dormant entries из нового manifest, не
пытаясь определить их identity и не удаляя content-addressed files синхронно.
Selected operation заменяет только object выбранной включённой partition и разрешена
только при совпадающем полном scope.

Существующий полный manifest работает без миграции: без `include` читаются все entries,
с selective `include` читаются только enabled entries. Повреждение dormant object не
блокирует snapshot. Selection epoch включает canonical policy plan, manifest bytes и
состояния только enabled objects. `check` всегда read-only; отсутствующий enabled entry
остаётся `missing_partition`.

### 5. CLI all/selected semantics

Все команды по-прежнему анализируют полную topology. Selector ограничивает только
baseline scope и машинный operation result.

| Команда | Без `--partition` | Включённая partition | `unsuppressed` partition |
|---|---|---|---|
| `create` | создаёт initial objects всех включённых partitions и manifest v1 | создаёт отсутствующий entry/object при глобальном Full либо чинит существующий object | ошибка `partition_unsuppressed` |
| `check` | проверяет drift всех включённых; unsuppressed findings не влияют на exit | проверяет `new/resolved` выбранной | read-only success при полном coverage, возвращает `unsuppressed` count |
| `update` | обновляет все включённые и публикует новое поколение manifest schema v1 | обновляет только выбранную active entry | ошибка `partition_unsuppressed` |

Первый selective `create` не требует baseline objects для всей topology: полнота
определяется effective enabled set. Он всё равно требует полного semantic coverage,
чтобы owner routing, protected diagnostics и topology были доказаны.

Каждая maintenance-команда, включая `check --partition` для `unsuppressed`, требует
глобальный `CoverageProof::Full` полного plan. Selected result ограничивает counts,
diagnostics и detail выбранным owner и дополнительно содержит общие selection metadata.
`check` успешен, если среди включённых проверяемых partition нет `new`/`resolved`.
Diagnostics `unsuppressed` отображаются в обычном analyze/reporters, но не превращают
baseline maintenance check в ошибку; protected diagnostics остаются ошибкой.

Машинный result сохраняет существующие поля и добавляет:

- `selection: "all" | "selective"`;
- `partitions_enabled` и `partitions_unsuppressed`;
- `unsuppressed`;
- per-partition `policy` и существующий coverage `state`;
- при selected command — прежний `selected_partition`.

Unknown selector, selector вне текущего plan и изменяющая операция над
`unsuppressed` завершаются до записи. Если enabled entry существует,
`create --partition <id>` использует predecessor repair и не принимает текущие новые
diagnostics. Если entry отсутствует после явного повторного включения, та же команда
при глобальном Full намеренно создаёт baseline из текущих diagnostics только выбранного
owner; это единственное новое выбранное acceptance. Full `create` применяется только
при отсутствующем manifest, full `update` — при существующем. `create --from-v1`
конфликтует с `--partition` и всегда маршрутизирует
все enabled owners.

### 6. Coverage и resolved остаются owner-aware

Coverage вычисляется для всех plan partitions из одного множества completed files.
`Full`/`Partial` включённой partition определяет возможность вычислить `resolved` как
раньше. Для `unsuppressed` resolved всегда 0, потому что сравнивать не с чем; её
`complete` всё равно показывает, полностью ли исследован owner.

Общая `complete=true` требует полного coverage всей запрошенной analysis surface, а не
только enabled subset. Это предотвращает ложное утверждение, что workspace полностью
проанализирован, когда unsuppressed extension не завершён. При Partial/stale/reload
per-partition summaries нормализуются согласованно; `unsuppressed` count отражает
только реально покрытые текущие diagnostics.

### 7. Migration не принимает diagnostics автоматически

Legacy schema v1 path не меняется. При selective directory:

- `create --from-v1` проверяет полный v1 scope и потоково маршрутизирует записи;
- записи включённых owners пишутся в staged schema v2 objects;
- записи `unsuppressed` owners не записываются, учитываются в
  `skipped_unsuppressed` и остаются в неизменном v1 source для отката;
- skipped entries проходят path/protected-code/fingerprint-recipe/owner validation,
  но uniqueness set ведётся только для enabled entries; duplicate skipped content
  не переносится и не влияет на selective output; будущий baseline этого owner
  валидирует заново создаваемые current entries;
- текущие diagnostics, которых не было в v1, не принимаются;
- atomically публикуется manifest v1 с enabled entries.

Для существующего полного partitioned manifest отдельная команда не нужна: после
добавления `include` entries вне allowlist становятся dormant без записи. При
неизменном полном scope следующая публикация переносит их metadata без object I/O.
При изменившемся scope только full update разрешён и удаляет все dormant metadata;
выборочные изменяющие операции блокируются до согласования. Re-enable проверяет object
fail-closed; при отсутствующей entry явный `create --partition` принимает текущие
diagnostics только этого owner согласно решению 5.

### 8. MCP и LSP используют штатный config reload

`ide-host-core` snapshot содержит полный policy plan, snapshots только включённых
objects и set epoch. Observation set состоит из config/manifest и активных enabled
objects; dormant/unsuppressed objects не наблюдаются. Изменение manifest либо enabled
object создаёт новый epoch и использует существующий reload без пересоздания Salsa.
Изменение `include` является изменением project config и намеренно проходит уже
существующий полный config reload в MCP и LSP. Отдельный fast path не проектируется:
он не нужен для пользовательской семантики и удвоил бы проверку эквивалентности config.

MCP diagnostics schema/outputSchema повышается с 14 до 15. File/workspace response
добавляет общие поля `selection`, `partitions_enabled`, `partitions_unsuppressed`,
`unsuppressed`; detail каждой
partition содержит `policy`. `result_id` включает selection epoch. Budget algorithm
остаётся единым и линейным; owner partition получает приоритет, обязательная общая
summary не может быть вытеснена details.

MCP file для owner `unsuppressed` возвращает активные findings с classification
`unsuppressed`; workspace объединяет их с `new` и protected. Ошибка enabled partition
возвращает существующий bounded error envelope без частичных findings/counts.

LSP публикует `new`, `unsuppressed` и protected, скрывает `known`. После config reload
он перепубликует открытые documents и сбрасывает текущую workspace batch. Ошибка
enabled partition публикует все текущие diagnostics; ошибки неактивных objects не
наблюдаются.

### 9. Reporters сохраняют существующие finding containers

Console печатает общую selection summary и строку каждой partition. JSON хранит её в
`baseline`, JSONL — в `done.baseline`, SARIF — в `runs[].properties.baseline`, JUnit —
в property `diagnostics.baseline`. Additive `policy`, `selection`,
`partitions_enabled`, `partitions_unsuppressed`, `unsuppressed` не меняют прежние поля.

Активные diagnostics `unsuppressed` входят в те же findings/results/testcases, что и
обычные `new`. SARIF не устанавливает им `baselineState`, потому что baseline к owner
не применялся. JUnit counts продолжают отражать активные diagnostics, а не число
baseline entries.

GitLab Code Quality остаётся корневым массивом активных findings, включает
`unsuppressed` и не получает summary/service elements. Его fingerprint не включает
partition policy.

### 10. Безопасность, производительность и наблюдаемость

Новый manifest использует существующую capability-boundary: project-relative paths,
закреплённые directory handles, no-follow/reparse, content hashes и атомарный replace.
`include` содержит только логические id и никогда не становится filesystem path.

Loader не должен разбирать либо хешировать objects `unsuppressed` partition. Scale
gate строит полный plan на 1,6 млн записей, включает малое подмножество и доказывает:

- semantic run/owner routing остаются полными;
- baseline parse/index memory и work пропорциональны enabled entries;
- reload manifest/active object при неизменном config переиспользует enabled `Arc`;
- отключённый corrupt object не читается, а включение того же id немедленно даёт
  fail-closed error;
- classifier не создаёт общий rich-entry vector.

В парном loader-only замере, где enabled objects содержат не более 10% entries,
добавочный peak RSS selective load над пустым процессом должен быть не больше
`max(128 MiB, 2 * enabled_object_bytes)` и не больше 25% добавочного RSS full load.
Selective v1 migration с тем же распределением должна иметь peak RSS не больше 25%
peak RSS predecessor full migration и подтверждать
`migrated + skipped_unsuppressed = 1_600_000`. Эти сравнительные пределы отделяют
baseline index от постоянной памяти полной topology.

Новая телеметрия не вводится. Существующих selection fingerprint и manifest generation
достаточно для result_id и reload deduplication.

## Audit Matrix

| Область | Решение | Автоматизированное доказательство |
|---|---|---|
| Correctness | полный owner plan + отдельный policy allowlist | config/ownership и shared-topology integration tests |
| Compatibility | `include` absent = all; path/v1 unchanged | legacy config/CLI/reporter goldens |
| Reliability | enabled set fail-closed; прежний atomic manifest | corruption, repair и fault-injection tests |
| Security | selector не путь; прежняя capability boundary | config/path adversarial tests |
| Performance | не читать unsuppressed objects; Arc reuse | selective 1,6M release gate, loader stats и observation paths |
| Operability | all/selected table и точные errors | CLI end-to-end matrix |
| Protocols | schema 15, selection epoch, fail-visible LSP | MCP budget/schema и LSP reload/parity tests |
| Reports | additive summary, прежние containers/fingerprint | console/JSON/JSONL/SARIF/JUnit/Code Quality goldens |

**Audit verdict:** архитектура готова к реализации после прохождения перечисленных в
`tasks.md` доказательств. Блокирующими отклонениями считаются
фильтрация source roots до semantic analysis, трактовка missing enabled object как
`unsuppressed`, чтение dormant objects и включение policy в diagnostic fingerprint.

## Risks / Trade-offs

- Явный allowlist требует ручной правки при rename выбранного extension/group; это
  предпочтительнее неявного переноса долга.
- Diagnostics `unsuppressed` видимы, но не делают baseline `check` красным. CI должен
  запускать обычный analyze/reporter, если хочет запрещать любые активные findings.
- Dormant entries не активны и не наблюдаются, но их objects могут занимать место;
  это осознанная цена обратимого переключения без нового формата.
- После selective create либо identity change возвращение к all может потребовать
  `create` отсутствующих partitions; dormant object не является бессрочным архивом.
- LSP fail-visible при enabled error показывает больше diagnostics. Это безопаснее
  частичного подавления и соответствует существующему editor contract.
- Изменение `include` использует полный config reload и может быть дороже baseline-file
  reload; быстрый путь добавляется только после измеренного запроса, а не в этом change.
- Dormant objects могут занимать диск. Это единственные дополнительные эксплуатационные
  расходы; внешних сервисов, зависимостей и денежных затрат change не добавляет.
- Windows atomic replace и watcher semantics сохраняют зависимость от CI-доказательств.

## Migration Plan

1. Добавить `include`/policy plan и сохранить default all.
2. Расширить common classifier/summary policy и count `unsuppressed`.
3. Научить существующий manifest loader строить effective set и не читать dormant objects.
4. Подключить CLI all/selected и selective `create --from-v1`.
5. Подключить analyze/reporters, MCP schema 15 и штатный LSP config reload.
6. Выполнить parity, fault-injection, 1,6M selective-load и Windows transaction gates.
7. Документировать rollout: добавить `include` и выполнить `check`; файловая миграция
   полного partitioned set не требуется, manifest/v1 рекомендуется хранить в VCS.

Откат — удалить `include`: сохранённые dormant entries снова становятся enabled.
После topology change full update мог удалить dormant metadata; отсутствующие entries
требуют восстановления manifest из VCS либо явного `create --partition`. Ни один шаг
не изменяет legacy v1 source автоматически.

## Execution Plan

1. Configuration/policy planner и manifest contract.
2. Loader/classifier/summary/migration.
3. CLI transaction и operation matrix.
4. Analyze/reporters.
5. MCP/LSP reload и schemas.
6. Документация, scale/security/parity и полные quality gates.

Следующий слой начинается только после автоматического доказательства общей policy
model; CLI/MCP/LSP не получают собственных selector algorithms.

## Assumptions and Open Questions

Блокирующих открытых вопросов нет. Предполагается, что change
`add-partitioned-diagnostics-baselines` будет принят как базовый контракт до
архивирования этого change. Пустой `include` намеренно запрещён; для полностью
отключённого baseline используется отсутствие `[diagnostics.baseline]`.

## Exact Wording Fixes Relative to Predecessor

- «все настроенные partition применяются одновременно» уточняется как «все partition
  с policy `Baseline` применяются одним atomic effective snapshot».
- «каждый ожидаемый partition обязан иметь object» остаётся верным для default all и
  заменяется effective enabled set при явном `include`.
- «общие counts равны сумме partition counts» расширяется отдельной суммой
  `unsuppressed`; эти diagnostics не входят в `new`.
- «ошибка любого partition инвалидирует set» относится к active enabled entries;
  dormant/unsuppressed objects не читаются.
- PDB-11 `orphan_partition` в selective mode не возникает из non-enabled metadata при
  чтении; full update удаляет её, а enabled selector по-прежнему валидируется строго.
- PDB-13 no-Salsa reload сохраняется для manifest/active objects; изменение `include`
  является project config reload и может перестроить Salsa.
- «операции без selector работают со всем набором» означает все enabled baseline
  partitions, но summary по-прежнему описывает полный owner plan.
