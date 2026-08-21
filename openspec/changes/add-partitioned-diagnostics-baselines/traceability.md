# Traceability

Источники истины для реализации:

1. normative scenarios в `specs/partitioned-diagnostics-baselines/spec.md`;
2. архитектурные ограничения и commit point в `design.md`;
3. этапы и команды проверки в `tasks.md`.

Каждый Scenario в delta spec содержит строку `AUTOMATED EVIDENCE` с точным именем
теста. Перед завершением реализации исполнитель сверяет их с `cargo test -- --list`
либо с объявленным Linux/Windows CI job. Новый Markdown-парсер ради этой сверки не
вводится. Ручная проверка не считается закрытием сценария.

## Requirement -> Code area -> Test

| Requirement | Основные области кода | Автоматизированное доказательство |
|---|---|---|
| PDB-01 Configuration | `project-model::ProjectDiagnosticsConfig`, TOML/JSON loader, `check-config` | `partitioned_baseline_config_contract`; `migration_preserves_v1_and_rejects_unknown_selector_before_publish`; legacy config goldens |
| PDB-02 Identity/ownership | `project-model::ExtensionTopology`, новый partition planner, `SourceSet/WalkedFile` owner | `diagnostics_partition_identity_and_ownership`, `diagnostics_partition_identity_bounds_machine_ids` |
| PDB-03 Shared semantics | workspace loader, `ide::Analysis`, extension visibility/dependency closure | `one_semantic_run_classifies_main_and_extension_before_reporting` |
| PDB-04 Fingerprint/protected | `ide::diagnostics_baseline`, Code Quality adapter, common candidates | `partitioned_baseline_fingerprint_contract`; `partitioned_diagnostics_baseline_parity` |
| PDB-05 Manifest/schema | `ide::diagnostics_baseline` schema v2, manifest scope/codec/validator | `partitioned_baseline_schema_v2`; determinism/cross-duplicate tests |
| PDB-06 Migration | streaming v1 parser, owner router, staged partition writers, CLI `create --from-v1` | `partitioned_baseline_v1_migration_preserves_entries_without_current_diagnostics`; `migration_preserves_v1_and_rejects_unknown_selector_before_publish`; `large_v1_migration_streams_with_bounded_rss` |
| PDB-07 Set classification | compact set index, common classifier, summary aggregation | `partitioned_baseline_classification_and_summary` |
| PDB-08 Coverage | общий классификатор покрытия, MCP workspace sweep, completed-file ownership | `partitioned_baseline_classification_and_summary_routes_once`; `partitioned_baseline_uses_one_snapshot_for_file_workspace_error_and_recovery` |
| PDB-09 CLI operations | `bin/cli/diagnostics_baseline.rs`, machine result, selectors | `operations_all_and_selected_are_atomic_and_scoped` |
| PDB-10 Transaction | content-addressed objects, writer lock, manifest replace, cleanup | `partitioned_baseline_transaction`; шаг `Partitioned diagnostics baseline transaction` задания `windows-mcp` |
| PDB-11 Topology changes | partition planner diff, полный CLI update | `operations_all_and_selected_are_atomic_and_scoped` |
| PDB-12 Path security | origin-relative resolution, directory handles, no-follow/reparse, hashed keys, manifest path decoder | `partitioned_baseline_path_security` |
| PDB-13 Resident reload | `ide-host-core` set snapshot, MCP lifecycle/resident, LSP workspace-wide republish | `partitioned_baseline_reload_reuses_unchanged_arcs_and_observes_every_object`; `lsp_partitioned_baseline_reload_reuses_arcs_and_preserves_salsa` |
| PDB-14 Surfaces/reporters | MCP diagnostics schema 14, LSP publication, console/JSON/JSONL/SARIF/JUnit/Code Quality | `partitioned_baseline_reporters_keep_their_existing_containers`; `diagnostics_partitioned_baseline_response_covers_schema_errors_and_minimum_budget` |
| PDB-15 Scale | streaming loader/migration writer, compact fingerprint index/counters, CLI object reuse | задание `partitioned-baseline-scale`, выполняющее release-тесты `ide` и `bsl-analyzer`, включая `large_v1_migration_streams_with_bounded_rss` |

## Cross-layer invariants

| Инвариант | Producer | Consumers | Доказательство |
|---|---|---|---|
| Один portable project scope fingerprint | `project-model` partition plan | manifest codec, CLI, MCP, LSP | `partitioned_baseline_validates_scope_and_partition_identity` |
| Один diagnostic fingerprint recipe | `ide` | CLI, MCP, LSP, Code Quality | `partitioned_baseline_cli_mcp_lsp_parity`, Code Quality golden |
| Один root owner | partition planner | migration, classifier, reporters | `diagnostics_partition_identity_and_ownership` |
| Один semantic snapshot | workspace/AnalysisHost | all partition classifiers | `one_semantic_run_classifies_main_and_extension_before_reporting` |
| Один set commit point | atomic manifest | CLI readers, MCP, LSP | `partitioned_baseline_transaction_failure_before_manifest_preserves_old_generation` |
| Один set epoch | set snapshot | MCP file/workspace `result_id`, LSP error dedup | `partitioned_baseline_reload_reuses_unchanged_arcs_and_observes_every_object` |
| Fail-closed set load | set loader | CLI/MCP; LSP fail-open diagnostics only | `partitioned_baseline_set_loader_fails_closed_on_corruption`; `partitioned_baseline_lsp_main_extension_group_partial_and_recovery` |
| Per-partition coverage before aggregation | common coverage classifier | CLI/MCP summaries | `partitioned_baseline_classification_and_summary_routes_once` |

## Scenario evidence index

Ниже сгруппированы обязательные наборы тестов; точное соответствие каждого Scenario
указано непосредственно под ним в delta spec.

- Конфигурация/ownership: `partitioned_baseline_config_contract`,
  `diagnostics_partition_identity_and_ownership`, `partitioned_baseline_path_security`.
- Семантика/fingerprint: `partitioned_diagnostics_baseline_semantics`,
  `partitioned_baseline_fingerprint_contract`, `partitioned_baseline_cli_mcp_lsp_parity`.
- Формат/migration/classification: `partitioned_baseline_schema_v2`,
  `partitioned_baseline_v1_migration`, `partitioned_baseline_classification_and_summary`,
  `partitioned_baseline_coverage`.
- CLI/transaction/topology: `partitioned_diagnostics_baseline_cli`,
  `partitioned_baseline_transaction` и шаг `Partitioned diagnostics baseline transaction`
  задания `windows-mcp`.
- MCP/LSP: `partitioned_diagnostics_baseline`,
  `diagnostics_partitioned_baseline_response`, `partitioned_diagnostics_baseline_lsp`,
  `partitioned_diagnostics_baseline_parity`.
- Reporters/scale: `partitioned_baseline_reporters`, золотые данные Code Quality и
  задание `partitioned-baseline-scale`.

## Quality gates

1. `openspec validate add-partitioned-diagnostics-baselines --strict --no-interactive`.
2. Статическая сверка `AUTOMATED EVIDENCE` с `cargo test -- --list` и объявленными CI jobs.
3. Target crates format/Clippy/tests.
4. Full workspace tests.
5. Linux-шлюз RSS на 1,6 млн записей и Windows-шлюз атомарной транзакции.

Ни один checkbox в `tasks.md` не меняется на `[x]` только по design review: требуется
фактическое зелёное доказательство указанной команды.
