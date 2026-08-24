# Acceptance evidence template

Не записывать URL стенда, connection names, пользователей, пароли, токены и приватные пути.
Для live-проверок использовать только обезличенный идентификатор запуска.

## Offline artifacts

| Artifact | Version/source | SHA-256 |
|---|---|---|
| change binary | `0.2.70`, `3230efffe2248e0b751cc56b45842c197ff8544a` + uncommitted change diff | `ee9fbe0e69d93c700f77d078ae2692311b9d36d05b4e43bfee140c9e2b82746f` |
| frozen legacy binary | tag `v0.2.70`, commit `d7e50c494995ee8dee742a6236b16134d2a42e87` | `1295570d74fe74dd5b6d428b7c05d3df3ba23fd3ab8d4ad6c874db99666958ab` |
| exported extension tree | 8 relative files, sorted per-file SHA-256 manifest | `e4b093cecc94d237356cceb5336a675a9ab90e0108dd709ff9ab253441194487` |

Команды хеширования бинарников: `sha256sum <artifact>`. Хеш дерева расширения — SHA-256
от отсортированного по относительному пути списка строк `sha256sum` всех файлов с корнем
`extension/src`; он совпадает с деревом, которое release-команда `extension export` выгружает
без промежуточного каталога `src`.

## Offline compatibility

| Check | Evidence |
|---|---|
| new consumer x legacy/new live fixtures | `onec-client`: 6 passed |
| frozen legacy response compatibility and retained `type` | fixture matrix included above; release/source identity recorded |
| producer raw `typeVariants` contract | `extension_live_metadata_contract`: 3 passed |
| concurrent absent/stale reference cache publication | subprocess smoke: 1 passed; exactly one writer commits |

## Scenario-to-test traceability

Every scenario in both delta specs has executable automated evidence. Test names below are
fully qualified as `path::test_name`; repeated tests intentionally prove related scenarios in
one vertical contract.

### `contextual-applied-type-info` (36/36)

| Scenario | Automated evidence |
|---|---|
| Объектная грань справочника | `crates/ide/tests/symbol_info.rs::named_applied_facets_resolve_without_mixing_surfaces`; `crates/mcp-server/tests/symbol_info.rs::symbol_info_serves_semantic_cards_over_the_transport` |
| Ссылочная грань справочника | `crates/ide/tests/symbol_info.rs::reference_and_manager_facets_keep_their_own_surfaces` |
| Менеджерская грань справочника | `crates/ide/tests/symbol_info.rs::reference_and_manager_facets_keep_their_own_surfaces` |
| Объектная грань обработки | `crates/ide/tests/symbol_info.rs::named_applied_facets_resolve_without_mixing_surfaces`; `crates/mcp-server/tests/symbol_info.rs::symbol_info_serves_semantic_cards_over_the_transport` |
| Неизвестная грань | `crates/mcp-server/tests/symbol_info.rs::symbol_info_serves_semantic_cards_over_the_transport`; `crates/ide/tests/symbol_info.rs::qualified_symbol_shape_rejects_empty_or_non_identifier_segments` |
| Реквизит и экспортный метод объекта | `crates/ide/tests/symbol_info.rs::object_facets_merge_effective_metadata_module_exports_and_platform_surface` |
| Экспортная переменная объекта | `crates/ide-db/src/database_impl_tests.rs::ba010_effective_module_variables_resolve_for_object_and_manager_facets`; `crates/ide/tests/symbol_info.rs::object_facets_merge_effective_metadata_module_exports_and_platform_surface` |
| Экспортная переменная менеджера | `crates/ide-db/src/database_impl_tests.rs::ba010_effective_module_variables_resolve_for_object_and_manager_facets`; `crates/ide/tests/symbol_info.rs::reference_and_manager_facets_keep_their_own_surfaces` |
| Модуль отсутствует или не читается | `crates/ide/tests/symbol_info.rs::an_unread_object_module_does_not_hide_metadata_or_platform_members` |
| Одно имя в нескольких источниках | `crates/ide/src/symbol_info.rs::full_and_exact_collection_share_stable_source_preserving_order` |
| Составной статический тип | `crates/ide/src/symbol_info.rs::composite_static_type_keeps_each_machine_variant` |
| Одинаковые представления статических типов | `crates/mcp-server/src/tools/symbol_info.rs::member_contract_keeps_sources_and_type_signature_branches_separate` |
| Новый сервис возвращает составной тип | `crates/bsl-analyzer/tests/extension_live_metadata_contract.rs::metadata_structure_exposes_locale_independent_type_variants`; `crates/onec-client/src/lib.rs::metadata_structure_types_known_variants_and_tolerates_future_fields` |
| Одинаковые представления живых типов | `crates/bsl-analyzer/tests/extension_live_metadata_contract.rs::metadata_structure_exposes_locale_independent_type_variants`; `crates/onec-client/src/lib.rs::metadata_structure_types_known_variants_and_tolerates_future_fields` |
| Ответ старого сервиса | `crates/onec-client/src/lib.rs::legacy_metadata_type_stays_unresolved_across_infobases` |
| Неподдержанный машинный вариант | `crates/onec-client/src/lib.rs::metadata_structure_types_known_variants_and_tolerates_future_fields` |
| Другая информационная база | `crates/onec-client/src/lib.rs::legacy_metadata_type_stays_unresolved_across_infobases` |
| Старый бинарник с новым сервисом | `crates/onec-client/src/lib.rs::metadata_structure_compatibility_matrix_keeps_legacy_type` |
| Именованный тип в серверном контексте | `crates/ide/src/symbol_info.rs::availability_evaluates_client_server_without_filtering_members`; `crates/mcp-server/tests/symbol_info.rs::symbol_info_serves_semantic_cards_over_the_transport` |
| Именованный тип без позиции | `crates/ide/src/symbol_info.rs::availability_expands_generic_thick_and_reports_unknown_inputs`; `crates/mcp-server/tests/symbol_info.rs::symbol_info_serves_semantic_cards_over_the_transport` |
| Старый позиционный запрос | `crates/mcp-server/tests/symbol_info.rs::symbol_info_serves_semantic_cards_over_the_transport`; `crates/ide/tests/symbol_info.rs::positional_local_or_parameter_resolves` |
| Неполная позиция | `crates/mcp-server/src/lib.rs::symbol_info_input_schema_accepts_name_with_optional_complete_position`; `crates/mcp-server/tests/symbol_info.rs::symbol_info_serves_semantic_cards_over_the_transport` |
| Уникальный реквизит расширения | `crates/ide-db/src/database_impl_tests.rs::effective_metadata_members_keep_topological_winner_and_source` |
| Замена одноимённого члена | `crates/ide-db/src/database_impl_tests.rs::effective_metadata_members_keep_topological_winner_and_source` |
| Два расширения | `crates/ide-db/src/database_impl_tests.rs::effective_metadata_members_keep_topological_winner_and_source` |
| Effective module расширения | `crates/ide-db/src/database_impl_tests.rs::effective_module_exports_cover_composition_matrix_for_all_module_roles`; `crates/ide-db/src/database_impl_tests.rs::ba010_effective_module_variables_resolve_for_object_and_manager_facets` |
| Weaving и модуль формы | `crates/ide-db/src/database_impl_tests.rs::effective_module_exports_cover_composition_matrix_for_all_module_roles`; `crates/ide/tests/symbol_info.rs::managed_form_uses_effective_extension_exports_for_full_and_exact_cards` |
| Управляемая форма объекта | `crates/ide/tests/symbol_info.rs::symbol_info_whole_object_form_card_lists_attributes_items_and_handlers`; `crates/ide/tests/symbol_info.rs::managed_form_platform_extensions_follow_main_attribute_type` |
| Тип элемента через DataPath | `crates/hir-ty/src/form_items.rs::lower_form_element_resolves_object_attribute_data_path_type`; `crates/ide/tests/symbol_info.rs::symbol_info_form_item_card_shows_inferred_data_path_type` |
| Одноимённые члены формы | `crates/ide/tests/symbol_info.rs::managed_form_full_and_exact_queries_share_candidates_and_keep_private_fallback` |
| Обычная форма не меняется | `crates/ide/tests/symbol_info.rs::ordinary_form_keeps_legacy_full_card_and_exact_local_method` |
| Закрытый helper управляемой формы | `crates/ide/tests/symbol_info.rs::managed_form_full_and_exact_queries_share_candidates_and_keep_private_fallback` |
| Старый include-запрос | `crates/mcp-server/tests/symbol_info.rs::symbol_info_serves_semantic_cards_over_the_transport` |
| Фильтр точного члена | `crates/mcp-server/src/tools/symbol_info.rs::member_filters_are_exact_and_do_not_change_card_sections`; `crates/mcp-server/tests/symbol_info.rs::symbol_info_serves_semantic_cards_over_the_transport` |
| Большая карточка усечена | `crates/mcp-server/src/tools/symbol_info.rs::card_members_truncate_under_budget`; `crates/mcp-server/tests/symbol_info.rs::symbol_info_serves_semantic_cards_over_the_transport` |
| Полный wire-ответ проверен | `crates/mcp-server/src/tools/symbol_info.rs::card_members_truncate_under_budget`; `crates/mcp-server/src/lib.rs::symbol_info_output_schema_requires_discriminators_and_member_branches`; `crates/mcp-server/tests/symbol_info.rs::symbol_info_serves_semantic_cards_over_the_transport` |

### `workspace-platform-reference` (28/28)

| Scenario | Automated evidence |
|---|---|
| Один сеанс обслуживает исходники и справочник | `crates/mcp-server/src/lib.rs::profile_search_inputs_are_tagged_and_legacy_branches_stay_permissive`; `crates/mcp-server/tests/contract.rs::workspace_tools_need_nothing_the_contract_omits` |
| Старый поиск по исходникам не меняет смысл | `crates/mcp-server/src/lib.rs::profile_search_outputs_publish_every_discriminated_branch`; `crates/mcp-server/src/tools/search/render.rs::code_hit_structure_carries_every_field_the_listing_prints`; `crates/mcp-server/src/tools/search/hybrid.rs::the_whole_response_stays_within_the_budget_it_was_given` |
| Обязательный запрос отсутствует | `crates/mcp-server/tests/contract.rs::workspace_tools_enforce_their_declared_required_params`; `crates/mcp-server/tests/contract.rs::reference_tools_enforce_their_declared_required_params` |
| Перечень типов | `crates/mcp-server/src/tools/platform.rs::list_platform_filters_bilingual_names_and_preserves_sorted_dto`; `crates/mcp-server/src/tools/platform.rs::platform_catalog_contains_all_five_kinds_and_distinguishes_type_homonyms` |
| Отбор методов | `crates/mcp-server/src/tools/platform.rs::list_platform_filters_bilingual_names_and_preserves_sorted_dto` |
| Стабильный идентификатор | `crates/mcp-server/src/tools/platform.rs::reference_ids_are_encoded_identity_digests_and_stable_when_a_peer_is_added`; `crates/mcp-server/src/tools/platform.rs::every_catalog_kind_round_trips_by_exact_reference_id` |
| Омонимы платформенных типов | `crates/mcp-server/src/tools/platform.rs::homonymous_type_ids_open_distinct_exact_cards` |
| Добавление омонима не меняет старый ID | `crates/mcp-server/src/tools/platform.rs::reference_ids_are_encoded_identity_digests_and_stable_when_a_peer_is_added` |
| Лексический поиск известного свойства | `crates/mcp-server/src/tools/search/docs.rs::find_docs_returns_a_real_property_identity` |
| Семантический поиск конструктора | `crates/mcp-server/src/tools/search/docs.rs::search_docs_returns_a_real_constructor_identity_from_semantic_corpus` |
| Кэш прежнего состава | `crates/bsl-search/src/engine.rs::reference_collection_replace_is_atomic_stamped_and_idempotent` |
| Два процесса впервые открывают общий кэш | `crates/bsl-search/src/engine.rs::reference_collection_publish_is_process_safe_for_absent_and_stale_db` |
| Читатель не видит частичную перестройку | `crates/bsl-search/src/engine.rs::reference_collection_publish_is_process_safe_for_absent_and_stale_db`; `crates/bsl-search/src/engine.rs::concurrent_reference_writers_commit_one_generation` |
| Локальный справочный кэш | `crates/mcp-server/src/state/bootstrap.rs::reference_search_loading_is_single_flight_and_shutdown_joins_worker`; `crates/mcp-server/src/state/bootstrap.rs::clear_reference_docs_cache_removes_stale_local_and_external_docs` |
| Внешний справочный корпус | `crates/mcp-server/src/tools/search/docs.rs::search_docs_with_external_reference_baseline_uses_standard_semantic_validation` |
| Справочник не готов | `crates/mcp-server/src/tools/search/docs.rs::doc_search_not_ready_and_empty_answers_are_structured_too`; `crates/mcp-server/tests/contract.rs::shared_platform_listing_is_identical_in_workspace_and_reference_profiles` |
| Настроенный внешний корпус недоступен | `crates/mcp-server/src/state/bootstrap.rs::reference_search_does_not_fall_back_for_unavailable_postgres_intent`; `crates/mcp-server/src/tools/search/docs.rs::find_docs_rejects_local_fallback_when_reference_postgres_baseline_is_unavailable`; `crates/mcp-server/src/tools/search/docs.rs::search_docs_rejects_reference_postgres_baseline_unavailability_before_semantic_validation` |
| Два одновременных первых запроса | `crates/mcp-server/src/state/bootstrap.rs::reference_search_loading_is_single_flight_and_shutdown_joins_worker`; `crates/mcp-server/src/state/bootstrap.rs::reference_search_keeps_pending_external_baseline_loading_until_shutdown`; `crates/mcp-server/src/baseline.rs::deferred_slot_shutdown_wakes_a_pending_waiter` |
| Карточка типа | `crates/mcp-server/src/tools/platform.rs::test_syntax_help_type_lookup`; `crates/mcp-server/src/tools/platform.rs::test_syntax_help_constructor_only_type_renders_constructor` |
| Карточка метода по идентификатору поиска | `crates/mcp-server/src/tools/platform.rs::every_catalog_kind_round_trips_by_exact_reference_id` |
| Коллизия свойства и метода | `crates/mcp-server/src/tools/platform.rs::method_property_collisions_and_constructor_overloads_open_exact_cards` |
| Перегрузки конструктора | `crates/mcp-server/src/tools/platform.rs::method_property_collisions_and_constructor_overloads_open_exact_cards` |
| Legacy-вход карточки | `crates/mcp-server/src/tools/platform.rs::test_syntax_help_type_lookup`; `crates/mcp-server/src/tools/platform.rs::test_syntax_help_method_with_type` |
| Неизвестный тип | `crates/mcp-server/src/tools/platform.rs::test_syntax_help_not_found`; `crates/mcp-server/src/tools/platform.rs::test_syntax_help_method_not_found_on_type` |
| Совпадение профилей | `crates/mcp-server/tests/contract.rs::shared_platform_listing_is_identical_in_workspace_and_reference_profiles` |
| Проверка опубликованной схемы | `crates/mcp-server/src/lib.rs::profile_search_outputs_publish_every_discriminated_branch`; `crates/mcp-server/tests/contract.rs::contract_is_discoverable_and_readable_over_a_session`; `crates/mcp-server/src/tools/search/docs.rs::doc_search_not_ready_and_empty_answers_are_structured_too` |
| Большой перечень усечён | `crates/mcp-server/src/tools/platform.rs::list_platform_budget_keeps_a_stable_prefix_and_a_tiny_empty_envelope`; `crates/mcp-server/tests/contract.rs::shared_platform_listing_is_identical_in_workspace_and_reference_profiles` |
| Бюджет меньше обязательного конверта | `crates/mcp-server/src/tools/platform.rs::list_platform_budget_keeps_a_stable_prefix_and_a_tiny_empty_envelope`; `crates/mcp-server/src/tools/platform.rs::a_budget_below_the_cards_identity_overshoots_by_that_much_and_says_so`; `crates/mcp-server/src/tools/search/render.rs::one_oversized_hit_returns_the_empty_budget_envelope` |

## Sequential rollback smoke

The smoke uses an isolated cache and no live infobase.

| Stage | Fingerprint/count | Integrity |
|---|---|---|
| change binary published reference cache | reference fingerprint `05eb9455611f10634cc3b9367817202c5ef61a017887f73d7ba9d5eee90853d6`; 23,372 documents |
| synthetic new-only fixture | sentinel fingerprint `fp-new-only-smoke`; 23,373 documents; marker present in table and FTS |
| frozen `v0.2.70` published legacy engine | authoritative `files.hash=0.2.70`; 9,517 documents; marker absent from table and FTS |

Final checks: `PRAGMA integrity_check=ok`; `COUNT(chunks)=COUNT(chunks_fts)`; FTS5
`integrity-check` succeeds. The legacy binary ignores the new-only fingerprint meta key and
rebuilds from its authoritative `files.hash`; the key may remain inert and causes the next new
binary to revalidate/rebuild rather than trust stale content.

## Live deployment

- run id: `live-20260822-a1`
- change binary SHA-256: `ee9fbe0e69d93c700f77d078ae2692311b9d36d05b4e43bfee140c9e2b82746f`
- extension tree SHA-256: `e4b093cecc94d237356cceb5336a675a9ab90e0108dd709ff9ab253441194487`
- raw `typeVariants`: `pass`; 6 typed members retain `type` and expose 6 variants; a negative missing-`typeVariants` self-check is rejected
- normalized `metadata object`: `pass`; schema v1, source `infobase`, 6 typed members and 6 normalized variants
- BA-007: `pass`; all 6 live variants use producer machine identity rather than workspace presentation lookup; legacy cross-infobase behavior remains covered offline
- frozen legacy preserved `type`: `pass`; source tag `v0.2.70` / commit `d7e50c494995ee8dee742a6236b16134d2a42e87`, live rebuild SHA-256 `8f33620841bac45a88d99592d66fffe0e4863d453391e8a96618b8e9634f0f71`, 6/6 typed members retained
- BA-010 exported-variable access: `pass`; the agreed base was dumped read-only, a temporary manager export plus one qualified use were added only to that dump, baseline and probe runs both reported 47 pre-existing `UnresolvedField` diagnostics while the probe file reported 0
- concurrent mixed-version writers observed: `false`

## Rollback gate

- new writers drained: `<timestamp/run-id>`
- frozen legacy binary is sole writer: `<pass|fail>`
- legacy engine published only after rebuild: `<pass|fail>`
- legacy fingerprint/count: `<fingerprint>/<count>`
- new-only documents absent: `<pass|fail>`
- SQLite integrity: `<result>`
- FTS consistency: `<result>`
