## ADDED Requirements

### Requirement: SDBP-01 — Явный allowlist и нормативная специализация
Partitioned-конфигурация с `directory` `MAY` содержать непустой `include` из точных id
`main`, `extension:<name>` и `group:<name>`. Отсутствующий `include` `MUST` сохранять
predecessor default-all. При наличии `include` только перечисленные owners `MUST` иметь
policy `Baseline`, остальные owners полного plan — policy `Unsuppressed`. Пустой,
дублирующий, неизвестный или конфликтующий с legacy `path` список `MUST` отклоняться до
baseline I/O.

При явном `include` упоминания «всех ожидаемых baseline partitions» в требованиях
PDB-05/PDB-07/PDB-09/PDB-11/PDB-13/PDB-14 `MUST` означать enabled subset. Для
PDB-11 изменения non-enabled owners `MUST NOT` давать `orphan_partition` при чтении;
для PDB-13 no-Salsa гарантия `MUST` относиться к manifest/object reload, тогда как
изменение `include` `MUST` использовать штатный full config reload. Это `MUST NOT`
сужать полный owner/topology plan или coverage proof.

**Автоматизированное доказательство Requirement:**
`cargo test -p project-model selective_baseline_config_contract` и
`cargo test -p bsl-analyzer --test selective_diagnostics_baseline_cli check_config_cli`.

#### Scenario: Обратная совместимость без include
- **WHEN** настроен `directory`, но `include` отсутствует
- **THEN** все owners имеют policy `Baseline`, а plan/scope совпадает с predecessor
- **AUTOMATED EVIDENCE** `selective_baseline_default_includes_all`

#### Scenario: Непустой allowlist
- **WHEN** `include = ["main", "extension:Sales"]`, а полный plan содержит других owners
- **THEN** два id enabled, остальные intentional `Unsuppressed`, порядок канонический
- **AUTOMATED EVIDENCE** `selective_baseline_include_selects_exact_partition_ids`

#### Scenario: Невалидный allowlist
- **WHEN** `include` пуст, содержит duplicate/unknown/case-fold collision, отдельного участника group либо задан вместе с `path`
- **THEN** `check-config` возвращает точный config error до файлового доступа
- **AUTOMATED EVIDENCE** `check_config_cli_rejects_invalid_include_before_baseline_io`

### Requirement: SDBP-02 — Полная topology и один owner
Selection `MUST NOT` фильтровать source loading, extension dependencies, semantic run
или Salsa inputs. Каждая diagnostic `MUST` получить ровно одного owner из полного
partition plan до применения policy. Группировка `MUST NOT` менять dependency graph.

**Автоматизированное доказательство Requirement:**
`cargo test -p bsl-analyzer --test selective_diagnostics_baseline_semantics`.

#### Scenario: Unsuppressed extension видит main и dependencies
- **WHEN** вызов в unsuppressed extension разрешается через main либо dependency
- **THEN** единый анализ не создаёт ложный `UnresolvedMethodCall`
- **AUTOMATED EVIDENCE** `selective_semantics_keeps_full_topology_for_unsuppressed_extensions`

#### Scenario: Назначение owner не зависит от policy
- **WHEN** один и тот же полный plan строится с отсутствующим и явным `include`
- **THEN** root-to-owner mapping идентичен и покрывает каждый source root ровно один раз
- **AUTOMATED EVIDENCE** `selective_policy_does_not_change_partition_ownership`

#### Scenario: Изменение только невключённой topology
- **WHEN** добавлена, удалена или изменена extension, id которой не входит в `include`
- **THEN** read-only enabled set готов; добавленный/изменённый current owner имеет `Unsuppressed`, а удалённый исчезает из plan и его metadata игнорируется до full update
- **AUTOMATED EVIDENCE** `selective_scope_ignores_changes_outside_enabled_owners`

### Requirement: SDBP-03 — Общая классификация и защитные diagnostics
Общий classifier `MUST` возвращать `known`/`new` для enabled owners и `unsuppressed`
для остальных без baseline lookup. `unsuppressed` `MUST NOT` входить в `new`, `known`
или `resolved`. `UnknownSuppressionCode` и `SuppressionWithoutCode` `MUST` оставаться
активными `new` при обеих policy и `MUST NOT` записываться в baseline.

**Автоматизированное доказательство Requirement:**
`cargo test -p ide selective_baseline_classification`.

#### Scenario: Intentional unsuppressed остаётся видимой
- **WHEN** незащитная diagnostic принадлежит unsuppressed owner
- **THEN** finding активен, classification=`unsuppressed`, count=1, а new=known=resolved=0
- **AUTOMATED EVIDENCE** `selective_classifier_keeps_unsuppressed_diagnostics_visible`

#### Scenario: Нет cross-partition lookup
- **WHEN** одинаковый fingerprint сохранён у enabled owner и текущий у unsuppressed owner
- **THEN** только enabled finding становится known
- **AUTOMATED EVIDENCE** `selective_classifier_never_looks_up_another_partition`

#### Scenario: Защитные diagnostics при обеих policy
- **WHEN** защитная diagnostic принадлежит enabled либо unsuppressed owner
- **THEN** она остаётся active new, делает check неуспешным и не попадает в create/update
- **AUTOMATED EVIDENCE** `selective_classifier_preserves_protected_diagnostics_for_both_policies`

### Requirement: SDBP-04 — Effective set, scope и fail-closed
Manifest `MUST` оставаться schema v1, object — schema v2; selection `MUST NOT`
дублироваться в файлах. Loader `MUST` читать, хешировать и наблюдать только enabled
objects. Missing, corrupt, unsafe path, hash/JSON/schema/fingerprint/identity mismatch
любого enabled id `MUST` инвалидировать весь effective snapshot без частичного
подавления.

При явном `include` loader `MUST` валидировать identity только enabled owners и `MUST`
игнорировать non-enabled/orphan entries без object I/O. Duplicate manifest id и unsafe
entry path `MUST` отклоняться по manifest metadata; cross-object duplicates `MUST`
проверяться среди enabled objects и при последующем включении dormant object. Без
`include` полная scope-проверка predecessor `MUST` сохраниться.

**Автоматизированное доказательство Requirement:**
`cargo test -p ide selective_baseline_manifest` и
`cargo test -p ide-host-core selective_baseline_reload`.

#### Scenario: Совместимые схемы и epoch
- **WHEN** allowlist включает часть plan
- **THEN** file schemas неизменны, а selection fingerprint/epoch детерминирован по versioned recipe и ordered `{id, policy, identity}` полного current plan без чтения dormant objects
- **AUTOMATED EVIDENCE** `selective_manifest_keeps_existing_schemas_and_deterministic_effective_epoch`

#### Scenario: Любая ошибка enabled object закрывает set
- **WHEN** enabled entry missing/corrupt/unsafe либо имеет неверные hash, JSON, schema, identity или fingerprint
- **THEN** state=`error`, ошибка называет id, и ни один enabled baseline не применяется
- **AUTOMATED EVIDENCE** `selective_loader_fails_closed_for_every_enabled_object_error`

#### Scenario: Symlink и traversal enabled entry
- **WHEN** enabled path содержит traversal, symlink или Windows reparse contour
- **THEN** capability/no-follow boundary отклоняет его без чтения вне project root
- **AUTOMATED EVIDENCE** `selective_loader_rejects_unsafe_enabled_paths_and_links`

#### Scenario: Dormant object не открывается
- **WHEN** non-enabled object отсутствует, повреждён или является ссылкой
- **THEN** snapshot готов, load stats и observation paths доказывают отсутствие read/hash/watch
- **AUTOMATED EVIDENCE** `selective_loader_never_reads_or_watches_unsuppressed_objects`

#### Scenario: Scope change вне enabled owners
- **WHEN** полный scope fingerprint изменился только из-за non-enabled/orphan owner
- **THEN** enabled identities валидируются и snapshot остаётся готовым без dormant I/O
- **AUTOMATED EVIDENCE** `selective_loader_validates_enabled_identity_instead_of_global_scope`

#### Scenario: Duplicate metadata и deferred duplicate validation
- **WHEN** manifest содержит duplicate id либо dormant object с внутренним duplicate fingerprint
- **THEN** duplicate id отклоняется сразу, а внутренний duplicate проверяется fail-closed при re-enable
- **AUTOMATED EVIDENCE** `selective_loader_defers_dormant_content_validation_until_reenable`

### Requirement: SDBP-05 — Coverage и summary
Coverage `MUST` вычисляться для каждого owner полного plan. Policy и state `MUST` быть
раздельны: enabled и unsuppressed detail имеют `state=full|partial`, а
`policy=baseline|unsuppressed`. Для незащитных diagnostics unsuppressed owner имеет
`resolved=0` и `new=known=0`; protected diagnostics увеличивают per-partition и общий
`new`, но не `unsuppressed`. Общая
summary `MUST` содержать `selection`, `partitions_enabled`,
`partitions_unsuppressed`, `unsuppressed` и ordered details. `complete=true` `MUST`
требовать полного coverage запрошенной surface.

**Автоматизированное доказательство Requirement:**
`cargo test -p ide selective_baseline_coverage_and_summary`.

#### Scenario: Полный mixed-policy workspace
- **WHEN** workspace Full, enabled baseline без drift, а unsuppressed owner имеет три diagnostics
- **THEN** state=full, complete=true, отдельный unsuppressed=3 и точные policy counts
- **AUTOMATED EVIDENCE** `selective_summary_separates_enabled_counts_from_unsuppressed`

#### Scenario: Partial unsuppressed owner
- **WHEN** unsuppressed owner покрыт частично
- **THEN** его state=partial, policy=unsuppressed, complete=false и общий complete=false
- **AUTOMATED EVIDENCE** `selective_coverage_does_not_hide_partial_unsuppressed_owner`

#### Scenario: Resolved только для enabled Full
- **WHEN** сохранённый fingerprint исчез у enabled owner при Full coverage
- **THEN** resolved вычислен только там; у unsuppressed owner resolved=0
- **AUTOMATED EVIDENCE** `selective_resolved_is_computed_only_for_full_enabled_partitions`

### Requirement: SDBP-06 — CLI all/selected и атомарность
`create|check|update [--partition <id>]` `MUST` выполнять один анализ полной topology и
требовать глобальный `CoverageProof::Full`, включая selected unsuppressed check. Без
selector операции `MUST` охватывать все enabled owners. Selected result `MUST`
ограничивать diagnostics/counts/detail выбранным owner и сохранять global selection
metadata. `check --partition` `MUST` принимать любую plan partition; mutation
unsuppressed owner `MUST` вернуть `partition_unsuppressed` до записи.

Все multi-file операции `MUST` сохранить writer lock, immutable object publish, sync и
один atomic manifest commit point. При partial failure readers `MUST` видеть только
старое или новое поколение. `create --from-v1` и `--partition` `MUST` конфликтовать.

**Автоматизированное доказательство Requirement:**
`cargo test -p bsl-analyzer --test selective_diagnostics_baseline_cli` и
`cargo test -p bsl-analyzer selective_baseline_transaction`.

#### Scenario: Первый selective create
- **WHEN** manifest отсутствует, Full доказан, а allowlist включает часть plan
- **THEN** атомарно публикуются objects только enabled owners
- **AUTOMATED EVIDENCE** `selective_cli_create_publishes_only_enabled_partitions_atomically`

#### Scenario: Intentional findings не ломают check
- **WHEN** enabled owners не имеют new/resolved, а unsuppressed owner имеет findings
- **THEN** check exit=0, ничего не пишет и возвращает unsuppressed count
- **AUTOMATED EVIDENCE** `selective_cli_check_ignores_intentional_unsuppressed_drift`

#### Scenario: Selected operation и глобальный Full
- **WHEN** selected owner Full, но любой другой owner полного plan Partial
- **THEN** create/check/update блокируется coverage error до чтения либо записи result scope
- **AUTOMATED EVIDENCE** `selective_cli_selected_operations_require_global_full_coverage`

#### Scenario: Selected enabled и unsuppressed matrix
- **WHEN** каждая команда вызывается для enabled и unsuppressed id
- **THEN** enabled scope корректен; unsuppressed check read-only, а mutations отклонены
- **AUTOMATED EVIDENCE** `selective_cli_all_selected_policy_matrix`

#### Scenario: Создание отсутствующей повторно включённой entry
- **WHEN** enabled id отсутствует в manifest и global Full доказан
- **THEN** `create --partition` явно принимает только текущие diagnostics выбранного owner и атомарно добавляет entry
- **AUTOMATED EVIDENCE** `selective_cli_create_selected_missing_entry_accepts_only_selected_owner`

#### Scenario: Repair существующей повреждённой entry
- **WHEN** entry существует, но object missing/corrupt
- **THEN** `create --partition` восстанавливает прежние entries без принятия current new diagnostics
- **AUTOMATED EVIDENCE** `selective_cli_repair_preserves_no_acceptance_contract`

#### Scenario: Fault, concurrency и Windows replace
- **WHEN** fault injected до/после object publish либо два writers конкурируют
- **THEN** manifest old-or-new, lock детерминирован, temp очищен; Windows replace равносилен
- **AUTOMATED EVIDENCE** `selective_baseline_transaction_is_atomic_under_fault_and_concurrency`

### Requirement: SDBP-07 — Миграция и dormant lifecycle
Legacy `path` `MUST` остаться неизменным. `create --from-v1` `MUST` потоково
маршрутизировать все legacy entries, писать только enabled owners, считать
`skipped_unsuppressed`, сохранять source bytes и `MUST NOT` принимать current new
diagnostics. Команда `MUST` обрабатывать все enabled owners и конфликтовать с
`--partition`. Skipped entries `MUST` пройти path/protected-code/fingerprint-recipe/
owner validation, но uniqueness set `MUST` хранить только enabled entries; duplicate
skipped content `MUST NOT` переноситься или влиять на selective output; будущий
baseline этого owner `MUST` валидировать заново создаваемые current entries.

Существующий full manifest `MUST` работать selective без записи. При неизменном полном
scope следующая публикация `MUST` переносить structurally safe dormant metadata только
для current-plan owners без object I/O и `MUST` удалять orphan metadata. При scope
change только full `update` существующего manifest `MUST` согласовать topology и удалить все dormant
metadata; selected mutations `MUST` блокироваться. Dormant content `MUST`
валидироваться fail-closed при re-enable.

**Автоматизированное доказательство Requirement:**
`cargo test -p bsl-analyzer --test selective_diagnostics_baseline_migration`.

#### Scenario: Потоковая selective v1 migration
- **WHEN** v1 содержит enabled и unsuppressed owners
- **THEN** migrated+skipped_unsuppressed равно input count, source bytes неизменны и current new не приняты
- **AUTOMATED EVIDENCE** `selective_v1_migration_streams_enabled_entries_and_preserves_source`

#### Scenario: Нельзя сочетать migration и selector
- **WHEN** переданы `create --from-v1 ... --partition main`
- **THEN** parser возвращает usage error до анализа и файлового I/O
- **AUTOMATED EVIDENCE** `selective_cli_rejects_from_v1_with_partition`

#### Scenario: Uniqueness хранится только для enabled entries
- **WHEN** v1 содержит duplicate fingerprint отдельно среди enabled и skipped owners
- **THEN** enabled duplicate отклоняется, skipped duplicate не удерживается и не переносится, а migrated+skipped остаётся точным
- **AUTOMATED EVIDENCE** `selective_v1_migration_tracks_uniqueness_only_for_enabled_entries`

#### Scenario: Existing full manifest без миграции
- **WHEN** к совместимому full manifest добавлен `include`
- **THEN** loader читает только enabled entries, check read-only, dormant metadata переносится без object read при неизменном scope
- **AUTOMATED EVIDENCE** `selective_existing_full_manifest_needs_no_file_migration`

#### Scenario: Topology reconciliation
- **WHEN** scope изменился и manifest содержит dormant/orphan metadata
- **THEN** check/selected mutations не пишут, а full update атомарно удаляет dormant metadata и создаёт требуемые enabled entries
- **AUTOMATED EVIDENCE** `selective_full_update_reconciles_topology_and_prunes_dormant_metadata`

#### Scenario: Re-enable missing/corrupt
- **WHEN** id добавлен в include, а entry missing либо object corrupt
- **THEN** runtime fail-closed до selected create; full create допустим только без manifest, full update — при существующем manifest
- **AUTOMATED EVIDENCE** `selective_reenable_is_fail_closed_until_explicit_creation_or_repair`

### Requirement: SDBP-08 — MCP schema, budget и result_id
MCP diagnostics schema/outputSchema `MUST` стать 15 и additive возвращать selection,
partition policy и unsuppressed counts. File/workspace `result_id` `MUST` включать
selection epoch. Один линейный budget algorithm `MUST` ограничивать весь success/error
envelope и сохранять обязательную summary и точную первую error identity. Enabled
error `MUST` возвращаться без partial findings/counts.

**Автоматизированное доказательство Requirement:**
`cargo test -p mcp-server diagnostics_selective_baseline_response` и
`cargo test -p mcp-server --test selective_diagnostics_baseline`.

#### Scenario: File owner unsuppressed
- **WHEN** запрошен file unsuppressed owner
- **THEN** findings active/classification=unsuppressed, detail policy точна, lookup отсутствует
- **AUTOMATED EVIDENCE** `diagnostics_selective_baseline_file_owner_unsuppressed`

#### Scenario: Workspace mixed policy
- **WHEN** workspace содержит enabled known/new и unsuppressed findings
- **THEN** counts раздельны, ordered details и selection epoch едины
- **AUTOMATED EVIDENCE** `diagnostics_selective_baseline_workspace_mixed_policy`

#### Scenario: Config selection меняет result_id
- **WHEN** source/object bytes те же, но `include` изменён и штатный config reload завершён
- **THEN** file/workspace result_id меняются и старый id stale
- **AUTOMATED EVIDENCE** `diagnostics_selective_baseline_config_reload_changes_result_id`

#### Scenario: Минимальный budget и enabled error
- **WHEN** success/error содержит много details либо длинную enabled error при минимальном budget
- **THEN** весь JSON помещается, truncation/totals и первая error identity сохранены
- **AUTOMATED EVIDENCE** `diagnostics_selective_baseline_schema_15_bounds_success_and_error_envelopes`

### Requirement: SDBP-09 — LSP publication, reload и recovery
LSP `MUST` публиковать `new`, `unsuppressed` и protected, скрывать `known`. Изменение
`include` `MUST` проходить штатный project config reload, перепубликовывать открытые
documents и сбрасывать workspace batch. Manifest/active-object reload `MUST` сохранить
predecessor no-Salsa path и `Arc` reuse. Enabled error `MUST` fail-visible публиковать
все current diagnostics и одно уведомление на error epoch; dormant object `MUST NOT`
наблюдаться. Recovery `MUST` возвращать filtering без повторного уведомления.

**Автоматизированное доказательство Requirement:**
`cargo test -p bsl-analyzer --test selective_diagnostics_baseline_lsp`.

#### Scenario: Mixed-policy publication
- **WHEN** открыты enabled и unsuppressed documents
- **THEN** known скрыт, new/unsuppressed/protected опубликованы с общими owner/fingerprint
- **AUTOMATED EVIDENCE** `selective_lsp_publishes_new_unsuppressed_and_protected`

#### Scenario: Config reload selection
- **WHEN** `include` изменён
- **THEN** штатный reload применяет новый Project, перепубликует documents и заменяет batch без специального fast path
- **AUTOMATED EVIDENCE** `selective_lsp_config_reload_applies_selection_and_republishes`

#### Scenario: Baseline-file reload без Salsa rebuild
- **WHEN** атомарно заменён manifest либо enabled object при неизменном config
- **THEN** epoch меняется, DB address/Salsa generation стабильны и unchanged Arc reused
- **AUTOMATED EVIDENCE** `selective_lsp_enabled_object_reload_reuses_salsa_and_arcs`

#### Scenario: Enabled corruption и recovery
- **WHEN** enabled object повреждён, затем восстановлен
- **THEN** LSP публикует все diagnostics и одно уведомление, затем молча возвращает filtering
- **AUTOMATED EVIDENCE** `selective_lsp_enabled_error_is_fail_visible_and_recovers`

#### Scenario: Dormant corruption ignored
- **WHEN** dormant object изменён/удалён
- **THEN** watcher, epoch, publication и notification не меняются
- **AUTOMATED EVIDENCE** `selective_lsp_does_not_watch_unsuppressed_objects`

### Requirement: SDBP-10 — Reporters, parity и масштаб
Console, JSON, JSONL, SARIF и JUnit `MUST` сохранить containers и additive показать
selection/policy/unsuppressed summary. Unsuppressed findings `MUST` оставаться в обычных
containers; SARIF `MUST NOT` назначать им `baselineState`. Code Quality `MUST` сохранить
root array и fingerprint. Все surfaces `MUST` использовать один classifier/fingerprint.

На fixture 1 600 000 entries loader `MUST` иметь parse/hash/watch=0 для dormant
objects. Если enabled objects содержат не более 10% entries, loader-only incremental
peak RSS `MUST` быть <= `max(128 MiB, 2 * enabled_object_bytes)` и <=25% full-load
incremental RSS. Selective v1 migration `MUST` иметь peak RSS <=25% predecessor full
migration при том же input и доказать migrated+skipped=1 600 000.

**Автоматизированное доказательство Requirement:**
reporter/parity integration tests и ignored release scale tests.

#### Scenario: Совместимые reporter containers
- **WHEN** анализ содержит enabled new/known и unsuppressed findings
- **THEN** additive summaries точны, known исключён из active containers, пути containers неизменны
- **AUTOMATED EVIDENCE** `selective_baseline_reporters_keep_existing_containers_and_show_policy`

#### Scenario: Code Quality и SARIF
- **WHEN** unsuppressed finding сериализуется в Code Quality и SARIF
- **THEN** fingerprint прежний, служебных элементов нет, SARIF baselineState отсутствует
- **AUTOMATED EVIDENCE** `selective_baseline_codequality_and_sarif_preserve_fingerprint_semantics`

#### Scenario: Сквозная parity
- **WHEN** CLI, MCP file/workspace и LSP обрабатывают одну revision/selection
- **THEN** fingerprint, owner, policy и classification совпадают
- **AUTOMATED EVIDENCE** `selective_baseline_cli_mcp_lsp_parity`

#### Scenario: Selective load 1,6 млн
- **WHEN** парный full/selective loader test включает <=10% из 1,6 млн entries
- **THEN** dormant parse/hash/watch=0, RSS проходит оба предела, owner plan полон и Arc reused
- **AUTOMATED EVIDENCE** `large_selective_baseline_load_skips_unsuppressed_objects`

#### Scenario: Selective migration 1,6 млн
- **WHEN** mostly-unsuppressed v1 из 1,6 млн entries мигрируется release build
- **THEN** RSS <=25% full migration, migrated+skipped=1,6 млн и combined rich-entry vector отсутствует
- **AUTOMATED EVIDENCE** `large_selective_v1_migration_streams_skipped_entries_with_bounded_rss`
