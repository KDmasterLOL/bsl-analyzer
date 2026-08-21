## ADDED Requirements

### Requirement: PDB-01 — Минимальный partitioned-конфигурационный контракт
Анализатор `MUST` поддерживать взаимоисключающие режимы `[diagnostics.baseline].path`
и `[diagnostics.baseline].directory`. Режим `directory` `MUST` автоматически выводить
`main` и расширения из нормализованной `ExtensionTopology`; необязательные `groups`
`MAY` объединять явно перечисленные расширения без повторного задания их путей,
dependencies или параметров диагностик.
Partitioned-режим `MUST` требовать structured уникальные имена расширений; это
ограничение `MUST NOT` менять legacy `path`.

**Автоматизированное доказательство Requirement:**
`cargo test -p project-model partitioned_baseline_config_contract`.

#### Scenario: Прежний одиночный path
- **WHEN** настроен только `[diagnostics.baseline].path`
- **THEN** анализатор использует прежний schema v1 файл и не создаёт partition planner
- **AUTOMATED EVIDENCE** `diagnostics_baseline_cli`

#### Scenario: Автоматическое разбиение
- **WHEN** настроен `directory` и проект содержит main и три расширения без групп
- **THEN** план содержит `main` и по одному `extension:<name>` для каждого расширения без повторного объявления topology
- **AUTOMATED EVIDENCE** `diagnostics_partition_identity_and_ownership`

#### Scenario: Явная группа как исключение
- **WHEN** два расширения включены в одну группу, а третье не включено
- **THEN** план содержит один group partition, один самостоятельный extension partition и не содержит индивидуальные partition участников группы
- **AUTOMATED EVIDENCE** `diagnostics_partition_identity_and_ownership`

#### Scenario: Конфликт режимов
- **WHEN** одновременно заданы `path` и `directory` либо `groups` заданы без `directory`
- **THEN** `check-config` возвращает ошибку до анализа и чтения baseline
- **AUTOMATED EVIDENCE** `partitioned_baseline_config_contract_rejects_conflicting_modes`

#### Scenario: Относительный явный путь конфигурации
- **WHEN** первая `create --from-v1` получает `-c <relative-config.toml>`, а каталог baseline ещё не существует
- **THEN** origin конфигурации разрешается в абсолютный путь, а `directory` создаётся относительно этого origin внутри проекта
- **AUTOMATED EVIDENCE** `migration_preserves_v1_and_rejects_unknown_selector_before_publish`

#### Scenario: Неоднозначное имя расширения
- **WHEN** `directory` включён, а topology содержит legacy/пустое либо неуникальное после case-fold имя расширения
- **THEN** `check-config` отклоняет partitioned-режим, тогда как тот же проект с legacy `path` сохраняет прежнее поведение
- **AUTOMATED EVIDENCE** `diagnostics_partition_identity_and_ownership_rejects_legacy_and_nested_roots`

### Requirement: PDB-02 — Детерминированная идентичность и полное покрытие partition
Каждый partition `MUST` иметь устойчивый id и identity, выведенные из main path либо
из имени, path и прямых dependencies расширения. Группа `MUST` включать упорядоченные
identity всех members. Каждый source root и каждая диагностика `MUST` принадлежать
ровно одному partition.

**Автоматизированное доказательство Requirement:**
`cargo test -p project-model diagnostics_partition_identity_and_ownership`.

#### Scenario: Идентичность main и extension
- **WHEN** план строится дважды для одной переносимой topology
- **THEN** `main`, `extension:<name>` и их identities побайтово совпадают, включая path и dependencies
- **AUTOMATED EVIDENCE** `diagnostics_partition_identity_and_ownership`

#### Scenario: Ограниченная машинная идентичность
- **WHEN** вычисленный partition id превышает 64 байта UTF-8
- **THEN** конфигурация отклоняется до анализа, чтобы обязательная идентичность ошибки помещалась в минимальный поддерживаемый MCP-бюджет
- **AUTOMATED EVIDENCE** `diagnostics_partition_identity_bounds_machine_ids`

#### Scenario: Одна диагностика — один владелец
- **WHEN** полный анализ возвращает диагностики из main, самостоятельного extension и member группы
- **THEN** маршрутизатор назначает каждую диагностику ровно одному ожидаемому partition и не дублирует main в extension/group
- **AUTOMATED EVIDENCE** `diagnostics_partition_identity_and_ownership`

#### Scenario: Пересекающиеся правила
- **WHEN** расширение входит в две группы, имена групп сталкиваются после case-fold либо source roots совпадают/вложены
- **THEN** конфигурация отклоняется как неоднозначная до классификации
- **AUTOMATED EVIDENCE** `diagnostics_partition_identity_and_ownership_rejects_legacy_and_nested_roots`

#### Scenario: Некорректный участник группы
- **WHEN** группа пуста, ссылается на неизвестное расширение либо пытается включить main
- **THEN** `check-config` отклоняет группу до построения owner table
- **AUTOMATED EVIDENCE** `diagnostics_partition_identity_and_ownership_rejects_bad_groups`

#### Scenario: Правило отсутствует
- **WHEN** файл полного анализа не имеет owner root в partition plan
- **THEN** partitioned baseline переходит в проверяемую ошибку `unowned_source` и не фильтрует диагностику частичным набором
- **AUTOMATED EVIDENCE** `diagnostics_partition_identity_and_ownership`

### Requirement: PDB-03 — Один семантический анализ полной topology
Partitioning `MUST NOT` создавать отдельную семантическую базу или анализировать
расширение изолированно. Все partition `MUST` классифицировать результаты одной
актуальной Salsa-базы, построенной для main, всех extensions и dependency graph.

**Автоматизированное доказательство Requirement:**
`cargo test -p bsl-analyzer --test partitioned_diagnostics_baseline_semantics`.

#### Scenario: Вызов main из extension
- **WHEN** extension вызывает экспортный метод main и анализируется с включёнными partition baselines
- **THEN** вызов разрешается в общем semantic snapshot и не создаёт ложный `UnresolvedMethodCall`
- **AUTOMATED EVIDENCE** `one_semantic_run_classifies_main_and_extension_before_reporting`

#### Scenario: Вызов dependency из extension
- **WHEN** extension вызывает метод объявленной dependency
- **THEN** семантика использует прежнюю dependency closure независимо от того, находятся extensions в одном или разных baseline partitions
- **AUTOMATED EVIDENCE** `partitioned_baseline_lsp_main_extension_group_partial_and_recovery`

#### Scenario: Выбран один partition
- **WHEN** CLI выполняет `check` или `update --partition extension:<name>`
- **THEN** source universe и topology загружаются один раз полностью, а selector ограничивает только классификацию/изменение baseline
- **AUTOMATED EVIDENCE** `operations_all_and_selected_are_atomic_and_scoped`

### Requirement: PDB-04 — Единый отпечаток и защитные диагностики
CLI, MCP, LSP и все reporters `MUST` использовать существующий fingerprint recipe
`path + code + normalized snippet + occurrence`. Partition identity `MUST NOT` входить
в отпечаток. `UnknownSuppressionCode` и `SuppressionWithoutCode` `MUST` оставаться
активными и `MUST NOT` сохраняться ни в одном partition.

**Автоматизированное доказательство Requirement:**
`cargo test -p ide partitioned_baseline_fingerprint_contract` и
`cargo test -p bsl-analyzer --test partitioned_diagnostics_baseline_parity partitioned_baseline_cli_mcp_lsp_parity`.

#### Scenario: Перенос v1 записи без смены fingerprint
- **WHEN** schema v1 запись маршрутизируется в schema v2 partition
- **THEN** её fingerprint сохраняется побайтово и совпадает с Code Quality при том же нормализованном пути
- **AUTOMATED EVIDENCE** `partitioned_baseline_fingerprint_contract_matches_v1`

#### Scenario: Одинаковая диагностика в трёх поверхностях
- **WHEN** CLI, MCP и LSP классифицируют одну ревизию и один partition set
- **THEN** owner partition и состояние new/known совпадают без отдельных алгоритмов
- **AUTOMATED EVIDENCE** `partitioned_baseline_cli_mcp_lsp_parity`

#### Scenario: Защитная диагностика
- **WHEN** текущая либо сохранённая запись имеет код `UnknownSuppressionCode` или `SuppressionWithoutCode`
- **THEN** текущая диагностика остаётся new/active, а файл с сохранённой записью отклоняется
- **AUTOMATED EVIDENCE** `partitioned_baseline_classification_and_summary_routes_once`

### Requirement: PDB-05 — Версионируемый manifest и partition schema v2
Partitioned baseline `MUST` состоять из атомарно публикуемого manifest schema v1 и
детерминированных partition-файлов schema v2. Manifest `MUST` содержать один portable
project scope fingerprint и список файлов с hashes. Каждый partition-файл `MUST`
содержать точную partition identity и упорядоченные diagnostics, но `MUST NOT` повторять
project scope, содержать абсолютные пути или topology вне собственной identity.

**Автоматизированное доказательство Requirement:**
`cargo test -p ide partitioned_baseline_schema_v2`.

#### Scenario: Повторное формирование
- **WHEN** неизменный проект и baseline формируются повторно
- **THEN** partition bytes, hashes, generation id и canonical manifest побайтово идентичны
- **AUTOMATED EVIDENCE** `partitioned_baseline_schema_v2_generation_is_byte_deterministic`

#### Scenario: Несовместимая схема
- **WHEN** manifest либо один partition имеет неподдерживаемую schema version
- **THEN** весь set получает error с partition/path detail и не применяется частично
- **AUTOMATED EVIDENCE** `partitioned_baseline_schema_v2_rejects_bad_schema_identity_and_paths`

#### Scenario: Несовпадающая identity
- **WHEN** scope fingerprint manifest либо kind, name, path, dependencies или members partition не совпадают с текущим plan
- **THEN** set отклоняется как `scope_mismatch`/`partition_identity_mismatch`
- **AUTOMATED EVIDENCE** `partitioned_baseline_set_loader_streams_and_validates_all_partitions`

#### Scenario: Дубликат между файлами
- **WHEN** один fingerprint либо одна source path entry встречается в двух partition
- **THEN** set отклоняется до классификации и не подавляет диагностику дважды
- **AUTOMATED EVIDENCE** `partitioned_baseline_set_loader_fails_closed_on_corruption`

### Requirement: PDB-06 — Явная безопасная миграция schema v1
Одиночный schema v1 `MUST` продолжать читаться в режиме `path`. В режиме `directory`
анализатор `MUST` предоставлять явную миграцию `create --from-v1`, которая проверяет
старый scope, раскладывает существующие записи по owner partitions и `MUST NOT`
автоматически принимать текущие новые diagnostics. Исходный v1 path `MUST` быть
обычным файлом без symlink/reparse point внутри канонического project root. Миграция
`MUST` потоково читать v1 и писать staged partition files без общего raw/rich vector и
повторной загрузки опубликованного set; operation-result `MUST` сохранить поле
`diagnostics` пустым и сообщить число перенесённых записей через `added`.

**Автоматизированное доказательство Requirement:**
`cargo test -p bsl-analyzer --test partitioned_diagnostics_baseline_cli migration`.

#### Scenario: Успешное разбиение v1
- **WHEN** корректный v1 scope совпадает с текущей topology
- **THEN** создаётся полный v2 set, сумма записей равна v1 и каждый fingerprint сохранён ровно один раз
- **AUTOMATED EVIDENCE** `partitioned_baseline_v1_migration_preserves_entries_without_current_diagnostics`

#### Scenario: Новая диагностика во время миграции
- **WHEN** после создания v1 в исходниках появилась новая диагностика
- **THEN** миграция не добавляет её, а последующий `check` сообщает её как new в owner partition
- **AUTOMATED EVIDENCE** `partitioned_baseline_v1_migration_preserves_entries_without_current_diagnostics`

#### Scenario: Исходный v1 сохраняется для отката
- **WHEN** миграция успешно публикует v2 set, после чего конфигурация возвращается к `path`
- **THEN** bytes исходного v1 не изменены и классификация соответствует pre-migration snapshot; более поздние v2 update автоматически назад не переносятся
- **AUTOMATED EVIDENCE** `migration_preserves_v1_and_rejects_unknown_selector_before_publish`

#### Scenario: Несовместимый v1 scope
- **WHEN** v1 topology не совпадает либо запись нельзя назначить owner root
- **THEN** миграция завершается до публикации manifest и не оставляет активный частичный set
- **AUTOMATED EVIDENCE** `partitioned_baseline_v1_migration_rejects_scope_and_unowned_paths`

#### Scenario: Небезопасный путь миграции
- **WHEN** `--from-v1` абсолютный, выходит через `..`, находится вне project root либо является link/reparse point
- **THEN** миграция завершается до открытия источника и публикации manifest
- **AUTOMATED EVIDENCE** `partitioned_baseline_path_security_uses_pinned_relative_operations`

### Requirement: PDB-07 — Одновременная классификация и сводки
Все ожидаемые partition `MUST` загружаться как один set snapshot и применяться
одновременно. Внутренняя модель, CLI, Console, JSON, JSONL, SARIF и JUnit `MUST` содержать общую и все
упорядоченные per-partition сводки с `new`, `known`, `resolved`, `state`, `complete`,
а также partition identity/object path/schema и error fields. Общая сводка `MUST`
содержать configured directory, partition schema v2 и manifest schema v1, но не
фиктивную общую identity. MCP `MUST` представлять ту же модель с
детерминированным бюджетным усечением по PDB-14.

**Автоматизированное доказательство Requirement:**
`cargo test -p ide partitioned_baseline_classification_and_summary`.

#### Scenario: Полный корректный набор
- **WHEN** все partition корректны и полный анализ завершён
- **THEN** каждая сводка имеет state `full`, общие counts равны сумме partition counts и active output содержит только new/защитные diagnostics
- **AUTOMATED EVIDENCE** `partitioned_baseline_classification_and_summary_routes_once`

#### Scenario: Отсутствующий partition
- **WHEN** manifest не содержит ожидаемый partition либо перечисленный файл отсутствует
- **THEN** общий state `error`, ошибка называет `missing_partition`, findings не фильтруются частичным set
- **AUTOMATED EVIDENCE** `partitioned_baseline_set_loader_fails_closed_on_corruption`

#### Scenario: Повреждённый partition
- **WHEN** hash, JSON, fingerprint или identity одного файла повреждены
- **THEN** общий state `error`, complete=false и ни один другой partition не применяется отдельно
- **AUTOMATED EVIDENCE** `partitioned_baseline_set_loader_fails_closed_on_corruption`

#### Scenario: Baseline отключён
- **WHEN** отсутствуют и `path`, и `directory`
- **THEN** прежние findings остаются активными, общая сводка `disabled/complete=true`, а per-partition список пуст
- **AUTOMATED EVIDENCE** `diagnostics_partitioned_baseline_response_covers_schema_errors_and_minimum_budget`

### Requirement: PDB-08 — Per-partition Full/Partial и resolved
Полнота `MUST` вычисляться для каждого partition по полностью завершённым файлам при
актуальном общем semantic snapshot. `resolved` `MUST` включать только записи из
доказанно завершённых целых файлов owner partition. Общий `complete=true` `MUST`
требовать Full всех partition.

**Автоматизированное доказательство Requirement:**
`cargo test -p ide partitioned_baseline_coverage`.

#### Scenario: Один partition частичный
- **WHEN** все main-файлы завершены, но один extension file не прочитан
- **THEN** main может быть Full, owner extension/group является Partial, а общий set — Partial/complete=false
- **AUTOMATED EVIDENCE** `partitioned_baseline_classification_and_summary_routes_once`

#### Scenario: Partial resolved
- **WHEN** актуальный ограниченный запрос завершил только часть целых файлов partition
- **THEN** resolved считается только внутри этих файлов и не включает записи других файлов/partition
- **AUTOMATED EVIDENCE** `partitioned_baseline_classification_and_summary_routes_once`

#### Scenario: Stale или reload
- **WHEN** semantic snapshot stale либо reload не завершён/неуспешен
- **THEN** каждый partition имеет пустой completed set, state Partial и `resolved = 0`
- **AUTOMATED EVIDENCE** `partitioned_baseline_uses_one_snapshot_for_file_workspace_error_and_recovery`

#### Scenario: Группа
- **WHEN** хотя бы один member root группы не покрыт полностью
- **THEN** весь group partition Partial независимо от полноты остальных members
- **AUTOMATED EVIDENCE** `partitioned_baseline_lsp_main_extension_group_partial_and_recovery`

### Requirement: PDB-09 — CLI all/selected и read-only check
`diagnostics baseline create|check|update` `MUST` работать со всем набором без selector
и с одним стабильным `--partition <id>`. Любая операция `MUST` сохранять общий
semantic context и требовать глобальный `CoverageProof::Full`; `check` `MUST` быть
побайтово read-only для каждого исхода.

**Автоматизированное доказательство Requirement:**
`cargo test -p bsl-analyzer --test partitioned_diagnostics_baseline_cli operations`.

#### Scenario: Операция над всем набором
- **WHEN** selector отсутствует
- **THEN** create/check/update обрабатывает все ожидаемые partition и возвращает общую и per-partition сводку
- **AUTOMATED EVIDENCE** `operations_all_and_selected_are_atomic_and_scoped`

#### Scenario: Выбранный check
- **WHEN** пользователь запускает `check --partition extension:Ext`
- **THEN** успех требует глобального Full и отсутствия new/resolved выбранного partition, весь set валидируется, счётчики drift других partition не влияют, а bytes manifest/files не меняются
- **AUTOMATED EVIDENCE** `operations_all_and_selected_are_atomic_and_scoped`

#### Scenario: Выбранный update
- **WHEN** пользователь запускает `update --partition group:vendor`
- **THEN** новое поколение меняет логическое содержимое только выбранного partition, а hashes остальных остаются прежними
- **AUTOMATED EVIDENCE** `operations_all_and_selected_are_atomic_and_scoped`

#### Scenario: Первичный create выбранного partition
- **WHEN** manifest отсутствует
- **THEN** `create --partition` отказывает до публикации, поскольку selected create является только repair-операцией; первый set создаёт `create` без selector
- **AUTOMATED EVIDENCE** `operations_all_and_selected_are_atomic_and_scoped`

#### Scenario: Выбранный create восстанавливает отсутствующий object
- **WHEN** scope/plan совпадают с manifest, перечисленный object выбранного partition отсутствует и повторное формирование даёт ожидаемый hash
- **THEN** `create --partition` атомарно восстанавливает только этот object, не меняя manifest и не принимая новые diagnostics
- **AUTOMATED EVIDENCE** `operations_all_and_selected_are_atomic_and_scoped`

#### Scenario: Выбранный create восстанавливает повреждённый object
- **WHEN** object существует с неверными bytes, а регенерация даёт hash, уже записанный в неизменном manifest
- **THEN** `create --partition` атомарно заменяет object ожидаемыми bytes, не меняя manifest и baseline-состав
- **AUTOMATED EVIDENCE** `operations_all_and_selected_are_atomic_and_scoped`

#### Scenario: Неизвестный selector
- **WHEN** selector не соответствует текущему partition plan
- **THEN** команда завершается до анализа и записи с перечнем допустимых ids
- **AUTOMATED EVIDENCE** `migration_preserves_v1_and_rejects_unknown_selector_before_publish`

### Requirement: PDB-10 — Атомарность многофайловой операции
Create/update/migrate `MUST` публиковать immutable content-addressed objects и делать набор активным
единственной атомарной операцией над manifest. Ни один сбой подготовки, проверки,
публикации partition или manifest `MUST NOT` оставлять смешанный активный набор.
Изменяющие команды `MUST` сериализоваться межпроцессной блокировкой и `MUST` проверить
исходный manifest generation непосредственно перед publish.
Они `MUST` синхронизировать новые objects и временный manifest до replace, а доступные
directory entries — насколько поддерживает платформа; стабильный lock-file `MUST NOT`
удаляться или заменяться. Atomic manifest replace является commit point. Гарантия
распространяется на process-visible сбои/гонки, но `MUST NOT` трактоваться как обещание
durability после потери питания на неподдерживающей файловой системе.

**Автоматизированное доказательство Requirement:**
`cargo test -p bsl-analyzer partitioned_baseline_transaction`.

#### Scenario: Сбой до переключения manifest
- **WHEN** запись, sync, hash validation или публикация любого нового partition завершается ошибкой
- **THEN** прежний manifest и все его bytes остаются активными
- **AUTOMATED EVIDENCE** `partitioned_baseline_transaction_failure_before_manifest_preserves_old_generation`

#### Scenario: Сбой замены manifest
- **WHEN** новое поколение готово, но atomic manifest persist завершается ошибкой
- **THEN** читатели продолжают видеть прежнее поколение, а новое остаётся неактивным
- **AUTOMATED EVIDENCE** `partitioned_baseline_transaction_failure_before_manifest_preserves_old_generation`

#### Scenario: Гонка читателя с update
- **WHEN** manifest меняется между первым чтением и проверкой partition-файлов
- **THEN** loader ограниченно повторяет чтение и устанавливает целиком старый либо целиком новый snapshot
- **AUTOMATED EVIDENCE** `partitioned_baseline_set_loader_fails_closed_on_corruption`

#### Scenario: Сбой cleanup
- **WHEN** удаление временного файла текущей операции либо точно известного прежнего object из исходного manifest невозможно
- **THEN** успешность commit определяется manifest, активный set остаётся корректным, orphan не считается partition, а произвольный каталог не сканируется и не очищается
- **AUTOMATED EVIDENCE** `cleanup_failure_does_not_change_committed_generation`

#### Scenario: Два конкурентных писателя
- **WHEN** два selected update стартуют от одного manifest
- **THEN** только владелец блокировки публикует новый manifest, второй получает `concurrent_update`/busy и не теряет результат первого
- **AUTOMATED EVIDENCE** `concurrent_selected_updates_never_lose_a_commit`

#### Scenario: Object с ожидаемым именем уже существует
- **WHEN** content-addressed path существует, но его bytes не соответствуют ожидаемому hash во время обычного create/update/migrate
- **THEN** операция fail-closed до переключения manifest и не перезаписывает object; восстановление возможно только явным selected create по его отдельному контракту
- **AUTOMATED EVIDENCE** `existing_content_addressed_object_is_verified_before_reuse`

### Requirement: PDB-11 — Явное изменение topology
Добавление, удаление, переименование, смена path/dependencies расширения и изменение
группы `MUST` менять portable scope/partition identity. Анализатор `MUST NOT`
эвристически переносить известные записи между старой и новой identity.
Ошибки `MUST` иметь порядок: missing ids, orphan ids, identity общих ids, затем
`scope_mismatch` только при совпавших множествах и identities.

**Автоматизированное доказательство Requirement:**
`cargo test -p bsl-analyzer --test partitioned_diagnostics_baseline_cli topology_changes`.

#### Scenario: Добавлено расширение
- **WHEN** topology получила новое расширение
- **THEN** старый set сообщает `missing_partition`, selected create/update отказывает, а `update` без selector публикует новый полный plan
- **AUTOMATED EVIDENCE** `operations_all_and_selected_are_atomic_and_scoped`

#### Scenario: Расширение удалено
- **WHEN** topology больше не содержит extension из активного manifest
- **THEN** set сообщает `orphan_partition` и удаляет partition из нового manifest только при явно запущенном `update` без selector
- **AUTOMATED EVIDENCE** `operations_all_and_selected_are_atomic_and_scoped`

#### Scenario: Переименование или dependency change
- **WHEN** меняется name, path или depends_on
- **THEN** старый partition несовместим, изменение трактуется как remove+add и не сохраняет known автоматически
- **AUTOMATED EVIDENCE** `diagnostics_partition_identity_and_ownership`

#### Scenario: Изменилась группа
- **WHEN** меняется имя либо состав group
- **THEN** identities и scope меняются, а новый set публикуется только полным `update` без selector
- **AUTOMATED EVIDENCE** `diagnostics_partition_identity_and_ownership`

### Requirement: PDB-12 — Безопасность управляемых путей
`directory` `MUST` разрешаться относительно origin-файла конфигурации как legacy
`path`, с project root только для программно созданной конфигурации; CLI `--from-v1`
`MUST` разрешаться относительно project root. Все configured,
migration source, manifest, object, temporary и partition paths `MUST` оставаться внутри
канонического project root и `MUST NOT` проходить через symlink/reparse point. Хранимые
пути `MUST` быть нормализованными POSIX-relative и свободными от case-fold collisions.
Операции файловой системы `MUST` использовать закреплённый directory handle и
no-follow/reparse checks, исключающие подмену компонента после проверки.
Физический `partition-key` `MUST` быть lowercase hex BLAKE3 канонических UTF-8 bytes
`PartitionId`; display name `MUST` храниться только в identity/manifest.

**Автоматизированное доказательство Requirement:**
`cargo test -p project-model partitioned_baseline_path_security`.

#### Scenario: Directory выходит за проект
- **WHEN** `directory` абсолютный, содержит `..` либо канонически выходит из project root
- **THEN** конфигурация отклоняется до чтения/записи
- **AUTOMATED EVIDENCE** `partitioned_baseline_config_contract_rejects_absolute_and_parent_paths`

#### Scenario: База разрешения directory
- **WHEN** одинаковая TOML/JSON конфигурация загружена из разных CWD
- **THEN** `directory` разрешается относительно origin-файла и получает одинаковый canonical target
- **AUTOMATED EVIDENCE** `diagnostics_baseline_scope_is_independent_of_working_directory`

#### Scenario: Ссылка в управляемом layout
- **WHEN** directory, manifest, object/temporary component или partition file является symlink/reparse point
- **THEN** операция fail-closed и не следует ссылке
- **AUTOMATED EVIDENCE** `partitioned_baseline_path_security_rejects_links`

#### Scenario: Подмена компонента после проверки
- **WHEN** атакующий заменяет проверенный компонент пути ссылкой до open/create/rename
- **THEN** операция через закреплённый handle и no-follow завершается ошибкой и не касается внешней цели
- **AUTOMATED EVIDENCE** `partitioned_baseline_path_security_uses_pinned_relative_operations`

#### Scenario: Подмена пути через manifest
- **WHEN** manifest перечисляет абсолютный, traversal, непереносимый или неканонический file path
- **THEN** loader отклоняет весь set до открытия внешней цели
- **AUTOMATED EVIDENCE** `partitioned_baseline_path_security_rejects_non_portable_paths`

#### Scenario: Переносимое кодирование partition key
- **WHEN** ids содержат `/`, `\\`, `%`, `CON`, trailing dot/space, разный регистр либо NFC/NFD
- **THEN** golden keys состоят только из фиксированного lowercase hex, детерминированы и не создают файловых коллизий
- **AUTOMATED EVIDENCE** `diagnostics_partition_identity_and_ownership`

#### Scenario: Чужой файл рядом
- **WHEN** в directory существует файл, не перечисленный manifest и не принадлежащий управляемому temporary/object layout
- **THEN** loader его игнорирует, а cleanup не удаляет
- **AUTOMATED EVIDENCE** `partitioned_baseline_path_security_uses_pinned_relative_operations`

### Requirement: PDB-13 — Резидентная reload без пересоздания Salsa
MCP и LSP `MUST` наблюдать manifest и активные partition-файлы, атомарно заменять set
snapshot и переиспользовать неизменившиеся partition по hash. Изменение baseline
`MUST NOT` пересоздавать Salsa/VFS semantic state. Set epoch `MUST` входить в MCP
`result_id`.
Переход Ready↔Error либо смена set epoch `MUST` повторно классифицировать все открытые
LSP-документы и активную workspace batch, а не только документы изменённого partition.

**Автоматизированное доказательство Requirement:**
`cargo test -p mcp-server partitioned_baseline_reload` и
`cargo test -p bsl-analyzer lsp_partitioned_baseline_reload`.

#### Scenario: Изменён один partition
- **WHEN** manifest переключён на поколение с одним новым partition hash
- **THEN** MCP/LSP загружают новый set, переиспользуют остальные Arc snapshots и сохраняют адрес Salsa database
- **AUTOMATED EVIDENCE** `lsp_partitioned_baseline_reload_reuses_arcs_and_preserves_salsa`

#### Scenario: Изменился result_id
- **WHEN** active generation либо error state меняется без изменения source generation
- **THEN** file/workspace MCP `result_id` получает новую set epoch
- **AUTOMATED EVIDENCE** `partitioned_baseline_reload_reuses_unchanged_arcs_and_observes_every_object`

#### Scenario: Повреждение и восстановление
- **WHEN** active partition повреждён, удалён и затем исправлен новым manifest
- **THEN** MCP возвращает error branch; LSP публикует все текущие diagnostics и уведомляет один раз на partition error epoch; recovery возвращает фильтрацию без Salsa rebuild
- **AUTOMATED EVIDENCE** `partitioned_baseline_lsp_main_extension_group_partial_and_recovery`

#### Scenario: Ошибка одного partition меняет публикацию другого
- **WHEN** partition A повреждён, а открытый документ partition B содержит known диагностику
- **THEN** LSP повторно публикует документ B fail-open, а после recovery снова скрывает known без Salsa rebuild
- **AUTOMATED EVIDENCE** `partitioned_baseline_lsp_main_extension_group_partial_and_recovery`

### Requirement: PDB-14 — Совместимость CLI, MCP, LSP и репортёров
CLI и MCP `MUST` публиковать общую и per-partition сводки; LSP `MUST` применять тот же
set без нового protocol extension. Console, JSON, JSONL, SARIF и JUnit `MUST` сохранить
прежние поля/счётчики и добавить partition details обратно совместимо. GitLab Code
Quality `MUST` сохранить корневой массив и прежние fingerprints.
При ошибке набора CLI analyze `MUST` завершаться без отчёта, MCP schema version 14
`MUST` возвращать error-ветвь без findings/classification counts, а только LSP `MUST`
работать fail-open.
JSON/JSONL/SARIF/JUnit `MUST` размещать `baseline` соответственно в root,
`done.baseline`, `runs[].properties.baseline` и property `diagnostics.baseline`.
CLI operation-result `MUST` сохранять legacy-поля; без selector counts/diagnostics
агрегируются, с selector относятся только к нему и добавляется `selected_partition`.
`create --from-v1` является bounded-memory исключением: `diagnostics` остаётся пустым,
а полный размер миграции сообщается через `added`.

**Автоматизированное доказательство Requirement:**
`cargo test -p bsl-analyzer partitioned_baseline_reporters` и
`cargo test -p mcp-server diagnostics_partitioned_baseline_response`.

#### Scenario: MCP file/workspace response
- **WHEN** клиент запрашивает diagnostics с корректным set
- **THEN** структурированный и текстовый ответы schema/outputSchema 14 всегда содержат общую сводку, `partitions_total`, `partitions_returned`, `partitions_truncated`, а упорядоченные per-partition details заполняются только в пределах бюджета; file-запрос при возможности включает owner первым; явно заданный бюджет меньше 256 токенов отклоняется как некорректный параметр
- **AUTOMATED EVIDENCE** `diagnostics_partitioned_baseline_response_covers_schema_errors_and_minimum_budget`

#### Scenario: Ошибка набора в CLI и MCP
- **WHEN** один ожидаемый partition отсутствует, повреждён или несовместим
- **THEN** CLI analyze не формирует отчёт, а MCP schema 14 возвращает общую error-ветвь, первую детерминированную ошибку и `errors_total` без findings и классификационных counts
- **AUTOMATED EVIDENCE** `partitioned_baseline_uses_one_snapshot_for_file_workspace_error_and_recovery`

#### Scenario: LSP публикация
- **WHEN** LSP публикует документ main либо extension
- **THEN** known owner-partition diagnostics скрыты, new/защитные активны и main не дублируется в extension
- **AUTOMATED EVIDENCE** `lsp_partitioned_baseline_publish_uses_the_owner_partition`

#### Scenario: JSON, JSONL, SARIF и JUnit
- **WHEN** обычный analyze формирует каждый поддерживаемый отчёт
- **THEN** прежняя общая summary/count contract сохраняется, per-partition массив добавлен, SARIF Partial не получает `baselineState`, JUnit counts не меняются
- **AUTOMATED EVIDENCE** `partitioned_baseline_reporters_keep_their_existing_containers`

#### Scenario: GitLab Code Quality
- **WHEN** включён partitioned baseline и выбран Code Quality reporter
- **THEN** результат остаётся корневым массивом active findings без служебных элементов и с прежними fingerprints
- **AUTOMATED EVIDENCE** `codequality_shape_and_fingerprints_survive_partitioning`

### Requirement: PDB-15 — Масштаб 1,6 млн записей
Загрузка и классификация `MUST` быть линейными по сумме записей, использовать потоковое
чтение и компактные binary fingerprints и `MUST NOT` создавать объединённые raw-bytes
или rich-entry vectors всех partition. Reload одного файла `MUST` переиспользовать
неизменившиеся индексы. Каждая baseline-запись и текущая диагностика `MUST`
обрабатываться не более одного раза соответствующей стадией parse/fingerprint.

**Автоматизированное доказательство Requirement:**
`cargo test -p ide --release --test partitioned_baseline_scale -- --ignored` на Linux CI
и `cargo test -p bsl-analyzer --release --test partitioned_baseline_scale -- --ignored`.

#### Scenario: Полная загрузка 1,6 млн
- **WHEN** set содержит 1 600 000 валидных записей в нескольких partition
- **THEN** load/classify завершаются корректно, peak RSS сверх pre-load resident не превышает 1,5 размера входного набора и не создаётся combined entry vector
- **AUTOMATED EVIDENCE** `loads_and_classifies_1_6m_entries_with_bounded_rss`

#### Scenario: Reload одного partition
- **WHEN** меняется один малый partition большого set
- **THEN** loader не перечитывает и не перераспределяет неизменившиеся partition indexes
- **AUTOMATED EVIDENCE** `loads_and_classifies_1_6m_entries_with_bounded_rss`

#### Scenario: Selected update
- **WHEN** обновляется один partition большого set
- **THEN** неизменившиеся content-addressed paths/hashes переиспользуются напрямую и не сериализуются повторно
- **AUTOMATED EVIDENCE** `large_selected_update_does_not_reserialize_unchanged_partitions`

#### Scenario: Миграция schema v1
- **WHEN** `create --from-v1` переносит 1 600 000 записей
- **THEN** исходный JSON разбирается однопроходно, записи сразу направляются во временные partition-файлы, опубликованный set не загружается повторно, а peak RSS не превышает 1,5 размера входного файла
- **AUTOMATED EVIDENCE** `large_v1_migration_streams_with_bounded_rss`
