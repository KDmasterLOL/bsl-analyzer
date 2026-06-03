use std::collections::HashSet;

use bsl_metadata::MdoType;
use bsl_platform::{GlobalFunction, PlatformDataInner};
use either::Either;
use hir::{DefWithBodyId, ExprScopes, ScopeDef};
use ide_db::base_db::Locale;
use ide_db::{RootDatabase, TextRange};
use syntax::{ast::AstNode, SyntaxKind};

use super::{CompletionItem, CompletionItemKind, CompletionPosition};
use crate::completion::platform_completion::{render_mdo_field, render_platform_property};

pub(super) fn bsl_completions<DB: RootDatabase>(
    db: &DB,
    position: CompletionPosition,
) -> Option<Vec<CompletionItem>> {
    let _span = tracing::info_span!("bsl_completions").entered();

    tracing::info!("bsl_completions called");

    let parse = db.parse(position.file_id);
    let root = parse.syntax_node();

    let token = match root.token_at_offset(position.offset) {
        syntax::TokenAtOffset::None => {
            tracing::info!("No token at position - returning None");
            return None;
        }
        syntax::TokenAtOffset::Single(t) => t,
        syntax::TokenAtOffset::Between(left, right) => {
            if left.kind() == SyntaxKind::IDENT || left.kind().is_keyword() {
                left
            } else {
                right
            }
        }
    };

    tracing::debug!(token_kind = ?token.kind(), token_text = ?token.text(), "BSL completion token");

    if let Some(prev) = token.prev_sibling_or_token() {
        if prev.kind() == SyntaxKind::DOT {
            tracing::info!("After DOT - skipping BSL completion");
            return None;
        }
    }

    tracing::debug!("Not after DOT, checking if typing...");

    let token_text = token.text();

    let is_typing = if token.kind() == SyntaxKind::IDENT {
        true
    } else if token.kind().is_keyword() {
        let token_start = token.text_range().start();
        let cursor_in_token = position.offset.checked_sub(token_start);
        if let Some(offset_in_token) = cursor_in_token {
            let offset_in_token: usize = offset_in_token.into();
            offset_in_token < token_text.len()
        } else {
            false
        }
    } else {
        false
    };

    tracing::debug!(is_typing = is_typing, "Checked is_typing");

    if is_typing {
        let token_start = token.text_range().start();
        let cursor_in_token = position.offset.checked_sub(token_start).unwrap_or_default();
        let cursor_in_token: usize = cursor_in_token.into();

        let prefix = &token_text[..cursor_in_token.min(token_text.len())];

        tracing::info!(
            prefix = ?prefix,
            token_kind = ?token.kind(),
            full_text = ?token_text,
            "Completing with prefix"
        );

        let mut completions = Vec::new();

        if token.kind().is_keyword() {
            let (detail, documentation) = get_keyword_info(token_text);
            let keyword_item = CompletionItem {
                label: token_text.to_string(),
                detail: Some(detail),
                kind: CompletionItemKind::Keyword,
                insert_text: token_text.to_string(),
                documentation: Some(documentation),
                sort_text: None,
                filter_text: None,
                source: None,
            };
            completions.push(keyword_item);
        }

        completions.extend(complete_top_level(
            db,
            position.file_id,
            position.offset,
            prefix,
            position.locale,
            false,
        ));

        tracing::info!(count = completions.len(), "Returning BSL completions");
        return Some(completions);
    }

    if is_expression_start_position(&token) {
        tracing::info!(token_kind = ?token.kind(), "Expression start position - completing with empty prefix");
        let completions =
            complete_top_level(db, position.file_id, position.offset, "", position.locale, true);

        tracing::info!(count = completions.len(), "Returning BSL completions (trigger position)");
        return Some(completions);
    }

    tracing::info!("No BSL completion context - returning None");
    None
}

fn complete_top_level<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    offset: syntax::TextSize,
    prefix: &str,
    locale: Locale,
    with_sort_text: bool,
) -> Vec<CompletionItem> {
    let _span = tracing::debug_span!("complete_top_level").entered();

    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    push_band(
        &mut out,
        &mut seen,
        with_sort_text,
        "00_",
        complete_local_symbols(db, file_id, offset, prefix),
    );
    push_band(
        &mut out,
        &mut seen,
        with_sort_text,
        "10_",
        complete_user_defined_symbols(db, file_id, prefix),
    );
    push_band(
        &mut out,
        &mut seen,
        with_sort_text,
        "15_",
        complete_module_self_attributes(db, file_id, prefix, locale),
    );
    push_band(&mut out, &mut seen, with_sort_text, "20_", complete_mdo_plurals(prefix));
    push_band(
        &mut out,
        &mut seen,
        with_sort_text,
        "25_",
        complete_hbk_globals(db, file_id, prefix, locale),
    );
    push_band(
        &mut out,
        &mut seen,
        with_sort_text,
        "30_",
        complete_user_common_modules(db, file_id, prefix),
    );

    out
}

fn push_band(
    out: &mut Vec<CompletionItem>,
    seen: &mut HashSet<String>,
    with_sort_text: bool,
    band_prefix: &str,
    items: Vec<CompletionItem>,
) {
    for mut item in items {
        let key = item.label.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        if with_sort_text {
            item.sort_text = Some(format!("{band_prefix}{}", item.label));
        }
        out.push(item);
    }
}

fn complete_module_self_attributes<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    prefix: &str,
    locale: ide_db::base_db::Locale,
) -> Vec<CompletionItem> {
    let _span = tracing::debug_span!("complete_module_self_attributes").entered();
    let prefix_lower = prefix.to_lowercase();
    hir::module_implicit_fields(db, file_id)
        .into_iter()
        .filter(|field| field.name.as_str().to_lowercase().starts_with(&prefix_lower))
        .map(|field| render_mdo_field(db, &field, locale))
        .collect()
}

fn complete_local_symbols<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    offset: syntax::TextSize,
    prefix: &str,
) -> Vec<CompletionItem> {
    let _span = tracing::debug_span!("complete_local_symbols").entered();

    let mut completions = Vec::new();
    let prefix_lower = prefix.to_lowercase();

    let parse = db.parse(file_id);
    let root = parse.syntax_node();
    let token = match root.token_at_offset(offset).right_biased() {
        Some(t) => t,
        None => return completions,
    };

    let (method_def, method_range) = match find_method_for_token(&token) {
        Some((def, range)) => (def, range),
        None => return completions,
    };

    let scopes = match &method_def {
        Either::Left(proc) => ExprScopes::from_procedure(proc),
        Either::Right(func) => ExprScopes::from_function(func),
    };

    let root_scope = scopes.root_scope();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (name, scope_def) in scopes.all_entries_in_scope(root_scope) {
        let name_str = name.as_str();
        seen.insert(name_str.to_lowercase());

        if !name_str.to_lowercase().starts_with(&prefix_lower) {
            continue;
        }

        let (kind, detail) = match scope_def {
            ScopeDef::Parameter => (CompletionItemKind::Field, "Параметр"),
            ScopeDef::LocalVariable => (CompletionItemKind::Field, "Локальная переменная"),
        };

        completions.push(CompletionItem {
            label: name_str.to_string(),
            detail: Some(detail.to_string()),
            kind,
            insert_text: name_str.to_string(),
            documentation: None,
            sort_text: None,
            filter_text: None,
            source: None,
        });
    }

    if let Some(owner) = owner_for_method_range(db, file_id, method_range) {
        let routed = hir::infer_owner(db, file_id, owner);
        let implicit_locals = routed.implicit_locals();
        if !implicit_locals.is_empty() {
            for (lower, info) in implicit_locals {
                if seen.contains(lower) {
                    continue;
                }
                if !lower.starts_with(&prefix_lower) {
                    seen.insert(lower.clone());
                    continue;
                }
                seen.insert(lower.clone());
                let text = info.name.as_str().to_string();
                completions.push(CompletionItem {
                    label: text.clone(),
                    detail: Some("Переменная".to_string()),
                    kind: CompletionItemKind::Field,
                    insert_text: text,
                    documentation: None,
                    sort_text: None,
                    filter_text: None,
                    source: None,
                });
            }
        }
    }

    tracing::debug!(
        count = completions.len(),
        prefix = ?prefix,
        method_range = ?method_range,
        "Completed local symbols"
    );

    completions
}

fn owner_for_method_range<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    method_range: TextRange,
) -> Option<DefWithBodyId> {
    let tree = db.item_tree(file_id);
    for (idx, item) in tree.top_level_items().iter().enumerate() {
        let range = match item {
            hir::ModItem::Procedure(proc_idx) => tree.procedure(*proc_idx).source_range,
            hir::ModItem::Function(func_idx) => tree.function(*func_idx).source_range,
            _ => continue,
        };
        if range == method_range {
            return Some(DefWithBodyId::Method(idx as u32));
        }
    }
    None
}

fn find_method_for_token(
    token: &syntax::SyntaxToken,
) -> Option<(Either<syntax::ast::ProcedureDef, syntax::ast::FunctionDef>, TextRange)> {
    use syntax::ast;

    for ancestor in token.parent()?.ancestors() {
        if let Some(proc) = ast::ProcedureDef::cast(ancestor.clone()) {
            let method_range = proc.syntax().text_range();
            return Some((Either::Left(proc), method_range));
        }
        if let Some(func) = ast::FunctionDef::cast(ancestor.clone()) {
            let method_range = func.syntax().text_range();
            return Some((Either::Right(func), method_range));
        }
    }
    None
}

fn is_expression_start_position(token: &syntax::SyntaxToken) -> bool {
    match token.kind() {
        SyntaxKind::R_PAREN | SyntaxKind::L_PAREN | SyntaxKind::COMMA => true,
        SyntaxKind::SEMICOLON => true,
        SyntaxKind::WHITESPACE | SyntaxKind::NEWLINE => {
            let mut prev = token.prev_token();
            while let Some(ref t) = prev {
                if !t.kind().is_trivia() {
                    break;
                }
                prev = t.prev_token();
            }
            match prev {
                Some(t) => !matches!(t.kind(), SyntaxKind::DOT),
                None => true,
            }
        }
        _ => false,
    }
}

fn complete_mdo_plurals(prefix: &str) -> Vec<CompletionItem> {
    let _span = tracing::debug_span!("complete_mdo_plurals").entered();

    let prefix_lower = prefix.to_lowercase();
    let mut completions = Vec::new();

    for prop in PlatformDataInner::instance().all_global_properties() {
        let Some(mdo_type) = MdoType::from_plural(prop.name.as_str())
            .or_else(|| MdoType::from_plural(prop.english_name.as_str()))
        else {
            continue;
        };
        if !matches_prefix_bilingual(&prop.name, &prop.english_name, &prefix_lower) {
            continue;
        }
        completions.push(render_mdo_plural_with_hbk(mdo_type, prop));
    }

    completions
}

fn render_mdo_plural_with_hbk(
    mdo_type: bsl_metadata::MdoType,
    prop: &bsl_platform::PlatformProperty,
) -> CompletionItem {
    let label = prop.name.to_string();
    let mut detail = format!("Коллекция метаданных ({})", mdo_type.russian_name());
    if prop.is_readonly {
        detail.push_str(" [Только чтение]");
    }
    let documentation = compose_mdo_plural_documentation(mdo_type, prop);
    let filter_text = format!("{} {}", prop.name, prop.english_name);

    CompletionItem {
        label: label.clone(),
        detail: Some(detail),
        kind: CompletionItemKind::MdoType,
        insert_text: label,
        documentation: Some(documentation),
        sort_text: None,
        filter_text: Some(filter_text),
        source: None,
    }
}

fn compose_mdo_plural_documentation(
    mdo_type: bsl_metadata::MdoType,
    prop: &bsl_platform::PlatformProperty,
) -> String {
    let mut out = format!("{} / {}\n\n", prop.name, prop.english_name);
    let docs = PlatformDataInner::instance().get_property_docs(prop.id);
    match docs {
        Some(d) if !d.description.trim().is_empty() => {
            out.push_str(d.description.trim());
            if let Some(notes) = d.notes.as_ref().filter(|n| !n.trim().is_empty()) {
                out.push_str("\n\n");
                out.push_str(notes.trim());
            }
            if !d.see_also.is_empty() {
                out.push_str("\n\nСм. также:");
                for link in &d.see_also {
                    out.push_str("\n- ");
                    out.push_str(link);
                }
            }
        }
        _ => {
            out.push_str("Коллекция объектов метаданных типа ");
            out.push_str(mdo_type.russian_name());
            out.push('.');
        }
    }
    out
}

fn complete_user_common_modules<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    prefix: &str,
) -> Vec<CompletionItem> {
    let _span = tracing::debug_span!("complete_user_common_modules").entered();

    let prefix_lower = prefix.to_lowercase();
    let mut completions = Vec::new();

    for name in module_index_for(db, file_id).common_module_display_names() {
        if !name.to_lowercase().starts_with(&prefix_lower) {
            continue;
        }
        completions.push(CompletionItem {
            label: name.to_string(),
            detail: Some("Общий модуль".to_string()),
            kind: CompletionItemKind::MdoObject,
            insert_text: name.to_string(),
            documentation: Some(format!("{name}\n\nОбщий модуль конфигурации.")),
            sort_text: None,
            filter_text: None,
            source: None,
        });
    }

    completions
}

fn module_index_for<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
) -> std::sync::Arc<hir::ModuleIndex> {
    let source_root_id = db.file_source_root_input(file_id).source_root_id(db);
    db.module_index(source_root_id)
}

fn complete_hbk_globals<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    prefix: &str,
    locale: Locale,
) -> Vec<CompletionItem> {
    let _span = tracing::debug_span!("complete_hbk_globals").entered();

    let data = PlatformDataInner::instance();
    let module_id = hir::ModuleId::new(file_id);
    let resolver = hir::Resolver::with_workspace_scope(module_id);

    let mut items = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let prefix_lower = prefix.to_lowercase();

    for prop in data.all_global_properties() {
        let ru_lower = prop.name.to_lowercase();

        if MdoType::from_plural(prop.name.as_str()).is_some()
            || MdoType::from_plural(prop.english_name.as_str()).is_some()
        {
            continue;
        }
        if resolver.user_common_module_exists(db, &hir::Name::new(&prop.name)) {
            continue;
        }
        if !matches_prefix_bilingual(&prop.name, &prop.english_name, &prefix_lower) {
            continue;
        }
        if !seen.insert(ru_lower) {
            continue;
        }

        items.push(render_platform_property(prop, locale));
    }

    items.extend(complete_global_functions(prefix));
    items
}

fn matches_prefix_bilingual(ru: &str, en: &str, lower: &str) -> bool {
    ru.to_lowercase().starts_with(lower) || en.to_lowercase().starts_with(lower)
}

fn complete_user_defined_symbols<DB: RootDatabase>(
    db: &DB,
    file_id: vfs::FileId,
    prefix: &str,
) -> Vec<CompletionItem> {
    let _span = tracing::debug_span!("complete_user_defined_symbols").entered();

    let mut completions = Vec::new();
    let prefix_lower = prefix.to_lowercase();

    let sema = hir::Semantics::new(db);
    let module = sema.module_from_file(file_id);

    for procedure in module.procedures() {
        let name = procedure.name();
        let name_str = name.as_str();

        if !name_str.to_lowercase().starts_with(&prefix_lower) {
            continue;
        }

        let is_export = procedure.is_export();
        let detail =
            if is_export { "Процедура Экспорт" } else { "Процедура" };

        completions.push(CompletionItem {
            label: name_str.to_string(),
            detail: Some(detail.to_string()),
            kind: CompletionItemKind::Function,
            insert_text: format!("{}()$0", name_str),
            documentation: None,
            sort_text: None,
            filter_text: None,
            source: None,
        });
    }

    for function in module.functions() {
        let name = function.name();
        let name_str = name.as_str();

        if !name_str.to_lowercase().starts_with(&prefix_lower) {
            continue;
        }

        let is_export = function.is_export();
        let detail = if is_export { "Функция Экспорт" } else { "Функция" };

        completions.push(CompletionItem {
            label: name_str.to_string(),
            detail: Some(detail.to_string()),
            kind: CompletionItemKind::Function,
            insert_text: format!("{}()$0", name_str),
            documentation: None,
            sort_text: None,
            filter_text: None,
            source: None,
        });
    }

    for variable in module.variables() {
        let name = variable.name();
        let name_str = name.as_str();

        if !name_str.to_lowercase().starts_with(&prefix_lower) {
            continue;
        }

        let is_export = variable.is_export();
        let detail =
            if is_export { "Переменная Экспорт" } else { "Переменная" };

        completions.push(CompletionItem::simple(
            name_str.to_string(),
            CompletionItemKind::Field,
            name_str.to_string(),
        ));

        if let Some(item) = completions.last_mut() {
            item.detail = Some(detail.to_string());
        }
    }

    tracing::debug!(
        count = completions.len(),
        prefix = ?prefix,
        "Completed user-defined symbols"
    );

    completions
}

fn complete_global_functions(prefix: &str) -> Vec<CompletionItem> {
    let data = PlatformDataInner::instance();
    let all_functions = data.all_global_functions();

    let prefix_lower = prefix.to_lowercase();

    let matching: Vec<_> = all_functions
        .iter()
        .filter(|f| {
            f.name.to_lowercase().starts_with(&prefix_lower)
                || f.english_name.to_lowercase().starts_with(&prefix_lower)
        })
        .collect();

    tracing::debug!(
        total_functions = all_functions.len(),
        matching_count = matching.len(),
        prefix = ?prefix,
        "Filtered global functions"
    );

    matching.iter().map(|f| render_global_function(f)).collect()
}

fn render_global_function(function: &GlobalFunction) -> CompletionItem {
    let docs = PlatformDataInner::instance().get_global_function_docs(function.id);
    let sig = symbol_info::from_global_function(function, docs.as_ref());
    super::platform_completion::item_from_signature(&sig)
}

fn get_keyword_info(keyword: &str) -> (String, String) {
    if let Some(keyword_docs) = bsl_platform::PlatformData::instance().get_keyword_docs(keyword) {
        let mut doc = format!("{} / {}\n\n", keyword_docs.keyword_ru, keyword_docs.keyword_en);

        if !keyword_docs.syntax.is_empty() {
            doc.push_str("**Синтаксис:**\n```bsl\n");
            doc.push_str(&keyword_docs.syntax);
            doc.push_str("\n```\n\n");
        }

        if !keyword_docs.description.is_empty() {
            doc.push_str(&keyword_docs.description);
            doc.push_str("\n\n");
        }

        if !keyword_docs.params.is_empty() {
            doc.push_str("**Параметры:**\n");
            for param in &keyword_docs.params {
                doc.push_str(&format!("- **{}**: {}\n", param.name, param.description));
            }
            doc.push('\n');
        }

        if let Some(ref version) = keyword_docs.min_version {
            doc.push_str(&format!("**Доступен с версии:** {}", version));
        }

        return ("Ключевое слово BSL".to_string(), doc);
    }

    ("Ключевое слово BSL".to_string(), format!("**{}**\n\nКлючевое слово языка BSL.", keyword))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complete_global_functions_with_prefix() {
        use bsl_platform::PlatformDataInner;

        let data = PlatformDataInner::instance();
        if data.all_global_functions().is_empty() {
            println!("Skipping test: no global functions available");
            return;
        }

        let items = complete_global_functions("Начать");

        println!("Found {} completions for 'Начать'", items.len());
        assert!(!items.is_empty(), "Should find functions starting with 'Начать'");

        let has_begin_transaction = items.iter().any(|i| i.label == "НачатьТранзакцию");
        assert!(has_begin_transaction, "Should contain НачатьТранзакцию");

        for item in &items {
            assert_eq!(item.kind, CompletionItemKind::Function);
        }
    }

    #[test]
    fn test_complete_global_functions_case_insensitive() {
        use bsl_platform::PlatformDataInner;

        let data = PlatformDataInner::instance();
        if data.all_global_functions().is_empty() {
            println!("Skipping test: no global functions available");
            return;
        }

        let items_lower = complete_global_functions("начать");
        let items_upper = complete_global_functions("НАЧАТЬ");
        let items_mixed = complete_global_functions("Начать");

        assert_eq!(items_lower.len(), items_upper.len());
        assert_eq!(items_lower.len(), items_mixed.len());
    }

    #[test]
    fn test_render_global_function() {
        use bsl_platform::PlatformDataInner;

        let data = PlatformDataInner::instance();
        if data.all_global_functions().is_empty() {
            println!("Skipping test: no global functions available");
            return;
        }

        let function = data.get_global_function("НачатьТранзакцию").unwrap();
        let item = render_global_function(function);

        assert_eq!(item.label, "НачатьТранзакцию");
        assert_eq!(item.kind, CompletionItemKind::Function);
        assert!(item.detail.is_some());
        assert!(item.documentation.is_some());

        assert!(item.insert_text.ends_with("$0)"));
    }

    #[test]
    fn test_complete_user_defined_symbols() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        let source = r#"
Перем МояПеременная Экспорт;
Перем ПриватнаяПеременная;

Процедура МояПроцедура() Экспорт
    // тело
КонецПроцедуры

Функция МояФункция()
    Возврат 42;
КонецФункции

Процедура ДругаяПроцедура()
    Моя
КонецПроцедуры
"#;

        let mut db = RootDatabaseImpl::default();
        let file_id = vfs::FileId(0);

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, source);

        let items = complete_user_defined_symbols(&db, file_id, "Моя");

        println!("Found {} items for prefix 'Моя'", items.len());
        for item in &items {
            println!("  - {} ({:?})", item.label, item.kind);
        }

        assert_eq!(items.len(), 3, "Should find 3 items starting with 'Моя'");

        let has_procedure = items.iter().any(|i| i.label == "МояПроцедура");
        assert!(has_procedure, "Should contain МояПроцедура");

        let has_function = items.iter().any(|i| i.label == "МояФункция");
        assert!(has_function, "Should contain МояФункция");

        let has_variable = items.iter().any(|i| i.label == "МояПеременная");
        assert!(has_variable, "Should contain МояПеременная");

        let procedure_item = items.iter().find(|i| i.label == "МояПроцедура").unwrap();
        assert_eq!(
            procedure_item.detail,
            Some("Процедура Экспорт".to_string()),
            "МояПроцедура should be marked as Export"
        );

        let function_item = items.iter().find(|i| i.label == "МояФункция").unwrap();
        assert_eq!(
            function_item.detail,
            Some("Функция".to_string()),
            "МояФункция should NOT be marked as Export"
        );
    }

    #[test]
    fn test_complete_user_defined_symbols_case_insensitive() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        let source = r#"
Процедура ТестоваяПроцедура()
КонецПроцедуры
"#;

        let mut db = RootDatabaseImpl::default();
        let file_id = vfs::FileId(0);

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, source);

        let items_lower = complete_user_defined_symbols(&db, file_id, "тест");
        let items_upper = complete_user_defined_symbols(&db, file_id, "ТЕСТ");
        let items_mixed = complete_user_defined_symbols(&db, file_id, "Тест");

        assert_eq!(items_lower.len(), 1);
        assert_eq!(items_upper.len(), 1);
        assert_eq!(items_mixed.len(), 1);

        assert_eq!(items_lower[0].label, "ТестоваяПроцедура");
        assert_eq!(items_upper[0].label, "ТестоваяПроцедура");
        assert_eq!(items_mixed[0].label, "ТестоваяПроцедура");
    }

    #[test]
    fn test_complete_mdo_plural_forms() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        let source = r#"
Процедура Тест()
    Справ
КонецПроцедуры
"#;

        let mut db = RootDatabaseImpl::default();
        let file_id = vfs::FileId(0);

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, source);

        let items = complete_mdo_plurals("Справ");
        let _ = (&db, file_id);

        println!("Found {} MDO items for prefix 'Справ'", items.len());
        for item in &items {
            println!("  - {} ({:?})", item.label, item.kind);
        }

        let has_catalogs = items.iter().any(|i| i.label == "Справочники");
        assert!(has_catalogs, "Should contain Справочники plural form");

        let catalogs_item = items.iter().find(|i| i.label == "Справочники").unwrap();
        assert_eq!(catalogs_item.kind, CompletionItemKind::MdoType);
    }

    #[test]
    fn test_complete_mdo_symbols_bilingual() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        let source = r#"
Процедура Тест()
    Docu
КонецПроцедуры
"#;

        let mut db = RootDatabaseImpl::default();
        let file_id = vfs::FileId(0);

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, source);

        let items = complete_mdo_plurals("Docu");
        let _ = (&db, file_id);

        println!("Found {} MDO items for prefix 'Docu'", items.len());

        let has_documents = items.iter().any(|i| i.label == "Документы");
        assert!(has_documents, "Should contain Документы (matched by English 'Documents')");
    }

    #[test]
    fn test_complete_local_symbols() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        let source = r#"
Процедура Тест(Первый, Второй)
    Перем МояПеременная;
    Перем ДругаяПеременная;

    // Курсор здесь - вводим "Мо"
    Мо
КонецПроцедуры
"#;

        let mut db = RootDatabaseImpl::default();
        let file_id = vfs::FileId(0);

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, source);

        let offset = source.find("Мо").expect("Should find 'Мо' in source");
        let offset = syntax::TextSize::from(offset as u32);

        let items = complete_local_symbols(&db, file_id, offset, "Мо");

        println!("Found {} local items for prefix 'Мо'", items.len());
        for item in &items {
            println!("  - {} ({:?}, {:?})", item.label, item.kind, item.detail);
        }

        assert_eq!(items.len(), 1, "Should find 1 local variable starting with 'Мо'");
        assert_eq!(items[0].label, "МояПеременная");
        assert_eq!(items[0].detail, Some("Локальная переменная".to_string()));
    }

    #[test]
    fn test_complete_local_symbols_parameters() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        let source = r#"
Функция Тест(ПервыйПараметр, ВторойПараметр)
    Перем ЛокальнаяПеременная;

    // Курсор здесь - вводим "Перв"
    Перв
КонецФункции
"#;

        let mut db = RootDatabaseImpl::default();
        let file_id = vfs::FileId(0);

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, source);

        let offset = source.find("Перв").expect("Should find 'Перв' in source");
        let offset = syntax::TextSize::from(offset as u32);

        let items = complete_local_symbols(&db, file_id, offset, "Перв");

        println!("Found {} local items for prefix 'Перв'", items.len());
        for item in &items {
            println!("  - {} ({:?}, {:?})", item.label, item.kind, item.detail);
        }

        assert_eq!(items.len(), 1, "Should find 1 parameter starting with 'Перв'");
        assert_eq!(items[0].label, "ПервыйПараметр");
        assert_eq!(items[0].detail, Some("Параметр".to_string()));
    }

    #[test]
    fn test_complete_local_symbols_all() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        let source = r#"
Процедура Тест(Параметр1, Параметр2)
    Перем Переменная1;
    Перем Переменная2;

    // Empty prefix - should return all

КонецПроцедуры
"#;

        let mut db = RootDatabaseImpl::default();
        let file_id = vfs::FileId(0);

        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        db.set_file_text(file_id, source);

        let offset = source.find("// Empty prefix").expect("Should find comment") + 20;
        let offset = syntax::TextSize::from(offset as u32);

        let items = complete_local_symbols(&db, file_id, offset, "");

        println!("Found {} total local items", items.len());
        for item in &items {
            println!("  - {} ({:?}, {:?})", item.label, item.kind, item.detail);
        }

        assert_eq!(items.len(), 4, "Should find all local symbols");

        let param_count = items.iter().filter(|i| i.detail == Some("Параметр".to_string())).count();
        assert_eq!(param_count, 2, "Should have 2 parameters");

        let var_count =
            items.iter().filter(|i| i.detail == Some("Локальная переменная".to_string())).count();
        assert_eq!(var_count, 2, "Should have 2 local variables");
    }

    #[test]
    fn test_implicit_variables_from_assignments() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        let source = r#"
Процедура Тест(Запрос)
    Партнер = Справочники.Партнеры.НайтиПоКоду("001");
    Результат = Новый Структура;
    Результат.Вставить("Партнер", Партнер);
    ВременнаяПеременная = 42;
КонецПроцедуры
"#;
        let mut db = RootDatabaseImpl::default();
        let file_id = vfs::FileId(0);
        db.set_file_text(file_id, source);
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));

        let offset = syntax::TextSize::from(source.find("ВременнаяПеременная").unwrap() as u32);

        let items = complete_local_symbols(&db, file_id, offset, "");

        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        println!("Found local symbols: {:?}", labels);

        assert!(labels.contains(&"Запрос"), "Should find parameter Запрос");

        assert!(labels.contains(&"Партнер"), "Should find implicit var Партнер");
        assert!(labels.contains(&"Результат"), "Should find implicit var Результат");
        assert!(
            labels.contains(&"ВременнаяПеременная"),
            "Should find implicit var ВременнаяПеременная"
        );

        let implicit_count =
            items.iter().filter(|i| i.detail == Some("Переменная".to_string())).count();
        assert_eq!(implicit_count, 3, "Should have 3 implicit variables");
    }

    #[test]
    fn test_complete_module_self_attributes_skips_non_self_module() {
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use vfs::{file_set::FileSet, VfsPath};

        let source = "Процедура Test() КонецПроцедуры\n";
        let mut db = RootDatabaseImpl::default();
        let file_id = vfs::FileId(0);
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, source);

        let items = complete_module_self_attributes(&db, file_id, "", ide_db::base_db::Locale::Ru);
        assert!(items.is_empty(), "non-self file must not surface implicit attributes");
    }
}
