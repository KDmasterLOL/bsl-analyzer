//! Central registry for diagnostic metadata.
//!
//! Provides const metadata definitions for all diagnostics.
//! Progress: 150/150 diagnostics defined (100%)
//! - 11 DISABLED_BY_DEFAULT diagnostics
//! - 39 Tier 1 diagnostics (syntax-only)
//! - 56 Tier 2 diagnostics (semantic analysis)
//! - 44 Tier 3 + SDBL + Additional diagnostics (metadata-based + queries + special cases)

use crate::handlers;
use crate::metadata::*;
use crate::DiagnosticCode;

/// Get metadata for a diagnostic code.
///
/// Returns `None` if metadata is not yet defined for this diagnostic.
pub fn get_metadata(code: DiagnosticCode) -> Option<&'static DiagnosticMetadata> {
    match code {
        // DISABLED_BY_DEFAULT diagnostics (11 total)
        DiagnosticCode::BadWords => Some(&handlers::bad_words::METADATA),
        DiagnosticCode::CodeAfterAsyncCall => Some(&handlers::code_after_async_call::METADATA),
        DiagnosticCode::DenyIncompleteValues => Some(&handlers::deny_incomplete_values::METADATA),
        DiagnosticCode::ForbiddenMetadataName => Some(&handlers::forbidden_metadata_name::METADATA),
        DiagnosticCode::FieldsFromJoinsWithoutIsNull => Some(&handlers::fields_from_joins_without_is_null::METADATA),
        DiagnosticCode::FileSystemAccess => Some(&handlers::file_system_access::METADATA),
        DiagnosticCode::FunctionNameStartsWithGet => Some(&handlers::function_name_starts_with_get::METADATA),
        DiagnosticCode::FunctionOutParameter => Some(&handlers::function_out_parameter::METADATA),
        DiagnosticCode::InternetAccess => Some(&handlers::internet_access::METADATA),
        DiagnosticCode::MissingTempStorageDeletion => Some(&handlers::missing_temp_storage_deletion::METADATA),
        DiagnosticCode::TernaryOperatorUsage => Some(&handlers::ternary_operator_usage::METADATA),
        DiagnosticCode::TooManyReturns => Some(&handlers::too_many_returns::METADATA),

        // Tier 1 diagnostics (syntax-only) - 39 total
        DiagnosticCode::ParseError => Some(&handlers::parse_error::METADATA),
        DiagnosticCode::CanonicalSpellingKeywords => Some(&handlers::canonical_spelling_keywords::METADATA),
        DiagnosticCode::ConsecutiveEmptyLines => Some(&handlers::consecutive_empty_lines::METADATA),
        DiagnosticCode::LineLength => Some(&handlers::line_length::METADATA),
        DiagnosticCode::MissingSpace => Some(&handlers::missing_space::METADATA),
        DiagnosticCode::OneStatementPerLine => Some(&handlers::one_statement_per_line::METADATA),
        DiagnosticCode::SemicolonPresence => Some(&handlers::semicolon_presence::METADATA),
        DiagnosticCode::SpaceAtStartComment => Some(&handlers::space_at_start_comment::METADATA),
        DiagnosticCode::IncorrectLineBreak => Some(&handlers::incorrect_line_break::METADATA),
        DiagnosticCode::IncorrectUseOfStrTemplate => Some(&handlers::incorrect_use_of_str_template::METADATA),
        DiagnosticCode::ExtraCommas => Some(&handlers::extra_commas::METADATA),
        DiagnosticCode::CommentedCode => Some(&handlers::commented_code::METADATA),
        DiagnosticCode::EmptyCodeBlock => Some(&handlers::empty_code_block::METADATA),
        DiagnosticCode::EmptyRegion => Some(&handlers::empty_region::METADATA),
        DiagnosticCode::EmptyStatement => Some(&handlers::empty_statement::METADATA),
        DiagnosticCode::UnreachableCode => Some(&handlers::unreachable_code::METADATA),
        DiagnosticCode::CodeBlockBeforeSub => Some(&handlers::code_block_before_sub::METADATA),
        DiagnosticCode::CodeOutOfRegion => Some(&handlers::code_out_of_region::METADATA),
        DiagnosticCode::MagicNumber => Some(&handlers::magic_number::METADATA),
        DiagnosticCode::MagicDate => Some(&handlers::magic_date::METADATA),
        DiagnosticCode::YoLetterUsage => Some(&handlers::yo_letter_usage::METADATA),
        DiagnosticCode::LatinAndCyrillicSymbolInWord => Some(&handlers::latin_and_cyrillic_symbol_in_word::METADATA),
        DiagnosticCode::InvalidCharacterInFile => Some(&handlers::invalid_character_in_file::METADATA),
        DiagnosticCode::DoubleNegatives => Some(&handlers::double_negatives::METADATA),
        DiagnosticCode::NestedTernaryOperator => Some(&handlers::nested_ternary_operator::METADATA),
        DiagnosticCode::NonExportMethodsInApiRegion => Some(&handlers::non_export_methods_in_api_region::METADATA),
        DiagnosticCode::UnaryPlusInConcatenation => Some(&handlers::unary_plus_in_concatenation::METADATA),
        DiagnosticCode::UselessTernaryOperator => Some(&handlers::useless_ternary_operator::METADATA),
        DiagnosticCode::DuplicateStringLiteral => Some(&handlers::duplicate_string_literal::METADATA),
        DiagnosticCode::DuplicateRegion => Some(&handlers::duplicate_region::METADATA),
        DiagnosticCode::NonStandardRegion => Some(&handlers::non_standard_region::METADATA),
        DiagnosticCode::DuplicatedInsertionIntoCollection => {
            Some(&handlers::duplicated_insertion_into_collection::METADATA)
        }
        DiagnosticCode::ExcessiveAutoTestCheck => Some(&handlers::excessive_auto_test_check::METADATA),
        DiagnosticCode::IdenticalExpressions => Some(&handlers::identical_expressions::METADATA),
        DiagnosticCode::IfElseDuplicatedCodeBlock => Some(&handlers::if_else_duplicated_code_block::METADATA),
        DiagnosticCode::IfElseDuplicatedCondition => Some(&handlers::if_else_duplicated_condition::METADATA),
        DiagnosticCode::IfElseIfEndsWithElse => Some(&handlers::if_else_if_ends_with_else::METADATA),
        DiagnosticCode::MultilingualStringHasAllDeclaredLanguages => {
            Some(&handlers::multilingual_string_has_all_declared_languages::METADATA)
        }
        DiagnosticCode::MultilingualStringUsingWithTemplate => {
            Some(&handlers::multilingual_string_using_with_template::METADATA)
        }
        DiagnosticCode::NestedConstructorsInStructureDeclaration => {
            Some(&handlers::nested_constructors_in_structure_declaration::METADATA)
        }
        DiagnosticCode::NestedFunctionInParameters => Some(&handlers::nested_function_in_parameters::METADATA),

        // Tier 2 diagnostics (semantic analysis) - 52 total
        DiagnosticCode::AllFunctionPathMustHaveReturn => Some(&handlers::all_function_path_must_have_return::METADATA),
        DiagnosticCode::FunctionShouldHaveReturn => Some(&handlers::function_should_have_return::METADATA),
        DiagnosticCode::ProcedureReturnsValue => Some(&handlers::procedure_returns_value::METADATA),
        DiagnosticCode::FunctionReturnsSamePrimitive => Some(&handlers::function_returns_same_primitive::METADATA),
        DiagnosticCode::NumberOfParams => Some(&handlers::number_of_params::METADATA),
        DiagnosticCode::NumberOfOptionalParams => Some(&handlers::number_of_optional_params::METADATA),
        DiagnosticCode::NumberOfValuesInStructureConstructor => {
            Some(&handlers::number_of_values_in_structure_constructor::METADATA)
        }
        DiagnosticCode::OrderOfParams => Some(&handlers::order_of_params::METADATA),
        DiagnosticCode::MissedRequiredParameter => Some(&handlers::missed_required_parameter::METADATA),
        DiagnosticCode::UnusedParameters => Some(&handlers::unused_parameters::METADATA),
        DiagnosticCode::MissingParameterDescription => Some(&handlers::missing_parameter_description::METADATA),
        DiagnosticCode::MissingReturnedValueDescription => {
            Some(&handlers::missing_returned_value_description::METADATA)
        }
        DiagnosticCode::ReservedParameterNames => Some(&handlers::reserved_parameter_names::METADATA),
        DiagnosticCode::RewriteMethodParameter => Some(&handlers::rewrite_method_parameter::METADATA),
        DiagnosticCode::UnusedLocalMethod => Some(&handlers::unused_local_method::METADATA),
        DiagnosticCode::ExportVariables => Some(&handlers::export_variables::METADATA),
        DiagnosticCode::MissingVariablesDescription => Some(&handlers::missing_variables_description::METADATA),
        DiagnosticCode::SelfAssign => Some(&handlers::self_assign::METADATA),
        DiagnosticCode::ThisObjectAssign => Some(&handlers::this_object_assign::METADATA),
        DiagnosticCode::CyclomaticComplexity => Some(&handlers::cyclomatic_complexity::METADATA),
        DiagnosticCode::CognitiveComplexity => Some(&handlers::cognitive_complexity::METADATA),
        DiagnosticCode::NestedStatements => Some(&handlers::nested_statements::METADATA),
        DiagnosticCode::MethodSize => Some(&handlers::method_size::METADATA),
        DiagnosticCode::IfConditionComplexity => Some(&handlers::if_condition_complexity::METADATA),
        DiagnosticCode::MissingCodeTryCatchEx => Some(&handlers::missing_code_try_catch_ex::METADATA),
        DiagnosticCode::MissingTemporaryFileDeletion => Some(&handlers::missing_temporary_file_deletion::METADATA),
        DiagnosticCode::UseLessForEach => Some(&handlers::useless_for_each::METADATA),
        DiagnosticCode::UsingGoto => Some(&handlers::using_goto::METADATA),
        DiagnosticCode::BeginTransactionBeforeTryCatch => Some(&handlers::begin_transaction_before_try_catch::METADATA),
        DiagnosticCode::CommitTransactionOutsideTryCatch => {
            Some(&handlers::commit_transaction_outside_try_catch::METADATA)
        }
        DiagnosticCode::CompilationDirectiveLost => Some(&handlers::compilation_directive_lost::METADATA),
        DiagnosticCode::CompilationDirectiveNeedLess => Some(&handlers::compilation_directive_need_less::METADATA),
        DiagnosticCode::CreateQueryInCycle => Some(&handlers::create_query_in_cycle::METADATA),
        DiagnosticCode::DeletingCollectionItem => Some(&handlers::deleting_collection_item::METADATA),
        DiagnosticCode::SelfInsertion => Some(&handlers::self_insertion::METADATA),
        DiagnosticCode::SeveralCompilerDirectives => Some(&handlers::several_compiler_directives::METADATA),
        DiagnosticCode::StyleElementConstructors => Some(&handlers::style_element_constructors::METADATA),
        DiagnosticCode::DeprecatedCurrentDate => Some(&handlers::deprecated_current_date::METADATA),
        DiagnosticCode::DeprecatedFind => Some(&handlers::deprecated_find::METADATA),
        DiagnosticCode::DeprecatedMessage => Some(&handlers::deprecated_message::METADATA),
        DiagnosticCode::DeprecatedTypeManagedForm => Some(&handlers::deprecated_type_managed_form::METADATA),
        DiagnosticCode::DeprecatedMethods8310 => Some(&handlers::deprecated_method::DEPRECATED_METHODS_8310),
        DiagnosticCode::DeprecatedMethods8317 => Some(&handlers::deprecated_method::DEPRECATED_METHODS_8317),
        DiagnosticCode::DeprecatedAttributes8312 => Some(&handlers::deprecated_attributes_8312::METADATA),
        DiagnosticCode::DeprecatedMethodCall => Some(&handlers::deprecated_method_call::METADATA),
        DiagnosticCode::DisableSafeMode => Some(&handlers::disable_safe_mode::METADATA),
        DiagnosticCode::ExternalAppStarting => Some(&handlers::external_app_starting::METADATA),
        DiagnosticCode::OSUsersMethod => Some(&handlers::os_users_method::METADATA),
        DiagnosticCode::TempFilesDir => Some(&handlers::temp_files_dir::METADATA),
        DiagnosticCode::FormDataToValue => Some(&handlers::form_data_to_value::METADATA),
        DiagnosticCode::GetFormMethod => Some(&handlers::get_form_method::METADATA),
        DiagnosticCode::GlobalContextMethodCollision8312 => {
            Some(&handlers::global_context_method_collision8312::METADATA)
        }
        DiagnosticCode::IsInRoleMethod => Some(&handlers::is_in_role_method::METADATA),
        DiagnosticCode::PairingBrokenTransaction => Some(&handlers::pairing_broken_transaction::METADATA),
        DiagnosticCode::WrongUseOfRollbackTransactionMethod => {
            Some(&handlers::wrong_use_of_rollback_transaction_method::METADATA)
        }

        // Tier 3 + SDBL diagnostics (35 total)
        DiagnosticCode::AssignAliasFieldsInQuery => Some(&handlers::assign_alias_fields_in_query::METADATA),
        DiagnosticCode::CachedPublic => Some(&handlers::cached_public::METADATA),
        DiagnosticCode::CommandModuleExportMethods => Some(&handlers::command_module_export_methods::METADATA),
        DiagnosticCode::CommonModuleAssign => Some(&handlers::common_module_assign::METADATA),
        DiagnosticCode::CommonModuleInvalidType => Some(&handlers::common_module_invalid_type::METADATA),
        DiagnosticCode::CommonModuleMissingAPI => Some(&handlers::common_module_missing_api::METADATA),
        DiagnosticCode::CommonModuleNameCached => Some(&handlers::common_module_name_cached::METADATA),
        DiagnosticCode::CommonModuleNameClient => Some(&handlers::common_module_name_client::METADATA),
        DiagnosticCode::CommonModuleNameClientServer => Some(&handlers::common_module_name_client_server::METADATA),
        DiagnosticCode::CommonModuleNameFullAccess => Some(&handlers::common_module_name_full_access::METADATA),
        DiagnosticCode::CommonModuleNameGlobal => Some(&handlers::common_module_name_global::METADATA),
        DiagnosticCode::CommonModuleNameGlobalClient => Some(&handlers::common_module_name_global_client::METADATA),
        DiagnosticCode::CommonModuleNameServerCall => Some(&handlers::common_module_name_server_call::METADATA),
        DiagnosticCode::CommonModuleNameWords => Some(&handlers::common_module_name_words::METADATA),
        DiagnosticCode::FullOuterJoinQuery => Some(&handlers::full_outer_join_query::METADATA),
        DiagnosticCode::IncorrectUseLikeInQuery => Some(&handlers::incorrect_use_like_in_query::METADATA),
        DiagnosticCode::JoinWithSubQuery => Some(&handlers::join_with_sub_query::METADATA),
        DiagnosticCode::JoinWithVirtualTable => Some(&handlers::join_with_virtual_table::METADATA),
        DiagnosticCode::LogicalOrInJoinQuerySection => Some(&handlers::logical_or_in_join_query_section::METADATA),
        DiagnosticCode::LogicalOrInTheWhereSectionOfQuery => {
            Some(&handlers::logical_or_in_the_where_section_of_query::METADATA)
        }
        DiagnosticCode::MetadataObjectNameLength => Some(&handlers::metadata_object_name_length::METADATA),
        DiagnosticCode::MissingCommonModuleMethod => Some(&handlers::missing_common_module_method::METADATA),
        DiagnosticCode::MissingEventSubscriptionHandler => {
            Some(&handlers::missing_event_subscription_handler::METADATA)
        }
        DiagnosticCode::MultilineStringInQuery => Some(&handlers::multiline_string_in_query::METADATA),
        DiagnosticCode::OrdinaryAppSupport => Some(&handlers::ordinary_app_support::METADATA),
        DiagnosticCode::PrivilegedModuleMethodCall => Some(&handlers::privileged_module_method_call::METADATA),
        DiagnosticCode::ProtectedModule => Some(&handlers::protected_module::METADATA),
        DiagnosticCode::PublicMethodsDescription => Some(&handlers::public_methods_description::METADATA),
        DiagnosticCode::QueryNestedFieldsByDot => Some(&handlers::query_nested_fields_by_dot::METADATA),
        DiagnosticCode::QueryParseError => Some(&handlers::query_parse_error::METADATA),
        DiagnosticCode::QueryToMissingMetadata => Some(&handlers::query_to_missing_metadata::METADATA),
        DiagnosticCode::RefOveruse => Some(&handlers::ref_overuse::METADATA),
        DiagnosticCode::SelectTopWithoutOrderBy => Some(&handlers::select_top_without_order_by::METADATA),
        DiagnosticCode::UnionAll => Some(&handlers::union_all::METADATA),
        DiagnosticCode::UsingLikeInQuery => Some(&handlers::using_like_in_query::METADATA),
        DiagnosticCode::VirtualTableCallWithoutParameters => {
            Some(&handlers::virtual_table_call_without_parameters::METADATA)
        }
        DiagnosticCode::ScheduledJobHandler => Some(&handlers::scheduled_job_handler::METADATA),
        DiagnosticCode::ServerCallsInFormEvents => Some(&handlers::server_calls_in_form_events::METADATA),
        DiagnosticCode::ServerSideExportFormMethod => Some(&handlers::server_side_export_form_method::METADATA),
        DiagnosticCode::SetPermissionsForNewObjects => Some(&handlers::set_permissions_for_new_objects::METADATA),
        DiagnosticCode::SetPrivilegedMode => Some(&handlers::set_privileged_mode::METADATA),
        DiagnosticCode::TransferringParametersBetweenClientAndServer => {
            Some(&handlers::transferring_parameters_between_client_and_server::METADATA)
        }
        DiagnosticCode::UnsafeFindByCode => Some(&handlers::unsafe_find_by_code::METADATA),

        // Additional diagnostics
        DiagnosticCode::DataExchangeLoading => Some(&handlers::data_exchange_loading::METADATA),
        DiagnosticCode::ExecuteExternalCode => Some(&handlers::execute_external_code::METADATA),
        DiagnosticCode::ExecuteExternalCodeInCommonModule => {
            Some(&handlers::execute_external_code_in_common_module::METADATA)
        }
        DiagnosticCode::RedundantAccessToObject => Some(&handlers::redundant_access_to_object::METADATA),
        DiagnosticCode::SameMetadataObjectAndChildNames => {
            Some(&handlers::same_metadata_object_and_child_names::METADATA)
        }
        DiagnosticCode::UnusedLocalVariable => Some(&handlers::unused_local_variable::METADATA),
        DiagnosticCode::TimeoutsInExternalResources => Some(&handlers::timeouts_in_external_resources::METADATA),
        DiagnosticCode::TryNumber => Some(&handlers::try_number::METADATA),
        DiagnosticCode::Typo => Some(&handlers::typo::METADATA),
        DiagnosticCode::UnknownPreprocessorSymbol => Some(&handlers::unknown_preprocessor_symbol::METADATA),
        DiagnosticCode::UnsafeSafeModeMethodCall => Some(&handlers::unsafe_safe_mode_method_call::METADATA),
        DiagnosticCode::UsageWriteLogEvent => Some(&handlers::usage_write_log_event::METADATA),
        DiagnosticCode::UseSystemInformation => Some(&handlers::use_system_information::METADATA),
        DiagnosticCode::UsingCancelParameter => Some(&handlers::using_cancel_parameter::METADATA),
        DiagnosticCode::UsingExternalCodeTools => Some(&handlers::using_external_code_tools::METADATA),
        DiagnosticCode::UsingFindElementByString => Some(&handlers::using_find_element_by_string::METADATA),
        DiagnosticCode::UsingHardcodeNetworkAddress => Some(&handlers::using_hardcode_network_address::METADATA),
        DiagnosticCode::UsingHardcodePath => Some(&handlers::using_hardcode_path::METADATA),
        DiagnosticCode::UsingHardcodeSecretInformation => Some(&handlers::using_hardcode_secret_information::METADATA),
        DiagnosticCode::UsingModalWindows => Some(&handlers::using_modal_windows::METADATA),
        DiagnosticCode::UsingObjectNotAvailableUnix => Some(&handlers::using_object_not_available_unix::METADATA),
        DiagnosticCode::UsingSynchronousCalls => Some(&handlers::using_synchronous_calls::METADATA),
        DiagnosticCode::UsingServiceTag => Some(&handlers::using_service_tag::METADATA),
        DiagnosticCode::UsingThisForm => Some(&handlers::using_this_form::METADATA),
        DiagnosticCode::WrongDataPathForFormElements => Some(&handlers::wrong_data_path_for_form_elements::METADATA),
        DiagnosticCode::WrongHttpServiceHandler => Some(&handlers::wrong_http_service_handler::METADATA),
        DiagnosticCode::WrongWebServiceHandler => Some(&handlers::wrong_web_service_handler::METADATA),
        DiagnosticCode::WrongUseFunctionProceedWithCall => {
            Some(&handlers::wrong_use_function_proceed_with_call::METADATA)
        }
    }
}

// ============================================================================
// DISABLED_BY_DEFAULT diagnostics (11 total)
// ============================================================================

// ============================================================================
// Tier 1 diagnostics (syntax-only) - 39 total
// ============================================================================

// ============================================================================
// Tier 2 diagnostics (semantic analysis) - 52 total
// ============================================================================

// ============================================================================
// Tier 3 + SDBL diagnostics (36 total)
// ============================================================================

// ============================================================================
// Additional diagnostics (5 total)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ternary_operator_usage_metadata() {
        let meta = get_metadata(DiagnosticCode::TernaryOperatorUsage).unwrap();

        // Verify matches Java @DiagnosticMetadata
        assert_eq!(meta.diagnostic_type, DiagnosticType::CodeSmell);
        assert_eq!(meta.severity, DiagnosticSeverityLevel::Minor);
        assert_eq!(meta.minutes_to_fix, 3);
        assert!(!meta.activated_by_default); // Disabled by default
        assert_eq!(meta.tags, &[MetadataTag::Brainoverload]);

        // Verify severity mapping: CODE_SMELL + MINOR → Information
        assert_eq!(meta.calculate_severity(), crate::Severity::Information);
    }

    #[test]
    fn test_bad_words_metadata() {
        let meta = get_metadata(DiagnosticCode::BadWords).unwrap();

        assert_eq!(meta.diagnostic_type, DiagnosticType::CodeSmell);
        assert_eq!(meta.severity, DiagnosticSeverityLevel::Major);
        assert_eq!(meta.minutes_to_fix, 1);
        assert!(!meta.activated_by_default);
        assert_eq!(meta.tags, &[MetadataTag::Design]);

        // CODE_SMELL + MAJOR → Warning
        assert_eq!(meta.calculate_severity(), crate::Severity::Warning);
    }

    #[test]
    fn test_fields_from_joins_without_is_null_metadata() {
        let meta = get_metadata(DiagnosticCode::FieldsFromJoinsWithoutIsNull).unwrap();

        assert_eq!(meta.diagnostic_type, DiagnosticType::Error);
        assert_eq!(meta.severity, DiagnosticSeverityLevel::Critical);
        assert!(!meta.activated_by_default);
        assert_eq!(
            meta.tags,
            &[MetadataTag::Sql, MetadataTag::Suspicious, MetadataTag::Unpredictable]
        );

        // ERROR + CRITICAL → Critical
        assert_eq!(meta.calculate_severity(), crate::Severity::Critical);
    }

    #[test]
    fn test_file_system_access_metadata() {
        let meta = get_metadata(DiagnosticCode::FileSystemAccess).unwrap();

        assert_eq!(meta.diagnostic_type, DiagnosticType::Vulnerability);
        assert_eq!(meta.scope, DiagnosticScope::Bsl);
        assert!(!meta.activated_by_default);

        // VULNERABILITY + MAJOR → Major
        assert_eq!(meta.calculate_severity(), crate::Severity::Major);
    }

    #[test]
    fn test_function_name_starts_with_get_metadata() {
        let meta = get_metadata(DiagnosticCode::FunctionNameStartsWithGet).unwrap();

        assert_eq!(meta.diagnostic_type, DiagnosticType::CodeSmell);
        assert_eq!(meta.severity, DiagnosticSeverityLevel::Info);
        assert!(!meta.activated_by_default);

        // CODE_SMELL + INFO → Hint
        assert_eq!(meta.calculate_severity(), crate::Severity::Hint);
    }

    #[test]
    fn test_all_disabled_by_default_have_metadata() {
        let codes = [
            DiagnosticCode::BadWords,
            DiagnosticCode::CodeAfterAsyncCall,
            DiagnosticCode::DenyIncompleteValues,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            DiagnosticCode::FileSystemAccess,
            DiagnosticCode::FunctionNameStartsWithGet,
            DiagnosticCode::FunctionOutParameter,
            DiagnosticCode::InternetAccess,
            DiagnosticCode::MissingTempStorageDeletion,
            DiagnosticCode::TernaryOperatorUsage,
            DiagnosticCode::TooManyReturns,
        ];

        for code in codes {
            let meta = get_metadata(code).unwrap();
            assert!(!meta.activated_by_default, "{:?} should be disabled by default", code);
        }
    }

    #[test]
    fn test_deny_incomplete_values_metadata() {
        let meta = get_metadata(DiagnosticCode::DenyIncompleteValues).unwrap();

        assert_eq!(meta.scope, DiagnosticScope::Bsl);
        assert!(meta.can_locate_on_project);
        assert!(!meta.activated_by_default);
    }

    // ============================================================================
    // Comprehensive metadata test suite (Phase 5.3)
    // ============================================================================

    #[test]
    fn test_all_diagnostics_have_metadata() {
        use crate::DiagnosticCode;
        use strum::IntoEnumIterator;

        let mut missing = Vec::new();
        let mut count = 0;

        for code in DiagnosticCode::iter() {
            count += 1;
            if get_metadata(code).is_none() {
                missing.push(code);
            }
        }

        assert!(
            missing.is_empty(),
            "Found {} diagnostics without metadata (total {}): {:#?}",
            missing.len(),
            count,
            missing
        );
    }

    #[test]
    fn test_lsp_severity_mapping() {
        use crate::Severity;

        // Test ERROR + CRITICAL → Critical
        let data_exchange = get_metadata(DiagnosticCode::DataExchangeLoading).unwrap();
        assert_eq!(data_exchange.diagnostic_type, DiagnosticType::Error);
        assert_eq!(data_exchange.severity, DiagnosticSeverityLevel::Critical);
        assert_eq!(data_exchange.calculate_severity(), Severity::Critical);

        // Test VULNERABILITY + CRITICAL → Critical
        let execute_ext = get_metadata(DiagnosticCode::ExecuteExternalCode).unwrap();
        assert_eq!(execute_ext.diagnostic_type, DiagnosticType::Vulnerability);
        assert_eq!(execute_ext.severity, DiagnosticSeverityLevel::Critical);
        assert_eq!(execute_ext.calculate_severity(), Severity::Critical);

        // Test CODE_SMELL + INFO → Hint
        let redundant = get_metadata(DiagnosticCode::RedundantAccessToObject).unwrap();
        assert_eq!(redundant.diagnostic_type, DiagnosticType::CodeSmell);
        assert_eq!(redundant.severity, DiagnosticSeverityLevel::Info);
        assert_eq!(redundant.calculate_severity(), Severity::Hint);

        // Test CODE_SMELL + MINOR → Information
        let ternary = get_metadata(DiagnosticCode::TernaryOperatorUsage).unwrap();
        assert_eq!(ternary.diagnostic_type, DiagnosticType::CodeSmell);
        assert_eq!(ternary.severity, DiagnosticSeverityLevel::Minor);
        assert_eq!(ternary.calculate_severity(), Severity::Information);

        // Test CODE_SMELL + MAJOR → Warning
        let unused = get_metadata(DiagnosticCode::UnusedLocalVariable).unwrap();
        assert_eq!(unused.diagnostic_type, DiagnosticType::CodeSmell);
        assert_eq!(unused.severity, DiagnosticSeverityLevel::Major);
        assert_eq!(unused.calculate_severity(), Severity::Warning);

        // Test SECURITY_HOTSPOT → Warning
        let privileged = get_metadata(DiagnosticCode::SetPrivilegedMode).unwrap();
        assert_eq!(privileged.diagnostic_type, DiagnosticType::SecurityHotspot);
        assert_eq!(privileged.calculate_severity(), Severity::Warning);
    }

    #[test]
    fn test_tags_coverage() {
        // Verify key tags are used
        let bad_words = get_metadata(DiagnosticCode::BadWords).unwrap();
        assert!(bad_words.tags.contains(&MetadataTag::Design));

        let same_meta = get_metadata(DiagnosticCode::SameMetadataObjectAndChildNames).unwrap();
        assert!(same_meta.tags.contains(&MetadataTag::Standard));
        assert!(same_meta.tags.contains(&MetadataTag::Sql));
        assert!(same_meta.tags.contains(&MetadataTag::Design));

        let unused = get_metadata(DiagnosticCode::UnusedLocalVariable).unwrap();
        assert!(unused.tags.contains(&MetadataTag::Unused));
        assert!(unused.tags.contains(&MetadataTag::Brainoverload));
        assert!(unused.tags.contains(&MetadataTag::Badpractice));
    }

    #[test]
    fn test_activated_by_default_consistency() {
        // All DISABLED_BY_DEFAULT diagnostics should have activated_by_default = false
        let disabled_codes = [
            DiagnosticCode::BadWords,
            DiagnosticCode::CodeAfterAsyncCall,
            DiagnosticCode::DenyIncompleteValues,
            DiagnosticCode::FieldsFromJoinsWithoutIsNull,
            DiagnosticCode::FileSystemAccess,
            DiagnosticCode::FunctionNameStartsWithGet,
            DiagnosticCode::FunctionOutParameter,
            DiagnosticCode::InternetAccess,
            DiagnosticCode::MissingTempStorageDeletion,
            DiagnosticCode::TernaryOperatorUsage,
            DiagnosticCode::TooManyReturns,
        ];

        for code in disabled_codes {
            let meta = get_metadata(code).unwrap();
            assert!(!meta.activated_by_default, "{:?} should be disabled by default", code);
        }

        // Recently added diagnostics should be enabled by default
        let enabled_codes = [
            DiagnosticCode::DataExchangeLoading,
            DiagnosticCode::ExecuteExternalCode,
            DiagnosticCode::RedundantAccessToObject,
            DiagnosticCode::SameMetadataObjectAndChildNames,
            DiagnosticCode::UnusedLocalVariable,
        ];

        for code in enabled_codes {
            let meta = get_metadata(code).unwrap();
            assert!(meta.activated_by_default, "{:?} should be enabled by default", code);
        }
    }

    #[test]
    fn test_scope_consistency() {
        // Test BSL scope
        let data_exchange = get_metadata(DiagnosticCode::DataExchangeLoading).unwrap();
        assert_eq!(data_exchange.scope, DiagnosticScope::Bsl);

        // Test All scope (BSL + OneScript)
        let unused = get_metadata(DiagnosticCode::UnusedLocalVariable).unwrap();
        assert_eq!(unused.scope, DiagnosticScope::All);
    }

    #[test]
    fn test_can_locate_on_project() {
        // SameMetadataObjectAndChildNames should support project-level location
        let same_meta = get_metadata(DiagnosticCode::SameMetadataObjectAndChildNames).unwrap();
        assert!(same_meta.can_locate_on_project);

        // Most diagnostics don't support project-level location
        let unused = get_metadata(DiagnosticCode::UnusedLocalVariable).unwrap();
        assert!(!unused.can_locate_on_project);
    }

    #[test]
    fn test_minutes_to_fix_reasonable() {
        use strum::IntoEnumIterator;

        for code in DiagnosticCode::iter() {
            if let Some(meta) = get_metadata(code) {
                // minutes_to_fix should be reasonable (1-60 minutes)
                assert!(
                    meta.minutes_to_fix >= 1 && meta.minutes_to_fix <= 60,
                    "{:?} has unreasonable minutes_to_fix: {}",
                    code,
                    meta.minutes_to_fix
                );
            }
        }
    }

    #[test]
    fn test_new_diagnostics_metadata() {
        // Test DataExchangeLoading
        let data_exchange = get_metadata(DiagnosticCode::DataExchangeLoading).unwrap();
        assert_eq!(data_exchange.diagnostic_type, DiagnosticType::Error);
        assert_eq!(data_exchange.severity, DiagnosticSeverityLevel::Critical);
        assert_eq!(data_exchange.minutes_to_fix, 5);
        assert!(data_exchange.tags.contains(&MetadataTag::Standard));

        // Test ExecuteExternalCode
        let execute_ext = get_metadata(DiagnosticCode::ExecuteExternalCode).unwrap();
        assert_eq!(execute_ext.diagnostic_type, DiagnosticType::Vulnerability);
        assert_eq!(execute_ext.severity, DiagnosticSeverityLevel::Critical);
        assert!(execute_ext.tags.contains(&MetadataTag::Error));

        // Test RedundantAccessToObject
        let redundant = get_metadata(DiagnosticCode::RedundantAccessToObject).unwrap();
        assert_eq!(redundant.diagnostic_type, DiagnosticType::CodeSmell);
        assert_eq!(redundant.severity, DiagnosticSeverityLevel::Info);
        assert!(redundant.tags.contains(&MetadataTag::Clumsy));

        // Test SameMetadataObjectAndChildNames
        let same_meta = get_metadata(DiagnosticCode::SameMetadataObjectAndChildNames).unwrap();
        assert_eq!(same_meta.diagnostic_type, DiagnosticType::Error);
        assert_eq!(same_meta.minutes_to_fix, 30);
        assert!(same_meta.can_locate_on_project);

        // Test UnusedLocalVariable
        let unused = get_metadata(DiagnosticCode::UnusedLocalVariable).unwrap();
        assert_eq!(unused.diagnostic_type, DiagnosticType::CodeSmell);
        assert_eq!(unused.severity, DiagnosticSeverityLevel::Major);
        assert!(unused.tags.contains(&MetadataTag::Unused));
    }
}
