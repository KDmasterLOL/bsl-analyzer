# Verification traceability

Прямые test identifiers для каждого scenario delta spec:

| Scenario | Test identifier |
|---|---|
| Операция завершилась ошибкой под действующей арендой | `workspace_lease::tests::checkpoint_refreshes_heartbeat_and_preserves_callback_errors` |
| Lease lock временно недоступен | `workspace_lease::tests::typed_fence_distinguishes_transient_and_terminal_refusals` |
| Замечен живой чужой token | `workspace_lease::tests::publish_fence_latches_supersession` |
| Release остановил ограждённую операцию | `workspace_lease::tests::release_waits_for_only_the_admitted_batch_and_refuses_the_next` |
| Release пришёл после допуска атомарной операции | `workspace_lease::tests::release_during_checkpointed_callback_rolls_back_as_terminal` |
| Полный topology context refresh превышает stale interval | `workspace_lease::tests::checkpoint_refreshes_heartbeat_and_preserves_callback_errors`, `engine::tests::fenced_context_refresh_preserves_completed_batch_and_clears_mark_only_after_retry` |
| Вытеснение замечено между batch | `workspace_lease::tests::takeover_between_batches_preserves_the_first_and_terminates_the_next` |
| Shutdown пришёл во время короткой публикации | `workspace_lease::tests::release_waits_for_only_the_admitted_batch_and_refuses_the_next` |
| Shutdown пришёл во время атомарной SQLite transaction | `workspace_lease::tests::release_during_checkpointed_callback_rolls_back_as_terminal`, `engine::tests::checkpointed_atomic_transaction_rolls_back_at_batch_boundary` |
| Атомарная публикация потеряла аренду перед commit | `workspace_lease::tests::release_during_checkpointed_callback_rolls_back_as_terminal`, `engine::tests::cancelled_root_transition_publishes_nothing_and_retries_the_same_staging` |
| Full rescan превышает одну порцию | `state::sync::tests::drift_apply_keeps_its_cursor_on_refusal_and_advances_in_bounded_slices` |
| Fused ingest одного большого файла остаётся атомарным | `engine::tests::fused_file_rolls_back_hash_and_all_chunks_at_the_64_chunk_checkpoint` |
| Первый batch получил устойчивую Store error | `state::sync::tests::durable_drift_errors_advance_changes_and_coalesce_one_rescan_debt` |
| Удалённый файл появился снова до retry | `state::sync::tests::a_rescan_batch_removes_the_deletions_it_delivered_even_when_the_walk_is_incomplete` |
| Search marking упал для topology drift | `state::sync::tests::durable_drift_errors_advance_changes_and_coalesce_one_rescan_debt`, `state::sync::tests::continuous_events_do_not_reset_rescan_debt_backoff` |
| Аренда временно отказала до apply | `state::sync::tests::drift_apply_keeps_its_cursor_on_refusal_and_advances_in_bounded_slices` |
| Full rescan apply завершился собственной ошибкой | `state::sync::tests::durable_drift_errors_advance_changes_and_coalesce_one_rescan_debt` |
| Владение отсутствует до сетевого batch | `engine::tests::embedding_publish_fence` |
| Lock занят после получения vectors | `engine::tests::embedding_publish_fence` |
| Retry budget исчерпан | `state::overlay_retry::tests::transient_budget_fails_once_ignores_active_kicks_and_rearms_on_new_drift` |
| Retry budget настроен допустимым значением | `state::bootstrap::tests::embedding_publish_retry_budget_requires_positive_representable_seconds`, `state::overlay_retry::tests::transient_budget_fails_once_ignores_active_kicks_and_rearms_on_new_drift` |
| Retry budget настроен недопустимым значением | `state::bootstrap::tests::embedding_publish_retry_budget_requires_positive_representable_seconds` |
| Свежие сигналы приходят во время активного budget | `state::overlay_retry::tests::transient_budget_fails_once_ignores_active_kicks_and_rearms_on_new_drift` |
| Новый drift пришёл после исчерпания budget | `state::overlay_retry::tests::transient_budget_fails_once_ignores_active_kicks_and_rearms_on_new_drift` |
| Operation error завершил embedding obligation | `state::overlay_retry::tests::store_and_network_errors_wait_for_a_fresh_signal` |
| Fenced embedding сохраняет один prepared batch | `engine::tests::embedding_publish_fence` |
| Snapshot descriptor доступен | `graph::snapshot::tests::blocking_snapshot_classifies_preflight_identity_and_open_failures` |
| Publication не подготовила полный descriptor pool | `graph::snapshot::tests::publication_without_a_complete_descriptor_pool_cannot_be_ready`, `graph::build::tests::full_reload_install_refusal_keeps_the_old_snapshot_and_rearms` |
| Все descriptors заняты | `graph_supersession_contract::graph_handler_misses_immediately_when_preopened_handles_are_busy`, `graph_supersession_contract::resolve_names_misses_immediately_when_preopened_handles_are_busy`, `graph_supersession_contract::symbol_info_misses_immediately_when_preopened_handles_are_busy` |
| Вытесненный процесс видит заменённый общий файл | `graph::snapshot::tests::a_replaced_graph_file_is_not_opened_on_request_miss` |
| Background XML drift пришёл при занятом pool | `state::sync::tests::background_snapshot_failures_become_rescan_debt_instead_of_empty_xml_success` |
| Владение вернулось после временного отказа | `graph::build::tests::transient_publish_refusal_rearms_after_ownership_returns` |
| Реальная ошибка сборки при временно отрицательном probe | `graph::build::tests::rename_error_stays_operational_when_a_later_probe_would_refuse` |

## Issue #71 invariants

| Invariant | Direct test identifiers |
|---|---|
| HIGH-1: долгие shared mutations bounded либо checkpointed | `engine::tests::workspace_apply_n_plus_one_rows_use_two_fenced_transactions`, `store::tests::checkpointed_root_migration_rolls_back_at_64_rows`, `store::tests::checkpointed_fts_rebuild_rolls_back_and_retries`, `engine::tests::checkpointed_atomic_transaction_rolls_back_at_batch_boundary` |
| HIGH-2: operation error не маскируется lease retry и cursor продолжает движение | `workspace_lease::tests::checkpoint_refreshes_heartbeat_and_preserves_callback_errors`, `state::sync::tests::durable_drift_errors_advance_changes_and_coalesce_one_rescan_debt` |
| HIGH-3: network embedding не повторяется при transient publish refusal, retry bounded/configurable | `engine::tests::embedding_publish_fence`, `state::embed::tests::embedding_fence_distinguishes_retry_from_supersession`, `state::overlay_retry::tests::transient_budget_fails_once_ignores_active_kicks_and_rearms_on_new_drift`, `state::bootstrap::tests::embedding_publish_retry_budget_requires_positive_representable_seconds` |
| HIGH-4: async request path не ждёт flock | три `graph_supersession_contract::*_misses_immediately_when_preopened_handles_are_busy` tests выше |
| HIGH-5: request miss не открывает shared graph path | `graph::snapshot::tests::a_replaced_graph_file_is_not_opened_on_request_miss`, `graph_supersession_contract::graph_actions_fail_when_superseded_snapshot_is_gone` |

## Compatibility boundaries

- Runtime dependencies: `Cargo.toml` и `Cargo.lock` не изменены.
- Thread/scheduler: новый production worker/scheduler не добавлен; добавленные `thread::spawn` используются только test harnesses.
- SQLite/cache format: schema/version и persisted cache format не изменены; добавленные `CREATE TABLE applied` создают только test-local temporary databases.
- MCP wire: tool names, input/output schemas и response shape не изменены.
