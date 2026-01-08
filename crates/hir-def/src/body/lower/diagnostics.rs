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

use super::LoweringCtx;

// =============================================================================
// Duplicated code block detection
// =============================================================================

/// Check for duplicated code blocks in if/elsif/else branches.
///
/// Compares all pairs of branches and emits diagnostics for identical blocks.
pub(crate) fn check_duplicated_code_blocks(ctx: &mut LoweringCtx, branch_nodes: &[SyntaxNode]) {
    // Early exit: need at least 3 branches to potentially have duplicates
    // Most IFs are simple if-else (2 branches), skip them for performance
    if branch_nodes.len() < 3 {
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
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "текущаядата" | "currentdate")
}

/// Check if a method name is deprecated Найти() / Find().
/// Returns true if the method is the deprecated global find function.
pub(crate) fn is_deprecated_find(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "найти" | "find")
}

/// Check if a method name is deprecated Сообщить() / Message().
/// Returns true if the method is the deprecated global message function.
pub(crate) fn is_deprecated_message(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "сообщить" | "message")
}

/// Check if a method name is Тип() / Type().
/// Returns true if the method is the type construction function.
pub(crate) fn is_type_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "тип" | "type")
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
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        "установитьбезопасныйрежим"
            | "setsafemode"
            | "установитьотключениебезопасногорежима"
            | "setsafemodedisabled"
    )
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
    let lower = name.to_lowercase();
    lower == "очиститьжурналрегистрации" || lower == "cleareventlog"
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

    let name = ident.text().to_lowercase();
    name == "начатьтранзакцию" || name == "begintransaction"
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

    let name = ident.text().to_lowercase();
    name == "зафиксироватьтранзакцию" || name == "committransaction"
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

// =============================================================================
// CodeAfterAsyncCall diagnostic support
// =============================================================================

/// List of asynchronous methods that trigger CodeAfterAsyncCall diagnostic.
///
/// Contains 50 methods (25 Russian + 25 English):
/// - Dialog methods: ShowQueryBox/ПоказатьВопрос, ShowValue/ПоказатьЗначение, etc.
/// - Input methods: ShowInputNumber/ПоказатьВводЧисла, etc.
/// - File operations: BeginPutFile/НачатьПомещениеФайла, etc.
/// - Extension operations: BeginInstallAddIn/НачатьУстановкуВнешнейКомпоненты, etc.
const ASYNC_METHODS: &[&str] = &[
    // Russian names (25)
    "показатьвопрос",
    "показатьзначение",
    "показатьпредупреждение",
    "показатьвводдаты",
    "показатьвводзначения",
    "показатьвводстроки",
    "показатьвводчисла",
    "начатьустановкувнешнейкомпоненты",
    "начатьустановкурасширенияработысфайлами",
    "начатьустановкурасширенияработыскриптографией",
    "начатьподключениерасширенияработыскриптографией",
    "начатьподключениерасширенияработысфайлами",
    "начатьпомещениефайла",
    "начатькопированиефайла",
    "начатьперемещениефайла",
    "начатьпоискфайлов",
    "начатьудалениефайлов",
    "начатьсозданиекаталога",
    "начатьполучениекаталогавременныхфайлов",
    "начатьполучениекаталогадокументов",
    "начатьполучениерабочегокаталогаданныхпользователя",
    "начатьполучениефайлов",
    "начатьпомещениефайлов",
    "начатьзапросразрешенияпользователя",
    "начатьзапускприложения",
    // English names (25)
    "showquerybox",
    "showvalue",
    "showmessagebox",
    "showinputdate",
    "showinputvalue",
    "showinputstring",
    "showinputnumber",
    "begininstalladdin",
    "begininstallfilesystemextension",
    "begininstallcryptoextension",
    "beginattachingcryptoextension",
    "beginattachingfilesystemextension",
    "beginputfile",
    "begincopyingfile",
    "beginmovingfile",
    "beginfindingfiles",
    "begindeletingfiles",
    "begincreatingdirectory",
    "begingettingtempfilesdir",
    "begingettingdocumentsdir",
    "begingettinguserdataworkdir",
    "begingettingfiles",
    "beginputtingfiles",
    "beginrequestinguserpermission",
    "beginrunningapplication",
];

/// Check if a method name is an asynchronous method (case-insensitive).
fn is_async_method(name: &str) -> bool {
    let name_lower = name.to_lowercase();
    ASYNC_METHODS.contains(&name_lower.as_str())
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
