# Покрытие сценариев

Каждый из 38 сценариев `specs/diagnostics-baseline/spec.md` закреплён автоматической проверкой.

| Сценарий | Автоматическая проверка |
|---|---|
| Повторное формирование без изменений | `diagnostics_baseline_io_is_byte_deterministic` |
| Неподдерживаемая версия схемы | `diagnostics_baseline_io_rejects_bad_schema_and_json`, `check_config_baseline_reports_unsupported_schema` |
| Несовпадающая область проекта | `diagnostics_baseline_io_rejects_incompatible_scope_and_paths`, `diagnostics_baseline_scope_preserves_topology_and_normalized_paths` |
| Корень вне проекта | `diagnostics_baseline_scope_rejects_external_roots_and_target` |
| Сдвиг строк без изменения причины | `diagnostics_baseline_classify_survives_line_shift` |
| Изменение проблемного выражения | `diagnostics_baseline_classify_changed_expression_is_new_and_resolved` |
| Одинаковые срабатывания в одном файле | `diagnostics_baseline_classify_numbers_identical_lines` |
| Единый отпечаток отчёта Code Quality | `fingerprint_is_stable_across_line_shifts`, `reads_source_when_snippets_absent_matching_supplied_snippet` |
| Исходный фрагмент недоступен | `diagnostics_fingerprint_requires_snippet`, `diagnostics_baseline_create_rejects_missing_snippet_before_write` |
| Только известные диагностики | `diagnostics_baseline_check_is_read_only_for_every_outcome` |
| Появилась новая диагностика | `analyze_diagnostics_baseline_filters_known_after_existing_suppressions`, `diagnostics_baseline_cli` |
| Известная диагностика исчезла | `diagnostics_baseline_check_is_read_only_for_every_outcome`, `diagnostics_baseline_cli` |
| Полный снимок не доказан | `diagnostics_baseline_full_gate_rejects_every_incomplete_proof`, `analyze_baseline_partial_scope_resolves_only_completed_files` |
| Диагностика подавлена исходным кодом | `analyze_diagnostics_baseline_filters_known_after_existing_suppressions`, `lsp_cached_diagnostics_honour_suppression` |
| Ошибка директивы подавления | `diagnostics_baseline_protected_diagnostics_remain_active`, `lsp_diagnostics_baseline_publish_keeps_new_and_protected_only` |
| Базовая линия не настроена | `analyze_diagnostics_baseline_filters_known_after_existing_suppressions`, `diagnostics_baseline_response_covers_schema_errors_and_minimum_budget` |
| Обычный анализ проекта | `diagnostics_baseline_cli`, `diagnostics_baseline_check_is_read_only_for_every_outcome` |
| Настроенный файл отсутствует | `check_config_baseline_reports_missing_file`, `diagnostics_baseline_cli` |
| Путь выходит из проекта | `diagnostics_baseline_scope_rejects_external_roots_and_target`, `diagnostics_baseline_scope_rejects_symlink_target` |
| Повреждённый файл | `diagnostics_baseline_io_rejects_bad_schema_and_json`, `diagnostics_baseline_io_rejects_duplicates`, `diagnostics_baseline_check_is_read_only_for_every_outcome` |
| Первичное создание | `diagnostics_baseline_create_reports_text_and_machine_result`, `diagnostics_baseline_cli` |
| Создание без настроенного пути | `diagnostics_baseline_create_requires_configuration`, `create_without_configuration_fails_before_analysis` |
| Явное обновление | `diagnostics_baseline_update_refreshes_fields_and_reports_counts`, `diagnostics_baseline_cli` |
| Сбой записи | `diagnostics_baseline_update_preserves_old_bytes_when_replace_fails`, `diagnostics_baseline_create_cleans_temp_after_unsupported_link` |
| Попытка неявного роста | `diagnostics_baseline_check_is_read_only_for_every_outcome`, `diagnostics_baseline_cli` |
| Обновление при включённом diff_base | `diagnostics_baseline_full_gate_rejects_every_incomplete_proof`, `analyze_baseline_partial_scope_marks_every_cli_filter` |
| Чтение при частичном анализе | `analyze_baseline_partial_scope_resolves_only_completed_files`, `partial_document` |
| Ограниченный или отменённый запрос MCP | `baseline_parity_partial_stale_error_recovery_and_cancellation`, `pre_cancelled_sweep_is_partial_and_leaves_the_resident_usable` |
| Устаревший снимок MCP | `baseline_parity_partial_stale_error_recovery_and_cancellation` |
| Одинаковый проект в трёх поверхностях | `parity`, `baseline_parity_partial_stale_error_recovery_and_cancellation`, общий `diagnostic_fingerprint` проверяется `diagnostics_fingerprint_normalizes_and_preserves_recipe` |
| Ответ MCP | `diagnostics_baseline_snapshot_filters_file_and_workspace_without_rebuilding_salsa`, `diagnostics_baseline_response_covers_schema_errors_and_minimum_budget` |
| Ошибка базовой линии в MCP | `diagnostics_baseline_response_covers_schema_errors_and_minimum_budget`, `baseline_parity_partial_stale_error_recovery_and_cancellation` |
| Публикация LSP | `lsp_diagnostics_baseline_publish_keeps_new_and_protected_only`, `parity` |
| Ошибка базовой линии в LSP | `lsp_diagnostics_baseline_error_notifies_once_per_fingerprint_and_recovers` |
| Изменение файла в резидентной поверхности | `diagnostics_baseline_reload_observes_write_replace_and_delete_without_rebuilding_salsa`, `lsp_diagnostics_baseline_reload_handles_write_replace_and_delete_without_replacing_salsa` |
| Совместимость отчёта Code Quality | `emits_codeclimate_entries_with_expected_shape`, `fingerprint_is_stable_across_line_shifts` |
| Одновременно настроен search.baseline | `check_config_baseline_reports_corrupt_file_alongside_search_baseline`, `project_config_deserializes_search_baseline_settings` |
| Правило отключено конфигурацией | `disabled_rule_is_not_written` |
