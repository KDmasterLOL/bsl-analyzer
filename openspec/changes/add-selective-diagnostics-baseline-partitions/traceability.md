# Traceability

Нормативный источник — `specs/selective-diagnostics-baseline-partitions/spec.md`.
Каждый Scenario имеет исполняемое доказательство и задачу; checkbox либо ручная
проверка не заменяют тест/CI.

## Requirement -> Code area -> Test -> Task

| Requirement | Основные области кода | Автоматизированное доказательство | Task(s) |
|---|---|---|---|
| SDBP-01 Config/specialization | `project-model::DiagnosticsBaselineConfig`, existing partition plan | config/policy/fingerprint units | 1.1–1.3 |
| SDBP-02 Shared topology | source loading, `ExtensionTopology`, Salsa, owner router | semantic integration | 1.2, 4.1 |
| SDBP-03 Classification | `ide::partitioned_diagnostics_baseline`, protected diagnostics | classifier units | 2.3, 4.1 |
| SDBP-04 Effective set/security | manifest loader, capability observations | manifest/reload/adversarial tests | 2.1, 2.2, 3.4, 6.1 |
| SDBP-05 Coverage/summary | per-owner coverage and summary aggregation | coverage units | 2.4 |
| SDBP-06 CLI/transaction | CLI baseline command, transaction, machine result | CLI/transaction integrations | 3.1, 3.2 |
| SDBP-07 Migration/lifecycle | v1 streaming, dormant metadata, reconciliation | migration/scale integrations | 3.3, 3.4, 6.3 |
| SDBP-08 MCP | schema 15, resident snapshot, result_id, response fitter | MCP unit/process tests | 5.1, 5.2 |
| SDBP-09 LSP | workspace/config reload, watcher, publication, recovery | LSP process tests | 2.2, 5.3, 5.4 |
| SDBP-10 Reporters/parity/scale | shared summary, reporters, common classifier, CI | reporter/parity/release tests | 4.2, 5.5, 6.2, 6.3 |

## Scenario Evidence Index

| Automated evidence | Task(s) |
|---|---|
| `selective_baseline_default_includes_all` | 1.1 |
| `selective_baseline_include_selects_exact_partition_ids` | 1.1, 1.2 |
| `selective_baseline_rejects_empty_duplicate_unknown_and_legacy_include` | 1.1 |
| `selective_semantics_keeps_full_topology_for_unsuppressed_extensions` | 4.1 |
| `selective_policy_does_not_change_partition_ownership` | 1.2, 4.1 |
| `selective_scope_ignores_changes_outside_enabled_owners` | 1.2, 2.1 |
| `selective_classifier_keeps_unsuppressed_diagnostics_visible` | 2.3 |
| `selective_classifier_never_looks_up_another_partition` | 2.3 |
| `selective_classifier_preserves_protected_diagnostics_for_both_policies` | 2.3 |
| `selective_manifest_keeps_existing_schemas_and_deterministic_effective_epoch` | 1.3, 2.1 |
| `selective_loader_fails_closed_for_every_enabled_object_error` | 2.1 |
| `selective_loader_rejects_unsafe_enabled_paths_and_links` | 6.1 |
| `selective_loader_never_reads_or_watches_unsuppressed_objects` | 2.1, 2.2 |
| `selective_loader_validates_enabled_identity_instead_of_global_scope` | 2.1 |
| `selective_loader_defers_dormant_content_validation_until_reenable` | 2.1, 3.4 |
| `selective_summary_separates_enabled_counts_from_unsuppressed` | 2.4 |
| `selective_coverage_does_not_hide_partial_unsuppressed_owner` | 2.4 |
| `selective_resolved_is_computed_only_for_full_enabled_partitions` | 2.4 |
| `selective_cli_create_publishes_only_enabled_partitions_atomically` | 3.1, 3.2 |
| `selective_cli_check_ignores_intentional_unsuppressed_drift` | 3.1 |
| `selective_cli_selected_operations_require_global_full_coverage` | 3.1 |
| `selective_cli_all_selected_policy_matrix` | 3.1 |
| `selective_cli_create_selected_missing_entry_accepts_only_selected_owner` | 3.1 |
| `selective_cli_repair_preserves_no_acceptance_contract` | 3.1, 3.2 |
| `selective_baseline_transaction_is_atomic_under_fault_and_concurrency` | 3.2 |
| `selective_v1_migration_streams_enabled_entries_and_preserves_source` | 3.3 |
| `selective_cli_rejects_from_v1_with_partition` | 3.3 |
| `selective_v1_migration_tracks_uniqueness_only_for_enabled_entries` | 3.3, 6.3 |
| `selective_existing_full_manifest_needs_no_file_migration` | 3.4 |
| `selective_full_update_reconciles_topology_and_prunes_dormant_metadata` | 3.4 |
| `selective_reenable_is_fail_closed_until_explicit_creation_or_repair` | 3.4 |
| `diagnostics_selective_baseline_file_owner_unsuppressed` | 5.1 |
| `diagnostics_selective_baseline_workspace_mixed_policy` | 5.1 |
| `diagnostics_selective_baseline_config_reload_changes_result_id` | 5.1 |
| `diagnostics_selective_baseline_schema_15_bounds_success_and_error_envelopes` | 5.2 |
| `selective_lsp_publishes_new_unsuppressed_and_protected` | 5.3 |
| `selective_lsp_config_reload_applies_selection_and_republishes` | 5.4 |
| `selective_lsp_enabled_object_reload_reuses_salsa_and_arcs` | 2.2, 5.4 |
| `selective_lsp_enabled_error_is_fail_visible_and_recovers` | 5.3 |
| `selective_lsp_does_not_watch_unsuppressed_objects` | 2.2, 5.4 |
| `selective_baseline_reporters_keep_existing_containers_and_show_policy` | 4.2 |
| `selective_baseline_codequality_and_sarif_preserve_fingerprint_semantics` | 4.2 |
| `selective_baseline_cli_mcp_lsp_parity` | 5.5 |
| `large_selective_baseline_load_skips_unsuppressed_objects` | 6.2 |
| `large_selective_v1_migration_streams_skipped_entries_with_bounded_rss` | 6.3 |

Task 6.5 механически требует точное равенство этого списка всем значениям
`AUTOMATED EVIDENCE` delta spec и наличие test/CI route для каждого имени.

## Cross-layer invariants

| Инвариант | Producer | Consumers | Доказательство |
|---|---|---|---|
| Полный owner plan не зависит от include | existing partition plan | migration/classifier/all surfaces | topology tests |
| Policy не является coverage state | plan + coverage | summaries/protocols/reporters | coverage/schema goldens |
| Один diagnostic fingerprint | `ide` | CLI/MCP/LSP/Code Quality | parity test |
| Protected всегда active new | common classifier | check/all surfaces | both-policy protected test |
| Enabled set атомарен и fail-closed | effective loader | CLI/MCP/LSP | error/recovery tests |
| Dormant content не читается | loader/observations | MCP/LSP watcher | stats/paths/scale tests |
| Config и baseline-file reload различны | workspace/resident lifecycle | MCP/LSP | config + object reload tests |
| Global Full обязателен для maintenance | coverage proof | all/selected CLI | CLI coverage test |

## Audit Gates

| Gate | Команда/доказательство | Условие прохождения |
|---|---|---|
| OpenSpec | `openspec validate add-selective-diagnostics-baseline-partitions --strict --no-interactive` | exit 0 |
| Evidence inventory | task 6.5 exact test | все Scenario mapped |
| Rust quality | task 6.6 exact fmt/clippy/tests | exit 0 |
| Regression | `cargo test --all --no-fail-fast` | no failures |
| Selective load | task 6.2 first release command | dormant I/O=0; both RSS bounds |
| Selective migration | task 6.3 release command | RSS <=25%; total=1.6M |
| Windows atomicity/security | `.github/workflows/ci.yml` job `windows-mcp`, selective transaction and path-security steps | old/new manifest only; reparse rejected |
| Patch hygiene | `git diff --check` | exit 0 |

## Cost and locked boundaries

Внешний сервис, новая dependency, капитальные и эксплуатационные денежные расходы:
N/A. Dormant objects могут занимать локальный диск до следующей отдельной уборки; это
явный trade-off, а не скрытый сервисный контур.

- Selection не фильтрует semantic topology.
- Missing/corrupt enabled baseline не становится unsuppressed.
- Unsuppressed policy сохраняет Full/Partial state и видимые diagnostics.
- Config change использует штатный full reload; baseline-file reload сохраняет Salsa.
- Production-код, commit, push и archive не входят в design-only change.
