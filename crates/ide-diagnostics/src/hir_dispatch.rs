//! HIR-based diagnostics dispatch.
//!
//! This module collects diagnostics from HIR lowering and dispatches them
//! to the appropriate handler's `from_hir()` function.

use crate::{handlers, Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::BodyDiagnostic;

/// Diagnostics collected during HIR lowering (BodyDiagnostic dispatch)
pub(crate) const HIR_DIAGNOSTICS: &[DiagnosticCode] = &[
    DiagnosticCode::AllFunctionPathMustHaveReturn,
    DiagnosticCode::BeginTransactionBeforeTryCatch,
    DiagnosticCode::CodeAfterAsyncCall,
    DiagnosticCode::CognitiveComplexity,
    DiagnosticCode::CommitTransactionOutsideTryCatch,
    DiagnosticCode::CommonModuleAssign,
    DiagnosticCode::CreateQueryInCycle,
    DiagnosticCode::CyclomaticComplexity,
    DiagnosticCode::DeletingCollectionItem,
    DiagnosticCode::DeprecatedAttributes8312,
    DiagnosticCode::DeprecatedCurrentDate,
    DiagnosticCode::DeprecatedFind,
    DiagnosticCode::DeprecatedMessage,
    DiagnosticCode::DeprecatedMethods8310,
    DiagnosticCode::DeprecatedMethods8317,
    DiagnosticCode::DeprecatedMethodCall,
    DiagnosticCode::DeprecatedTypeManagedForm,
    DiagnosticCode::DisableSafeMode,
    DiagnosticCode::EmptyCodeBlock,
    DiagnosticCode::EmptyRegion,
    DiagnosticCode::EmptyStatement,
    DiagnosticCode::ExecuteExternalCode,
    DiagnosticCode::ExternalAppStarting,
    DiagnosticCode::ExtraCommas,
    DiagnosticCode::FileSystemAccess,
    DiagnosticCode::FormDataToValue,
    DiagnosticCode::FunctionNameStartsWithGet,
    DiagnosticCode::FunctionOutParameter,
    DiagnosticCode::FunctionReturnsSamePrimitive,
    DiagnosticCode::FunctionShouldHaveReturn,
    DiagnosticCode::GetFormMethod,
    DiagnosticCode::GlobalContextMethodCollision8312,
    DiagnosticCode::IfConditionComplexity,
    DiagnosticCode::IfElseDuplicatedCodeBlock,
    DiagnosticCode::IfElseDuplicatedCondition,
    DiagnosticCode::IfElseIfEndsWithElse,
    DiagnosticCode::IncorrectUseOfStrTemplate,
    DiagnosticCode::MagicNumber,
    DiagnosticCode::MethodSize,
    DiagnosticCode::MissedRequiredParameter,
    DiagnosticCode::MissingCommonModuleMethod,
    DiagnosticCode::NestedStatements,
    DiagnosticCode::NumberOfOptionalParams,
    DiagnosticCode::NumberOfParams,
    DiagnosticCode::OneStatementPerLine,
    DiagnosticCode::OSUsersMethod,
    DiagnosticCode::ProcedureReturnsValue,
    DiagnosticCode::RedundantAccessToObject,
    DiagnosticCode::RewriteMethodParameter,
    DiagnosticCode::SelfAssign,
    DiagnosticCode::SelfInsertion,
    DiagnosticCode::SemicolonPresence,
    DiagnosticCode::ServerCallsInFormEvents,
    DiagnosticCode::SetPrivilegedMode,
    DiagnosticCode::StyleElementConstructors,
    DiagnosticCode::TempFilesDir,
    DiagnosticCode::TernaryOperatorUsage,
    DiagnosticCode::ThisObjectAssign,
    DiagnosticCode::TooManyReturns,
    DiagnosticCode::TryNumber,
    DiagnosticCode::UnaryPlusInConcatenation,
    DiagnosticCode::UnsafeFindByCode,
    DiagnosticCode::UnsafeSafeModeMethodCall,
    DiagnosticCode::UsageWriteLogEvent,
    DiagnosticCode::UseLessForEach,
    DiagnosticCode::UseSystemInformation,
    DiagnosticCode::UsingCancelParameter,
    DiagnosticCode::UsingExternalCodeTools,
    DiagnosticCode::UsingFindElementByString,
    DiagnosticCode::UsingGoto,
    DiagnosticCode::UsingModalWindows,
    DiagnosticCode::UsingObjectNotAvailableUnix,
    DiagnosticCode::UsingSynchronousCalls,
    DiagnosticCode::UsingThisForm,
    DiagnosticCode::WrongUseFunctionProceedWithCall,
    DiagnosticCode::WrongUseOfRollbackTransactionMethod,
];

/// Collect HIR-based diagnostics from module_bodies().
///
/// This function retrieves diagnostics collected during HIR lowering
/// and dispatches them to the appropriate handler's `from_hir()` function.
///
pub fn collect_hir_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    // Early exit: skip expensive module_bodies() call if no HIR diagnostics are enabled
    if !ctx.config.any_enabled(HIR_DIAGNOSTICS) {
        return Vec::new();
    }

    let module_bodies = ctx.module_bodies();

    let mut diagnostics = Vec::new();

    for (method_id, body_diag) in module_bodies.all_diagnostics() {
        if let Some(diag) = dispatch_hir_diagnostic(body_diag, method_id, ctx) {
            diagnostics.push(diag);
        }
    }

    diagnostics
}

/// Dispatch BodyDiagnostic to appropriate handler's from_hir() function.
///
/// This is the single source of truth for HIR diagnostic dispatch.
/// Used by both production code and tests.
///
/// The exhaustive match (no wildcard `_` arm) is **by design**: each
/// `BodyDiagnostic` variant carries unique fields, so the compiler
/// enforces that every new variant gets a handler. Adding a wildcard
/// would silently swallow newly-added diagnostics.
pub fn dispatch_hir_diagnostic(
    body_diag: &BodyDiagnostic,
    method_id: &hir::MethodId,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    match body_diag {
        BodyDiagnostic::FunctionShouldHaveReturn { range } => {
            handlers::function_should_have_return::from_hir(*range, ctx)
        }
        BodyDiagnostic::EmptyCodeBlock { range } => {
            handlers::empty_code_block::from_hir(*range, ctx)
        }
        BodyDiagnostic::MagicNumber { value, range, context } => {
            handlers::magic_number::from_hir(value, *range, context, ctx)
        }
        BodyDiagnostic::SelfAssign { range } => handlers::self_assign::from_hir(*range, ctx),
        BodyDiagnostic::UnusedVariable { name: _, range: _ } => {
            // Skip HIR-based detection - using dataflow-based detection in unused_local_variable::check()
            None
        }
        BodyDiagnostic::MissingReturn { range } => {
            handlers::all_function_path_must_have_return::from_hir(*range, method_id, ctx)
        }
        BodyDiagnostic::DeprecatedMethod { name, range } => {
            handlers::deprecated_method::from_hir(name, *range, ctx)
        }
        BodyDiagnostic::DeprecatedCurrentDate { name, range } => {
            handlers::deprecated_current_date::from_hir(name, *range, ctx)
        }
        BodyDiagnostic::DeprecatedFind { name, range } => {
            handlers::deprecated_find::from_hir(name, *range, ctx)
        }
        BodyDiagnostic::DeprecatedMessage { name, range } => {
            handlers::deprecated_message::from_hir(name, *range, ctx)
        }
        BodyDiagnostic::DeprecatedTypeManagedForm { type_name, range } => {
            handlers::deprecated_type_managed_form::from_hir(type_name, *range, ctx)
        }
        BodyDiagnostic::DisableSafeMode { method_name, range } => {
            handlers::disable_safe_mode::from_hir(method_name, *range, ctx)
        }
        BodyDiagnostic::BeginTransactionBeforeTryCatch { range } => {
            handlers::begin_transaction_before_try_catch::from_hir(*range, ctx)
        }
        BodyDiagnostic::MissedRequiredParameter {
            callee,
            module,
            mdo_type,
            mdo_name,
            args,
            range,
        } => handlers::missed_required_parameter::from_hir(
            callee,
            module.as_deref(),
            mdo_type.as_deref(),
            mdo_name.as_deref(),
            args,
            *range,
            ctx,
        ),
        BodyDiagnostic::IfElseDuplicatedCodeBlock { range } => {
            handlers::if_else_duplicated_code_block::from_hir(*range, ctx)
        }
        BodyDiagnostic::CodeAfterAsyncCall { method_name, range } => {
            handlers::code_after_async_call::from_hir(method_name, *range, ctx)
        }
        BodyDiagnostic::CommitTransactionOutsideTryCatch { range } => {
            handlers::commit_transaction_outside_try_catch::from_hir(*range, ctx)
        }
        BodyDiagnostic::CommonModuleAssign { variable_name, range } => {
            handlers::common_module_assign::from_hir(variable_name, *range, ctx)
        }
        BodyDiagnostic::RewriteMethodParameter { param_id, stmt_id, stmt_range, ident_range } => {
            handlers::rewrite_method_parameter::from_hir(
                *param_id,
                *stmt_id,
                *stmt_range,
                *ident_range,
                ctx,
            )
        }
        BodyDiagnostic::CreateQueryInCycle { range } => {
            handlers::create_query_in_cycle::from_hir(*range, ctx)
        }
        BodyDiagnostic::DeletingCollectionItem { collection_text, range } => {
            handlers::deleting_collection_item::from_hir(collection_text, *range, ctx)
        }
        BodyDiagnostic::SelfInsertion { range } => handlers::self_insertion::from_hir(*range, ctx),
        BodyDiagnostic::DeprecatedAttribute8312 { name, kind, range } => {
            handlers::deprecated_attributes_8312::from_hir(name, *kind, *range, ctx)
        }
        BodyDiagnostic::ExecuteExternalCode { range } => {
            handlers::execute_external_code::from_hir(*range, ctx)
        }
        BodyDiagnostic::ExternalAppStarting { range } => {
            handlers::external_app_starting::from_hir(*range, ctx)
        }
        BodyDiagnostic::ExtraCommas { range } => handlers::extra_commas::from_hir(*range, ctx),
        BodyDiagnostic::FileSystemAccess { range } => {
            handlers::file_system_access::from_hir(*range, ctx)
        }
        BodyDiagnostic::FormDataToValue { range } => {
            handlers::form_data_to_value::from_hir(*range, ctx)
        }
        BodyDiagnostic::GetFormMethod { method_name, range } => {
            handlers::get_form_method::from_hir(method_name, *range, ctx)
        }
        BodyDiagnostic::GlobalContextMethodCollision8312 { method_name, range } => {
            handlers::global_context_method_collision8312::from_hir(method_name, *range, ctx)
        }
        BodyDiagnostic::FunctionNameStartsWithGet { name, range } => {
            handlers::function_name_starts_with_get::from_hir(name, *range, ctx)
        }
        BodyDiagnostic::FunctionOutParameter { name, range } => {
            handlers::function_out_parameter::from_hir(name, *range, ctx)
        }
        BodyDiagnostic::FunctionReturnsSamePrimitive { range } => {
            handlers::function_returns_same_primitive::from_hir(*range, ctx)
        }
        BodyDiagnostic::EmptyRegion { name, range } => {
            handlers::empty_region::from_hir(name, *range, ctx)
        }
        BodyDiagnostic::EmptyStatement { range } => {
            handlers::empty_statement::from_hir(*range, ctx)
        }
        BodyDiagnostic::MissingSemicolon { range } => {
            handlers::semicolon_presence::from_hir(*range, ctx)
        }
        BodyDiagnostic::IfConditionComplexity { complexity, max_complexity, range } => {
            handlers::if_condition_complexity::from_hir(*complexity, *max_complexity, *range, ctx)
        }
        BodyDiagnostic::IfElseDuplicatedCondition { first_occurrence_index, range } => {
            handlers::if_else_duplicated_condition::from_hir(*first_occurrence_index, *range, ctx)
        }
        BodyDiagnostic::IfElseIfEndsWithElse { range } => {
            handlers::if_else_if_ends_with_else::from_hir(*range, ctx)
        }
        BodyDiagnostic::IncorrectUseOfStrTemplate { range } => {
            handlers::incorrect_use_of_str_template::from_hir(*range, ctx)
        }
        BodyDiagnostic::MissingCommonModuleMethod { module, method, range } => {
            handlers::missing_common_module_method::from_hir(module, method, *range, ctx)
        }
        BodyDiagnostic::OneStatementPerLine { range } => {
            handlers::one_statement_per_line::from_hir(*range, ctx)
        }
        BodyDiagnostic::OSUsersMethod { range } => handlers::os_users_method::from_hir(*range, ctx),
        BodyDiagnostic::ProcedureReturnsValue { range } => {
            handlers::procedure_returns_value::from_hir(*range, ctx)
        }
        BodyDiagnostic::RedundantAccessToObject { kind, range } => {
            handlers::redundant_access_to_object::from_hir(kind, *range, ctx)
        }
        BodyDiagnostic::ServerCallsInFormEvents { callee, range } => {
            handlers::server_calls_in_form_events::from_hir(callee, *range, ctx)
        }
        BodyDiagnostic::SetPrivilegedModeCall { range } => {
            handlers::set_privileged_mode::from_hir(*range, ctx)
        }
        BodyDiagnostic::StyleElementConstructors { type_name, range } => {
            handlers::style_element_constructors::from_hir(type_name, *range, ctx)
        }
        BodyDiagnostic::TempFilesDir { name, range } => {
            handlers::temp_files_dir::from_hir(name, *range, ctx)
        }
        BodyDiagnostic::TernaryOperatorUsage { range } => {
            handlers::ternary_operator_usage::from_hir(*range, ctx)
        }
        BodyDiagnostic::TooManyReturns { method_name, method_name_range, returns } => {
            handlers::too_many_returns::from_hir(method_name, *method_name_range, returns, ctx)
        }
        BodyDiagnostic::UnaryPlusInConcatenation { range } => {
            handlers::unary_plus_in_concatenation::from_hir(*range, ctx)
        }
        BodyDiagnostic::UseSystemInformation { range } => {
            handlers::use_system_information::from_hir(*range, ctx)
        }
        BodyDiagnostic::UsingCancelParameter { range } => {
            handlers::using_cancel_parameter::from_hir(*range, ctx)
        }
        BodyDiagnostic::UsingExternalCodeTools { range } => {
            handlers::using_external_code_tools::from_hir(*range, ctx)
        }
        BodyDiagnostic::UsingFindElementByString { range } => {
            handlers::using_find_element_by_string::from_hir(*range, ctx)
        }
        BodyDiagnostic::UsingGoto { range } => handlers::using_goto::from_hir(*range, ctx),
        BodyDiagnostic::UsingModalWindows { method_name, replacement, range } => {
            handlers::using_modal_windows::from_hir(method_name, replacement, *range, ctx)
        }
        BodyDiagnostic::UsingSynchronousCalls { method_name, replacement, range } => {
            handlers::using_synchronous_calls::from_hir(method_name, replacement, *range, ctx)
        }
        BodyDiagnostic::UsingThisForm { range } => handlers::using_this_form::from_hir(*range, ctx),
        BodyDiagnostic::WrongUseFunctionProceedWithCall { range } => {
            handlers::wrong_use_function_proceed_with_call::from_hir(*range, ctx)
        }
        BodyDiagnostic::WrongUseOfRollbackTransactionMethod { range } => {
            handlers::wrong_use_of_rollback_transaction_method::from_hir(*range, ctx)
        }
        BodyDiagnostic::DeprecatedMethodCall { callee, module, range } => {
            handlers::deprecated_method_call::from_hir(
                callee,
                module.as_deref(),
                *range,
                method_id,
                ctx,
            )
        }
        BodyDiagnostic::ThisObjectAssign { range } => {
            handlers::this_object_assign::from_hir(*range, ctx)
        }
        // Phase 4: Method-scoped diagnostics
        BodyDiagnostic::CognitiveComplexity { method_name, complexity, is_function, range } => {
            handlers::cognitive_complexity::from_hir(
                method_name,
                *complexity,
                *is_function,
                *range,
                ctx,
            )
        }
        BodyDiagnostic::CyclomaticComplexity { method_name, complexity, is_function, range } => {
            handlers::cyclomatic_complexity::from_hir(
                method_name,
                *complexity,
                *is_function,
                *range,
                ctx,
            )
        }
        BodyDiagnostic::MethodSize { method_name, size, is_function, range } => {
            handlers::method_size::from_hir(method_name, *size, *is_function, *range, ctx)
        }
        BodyDiagnostic::NestedStatements { method_name, depth, is_function, range } => {
            handlers::nested_statements::from_hir(method_name, *depth, *is_function, *range, ctx)
        }
        BodyDiagnostic::NumberOfParams { method_name, count, is_function, range } => {
            handlers::number_of_params::from_hir(method_name, *count, *is_function, *range, ctx)
        }
        BodyDiagnostic::NumberOfOptionalParams { method_name, count, is_function, range } => {
            handlers::number_of_optional_params::from_hir(
                method_name,
                *count,
                *is_function,
                *range,
                ctx,
            )
        }
        BodyDiagnostic::TryNumber { range } => handlers::try_number::from_hir(*range, ctx),
        BodyDiagnostic::UsingObjectNotAvailableUnix { type_name, range } => {
            handlers::using_object_not_available_unix::from_hir(type_name, *range, ctx)
        }
        BodyDiagnostic::UnsafeSafeModeMethodCall { range } => {
            handlers::unsafe_safe_mode_method_call::from_hir(*range, ctx)
        }
        BodyDiagnostic::UselessForEach { iterator_name, range } => {
            handlers::useless_for_each::from_hir(iterator_name, *range, ctx)
        }
        BodyDiagnostic::UnsafeFindByCode { manager_name, object_name, range } => {
            handlers::unsafe_find_by_code::from_hir(manager_name, object_name, *range, ctx)
        }
        BodyDiagnostic::UsageWriteLogEvent {
            in_except_block,
            arg_count,
            log_level_empty,
            comment_empty,
            has_error_log_level,
            has_detail_error_description,
            except_has_raise,
            range,
        } => handlers::usage_write_log_event::from_hir(
            *in_except_block,
            *arg_count,
            *log_level_empty,
            *comment_empty,
            *has_error_log_level,
            *has_detail_error_description,
            *except_has_raise,
            *range,
            ctx,
        ),
    }
}
