//! Diagnostic helpers for lowering.
//!
//! This module contains helper functions for various diagnostics collected during lowering:
//! - Async call detection (CodeAfterAsyncCall)
//! - Transaction checking (BeginTransactionBeforeTryCatch)
//! - Deprecated method detection
//! - Duplicated code block detection

use std::collections::HashSet;

use syntax::{NodeOrToken, SyntaxKind, SyntaxNode};
use text_size::TextRange;

use crate::body::BodyDiagnostic;

use super::platform_helpers::{is_any_global_function, is_global_function};
use super::LoweringCtx;

// =============================================================================
// Duplicated code block detection
// =============================================================================

/// Check for duplicated code blocks in if/elsif/else branches.
///
/// Compares all pairs of branches and emits diagnostics for identical blocks.
pub(crate) fn check_duplicated_code_blocks(ctx: &mut LoweringCtx, branch_nodes: &[SyntaxNode]) {
    // Early exit: need at least 2 branches to potentially have duplicates
    if branch_nodes.len() < 2 {
        return;
    }

    // Track which blocks we've already reported as duplicates
    let mut reported: HashSet<usize> = HashSet::new();

    // Compare all pairs of code blocks
    for i in 0..branch_nodes.len() - 1 {
        if reported.contains(&i) {
            continue;
        }

        let current_block = &branch_nodes[i];
        let current_text = normalize_code_block(current_block);
        let current_count = count_statements(current_block);

        // Skip empty blocks
        if current_count == 0 {
            continue;
        }

        // Find all identical blocks after current one
        let mut has_duplicate = false;
        #[allow(clippy::needless_range_loop)] // Need index j for reported.insert(j)
        for j in (i + 1)..branch_nodes.len() {
            let other_block = &branch_nodes[j];
            let other_count = count_statements(other_block);

            // Skip empty blocks
            if other_count == 0 {
                continue;
            }

            // Quick check: same statement count
            if current_count != other_count {
                continue;
            }

            // Full check: compare normalized text
            let other_text = normalize_code_block(other_block);
            if current_text == other_text {
                has_duplicate = true;
                reported.insert(j);
            }
        }

        if has_duplicate {
            // Report diagnostic on the first block with duplicates
            ctx.emit(BodyDiagnostic::IfElseDuplicatedCodeBlock {
                range: current_block.text_range(),
            });
        }
    }
}

/// Normalize code block for comparison.
///
/// Removes whitespace and converts to lowercase (bilingual support).
fn normalize_code_block(block: &SyntaxNode) -> String {
    block
        .text()
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

/// Count the number of statement nodes in a code block.
fn count_statements(block: &SyntaxNode) -> usize {
    block
        .descendants()
        .filter(|node| {
            matches!(
                node.kind(),
                SyntaxKind::CALL_STMT
                    | SyntaxKind::ASSIGN_STMT
                    | SyntaxKind::RETURN_STMT
                    | SyntaxKind::IF_STMT
                    | SyntaxKind::WHILE_STMT
                    | SyntaxKind::FOR_STMT
                    | SyntaxKind::BREAK_STMT
                    | SyntaxKind::CONTINUE_STMT
                    | SyntaxKind::RAISE_STMT
                    | SyntaxKind::TRY_STMT
            )
        })
        .count()
}

// =============================================================================
// Deprecated method detection
// =============================================================================

/// Check if a method name is deprecated (8.3.10 or 8.3.17).
/// Returns true if the method is deprecated.
pub(crate) fn is_deprecated_method(name: &str) -> bool {
    let lower = name.to_lowercase();

    // Deprecated methods from 8.3.10 and 8.3.17
    matches!(
        lower.as_str(),
        // 8.3.10 - Client application methods
        "установитькраткийзаголовокприложения"
            | "получитькраткийзаголовокприложения"
            | "установитьзаголовокклиентскогоприложения"
            | "получитьзаголовокклиентскогоприложения"
            | "текущийвариантосновногошрифтаклиентскогоприложения"
            | "текущийвариантинтерфейсаклиентскогоприложения"
            | "setshortapplicationcaption"
            | "getshortapplicationcaption"
            | "setclientapplicationcaption"
            | "getclientapplicationcaption"
            | "clientapplicationbasefontcurrentvariant"
            | "clientapplicationinterfacecurrentvariant"
            // 8.3.17 - Error handling methods
            | "краткоепредставлениеошибки"
            | "подробноепредставлениеошибки"
            | "показатьинформациюобошибке"
            | "brieferrorrepresentation"
            | "detailederrorrepresentation"
            | "showerrorinformation"
            // Common
            | "получитьформу"
            | "getform"
    )
}

/// Check if a method name is deprecated ТекущаяДата() / CurrentDate().
/// Returns true if the method is the deprecated current date function.
pub(crate) fn is_deprecated_current_date(name: &str) -> bool {
    is_global_function(name, "CurrentDate")
}

/// Check if a method name is deprecated Найти() / Find().
/// Returns true if the method is the deprecated global find function.
pub(crate) fn is_deprecated_find(name: &str) -> bool {
    is_global_function(name, "Find")
}

/// Check if a method name is deprecated Сообщить() / Message().
/// Returns true if the method is the deprecated global message function.
pub(crate) fn is_deprecated_message(name: &str) -> bool {
    is_global_function(name, "Message")
}

/// Check if a method name is Тип() / Type().
/// Returns true if the method is the type construction function.
pub(crate) fn is_type_method(name: &str) -> bool {
    is_global_function(name, "Type")
}

/// Check if a type name is deprecated УправляемаяФорма / ManagedForm.
/// Returns true if the type name is the deprecated managed form type.
pub(crate) fn is_deprecated_managed_form(type_name: &str) -> bool {
    let lower = type_name.to_lowercase();
    matches!(lower.as_str(), "управляемаяформа" | "managedform")
}

/// Check if a method name is УстановитьБезопасныйРежим / SetSafeMode or
/// УстановитьОтключениеБезопасногоРежима / SetSafeModeDisabled.
/// Returns true if the method controls safe mode.
pub(crate) fn is_safe_mode_method(name: &str) -> bool {
    is_any_global_function(name, &["SetSafeMode", "SetSafeModeDisabled"])
}

/// Check if a method name is БезопасныйРежим / SafeMode (the getter, not setter).
/// Returns true if the method queries safe mode state.
pub(crate) fn is_safe_mode_query(name: &str) -> bool {
    is_global_function(name, "SafeMode")
}

/// Check if a method name is УстановитьПривилегированныйРежим / SetPrivilegedMode.
/// Returns true if the method sets privileged mode.
pub(crate) fn is_set_privileged_mode(name: &str) -> bool {
    is_global_function(name, "SetPrivilegedMode")
}

/// Check if a method name is КаталогВременныхФайлов / TempFilesDir.
/// Returns true if the method is the temp files directory function.
pub(crate) fn is_temp_files_dir(name: &str) -> bool {
    is_global_function(name, "TempFilesDir")
}

// =============================================================================
// Deprecated attributes 8.3.12 detection
// =============================================================================

use crate::body::DeprecatedKind8312;

/// Check if object.member is a deprecated attribute/method (8.3.12).
///
/// Returns Some(kind) if deprecated, None otherwise.
///
/// # Arguments
/// - `object`: Object/receiver name (e.g., "Диаграмма", "ChartPlotArea")
/// - `member`: Member name (property, method, or enum value)
/// - `is_call`: True if this is a method call, false for field access
pub(crate) fn is_deprecated_attribute_8312(
    object: &str,
    member: &str,
    is_call: bool,
) -> Option<DeprecatedKind8312> {
    let obj_lower = object.to_lowercase();
    let member_lower = member.to_lowercase();

    // Check ChartPlotArea attributes
    if is_chart_plot_area(&obj_lower)
        && !is_call
        && is_chart_plot_area_deprecated_attr(&member_lower)
    {
        return Some(DeprecatedKind8312::Attribute);
    }

    // Check Chart/GanttChart/PivotChart attributes and methods
    if is_chart(&obj_lower) {
        if is_call {
            if is_chart_deprecated_method(&member_lower) {
                return Some(DeprecatedKind8312::Method);
            }
        } else if is_chart_deprecated_attr(&member_lower) {
            return Some(DeprecatedKind8312::Attribute);
        }
    }

    // Check ChildFormItemsGroup enum values
    if is_child_form_items_group(&obj_lower)
        && !is_call
        && is_child_form_items_group_deprecated_attr(&member_lower)
    {
        return Some(DeprecatedKind8312::EnumValue);
    }

    // Check deprecated enum type names
    if is_chart_labels_orientation(&obj_lower) {
        return Some(DeprecatedKind8312::EnumName);
    }

    None
}

/// Check if a global method name is deprecated (8.3.12).
pub(crate) fn is_deprecated_global_method_8312(name: &str) -> bool {
    is_global_function(name, "ClearEventLog")
}

// Helper functions (private)

fn is_chart_plot_area(name: &str) -> bool {
    name == "областьпостроениядиаграммы" || name == "chartplotarea"
}

fn is_chart(name: &str) -> bool {
    matches!(
        name,
        "диаграмма" | "chart" | "диаграммаганта" | "ganttchart" | "своднаядиаграмма" | "pivotchart"
    )
}

fn is_child_form_items_group(name: &str) -> bool {
    name == "группировкаподчиненныхэлементовформы" || name == "childformitemsgroup"
}

fn is_chart_labels_orientation(name: &str) -> bool {
    name == "ориентацияметокдиаграммы"
}

fn is_chart_plot_area_deprecated_attr(name: &str) -> bool {
    matches!(
        name,
        "отображатьшкалу"
            | "showscale"
            | "линиишкалы"
            | "цветшкалы"
            | "отображатьподписишкалысерий"
            | "showseriesscalelabels"
            | "отображатьподписишкалыточек"
            | "showpointsscalelabels"
            | "отображатьподписишкалызначений"
            | "showvaluesscalelabels"
            | "отображатьлиниизначенийшкалы"
            | "showscalevaluelines"
            | "форматшкалызначений"
            | "valuescaleformat"
            | "ориентацияметок"
            | "labelsorientation"
    )
}

fn is_chart_deprecated_attr(name: &str) -> bool {
    matches!(
        name,
        "отображатьлегенду"
            | "showlegend"
            | "отображатьзаголовок"
            | "showtitle"
            | "палитрацветов"
            | "colorpalette"
            | "цветначалаградиентнойпалитры"
            | "gradientpalettestartcolor"
            | "цветконцаградиентнойпалитры"
            | "gradientpaletteendcolor"
            | "максимальноеколичествоцветовградиентнойпалитры"
            | "gradientpalettemaxcolors"
    )
}

fn is_child_form_items_group_deprecated_attr(name: &str) -> bool {
    name == "горизонтальная" || name == "horizontal"
}

fn is_chart_deprecated_method(name: &str) -> bool {
    matches!(name, "получитьпалитру" | "getpalette" | "установитьпалитру" | "setpalette")
}

// =============================================================================
// Transaction checking
// =============================================================================

/// Check if a statement is a global BeginTransaction/НачатьТранзакцию call.
///
/// Returns true if the statement is a non-qualified call to BeginTransaction/НачатьТранзакцию.
/// Filters out:
/// - Non-CALL_STMT nodes
/// - Qualified calls like `Connector.BeginTransaction()`
pub(crate) fn is_global_begin_transaction_call(node: &SyntaxNode) -> bool {
    // Must be CALL_STMT
    if node.kind() != SyntaxKind::CALL_STMT {
        return false;
    }

    // Skip if contains FIELD_EXPR (qualified call like Object.Method())
    if node.descendants().any(|n| n.kind() == SyntaxKind::FIELD_EXPR) {
        return false;
    }

    // Get first identifier token (method name)
    let ident = node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT);

    let Some(ident) = ident else {
        return false;
    };

    is_global_function(ident.text(), "BeginTransaction")
}

/// Check if a node is inside a Try-Catch block body.
///
/// Walks up the AST tree looking for TRY_STMT ancestors.
pub(crate) fn is_inside_try_body(node: &SyntaxNode) -> bool {
    let mut current = node.clone();
    while let Some(parent) = current.parent() {
        if parent.kind() == SyntaxKind::TRY_STMT {
            return true;
        }
        current = parent;
    }
    false
}

/// Check if a node is inside try body (not except clause).
///
/// Returns true if the node is inside a TRY_STMT but NOT inside EXCEPT_CLAUSE.
pub(crate) fn is_inside_try_body_not_except(node: &SyntaxNode) -> bool {
    let mut current = node.clone();
    while let Some(parent) = current.parent() {
        match parent.kind() {
            SyntaxKind::EXCEPT_CLAUSE => return false, // Inside except - not valid
            SyntaxKind::TRY_STMT => return true,       // Inside try body
            _ => {}
        }
        current = parent;
    }
    false
}

/// Check if a CALL_EXPR is a Число()/Number() call inside try body (TryNumber diagnostic).
///
/// Returns Some(range) if this is a Number call inside try block (not except).
pub(crate) fn check_try_number_call(node: &SyntaxNode) -> Option<TextRange> {
    // Must be CALL_EXPR
    if node.kind() != SyntaxKind::CALL_EXPR {
        return None;
    }

    // Must be inside try body (not except)
    if !is_inside_try_body_not_except(node) {
        return None;
    }

    // Get first child - should be IDENT for global call
    let first_child = node.children().next()?;
    if first_child.kind() != SyntaxKind::IDENT {
        return None;
    }

    // Check if it's Number/Число
    let name = first_child.text().to_string().to_lowercase();
    if name == "число" || name == "number" {
        return Some(node.text_range());
    }

    None
}

/// Check if a statement is a global CommitTransaction/ЗафиксироватьТранзакцию call.
///
/// Returns true if the statement is a non-qualified call to CommitTransaction/ЗафиксироватьТранзакцию.
/// Filters out:
/// - Non-CALL_STMT nodes
/// - Qualified calls like `Connector.CommitTransaction()`
pub(crate) fn is_global_commit_transaction_call(node: &SyntaxNode) -> bool {
    // Must be CALL_STMT
    if node.kind() != SyntaxKind::CALL_STMT {
        return false;
    }

    // Skip if contains FIELD_EXPR (qualified call like Object.Method())
    if node.descendants().any(|n| n.kind() == SyntaxKind::FIELD_EXPR) {
        return false;
    }

    // Get first identifier token (method name)
    let ident = node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT);

    let Some(ident) = ident else {
        return false;
    };

    is_global_function(ident.text(), "CommitTransaction")
}

/// Check CommitTransaction calls within a TRY_STMT body for proper placement.
///
/// Returns a list of CommitTransaction nodes that are NOT properly protected:
/// 1. Inside exception handler (should be in try body)
/// 2. Not the last statement in try body (code after commit)
/// 3. Try without except clause
///
/// Note: CommitTransaction calls OUTSIDE try-catch are detected in lower_stmt_list
/// similar to BeginTransactionBeforeTryCatch.
pub(crate) fn check_commit_transaction_in_try(
    try_stmt: &SyntaxNode,
) -> Vec<(SyntaxNode, CommitViolation)> {
    let mut violations = Vec::new();

    // Check if try has except clause
    let has_except = try_stmt.children().any(|n| n.kind() == SyntaxKind::EXCEPT_CLAUSE);

    // Find try body (first STMT_LIST)
    let try_body = try_stmt.children().find(|n| n.kind() == SyntaxKind::STMT_LIST);

    // Find except clause body
    let except_clause = try_stmt.children().find(|n| n.kind() == SyntaxKind::EXCEPT_CLAUSE);

    // Check commits in try body
    if let Some(body) = &try_body {
        let stmts: Vec<_> = body.children().filter(is_executable_stmt).collect();

        for (i, stmt) in stmts.iter().enumerate() {
            if is_global_commit_transaction_call(stmt) {
                if !has_except {
                    violations.push((stmt.clone(), CommitViolation::TryWithoutExcept));
                } else if i < stmts.len() - 1 {
                    // Not last statement - check if there's code after
                    violations.push((stmt.clone(), CommitViolation::CodeAfterCommit));
                }
                // Otherwise: properly protected (last in try, has except)
            }
        }
    }

    // Check commits in except clause (always error)
    if let Some(except) = &except_clause {
        for node in except.descendants() {
            if is_global_commit_transaction_call(&node) {
                violations.push((node, CommitViolation::InsideExceptHandler));
            }
        }
    }

    violations
}

/// Reason for CommitTransaction violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitViolation {
    /// CommitTransaction is inside exception handler (should be in try body)
    InsideExceptHandler,
    /// Try block has no except clause
    TryWithoutExcept,
    /// Code exists after CommitTransaction in try body
    CodeAfterCommit,
    /// CommitTransaction is outside try-catch entirely (detected separately in lower_stmt_list)
    #[allow(dead_code)]
    OutsideTryCatch,
}

/// Check if a node is an executable statement (for counting purposes).
fn is_executable_stmt(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::CALL_STMT
            | SyntaxKind::ASSIGN_STMT
            | SyntaxKind::RETURN_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::BREAK_STMT
            | SyntaxKind::CONTINUE_STMT
            | SyntaxKind::RAISE_STMT
            | SyntaxKind::GOTO_STMT
            | SyntaxKind::EXECUTE_STMT
    )
}

/// Check if a statement is a global RollbackTransaction/ОтменитьТранзакцию call.
///
/// Returns true if the statement is a non-qualified call to RollbackTransaction/ОтменитьТранзакцию.
/// Filters out:
/// - Non-CALL_STMT nodes
/// - Qualified calls like `Connector.RollbackTransaction()`
pub(crate) fn is_global_rollback_transaction_call(node: &SyntaxNode) -> bool {
    if node.kind() != SyntaxKind::CALL_STMT {
        return false;
    }

    if node.descendants().any(|n| n.kind() == SyntaxKind::FIELD_EXPR) {
        return false;
    }

    let ident = node
        .descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT);

    let Some(ident) = ident else {
        return false;
    };

    is_global_function(ident.text(), "RollbackTransaction")
}

/// Check RollbackTransaction calls within a TRY_STMT for proper placement.
///
/// Returns a list of RollbackTransaction nodes that are NOT properly used:
/// 1. Outside exception handler (should be in except block)
/// 2. Not first statement in exception handler (must be first)
pub(crate) fn check_rollback_transaction_in_try(try_stmt: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut violations = Vec::new();

    let try_body = try_stmt.children().find(|n| n.kind() == SyntaxKind::STMT_LIST);
    let except_clause = try_stmt.children().find(|n| n.kind() == SyntaxKind::EXCEPT_CLAUSE);

    if let Some(body) = &try_body {
        for node in body.descendants() {
            if is_global_rollback_transaction_call(&node) {
                violations.push(node);
            }
        }
    }

    if let Some(except) = &except_clause {
        let except_body = except.children().find(|n| n.kind() == SyntaxKind::STMT_LIST);

        if let Some(body) = except_body {
            let stmts: Vec<_> = body.children().filter(is_executable_stmt).collect();
            let first_global_call_idx = stmts.iter().position(|s| {
                s.kind() == SyntaxKind::CALL_STMT
                    && !s.descendants().any(|n| n.kind() == SyntaxKind::FIELD_EXPR)
            });

            for node in except.descendants() {
                if is_global_rollback_transaction_call(&node) {
                    let rollback_idx =
                        stmts.iter().position(|s| s.text_range() == node.text_range());
                    if let Some(idx) = rollback_idx {
                        if let Some(first_idx) = first_global_call_idx {
                            if idx != first_idx {
                                violations.push(node);
                            }
                        }
                    }
                }
            }
        }
    }

    violations
}

// =============================================================================
// CodeAfterAsyncCall diagnostic support
// =============================================================================

/// List of asynchronous method English names for CodeAfterAsyncCall diagnostic.
///
/// Contains 25 English names. Russian variants are matched via bsl-platform lookup.
/// - Dialog methods: ShowQueryBox, ShowValue, ShowMessageBox, etc.
/// - Input methods: ShowInputNumber, ShowInputDate, etc.
/// - File operations: BeginPutFile, BeginCopyingFile, etc.
/// - Extension operations: BeginInstallAddIn, etc.
const ASYNC_ENGLISH_NAMES: &[&str] = &[
    "ShowQueryBox",
    "ShowValue",
    "ShowMessageBox",
    "ShowInputDate",
    "ShowInputValue",
    "ShowInputString",
    "ShowInputNumber",
    "BeginInstallAddIn",
    "BeginInstallFileSystemExtension",
    "BeginInstallCryptoExtension",
    "BeginAttachingCryptoExtension",
    "BeginAttachingFileSystemExtension",
    "BeginPutFile",
    "BeginCopyingFile",
    "BeginMovingFile",
    "BeginFindingFiles",
    "BeginDeletingFiles",
    "BeginCreatingDirectory",
    "BeginGettingTempFilesDir",
    "BeginGettingDocumentsDir",
    "BeginGettingUserDataWorkDir",
    "BeginGettingFiles",
    "BeginPuttingFiles",
    "BeginRequestingUserPermission",
    "BeginRunningApplication",
];

/// Check if a method name is an asynchronous method (case-insensitive, bilingual).
fn is_async_method(name: &str) -> bool {
    is_any_global_function(name, ASYNC_ENGLISH_NAMES)
}

/// Check for CodeAfterAsyncCall diagnostic in a method body.
///
/// Takes pre-collected CALL_STMT nodes from control flow analysis and checks
/// if they are global async calls with code after them.
///
/// This version avoids a separate `descendants()` traversal by reusing nodes
/// collected during combined control flow analysis.
pub(crate) fn check_code_after_async_call(ctx: &mut LoweringCtx, call_stmts: &[SyntaxNode]) {
    for node in call_stmts {
        // Check if this is a global async call
        if !is_global_async_call(node) {
            continue;
        }

        // Get method name for diagnostic message
        let Some(method_name) = get_call_method_name(node) else {
            continue;
        };

        // Check if there's code after this async call
        if has_code_after_async(node) {
            let extended_range = extend_range_with_semicolon(node, node.text_range());
            ctx.emit(BodyDiagnostic::CodeAfterAsyncCall { method_name, range: extended_range });
        }
    }
}

/// Check if a CALL_STMT is a global call to an async method.
///
/// Returns false for:
/// - Non-CALL_STMT nodes
/// - Qualified calls (Object.Method())
/// - Non-async methods
fn is_global_async_call(node: &SyntaxNode) -> bool {
    if node.kind() != SyntaxKind::CALL_STMT {
        return false;
    }

    // Find ARG_LIST position to only check call structure, not arguments
    let arg_list_start = node
        .descendants()
        .find(|n| n.kind() == SyntaxKind::ARG_LIST)
        .map(|n| n.text_range().start());

    // Check for FIELD_EXPR only BEFORE ARG_LIST (in the call target, not in arguments)
    // Qualified calls like Object.Method() have FIELD_EXPR before the ARG_LIST
    for child in node.descendants() {
        if child.kind() == SyntaxKind::FIELD_EXPR {
            if let Some(al_start) = arg_list_start {
                if child.text_range().start() < al_start {
                    return false;
                }
            } else {
                return false;
            }
        }
    }

    // Get method name and check if it's async
    let Some(name) = get_call_method_name(node) else {
        return false;
    };

    is_async_method(&name)
}

/// Extract method name from a CALL_STMT node.
fn get_call_method_name(node: &SyntaxNode) -> Option<String> {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
}

/// Check if there's executable code after an async call statement.
///
/// Algorithm:
/// 1. Check immediate siblings in the same block
/// 2. If first sibling is Return → false (safe exit)
/// 3. If first sibling is Break → check parent blocks
/// 4. Skip code inside exception handlers
/// 5. If any executable statement found → true
/// 6. Recursively check parent blocks for code after control structures
fn has_code_after_async(stmt: &SyntaxNode) -> bool {
    let Some(parent) = stmt.parent() else {
        return false;
    };

    let mut first_stmt_is_return = false;
    let mut first_stmt_is_break = false;
    let mut has_any_stmts = false;
    let mut in_exception_handler = false;

    let mut sibling = stmt.next_sibling();
    while let Some(next) = sibling {
        // Track exception handler boundaries
        if is_except_keyword(&next) {
            in_exception_handler = true;
        }
        if is_end_try_keyword(&next) {
            in_exception_handler = false;
        }

        // Skip code inside exception handlers
        if in_exception_handler {
            sibling = next.next_sibling();
            continue;
        }

        // Check if this is an executable statement or return/break
        if is_executable_statement(&next) || is_return_or_break(&next) {
            if !has_any_stmts {
                if next.kind() == SyntaxKind::RETURN_STMT {
                    first_stmt_is_return = true;
                } else if next.kind() == SyntaxKind::BREAK_STMT {
                    first_stmt_is_break = true;
                }
            }
            has_any_stmts = true;
        }

        sibling = next.next_sibling();
    }

    // If first statement is Return, it's a safe exit
    if first_stmt_is_return {
        return false;
    }

    // If there are statements and first is NOT break, that's an error
    // If first is break, still need to check parent
    let immediate_error = !first_stmt_is_break && has_any_stmts;
    immediate_error || check_parent_block_for_async(&parent)
}

/// Recursively check parent blocks for code after control structures containing the async call.
fn check_parent_block_for_async(node: &SyntaxNode) -> bool {
    let mut current = node.clone();

    loop {
        match current.kind() {
            SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT => {
                let mut sibling = current.next_sibling();
                while let Some(next) = sibling {
                    if is_else_clause(&next) {
                        sibling = next.next_sibling();
                        continue;
                    }

                    if is_return_or_break(&next) {
                        return false;
                    }

                    if is_executable_statement(&next) {
                        return true;
                    }

                    sibling = next.next_sibling();
                }

                if let Some(parent) = current.parent() {
                    current = parent;
                } else {
                    return false;
                }
            }

            SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF => {
                return false;
            }

            _ => {
                if let Some(parent) = current.parent() {
                    current = parent;
                } else {
                    return false;
                }
            }
        }
    }
}

/// Check if a node is an executable statement.
fn is_executable_statement(node: &SyntaxNode) -> bool {
    matches!(
        node.kind(),
        SyntaxKind::ASSIGN_STMT
            | SyntaxKind::CALL_STMT
            | SyntaxKind::IF_STMT
            | SyntaxKind::WHILE_STMT
            | SyntaxKind::FOR_STMT
            | SyntaxKind::FOR_EACH_STMT
            | SyntaxKind::TRY_STMT
            | SyntaxKind::EXECUTE_STMT
            | SyntaxKind::RAISE_STMT
    )
}

/// Check if a node is a Return or Break statement.
fn is_return_or_break(node: &SyntaxNode) -> bool {
    matches!(node.kind(), SyntaxKind::RETURN_STMT | SyntaxKind::BREAK_STMT)
}

/// Check if a node contains the EXCEPT keyword (starts exception handler).
fn is_except_keyword(node: &SyntaxNode) -> bool {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|t| t.kind() == SyntaxKind::KW_EXCEPT)
}

/// Check if a node contains the END_TRY keyword (ends try-except block).
fn is_end_try_keyword(node: &SyntaxNode) -> bool {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|t| t.kind() == SyntaxKind::KW_END_TRY)
}

/// Check if a node is an Else or ElseIf clause.
fn is_else_clause(node: &SyntaxNode) -> bool {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|t| matches!(t.kind(), SyntaxKind::KW_ELSIF | SyntaxKind::KW_ELSE))
}

/// Extend a text range to include the following semicolon token if present.
///
/// Java BSLParser.StatementContext includes the SEMICOLON in the statement range.
/// Our CALL_STMT does not include SEMICOLON (it's a separate token).
/// To match Java ranges, we extend the range to include the semicolon.
pub(crate) fn extend_range_with_semicolon(
    node: &SyntaxNode,
    original_range: TextRange,
) -> TextRange {
    if let Some(NodeOrToken::Token(token)) = node.next_sibling_or_token() {
        if token.kind() == SyntaxKind::SEMICOLON {
            return original_range.cover(token.text_range());
        }
    }
    original_range
}

// =============================================================================
// ServerCallsInFormEvents detection
// =============================================================================

/// Forbidden form event suffixes (case-insensitive).
///
/// These events execute frequently during UI interactions and should not call server methods.
const FORBIDDEN_EVENT_SUFFIXES: &[&str] =
    &["приактивизациистроки", "onactivaterow", "началовыбора", "onstartchoice"];

/// Check if method name ends with a forbidden form event suffix (case-insensitive).
///
/// Returns true if the method name matches one of:
/// - ПриАктивизацииСтроки / OnActivateRow
/// - НачалоВыбора / OnStartChoice
///
/// These are form events that should not call server-side methods.
pub(crate) fn is_forbidden_form_event(method_name: &str) -> bool {
    let name_lower = method_name.to_lowercase();
    FORBIDDEN_EVENT_SUFFIXES.iter().any(|suffix| name_lower.ends_with(suffix))
}

// =============================================================================
// UsingExternalCodeTools detection
// =============================================================================

/// External code tools class names (case-insensitive).
/// These are global context objects that allow executing external code.
const EXTERNAL_CODE_TOOLS: &[&str] = &[
    "внешниеобработки",
    "externaldataprocessors",
    "внешниеотчеты",
    "externalreports",
    "расширенияконфигурации",
    "configurationextensions",
];

/// Dangerous method names for external code tools (case-insensitive).
/// These methods create or connect external code, which is a security risk.
const EXTERNAL_CODE_METHODS: &[&str] = &["создать", "create", "подключить", "connect"];

/// Check if an identifier is an external code tools class name.
/// Returns true for: ВнешниеОбработки, ExternalDataProcessors, ВнешниеОтчеты,
/// ExternalReports, РасширенияКонфигурации, ConfigurationExtensions.
pub(crate) fn is_external_code_tools_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    EXTERNAL_CODE_TOOLS.contains(&lower.as_str())
}

/// Check if a method name is a dangerous external code operation.
/// Returns true for: Создать, Create, Подключить, Connect.
pub(crate) fn is_external_code_tools_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    EXTERNAL_CODE_METHODS.contains(&lower.as_str())
}

// =============================================================================
// UsingFindElementByString detection
// =============================================================================

/// Method names for FindByDescription/НайтиПоНаименованию (case-insensitive).
const FIND_BY_DESCRIPTION: &[&str] = &["найтипонаименованию", "findbydescription"];

/// Method names for FindByCode/НайтиПоКоду (case-insensitive).
const FIND_BY_CODE: &[&str] = &["найтипокоду", "findbycode"];

/// Method names for FindByNumber/НайтиПоНомеру (case-insensitive).
const FIND_BY_NUMBER: &[&str] = &["найтипономеру", "findbynumber"];

/// Check if a method name is a FindElement method (FindByDescription, FindByCode, FindByNumber).
/// Returns true if the method name matches one of the search methods (case-insensitive).
pub(crate) fn is_find_element_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    FIND_BY_DESCRIPTION.contains(&lower.as_str())
        || FIND_BY_CODE.contains(&lower.as_str())
        || FIND_BY_NUMBER.contains(&lower.as_str())
}

// =============================================================================
// UsingModalWindows detection
// =============================================================================

/// Modal window method pairs: (modal_method, non_modal_replacement).
/// All names are lowercase for case-insensitive matching.
const MODAL_METHODS: &[(&str, &str, &str, &str)] = &[
    // (russian_modal, english_modal, russian_replacement, english_replacement)
    ("вопрос", "doquerybox", "ПоказатьВопрос", "ShowQueryBox"),
    ("открытьформумодально", "openformmodal", "ОткрытьФорму", "OpenForm"),
    ("открытьзначение", "openvalue", "ПоказатьЗначение", "ShowValue"),
    ("предупреждение", "domessagebox", "ПоказатьПредупреждение", "ShowMessageBox"),
    ("ввестидату", "inputdate", "ПоказатьВводДаты", "ShowInputDate"),
    ("ввестизначение", "inputvalue", "ПоказатьВводЗначения", "ShowInputValue"),
    ("ввестистроку", "inputstring", "ПоказатьВводСтроки", "ShowInputString"),
    ("ввестичисло", "inputnumber", "ПоказатьВводЧисла", "ShowInputNumber"),
    (
        "установитьвнешнююкомпоненту",
        "installaddin",
        "НачатьУстановкуВнешнейКомпоненты",
        "BeginInstallAddIn",
    ),
    (
        "установитьрасширениеработысфайлами",
        "installfilesystemextension",
        "НачатьУстановкуРасширенияРаботыСФайлами",
        "BeginInstallFileSystemExtension",
    ),
    (
        "установитьрасширениеработыскриптографией",
        "installcryptoextension",
        "НачатьУстановкуРасширенияРаботыСКриптографией",
        "BeginInstallCryptoExtension",
    ),
    ("поместитьфайл", "putfile", "НачатьПомещениеФайла", "BeginPutFile"),
];

/// Check if a method name is a modal window method.
/// Returns Some(replacement) if modal, None otherwise.
/// The replacement is returned in the same language as the original method name.
pub(crate) fn get_modal_method_replacement(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    for &(ru, en, replacement_ru, replacement_en) in MODAL_METHODS {
        if lower == ru {
            return Some(replacement_ru);
        }
        if lower == en {
            return Some(replacement_en);
        }
    }
    None
}

/// Check if an identifier is ЭтаФорма/ThisForm (case-insensitive).
pub(crate) fn is_this_form_identifier(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "этаформа" || lower == "thisform"
}

// =============================================================================
// UsingSynchronousCalls detection
// =============================================================================

/// Synchronous method pairs: (sync_method, async_replacement).
/// All names are lowercase for case-insensitive matching.
/// Format: (russian_sync, english_sync, russian_replacement, english_replacement)
const SYNCHRONOUS_METHODS: &[(&str, &str, &str, &str)] = &[
    ("вопрос", "doquerybox", "ПоказатьВопрос", "ShowQueryBox"),
    ("открытьформумодально", "openformmodal", "ОткрытьФорму", "OpenForm"),
    ("открытьзначение", "openvalue", "ПоказатьЗначение", "ShowValue"),
    ("предупреждение", "domessagebox", "ПоказатьПредупреждение", "ShowMessageBox"),
    ("ввестидату", "inputdate", "ПоказатьВводДаты", "ShowInputDate"),
    ("ввестизначение", "inputvalue", "ПоказатьВводЗначения", "ShowInputValue"),
    ("ввестистроку", "inputstring", "ПоказатьВводСтроки", "ShowInputString"),
    ("ввестичисло", "inputnumber", "ПоказатьВводЧисла", "ShowInputNumber"),
    (
        "установитьвнешнююкомпоненту",
        "installaddin",
        "НачатьУстановкуВнешнейКомпоненты",
        "BeginInstallAddIn",
    ),
    (
        "установитьрасширениеработысфайлами",
        "installfilesystemextension",
        "НачатьУстановкуРасширенияРаботыСФайлами",
        "BeginInstallFileSystemExtension",
    ),
    (
        "установитьрасширениеработыскриптографией",
        "installcryptoextension",
        "НачатьУстановкуРасширенияРаботыСКриптографией",
        "BeginInstallCryptoExtension",
    ),
    (
        "подключитьрасширениеработыскриптографией",
        "attachcryptoextension",
        "НачатьПодключениеРасширенияРаботыСКриптографией",
        "BeginAttachingCryptoExtension",
    ),
    (
        "подключитьрасширениеработысфайлами",
        "attachfilesystemextension",
        "НачатьПодключениеРасширенияРаботыСФайлами",
        "BeginAttachingFileSystemExtension",
    ),
    ("поместитьфайл", "putfile", "НачатьПомещениеФайла", "BeginPutFile"),
    ("копироватьфайл", "filecopy", "НачатьКопированиеФайла", "BeginCopyingFile"),
    ("переместитьфайл", "movefile", "НачатьПеремещениеФайла", "BeginMovingFile"),
    ("найтифайлы", "findfiles", "НачатьПоискФайлов", "BeginFindingFiles"),
    ("удалитьфайлы", "deletefiles", "НачатьУдалениеФайлов", "BeginDeletingFiles"),
    ("создатькаталог", "createdirectory", "НачатьСозданиеКаталога", "BeginCreatingDirectory"),
    (
        "каталогвременныхфайлов",
        "tempfilesdir",
        "НачатьПолучениеКаталогаВременныхФайлов",
        "BeginGettingTempFilesDir",
    ),
    (
        "каталогдокументов",
        "documentsdir",
        "НачатьПолучениеКаталогаДокументов",
        "BeginGettingDocumentsDir",
    ),
    (
        "рабочийкаталогданныхпользователя",
        "userdataworkdir",
        "НачатьПолучениеРабочегоКаталогаДанныхПользователя",
        "BeginGettingUserDataWorkDir",
    ),
    ("получитьфайлы", "getfiles", "НачатьПолучениеФайлов", "BeginGettingFiles"),
    ("поместитьфайлы", "putfiles", "НачатьПомещениеФайлов", "BeginPuttingFiles"),
    (
        "запроситьразрешениепользователя",
        "requestuserpermission",
        "НачатьЗапросРазрешенияПользователя",
        "BeginRequestingUserPermission",
    ),
    ("запуститьприложение", "runapp", "НачатьЗапускПриложения", "BeginRunningApplication"),
];

/// Check if a method name is a synchronous method.
/// Returns Some(replacement) if synchronous, None otherwise.
/// The replacement is returned in the same language as the original method name.
pub(crate) fn get_synchronous_call_replacement(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    for &(ru, en, replacement_ru, replacement_en) in SYNCHRONOUS_METHODS {
        if lower == ru {
            return Some(replacement_ru);
        }
        if lower == en {
            return Some(replacement_en);
        }
    }
    None
}

/// Check if IDENT node is the callee of a CALL_EXPR.
/// Returns true if node is used as a function/method name in a call expression.
pub(crate) fn is_call_expr_callee(node: &SyntaxNode) -> bool {
    if let Some(parent) = node.parent() {
        let actual_parent =
            if parent.kind() == SyntaxKind::EXPR { parent.parent() } else { Some(parent) };

        if let Some(p) = actual_parent {
            if p.kind() == SyntaxKind::CALL_EXPR {
                if let Some(first_child) = p.children().next() {
                    return first_child.text_range().contains_range(node.text_range());
                }
            }
        }
    }
    false
}

/// Check if IDENT node is a field access on another object (not the base).
/// For example in `Structure.ЭтаФорма`, ЭтаФорма is a field, not the base.
/// Returns true if node is used as a field name after DOT.
pub(crate) fn is_field_access_field(node: &SyntaxNode) -> bool {
    if let Some(parent) = node.parent() {
        let actual_parent =
            if parent.kind() == SyntaxKind::EXPR { parent.parent() } else { Some(parent) };

        if let Some(p) = actual_parent {
            if p.kind() == SyntaxKind::FIELD_EXPR {
                if let Some(first_child) = p.children().next() {
                    return !first_child.text_range().contains_range(node.text_range());
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_forbidden_form_event_russian() {
        // Russian event names
        assert!(is_forbidden_form_event("ПриАктивизацииСтроки"));
        assert!(is_forbidden_form_event("НачалоВыбора"));

        // With prefix
        assert!(is_forbidden_form_event("ТаблицаФормыПриАктивизацииСтроки"));
        assert!(is_forbidden_form_event("ПолеВыбораНачалоВыбора"));
    }

    #[test]
    fn test_is_forbidden_form_event_english() {
        // English event names
        assert!(is_forbidden_form_event("OnActivateRow"));
        assert!(is_forbidden_form_event("OnStartChoice"));

        // With prefix
        assert!(is_forbidden_form_event("FormTableOnActivateRow"));
        assert!(is_forbidden_form_event("ChoiceFieldOnStartChoice"));
    }

    #[test]
    fn test_is_forbidden_form_event_case_insensitive() {
        // Case insensitive
        assert!(is_forbidden_form_event("ПРИАКТИВИЗАЦИИСТРОКИ"));
        assert!(is_forbidden_form_event("приактивизациистроки"));
        assert!(is_forbidden_form_event("ONACTIVATEROW"));
        assert!(is_forbidden_form_event("onactivaterow"));
        assert!(is_forbidden_form_event("OnAcTiVaTeRoW"));
    }

    #[test]
    fn test_is_forbidden_form_event_not_matching() {
        // Not form events
        assert!(!is_forbidden_form_event("ОбычнаяПроцедура"));
        assert!(!is_forbidden_form_event("OnClick"));
        assert!(!is_forbidden_form_event("ПриИзменении"));
        assert!(!is_forbidden_form_event("ПриОткрытии"));

        // Partial matches should not match
        assert!(!is_forbidden_form_event("ПриАктивизации")); // Missing "Строки"
        assert!(!is_forbidden_form_event("OnActivate")); // Missing "Row"
    }
}
