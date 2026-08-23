# Verification evidence

## diagnostic-feedback-correctness

| Scenario | Direct test identifier |
|---|---|
| Опечатка в имени переменной-получателя | `ide_diagnostics::handlers::unresolved_method_call::tests::unregistered_module_emits_receiver_not_resolved` |
| Метод отсутствует у известного модуля | `ide_diagnostics::handlers::unresolved_method_call::tests::emits_when_method_not_found_on_existing_module` |
| Опечатка в КонецЕсли | `parser::recovery_boundaries::an_unknown_end_if_is_the_only_error_and_owns_its_token`; `ide_diagnostics::handlers::parse_error::tests::typoed_end_if_reports_only_its_own_range` |
| Восстановление после неизвестного оператора | `parser::recovery_boundaries::parsing_continues_after_an_unknown_statement` |
| Допустимый идентификатор не считается оператором с опечаткой | `parser::recovery_boundaries::valid_identifier_statements_do_not_enter_unknown_statement_recovery` |
| Отсутствующее поле прямого литерала | `ide_diagnostics::handlers::unresolved_field::tests::emits_only_for_missing_fields_of_nonempty_closed_structure` |
| Существующее поле прямого литерала | `ide_diagnostics::handlers::unresolved_field::tests::emits_only_for_missing_fields_of_nonempty_closed_structure` |
| Пустой литерал остаётся мягким | `ide_diagnostics::handlers::unresolved_field::tests::literal_insert_closes_keyed_structure_but_dynamic_shapes_stay_soft` |
| Динамически расширяемая структура | `ide_diagnostics::handlers::unresolved_field::tests::literal_insert_closes_keyed_structure_but_dynamic_shapes_stay_soft`; `ide_diagnostics::handlers::unresolved_field::tests::only_proven_by_value_call_preserves_closed_shape`; `ide_diagnostics::handlers::unresolved_field::tests::expression_context_escapes_keep_shape_soft`; `ide_diagnostics::handlers::unresolved_field::tests::nested_structure_completeness_is_independent` |
| Completion закрытой структуры | `completion_value_collections::completion_after_dot_on_same_body_structure_lists_constructor_and_insert_keys`; `query_projection_ide_surface::hover_on_literal_structure_lists_keys_as_typed_fields` |

## diagnostics-config-defaults

| Scenario | Direct test identifier |
|---|---|
| Analyze без файла конфигурации | `diagnostics_config_cli::analyze_distinguishes_absent_and_invalid_diagnostics_config` |
| Пустой объект диагностик | `ide_diagnostics::config::tests::from_project_json_accepts_empty_object_without_warning` |
| Строка вместо объекта | `ide_diagnostics::config::tests::from_project_json_falls_back_on_garbage` |
| Корректные параметры сохраняются | `ide_diagnostics::config::tests::from_project_json_parses_params_and_stamps_locale` |

## workspace-metadata-argument-contract

| Scenario | Direct test identifier |
|---|---|
| Подсказка для source mode | `metadata::tools_list_publishes_the_mode_dependent_object_type_contract` |
| Подсказка для infobase mode | `metadata::tools_list_publishes_the_mode_dependent_object_type_contract` |
| Неверная форма не угадывается | `metadata::infobase_and_auto_with_connection_pass_object_type_through_once` |
| Контрактный снимок metadata | `mcp_server::tool_descriptions::workspace_tools_contract`; `metadata::tools_list_publishes_the_mode_dependent_object_type_contract` |
