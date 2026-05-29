use std::collections::HashSet;

use syntax::{NodeOrToken, SyntaxKind, SyntaxNode};
use text_size::TextRange;

use crate::body::BodyDiagnostic;

use super::platform_helpers::{is_any_global_function, is_global_function};
use super::LoweringCtx;

pub(crate) fn check_duplicated_code_blocks(ctx: &mut LoweringCtx, branch_nodes: &[SyntaxNode]) {
    if branch_nodes.len() < 2 {
        return;
    }

    let mut reported: HashSet<usize> = HashSet::new();

    for i in 0..branch_nodes.len() - 1 {
        if reported.contains(&i) {
            continue;
        }

        let current_block = &branch_nodes[i];
        let current_text = normalize_code_block(current_block);
        let current_count = count_statements(current_block);

        if current_count == 0 {
            continue;
        }

        let mut has_duplicate = false;
        #[allow(clippy::needless_range_loop, reason = "reported stores the matched branch index")]
        for j in (i + 1)..branch_nodes.len() {
            let other_block = &branch_nodes[j];
            let other_count = count_statements(other_block);

            if other_count == 0 {
                continue;
            }

            if current_count != other_count {
                continue;
            }

            let other_text = normalize_code_block(other_block);
            if current_text == other_text {
                has_duplicate = true;
                reported.insert(j);
            }
        }

        if has_duplicate {
            ctx.emit(BodyDiagnostic::IfElseDuplicatedCodeBlock {
                range: current_block.text_range(),
            });
        }
    }
}

fn normalize_code_block(block: &SyntaxNode) -> String {
    block
        .text()
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

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

pub(crate) fn is_deprecated_method(name: &str) -> bool {
    let lower = name.to_lowercase();

    matches!(
        lower.as_str(),
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
            | "краткоепредставлениеошибки"
            | "подробноепредставлениеошибки"
            | "показатьинформациюобошибке"
            | "brieferrorrepresentation"
            | "detailederrorrepresentation"
            | "showerrorinformation"
            | "получитьформу"
            | "getform"
    )
}

pub(crate) fn is_deprecated_current_date(name: &str) -> bool {
    is_global_function(name, "CurrentDate")
}

pub(crate) fn is_deprecated_find(name: &str) -> bool {
    is_global_function(name, "Find")
}

pub(crate) fn is_deprecated_message(name: &str) -> bool {
    is_global_function(name, "Message")
}

pub(crate) fn is_type_method(name: &str) -> bool {
    is_global_function(name, "Type")
}

pub(crate) fn is_deprecated_managed_form(type_name: &str) -> bool {
    let lower = type_name.to_lowercase();
    matches!(lower.as_str(), "управляемаяформа" | "managedform")
}

pub(crate) fn is_safe_mode_query(name: &str) -> bool {
    bsl_platform::security::registry()
        .lookup_global(name)
        .is_some_and(|e| matches!(e.category, bsl_platform::security::Category::SafeModeQuery))
}

pub(crate) fn is_find_by_code_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(lower.as_str(), "найтипокоду" | "findbycode")
}

pub(crate) fn is_temp_files_dir(name: &str) -> bool {
    is_global_function(name, "TempFilesDir")
}

use crate::body::DeprecatedKind8312;

pub(crate) fn is_deprecated_attribute_8312(
    object: &str,
    member: &str,
    is_call: bool,
) -> Option<DeprecatedKind8312> {
    let obj_lower = object.to_lowercase();
    let member_lower = member.to_lowercase();

    if is_chart_plot_area(&obj_lower)
        && !is_call
        && is_chart_plot_area_deprecated_attr(&member_lower)
    {
        return Some(DeprecatedKind8312::Attribute);
    }

    if is_chart(&obj_lower) {
        if is_call {
            if is_chart_deprecated_method(&member_lower) {
                return Some(DeprecatedKind8312::Method);
            }
        } else if is_chart_deprecated_attr(&member_lower) {
            return Some(DeprecatedKind8312::Attribute);
        }
    }

    if is_child_form_items_group(&obj_lower)
        && !is_call
        && is_child_form_items_group_deprecated_attr(&member_lower)
    {
        return Some(DeprecatedKind8312::EnumValue);
    }

    if is_chart_labels_orientation(&obj_lower) {
        return Some(DeprecatedKind8312::EnumName);
    }

    None
}

pub(crate) fn is_deprecated_global_method_8312(name: &str) -> bool {
    is_global_function(name, "ClearEventLog")
}

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

pub(crate) fn is_global_begin_transaction_call(node: &SyntaxNode) -> bool {
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

    is_global_function(ident.text(), "BeginTransaction")
}

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

pub(crate) fn is_inside_try_body_not_except(node: &SyntaxNode) -> bool {
    let mut current = node.clone();
    while let Some(parent) = current.parent() {
        match parent.kind() {
            SyntaxKind::EXCEPT_CLAUSE => return false,
            SyntaxKind::TRY_STMT => return true,
            _ => {}
        }
        current = parent;
    }
    false
}

pub(crate) fn check_try_number_call(node: &SyntaxNode) -> Option<TextRange> {
    if node.kind() != SyntaxKind::CALL_EXPR {
        return None;
    }

    if !is_inside_try_body_not_except(node) {
        return None;
    }

    let first_child = node.children().next()?;
    if first_child.kind() != SyntaxKind::IDENT {
        return None;
    }

    let name = first_child.text().to_string().to_lowercase();
    if name == "число" || name == "number" {
        return Some(node.text_range());
    }

    None
}

pub(crate) fn is_global_commit_transaction_call(node: &SyntaxNode) -> bool {
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

    is_global_function(ident.text(), "CommitTransaction")
}

pub(crate) fn check_commit_transaction_in_try(
    try_stmt: &SyntaxNode,
) -> Vec<(SyntaxNode, CommitViolation)> {
    let mut violations = Vec::new();

    let has_except = try_stmt.children().any(|n| n.kind() == SyntaxKind::EXCEPT_CLAUSE);

    let try_body = try_stmt.children().find(|n| n.kind() == SyntaxKind::STMT_LIST);

    let except_clause = try_stmt.children().find(|n| n.kind() == SyntaxKind::EXCEPT_CLAUSE);

    if let Some(body) = &try_body {
        let stmts: Vec<_> = body.children().filter(is_executable_stmt).collect();

        for (i, stmt) in stmts.iter().enumerate() {
            if is_global_commit_transaction_call(stmt) {
                if !has_except {
                    violations.push((stmt.clone(), CommitViolation::TryWithoutExcept));
                } else if i < stmts.len() - 1 {
                    violations.push((stmt.clone(), CommitViolation::CodeAfterCommit));
                }
            }
        }
    }

    if let Some(except) = &except_clause {
        for node in except.descendants() {
            if is_global_commit_transaction_call(&node) {
                violations.push((node, CommitViolation::InsideExceptHandler));
            }
        }
    }

    violations
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitViolation {
    InsideExceptHandler,
    TryWithoutExcept,
    CodeAfterCommit,
}

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

fn is_async_method(name: &str) -> bool {
    is_any_global_function(name, ASYNC_ENGLISH_NAMES)
}

pub(crate) fn check_code_after_async_call(ctx: &mut LoweringCtx, call_stmts: &[SyntaxNode]) {
    for node in call_stmts {
        if !is_global_async_call(node) {
            continue;
        }

        let Some(method_name) = get_call_method_name(node) else {
            continue;
        };

        if has_code_after_async(node) {
            let extended_range = extend_range_with_semicolon(node, node.text_range());
            ctx.emit(BodyDiagnostic::CodeAfterAsyncCall { method_name, range: extended_range });
        }
    }
}

fn is_global_async_call(node: &SyntaxNode) -> bool {
    if node.kind() != SyntaxKind::CALL_STMT {
        return false;
    }

    let arg_list_start = node
        .descendants()
        .find(|n| n.kind() == SyntaxKind::ARG_LIST)
        .map(|n| n.text_range().start());

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

    let Some(name) = get_call_method_name(node) else {
        return false;
    };

    is_async_method(&name)
}

fn get_call_method_name(node: &SyntaxNode) -> Option<String> {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text().to_string())
}

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
        if is_except_keyword(&next) {
            in_exception_handler = true;
        }
        if is_end_try_keyword(&next) {
            in_exception_handler = false;
        }

        if in_exception_handler {
            sibling = next.next_sibling();
            continue;
        }

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

    if first_stmt_is_return {
        return false;
    }

    let immediate_error = !first_stmt_is_break && has_any_stmts;
    immediate_error || check_parent_block_for_async(&parent)
}

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

fn is_return_or_break(node: &SyntaxNode) -> bool {
    matches!(node.kind(), SyntaxKind::RETURN_STMT | SyntaxKind::BREAK_STMT)
}

fn is_except_keyword(node: &SyntaxNode) -> bool {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|t| t.kind() == SyntaxKind::KW_EXCEPT)
}

fn is_end_try_keyword(node: &SyntaxNode) -> bool {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|t| t.kind() == SyntaxKind::KW_END_TRY)
}

fn is_else_clause(node: &SyntaxNode) -> bool {
    node.descendants_with_tokens()
        .filter_map(|el| el.into_token())
        .any(|t| matches!(t.kind(), SyntaxKind::KW_ELSIF | SyntaxKind::KW_ELSE))
}

pub(crate) fn is_followed_by_loop_exit(expr_node: &SyntaxNode) -> bool {
    let Some(parent_stmt) = expr_node.parent() else {
        return false;
    };

    let Some(next_sibling) = parent_stmt.next_sibling() else {
        return false;
    };

    matches!(next_sibling.kind(), SyntaxKind::BREAK_STMT | SyntaxKind::RETURN_STMT)
}

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

const EXTERNAL_CODE_TOOLS: &[&str] = &[
    "внешниеобработки",
    "externaldataprocessors",
    "внешниеотчеты",
    "externalreports",
    "расширенияконфигурации",
    "configurationextensions",
];

const EXTERNAL_CODE_METHODS: &[&str] = &["создать", "create", "подключить", "connect"];

pub(crate) fn is_external_code_tools_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    EXTERNAL_CODE_TOOLS.contains(&lower.as_str())
}

pub(crate) fn is_external_code_tools_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    EXTERNAL_CODE_METHODS.contains(&lower.as_str())
}

const FIND_BY_DESCRIPTION: &[&str] = &["найтипонаименованию", "findbydescription"];

const FIND_BY_CODE: &[&str] = &["найтипокоду", "findbycode"];

const FIND_BY_NUMBER: &[&str] = &["найтипономеру", "findbynumber"];

pub(crate) fn is_find_element_method(name: &str) -> bool {
    let lower = name.to_lowercase();
    FIND_BY_DESCRIPTION.contains(&lower.as_str())
        || FIND_BY_CODE.contains(&lower.as_str())
        || FIND_BY_NUMBER.contains(&lower.as_str())
}

const MODAL_METHODS: &[(&str, &str, &str, &str)] = &[
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

pub(crate) fn is_this_form_identifier(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "этаформа" || lower == "thisform"
}

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
