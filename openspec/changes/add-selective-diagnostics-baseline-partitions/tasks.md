## 1. Configuration и полный policy plan

- [x] 1.1 Добавить `include` в `DiagnosticsBaselineConfig`, default-all и точную
  валидацию непустого allowlist без нового реестра topology. (SDBP-01; Decision 1)
  Проверка: `cargo test -p project-model selective_baseline_config_contract`.
- [x] 1.2 Расширить существующий `DiagnosticsBaselinePartitionPlan` enabled id/policy и
  переходами add/remove/rename, сохранив owner/dependency graph. (SDBP-01, SDBP-02;
  Decision 1) Проверка: `cargo test -p project-model selective_baseline_policy_plan`.
- [x] 1.3 Реализовать versioned canonical selection fingerprint отдельно от diagnostic
  fingerprint. (SDBP-01, SDBP-08; Decisions 1, 8) Проверка:
  `cargo test -p project-model selective_baseline_selection_fingerprint`.

## 2. Loader, classifier и summary

- [x] 2.1 Специализировать schema-v1 manifest loader: enabled identity validation,
  fail-closed errors и deferred dormant validation без чтения objects. (SDBP-04;
  Decisions 3, 4) Проверка: `cargo test -p ide selective_baseline_manifest`.
- [x] 2.2 Ограничить observations enabled objects, сохранить capability/no-follow и
  reload с `Arc` reuse; доказывать отсутствие dormant I/O через load stats и paths.
  (SDBP-04, SDBP-09; Decisions 3, 8) Проверка:
  `cargo test -p ide-host-core selective_baseline_reload`.
- [x] 2.3 Добавить в общий classifier `unsuppressed`, сохранив общий fingerprint и
  защитные diagnostics как active `new`. (SDBP-03; Decision 2) Проверка:
  `cargo test -p ide selective_baseline_classification`.
- [x] 2.4 Расширить coverage/summary отдельными policy, Full/Partial state и counts без
  combined rich-entry vector. (SDBP-05; Decisions 2, 6) Проверка:
  `cargo test -p ide selective_baseline_coverage_and_summary`.

## 3. CLI, transaction и migration

- [x] 3.1 Реализовать all/selected CLI matrix, global Full, selected result scope,
  missing-entry acceptance и no-acceptance repair. (SDBP-06; Decision 5) Проверка:
  `cargo test -p bsl-analyzer --test selective_diagnostics_baseline_cli`.
- [x] 3.2 Сохранить atomic manifest transaction, carry-through dormant metadata,
  concurrency/fault cleanup и Windows replace. (SDBP-06, SDBP-07; Decisions 4, 5)
  Проверки: `cargo test -p bsl-analyzer selective_baseline_transaction`; workflow
  `.github/workflows/ci.yml`, job `windows-mcp`, step
  `Selective diagnostics baseline transaction`.
- [x] 3.3 Реализовать streaming `create --from-v1` для всех enabled owners,
  `skipped_unsuppressed`, source preservation и конфликт с `--partition`. (SDBP-07;
  Decision 7) Проверка:
  `cargo test -p bsl-analyzer --test selective_diagnostics_baseline_migration`.
- [x] 3.4 Реализовать config-only переход full manifest, dormant carry-through при
  том же scope, full reconciliation при scope change и fail-closed re-enable.
  (SDBP-04, SDBP-07; Decisions 3, 4, 7) Проверка:
  `cargo test -p bsl-analyzer --test selective_diagnostics_baseline_migration`.
- [x] 4.1 Подключить общий policy classifier после одного полного post-suppression
  semantic run и до presentation filters. (SDBP-02, SDBP-03; Decisions 1, 2)
  Проверка: `cargo test -p bsl-analyzer --test selective_diagnostics_baseline_semantics`.
- [x] 4.2 Расширить shared summary и console/goldens; сохранить JSON/JSONL/SARIF/JUnit
  containers и Code Quality fingerprint без отдельных classifier-ов. (SDBP-10;
  Decision 9) Проверка:
  `cargo test -p bsl-analyzer --test selective_diagnostics_baseline_reporters`.
- [x] 5.1 Повысить MCP schema/outputSchema до 15, добавить file/workspace policy,
  selection epoch/result_id и schema goldens. (SDBP-08; Decision 8) Проверки:
  `cargo test -p mcp-server diagnostics_selective_baseline_response` и
  `cargo test -p mcp-server --test selective_diagnostics_baseline`.
- [x] 5.2 Расширить единый линейный MCP response fitter на mixed-policy success и
  enabled-error envelope. (SDBP-08; Decision 8) Проверка:
  `cargo test -p mcp-server --test selective_diagnostics_baseline`.
- [x] 5.3 Реализовать LSP mixed-policy publication, enabled fail-visible error и
  recovery с notification dedup. (SDBP-09; Decision 8) Проверка:
  `cargo test -p bsl-analyzer --test selective_diagnostics_baseline_lsp`.
- [x] 5.4 Провести изменение `include` через штатный config reload; сохранить no-Salsa
  baseline-file reload, batch reset, enabled watches и dormant silence. (SDBP-09;
  Decision 8) Проверка:
  `cargo test -p bsl-analyzer --test selective_diagnostics_baseline_lsp`.
- [x] 5.5 Доказать CLI/MCP file/workspace/LSP parity общего owner, fingerprint, policy и
  classification. (SDBP-10; Decisions 2, 8, 9) Проверка:
  `cargo test -p bsl-analyzer --test selective_diagnostics_baseline_parity`.
- [x] 6.1 Добавить adversarial enabled path/symlink/reparse regressions, переиспользуя
  capability boundary predecessor. (SDBP-04; Decisions 3, 10) Проверки:
  `cargo test -p project-model partitioned_baseline_path_security` и
  `cargo test -p ide selective_loader_rejects_unsafe_enabled_paths_and_links`;
  workflow `.github/workflows/ci.yml`, job `windows-mcp`, step
  `Selective baseline path security`.

## 4. Analyze и reporters


## 5. MCP и LSP


## 6. Безопасность, масштаб и quality gates

- [x] 6.2 Добавить парный ignored release gate selective/full load на 1,6 млн с
  dormant I/O=0 и обоими RSS limits. (SDBP-10; Decision 10) Проверка:
  `cargo test -p ide --release --test selective_baseline_scale large_selective_baseline_load_skips_unsuppressed_objects -- --ignored --exact`;
  workflow `.github/workflows/ci.yml`, job `partitioned-baseline-scale`.
- [x] 6.3 Добавить парный ignored release gate selective/full v1 migration на 1,6 млн
  с RSS <=25%, uniqueness только enabled и точным migrated+skipped. (SDBP-07,
  SDBP-10; Decisions 7, 10) Проверка:
  `cargo test -p bsl-analyzer --release --test selective_baseline_scale large_selective_v1_migration_streams_skipped_entries_with_bounded_rss -- --ignored --exact`;
  workflow `.github/workflows/ci.yml`, job `partitioned-baseline-scale`.
- [x] 6.4 Обновить только `docs/configuration/DIAGNOSTICS.md`,
  `docs/configuration/PROJECT_CONFIGURATION.md`, `docs/mcp/TOOLS_AND_EXTENSION.md`,
  `docs/CI_REPORTERS.md` и `crates/bsl-analyzer/src/bin/main.rs` с configuration,
  CLI help, rollout/rollback и reporter
  contract. (SDBP-01, SDBP-06, SDBP-08, SDBP-10; Decisions 5, 8, 9) Проверки:
  `cargo test -p bsl-analyzer --test selective_diagnostics_baseline_cli documented_selective_usage` и
  `cargo test -p mcp-server diagnostics_schema_json`.
- [x] 6.5 Добавить механический inventory test, который находит change в active либо
  archive path, извлекает все
  `AUTOMATED EVIDENCE` из delta spec и требует соответствующий test/CI mapping из
  `traceability.md`. (SDBP-01..SDBP-10) Проверка:
  `cargo test -p bsl-analyzer --test openspec_evidence_inventory selective_diagnostics_baseline_evidence_is_complete`.
- [x] 6.6 Выполнить целевые шлюзы: `cargo fmt --all -- --check`;
  `cargo clippy -p project-model -p ide -p ide-host-core -p bsl-analyzer -p mcp-server --all-targets --all-features -- -D warnings`;
  все команды задач 1.1–6.5; `git diff --check`. (SDBP-01..SDBP-10)
- [x] 6.7 Выполнить полный Linux/Windows/OpenSpec gate:
  `cargo test --all --no-fail-fast`; release-команды 6.2 и 6.3; workflow job
  `windows-mcp`, steps `Selective diagnostics baseline transaction` и
  `Selective baseline path security`;
  `openspec validate add-selective-diagnostics-baseline-partitions --strict --no-interactive`.
  (SDBP-01..SDBP-10) Windows evidence: локальная VM `win10`, Rust GNU 1.97.1;
  оба workflow step выполнены на NTFS-копии worktree с exit code 0.

## Разрывы ревью

- [x] 7.1 Заменить синтаксический surrogate в SDBP-02 evidence на реальный вызов из
  unsuppressed extension в main common module и отрицательный контроль
  `UnresolvedMethodCall`. (DSH H1) Проверка:
  `cargo test -p bsl-analyzer --test selective_diagnostics_baseline_semantics`.
- [x] 7.2 Сделать общий classifier fail-closed без panic для несовместимой пары
  snapshot/plan и оставить regression. (DSH M1) Проверка:
  `cargo test -p ide selective_classifier_rejects_incompatible_snapshot_and_plan_without_panic`.
- [x] 7.3 До selected missing-entry publish валидировать все enabled siblings и
  исправить `unsuppressed` в topology-reconciliation result. (DSH M2, M3; local review)
  Проверки: `cargo test -p bsl-analyzer --test selective_diagnostics_baseline_cli` и
  `cargo test -p bsl-analyzer --test selective_diagnostics_baseline_migration`.
- [x] 7.4 Очистить LSP error ledger после recovery и запускать Arc/Salsa evidence в
  настоящем selective config. (DSH L1, L2) Проверки:
  `cargo test -p bsl-analyzer selective_lsp_enabled_object_reload_reuses_salsa_and_arcs` и
  `cargo test -p bsl-analyzer --test selective_diagnostics_baseline_lsp selective_lsp_enabled_error_is_fail_visible_and_recovers -- --exact`.
- [x] 7.5 Сделать config conflict priority детерминированным, evidence inventory —
  fail-closed на test declaration/CI route, RSS differential — ненулевым. (DSH L3;
  local Ponytail/implementation review) Проверки:
  `cargo test -p project-model selective_baseline_rejects_empty_duplicate_unknown_and_legacy_include`,
  `cargo test -p bsl-analyzer --test openspec_evidence_inventory` и release-команда 6.2.
- [x] 7.6 Подтвердить, что CLI maintenance synthetic per-partition Full следует из
  обязательного global Full preflight; не добавлять недостижимый partial maintenance
  path. (DSH L4) Проверка:
  `cargo test -p bsl-analyzer --test selective_diagnostics_baseline_cli selective_cli_selected_operations_require_global_full_coverage -- --exact`.
- [x] 7.7 Принять partitioned `ReadySet` в `check-config`, вывести selection и policy
  partitions, а invalid `include` доказать настоящим CLI process test до baseline I/O.
  Проверка: `cargo test -p bsl-analyzer --test selective_diagnostics_baseline_cli check_config_cli`.

Ни один флажок не отмечается по design review: требуется фактическое
автоматизированное доказательство. Commit, push и archive выполняются только по
отдельному запросу пользователя.
