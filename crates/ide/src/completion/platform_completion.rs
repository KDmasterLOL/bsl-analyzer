//! Platform method completion.
//!
//! Provides completion for platform types and methods:
//! - Method completion after DOT (e.g., `Строка.` shows ВРег, НРег, etc.)
//! - CommonModule method completion (e.g., `ОбщегоНазначения.` shows exported methods)
//! - Snippets with parameter placeholders

use bsl_platform::{
    manager_methods_query, type_methods_query, type_properties_query, PlatformDataInner,
    PlatformMethod, PlatformProperty, TypeNameInput,
};
use hir::{Field, HirFieldOrigin, MethodSymbol, Name, Semantics, Ty, Type as HirType};
use ide_db::RootDatabase;
use symbol_info::{
    build_signature, from_platform_method, render_completion_detail, CalleeKind, CompletionDetail,
    MethodKind, SignatureSource, SymbolSignature,
};
use syntax::{SyntaxKind, SyntaxNode, SyntaxToken};
use vfs::FileId;

use super::{CompletionItem, CompletionItemKind, CompletionPosition};

/// Attempts to provide platform method completions.
///
/// Returns Some(items) if this is a method completion context (after DOT),
/// otherwise returns None to allow other completion providers to handle it.
///
/// Supports:
/// - Simple variable: `МойМассив.` → methods of Массив
/// - Direct type: `Строка.` → methods of Строка
/// - Fluent chains: `Запрос.Выполнить().Выбрать().` → methods of return type
pub(super) fn platform_completions<DB: RootDatabase>(
    db: &DB,
    position: CompletionPosition,
) -> Option<Vec<CompletionItem>> {
    let _span = tracing::info_span!("platform_completions").entered();

    let parse = db.parse(position.file_id);
    let root = parse.syntax_node();

    let token = root.token_at_offset(position.offset).left_biased()?;

    tracing::debug!(token_kind = ?token.kind(), token_text = ?token.text(), "Completion token");

    // Accept two cursor positions after a dot:
    //   1. cursor directly on `.` (`Сп.|`)          → anchor = DOT, no prefix.
    //   2. cursor inside/after an IDENT whose previous non-trivia token is `.`
    //      (`Сп.В|`)                                → anchor = that DOT,
    //      prefix = IDENT text up to the cursor.
    // Any other shape is not our context.
    let (dot_token, prefix) = resolve_dot_anchor(&token, position.offset)?;

    let receiver_expr = find_receiver_expr(&dot_token)?;

    // Fast path: bare-IDENT receiver (`ОбщегоНазначения.`) is almost always a
    // CommonModule call. Resolve via `module_index` (path-only, cheap) before
    // paying for `db.infer()` — which transitively warms `workspace_symbols`
    // across the whole source root (~50 s for a 12k-file workspace on cold
    // start, because every qualified call in the file triggers it).
    if let Some(receiver_name) = extract_receiver_ident(&receiver_expr) {
        tracing::debug!(receiver_name = %receiver_name, "Trying CommonModule fast path");
        if let Some(items) = complete_common_module_methods(db, &position, &receiver_name) {
            return Some(items);
        }
    }

    // Primary: `Semantics::type_of_expr` (M3 Task 9) walks the file's
    // `BodySourceMap` and looks up the inferred `Ty` for this exact
    // syntax node — same pipeline that `Expr::Field` / `Expr::MethodCall`
    // inference uses. Closes the Task 11 piece of Invariant #3: IDE
    // completion no longer dips into `PlatformData::instance()` for
    // receiver resolution.
    let sema = Semantics::new(db);
    let mut receiver_ty = sema.type_of_expr(position.file_id, &receiver_expr);

    // Fallback: a bare identifier that HIR couldn't resolve — typically
    // a literal type name (`Строка.`) or a platform constructor name
    // (`Запрос.`) without a variable binding. `Ty::from_type_name`
    // catches primitives / collections; anything else becomes a
    // `PlatformObject(name)` so `platform_type_name()` below can ask
    // `type_methods_query` for matching methods (empty result is safe
    // — completion just shows nothing).
    if receiver_ty.is_unknown() {
        if let Some(name) = extract_receiver_ident(&receiver_expr) {
            // If the receiver is a real workspace CommonModule, the fast
            // path `complete_common_module_methods` already had its turn
            // and either returned the module's methods or `None` (no
            // exported methods yet). Falling through into the platform
            // cascade here would mask the user's intent in two ways:
            //
            //  1. `get_global_property` could retype the receiver as a
            //     platform manager (`Метаданные` →
            //     `КонфигурацияМетаданныеОбъект`), surfacing unrelated
            //     methods.
            //  2. The trailing `Ty::PlatformObject(Name::new(&name))`
            //     fallback could collide with a same-named platform type
            //     (e.g. `БиблиотекаКартинок` ≡ `PictureLib`), and
            //     `type_methods_query` would happily surface its 294
            //     platform members.
            //
            // Both behaviours hide that the user's CommonModule is the
            // authoritative receiver here. Bail out instead.
            let name_node = Name::new(&name);
            let workspace_module_shadows = {
                let source_root_input = db.file_source_root_input(position.file_id);
                let source_root_id = source_root_input.source_root_id(db);
                db.module_index(source_root_id).resolve_common_module(&name_node).is_some()
            };
            // Same-file shadow: a module-level `Процедура ОбработкаОшибок()`
            // in the current file isn't in the cross-module `module_index`
            // but is still authoritative for `ОбработкаОшибок.|` here.
            // `infer_path_name` already returns `Ty::Unknown` for this case
            // (so `Semantics::type_of_expr` doesn't classify the receiver),
            // which is why we land in this fallback at all — without an
            // explicit symbol-tree probe we'd unmask the platform global.
            let same_file_shadows = {
                let module_id = hir::ModuleId::new(position.file_id);
                let tree = db.symbol_tree(module_id);
                tree.find_method(&name_node).is_some() || tree.find_variable(&name_node).is_some()
            };
            if workspace_module_shadows || same_file_shadows {
                return None;
            }

            // Platform global-context properties (`ОбработкаОшибок`,
            // `Метаданные`, `Справочники`, …) — the bare identifier names
            // a *property*, not a type, so the methods we want belong to
            // the declared type, not to a fake `PlatformObject(<property
            // name>)`. Without this branch the next fallback would index
            // `type_methods_query` with the property name and surface
            // zero methods.
            if let Some(prop) =
                bsl_platform::PlatformDataInner::instance().get_global_property(&name)
            {
                if let Some(declared) = prop.property_types.first() {
                    receiver_ty = Ty::PlatformObject(Name::new(declared.as_str()));
                }
            }
            if receiver_ty.is_unknown() {
                receiver_ty = Ty::from_type_name(&name);
            }
            if receiver_ty.is_unknown() {
                receiver_ty = Ty::PlatformObject(Name::new(&name));
            }
        }
    }

    tracing::debug!(receiver_ty = ?receiver_ty, "Resolved receiver type");

    // Manager / metadata-ref receivers are not indexed under a scalar
    // type key — their platform methods live behind composite
    // `type_name` prefixes (`"CatalogManager."`, `"CatalogObject."`,
    // …). Route them through `manager_methods_query` with the
    // `bsl-metadata` / `hir::MetadataKind` prefix tables.
    if let Some(items) =
        complete_prefix_methods_for_receiver(db, &receiver_ty, position.file_id, position.locale)
    {
        return Some(apply_prefix_filter(items, &prefix, db));
    }

    if let Some(type_name) = receiver_ty.platform_type_name() {
        tracing::debug!(type_name = ?type_name, "Platform type for completion");
        let items = complete_platform_methods(db, type_name, position.locale);
        return Some(apply_prefix_filter(items, &prefix, db));
    }

    // `Ty::Union` receivers show up whenever a platform method declares a
    // comma-joined return type (e.g. `Запрос.Выполнить` →
    // `"РезультатЗапроса, Неопределено"`, `QueryResult.Выгрузить` →
    // `"ТаблицаЗначений, ДеревоЗначений"`). Strip the `Undefined` / `Null`
    // sentinels — they have no instance members — and merge the surviving
    // branches' completion lists. Labels are deduped so members shared
    // across branches (e.g. `Количество`) surface once.
    if let Ty::Union(members) = &receiver_ty {
        let mut items: Vec<CompletionItem> = Vec::new();
        let mut seen_labels: std::collections::HashSet<String> = std::collections::HashSet::new();
        for m in members.iter().filter(|m| !matches!(m, Ty::Undefined | Ty::Null)) {
            let Some(type_name) = m.platform_type_name() else { continue };
            for item in complete_platform_methods(db, type_name, position.locale) {
                if seen_labels.insert(item.label.clone()) {
                    items.push(item);
                }
            }
        }
        if !items.is_empty() {
            return Some(apply_prefix_filter(items, &prefix, db));
        }
    }

    None
}

/// Enumerate platform methods for receivers that use a composite
/// `type_name` prefix instead of a scalar key, and — for `MetadataRef`
/// receivers — MDO fields (custom attributes, standard attributes,
/// tabular sections, register parts) from `hir::Type::fields()`.
///
/// - `Ty::ObjectManager { kind, .. }` / `Ty::ManagerCollection` — fast
///   path: only platform methods, no MDO fields.
/// - `Ty::MetadataRef { kind: TabularSection, .. }` /
///   `Ty::MetadataRef { kind: TabularSectionRow, .. }` — scalar-key path
///   returns platform methods only (the tabular row's column list comes
///   from `HirType::fields()` below).
/// - All other `Ty::MetadataRef` — merges MDO fields with platform methods.
///
/// Returns `None` for every other receiver shape.
fn complete_prefix_methods_for_receiver<DB: RootDatabase>(
    db: &DB,
    receiver_ty: &Ty,
    file_id: FileId,
    locale: ide_db::base_db::Locale,
) -> Option<Vec<CompletionItem>> {
    // Fast path: ObjectManager / ManagerCollection have no MDO fields.
    if matches!(receiver_ty, Ty::ObjectManager { .. } | Ty::ManagerCollection(_)) {
        return collect_platform_items_or_none(db, receiver_ty, locale);
    }

    // Coerce `ЭтотОбъект` so a catalog/document object module surfaces
    // attributes + tabular sections on `ЭтотОбъект.|`. Both `Type::fields`
    // and `enumerate_fields` would coerce internally, but the dispatch
    // gate below also needs the effective ty for Union recognition, so
    // we coerce once here.
    let coerced = hir::coerce_this_object_to_metadata_ref(receiver_ty);
    let effective_ty = coerced.as_ref().unwrap_or(receiver_ty);

    // MDO-field branch fires for direct `MetadataRef` receivers and for
    // unions containing at least one `MetadataRef` arm (typical shape:
    // `Найти(...) → Union(TabularSectionRow, Undefined)`). For unions
    // we still want to enumerate fields per arm (handled inside
    // `Type::fields()` via `enumerate_fields`).
    let is_union_with_metadata_ref = match effective_ty {
        Ty::Union(arms) => arms.iter().any(|a| matches!(a, Ty::MetadataRef { .. })),
        _ => false,
    };
    let is_metadata_ref = matches!(effective_ty, Ty::MetadataRef { .. });
    if !is_metadata_ref && !is_union_with_metadata_ref {
        return None;
    }

    let mdo_fields = HirType::new(db, file_id, effective_ty.clone()).fields();
    let platform_items = collect_platform_items_for_effective(db, effective_ty, locale);

    if mdo_fields.is_empty() && platform_items.is_empty() {
        return None;
    }

    let mut items: Vec<CompletionItem> =
        mdo_fields.iter().map(|f| render_mdo_field(f, locale)).collect();
    // Dedup keyed on the visible Russian label only. An MDO attribute and
    // a platform method are conceptually distinct symbols even when they
    // share an English alias (the platform method's English form is not
    // a name the user writes against the MDO), so we don't over-broaden
    // the key with bilingual filter-text tokens.
    let mut seen: std::collections::HashSet<String> =
        items.iter().map(|i| i.label.to_lowercase()).collect();
    for p in platform_items {
        if seen.insert(p.label.to_lowercase()) {
            items.push(p);
        }
    }
    tracing::debug!(
        mdo_field_count = mdo_fields.len(),
        platform_item_count = items.len(),
        "MDO+platform completion for MetadataRef receiver"
    );
    Some(items)
}

/// Wrap [`collect_platform_items`] for unions: visit every non-`Undefined`/
/// `Null` arm and merge the results, deduping by label so platform members
/// shared across arms (e.g. `Количество`) appear once. Non-union types
/// pass straight through.
fn collect_platform_items_for_effective<DB: RootDatabase>(
    db: &DB,
    effective_ty: &Ty,
    locale: ide_db::base_db::Locale,
) -> Vec<CompletionItem> {
    let Ty::Union(arms) = effective_ty else {
        return collect_platform_items(db, effective_ty, locale);
    };
    let mut out: Vec<CompletionItem> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for arm in arms.iter().filter(|t| !matches!(t, Ty::Undefined | Ty::Null)) {
        for item in collect_platform_items(db, arm, locale) {
            if seen.insert(item.label.to_lowercase()) {
                out.push(item);
            }
        }
    }
    out
}

/// Collect only platform methods/properties for a receiver — no MDO fields.
///
/// Handles four sub-cases:
/// - `TabularSection` / `TabularSectionRow` → flat-typename scalar path.
/// - Synthetic kinds with [`hir::MetadataKind::scalar_platform_key`] (e.g.
///   `RegisterFilter` → `"Filter"`) → flat-typename scalar path. Lets
///   `<recordSet>.Отбор.|` surface `Сбросить`, `Получить`, … from the
///   single `Filter` HBK row.
/// - `ObjectManager` → manager prefix.
/// - `MetadataRef` with a known [`hir::MetadataKind::platform_prefix`] →
///   manager prefix.
fn collect_platform_items<DB: RootDatabase>(
    db: &DB,
    receiver_ty: &Ty,
    locale: ide_db::base_db::Locale,
) -> Vec<CompletionItem> {
    if let Ty::MetadataRef { kind, .. } = receiver_ty {
        if let Some(scalar_key) = tabular_section_scalar_key(*kind) {
            tracing::debug!(scalar_key, "Tabular section scalar completion");
            return complete_platform_methods(db, scalar_key, locale);
        }
        if let Some(scalar_key) = kind.scalar_platform_key() {
            tracing::debug!(scalar_key, "Synthetic-kind scalar completion");
            return complete_platform_methods(db, scalar_key, locale);
        }
    }
    let prefix = match receiver_ty {
        Ty::ObjectManager { kind, .. } => kind.manager_type_prefix(),
        Ty::MetadataRef { kind, .. } => kind.platform_prefix(),
        _ => None,
    };
    let Some(prefix) = prefix else { return Vec::new() };
    tracing::debug!(prefix, "Prefix-based completion for manager / metadata-ref receiver");
    let input = TypeNameInput::new(db, prefix.to_string());
    let methods = manager_methods_query(db, input);
    methods.iter().map(render_manager_method).collect()
}

/// Thin wrapper used by the `ObjectManager` / `ManagerCollection` fast-path:
/// returns `None` when the platform item list is empty so the behaviour
/// stays identical to the pre-Phase-3 path (where no MDO branch ran).
fn collect_platform_items_or_none<DB: RootDatabase>(
    db: &DB,
    receiver_ty: &Ty,
    locale: ide_db::base_db::Locale,
) -> Option<Vec<CompletionItem>> {
    let items = collect_platform_items(db, receiver_ty, locale);
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

/// Pick the flat platform `type_name` for a TabularSection / row receiver.
/// Returns `None` for every other `MetadataKind` so the prefix path keeps
/// handling the dot-shaped receivers unchanged.
fn tabular_section_scalar_key(kind: hir::MetadataKind) -> Option<&'static str> {
    match kind {
        hir::MetadataKind::TabularSection { .. } => Some("Tabular section"),
        hir::MetadataKind::TabularSectionRow { .. } => Some("Line of a tabular section"),
        _ => None,
    }
}

/// Decide whether we are in `X.| ` / `X.Yyy|` position and, if so, return
/// the anchor DOT token plus the partial identifier typed after it.
///
/// The cursor-on-DOT case is trivial. The cursor-on-IDENT case walks
/// leftward over trivia (whitespace/newlines/comments) looking for a DOT
/// sibling — the same approach `new_expr_completion::is_after_new_keyword`
/// uses for the `Новый <type>` context.
///
/// Returns `None` when the cursor isn't in a member-access context;
/// callers short-circuit and let the next completion provider run.
fn resolve_dot_anchor(
    token: &SyntaxToken,
    offset: syntax::TextSize,
) -> Option<(SyntaxToken, String)> {
    if token.kind() == SyntaxKind::DOT {
        return Some((token.clone(), String::new()));
    }
    // Accept any name-token after `.` — keyword-shaped tails
    // (`Запрос.Выполнить|`) must trigger completion just like
    // `Запрос.Текст|` does. Layer B: same `is_name_token()`
    // predicate as the rest of the IDE-layer dispatch.
    if !token.kind().is_name_token() {
        return None;
    }
    let mut cur = token.prev_token();
    while let Some(t) = cur.clone() {
        if t.kind().is_trivia() {
            cur = t.prev_token();
        } else {
            break;
        }
    }
    let dot = cur.filter(|t| t.kind() == SyntaxKind::DOT)?;
    // Prefix = text from the name-token start up to the cursor. For
    // `Сп.В|` this is `"В"`; for `Сп.Вста|вить` it's `"Вста"`; for
    // `Зап.Выполнить|` it's `"Выполнить"`.
    let token_start = token.text_range().start();
    let cursor_in_token: usize = offset.checked_sub(token_start)?.into();
    let text = token.text();
    let prefix = text[..cursor_in_token.min(text.len())].to_string();
    Some((dot, prefix))
}

/// Case-insensitive starts-with match against the method's Russian *or*
/// English name, mirroring the filter in
/// `bsl_completion::complete_global_functions`. Pulls the English name off
/// the existing `PlatformData` index — the only lookup keyed by Russian
/// name that survives the M3 Task 11 facade migration.
fn apply_prefix_filter(
    items: Vec<CompletionItem>,
    prefix: &str,
    _db: &dyn RootDatabase,
) -> Vec<CompletionItem> {
    if prefix.is_empty() {
        return items;
    }
    let prefix_lower = prefix.to_lowercase();
    items
        .into_iter()
        .filter(|item| {
            let label_lc = item.label.to_lowercase();
            if label_lc.starts_with(&prefix_lower) {
                return true;
            }
            // `filter_text` carries the bilingual label (`"Массив Array"`) built
            // by `presenters::completion::render_completion_detail`; split on
            // whitespace to compare against each name individually.
            if let Some(ft) = &item.filter_text {
                ft.split_whitespace().any(|tok| tok.to_lowercase().starts_with(&prefix_lower))
            } else {
                false
            }
        })
        .collect()
}

/// Find the receiver expression node before the DOT token.
///
/// Walks up the syntax tree from DOT to find the parent expression,
/// then returns its first child (the receiver).
///
/// Handles cases like:
/// - `ident.` → parent is FIELD_EXPR, receiver is IDENT
/// - `expr.Method().` → parent is FIELD_EXPR, receiver is CALL_EXPR
fn find_receiver_expr(dot_token: &SyntaxToken) -> Option<SyntaxNode> {
    let Some(parent) = dot_token.parent() else {
        tracing::debug!("find_receiver_expr: dot has no parent");
        return None;
    };
    tracing::debug!(parent_kind = ?parent.kind(), "find_receiver_expr: DOT parent kind");

    // DOT is inside a FIELD_EXPR: the first child node is the receiver
    if parent.kind() == SyntaxKind::FIELD_EXPR {
        let child = parent.children().next();
        tracing::debug!(child_found = child.is_some(), child_kind = ?child.as_ref().map(|c| c.kind()), "find_receiver_expr: FIELD_EXPR first child");
        return child;
    }

    // Fallback: look at the previous sibling node of the DOT
    for sibling in dot_token.siblings_with_tokens(syntax::Direction::Prev).skip(1) {
        if sibling.kind() == SyntaxKind::WHITESPACE {
            continue;
        }
        if let Some(node) = sibling.as_node() {
            tracing::debug!(sibling_kind = ?node.kind(), "find_receiver_expr: fallback found node sibling");
            return Some(node.clone());
        }
        if let Some(token) = sibling.as_token() {
            tracing::debug!(token_kind = ?token.kind(), "find_receiver_expr: fallback returning token.parent()");
            return token.parent();
        }
        return None;
    }

    tracing::debug!("find_receiver_expr: fallback exhausted, returning None");
    None
}

// Former helpers `resolve_syntax_expr_type` / `resolve_call_expr_type` /
// `resolve_field_expr_type` / `resolve_ident_type` are removed by M3 Task 11.
// They duplicated the `method_lookup` pipeline via direct
// `PlatformData::instance()` access, and the entry-point now delegates to
// `Semantics::type_of_expr` (Task 9 bridge) with a small bare-identifier
// fallback covering literal type names.

/// Extract identifier text from receiver expression node.
fn extract_receiver_ident(node: &SyntaxNode) -> Option<String> {
    match node.kind() {
        SyntaxKind::IDENT | SyntaxKind::EXPR => {
            let token = node.first_token()?;
            if token.kind() == SyntaxKind::IDENT {
                return Some(token.text().to_string());
            }
            None
        }
        _ => {
            let token = node.first_token().filter(|t| t.kind() == SyntaxKind::IDENT)?;
            Some(token.text().to_string())
        }
    }
}

/// Completes exported methods from a CommonModule.
///
/// Uses module_index for O(1) name lookup, then symbol_tree for the specific module.
/// symbol_tree is typically already cached as a dependency of the open file.
fn complete_common_module_methods(
    db: &dyn RootDatabase,
    position: &CompletionPosition,
    module_name: &str,
) -> Option<Vec<CompletionItem>> {
    let source_root_input = db.file_source_root_input(position.file_id);
    let source_root_id = source_root_input.source_root_id(db);
    let module_index = db.module_index(source_root_id);

    let name = Name::new(module_name);
    let module_file_id = module_index.resolve_common_module(&name)?;

    tracing::debug!(
        module_name = %module_name,
        file_id = ?module_file_id,
        "Found CommonModule in module_index"
    );

    let module_id = hir::ModuleId::new(module_file_id);
    let symbol_tree = db.symbol_tree(module_id);

    let items: Vec<CompletionItem> = symbol_tree
        .methods()
        .filter(|m| m.is_export)
        .map(|method| render_common_module_method(db, position.file_id, &name, method))
        .collect();

    tracing::debug!(item_count = items.len(), "CommonModule completion items");

    if items.is_empty() {
        return None;
    }

    Some(items)
}

/// Renders a CommonModule method as a completion item via the unified
/// `symbol_info` pipeline.
fn render_common_module_method(
    db: &dyn RootDatabase,
    file_id: FileId,
    module_name: &Name,
    method: &MethodSymbol,
) -> CompletionItem {
    let callee =
        CalleeKind::CommonModuleMethod { module: module_name.clone(), method: method.name.clone() };
    match build_signature(db, file_id, &callee) {
        Some(sig) => item_from_signature(&sig),
        None => fallback_item(method),
    }
}

/// Wrap a [`CompletionDetail`] from `symbol_info` into the IDE's
/// [`CompletionItem`].
///
/// `kind` is derived from the signature *source* rather than from
/// `MethodKind`: platform members (including procedures) are surfaced as
/// `Method`, global procedures/functions as `Function`, and user-defined
/// items split by procedure-vs-function — matching the legacy classification
/// the editor presents.
pub(super) fn item_from_signature(sig: &SymbolSignature) -> CompletionItem {
    let detail = render_completion_detail(sig);
    let kind = match sig.source {
        SignatureSource::Platform | SignatureSource::PlatformManager => CompletionItemKind::Method,
        SignatureSource::PlatformConstructor => CompletionItemKind::Constructor,
        SignatureSource::GlobalFunction => CompletionItemKind::Function,
        SignatureSource::CommonModule | SignatureSource::ManagerModule | SignatureSource::Local => {
            match sig.kind {
                MethodKind::Function => CompletionItemKind::Function,
                MethodKind::Procedure => CompletionItemKind::Method,
            }
        }
    };
    item_from_detail(detail, kind)
}

fn item_from_detail(detail: CompletionDetail, kind: CompletionItemKind) -> CompletionItem {
    let CompletionDetail { label, detail, documentation, insert_text, filter_text } = detail;
    let documentation = if documentation.is_empty() { None } else { Some(documentation) };
    CompletionItem {
        label,
        detail: Some(detail),
        kind,
        insert_text,
        documentation,
        sort_text: None,
        filter_text,
        source: None,
    }
}

/// Minimal completion item used when `symbol_info` cannot build a signature
/// (e.g. metadata corruption). Keeps completion responsive.
fn fallback_item(method: &MethodSymbol) -> CompletionItem {
    let label = method.name.to_string();
    let kind =
        if method.is_function { CompletionItemKind::Function } else { CompletionItemKind::Method };
    let insert_text = format!("{}($0)", label);
    CompletionItem {
        label: label.clone(),
        detail: Some(label),
        kind,
        insert_text,
        documentation: None,
        sort_text: None,
        filter_text: None,
        source: None,
    }
}

/// Completes platform **members** — methods *and* properties — for a
/// receiver type.
///
/// Example: For receiver "Запрос", shows methods (`Выполнить`,
/// `УстановитьПараметр`, …) plus properties (`Текст`, `Параметры`,
/// `МенеджерВременныхТаблиц`, …). Properties are rendered with
/// `CompletionItemKind::Property` so the editor's icon and ranking
/// differ from methods; methods keep the existing insert-with-parens
/// snippet, properties insert just the label.
///
/// Keeping both lookups behind a single salsa-cached pair of queries
/// (`type_methods_query` + `type_properties_query`) means completion
/// doesn't pay for a live walk of `PlatformData` on every keystroke.
fn complete_platform_methods(
    db: &dyn RootDatabase,
    receiver_type: &str,
    locale: ide_db::base_db::Locale,
) -> Vec<CompletionItem> {
    let methods_input = TypeNameInput::new(db, receiver_type.to_string());
    let methods = type_methods_query(db, methods_input);
    let props_input = TypeNameInput::new(db, receiver_type.to_string());
    let properties = type_properties_query(db, props_input);

    tracing::debug!(
        method_count = methods.len(),
        property_count = properties.len(),
        "Found platform members"
    );

    let mut items: Vec<CompletionItem> = methods.iter().map(render_platform_method).collect();
    items.extend(properties.iter().map(|p| render_platform_property(p, locale)));
    items
}

/// Renders a platform manager method (`Справочники.Склады.НайтиПоКоду`, …) as
/// a completion item via the unified `symbol_info` pipeline.
///
/// Manager methods carry `name="<Имя"` in platform data; the real Russian
/// name lives in `MethodDocs.syntax` and is recovered by `from_platform_method`.
pub(super) fn render_manager_method(method: &PlatformMethod) -> CompletionItem {
    let docs = PlatformDataInner::instance().get_method_docs(method.id);
    let mut sig = from_platform_method(method, docs.as_ref());
    sig.source = SignatureSource::PlatformManager;
    item_from_signature(&sig)
}

/// Renders a platform method as a completion item via the unified
/// `symbol_info` pipeline.
pub(super) fn render_platform_method(method: &PlatformMethod) -> CompletionItem {
    let docs = PlatformDataInner::instance().get_method_docs(method.id);
    let sig = from_platform_method(method, docs.as_ref());
    item_from_signature(&sig)
}

/// Render a platform property as a completion item.
///
/// Unlike methods, properties don't go through the `symbol_info` signature
/// pipeline: there are no parameters to format, no parentheses to insert,
/// and their `detail` only needs the value-type summary plus the optional
/// `[Только чтение]` / `[Read-only]` marker (driven by `locale`). Keeping
/// the renderer local keeps the property item shape obvious at the call
/// site.
///
/// - `label` — Russian name (primary display).
/// - `filter_text` — `"{russian} {english}"` so typing either language
///   narrows the list (same shape as `symbol_info::render_completion_detail`
///   builds for methods).
/// - `detail` — `"{property_types} [Только чтение?]"`, e.g.
///   `"Структура [Только чтение]"` or `"Строка"`.
/// - `insert_text` — just the Russian name. Properties have no parens.
/// - `kind` — `CompletionItemKind::Property`, distinguishing them from
///   methods in the editor's completion popup.
pub(super) fn render_platform_property(
    prop: &PlatformProperty,
    locale: ide_db::base_db::Locale,
) -> CompletionItem {
    let type_summary = if prop.property_types.is_empty() {
        String::from("Произвольный")
    } else {
        prop.property_types.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
    };
    let detail = if prop.is_readonly {
        format!("{type_summary} {}", read_only_marker(locale))
    } else {
        type_summary
    };

    let documentation = PlatformDataInner::instance()
        .get_property_docs(prop.id)
        .map(|d| {
            let mut out = d.description;
            if let Some(notes) = d.notes {
                if !out.is_empty() {
                    out.push_str("\n\n");
                }
                out.push_str(&notes);
            }
            out
        })
        .filter(|s| !s.is_empty());

    CompletionItem {
        label: prop.name.to_string(),
        detail: Some(detail),
        kind: CompletionItemKind::Property,
        insert_text: prop.name.to_string(),
        documentation,
        sort_text: None,
        filter_text: Some(format!("{} {}", prop.name, prop.english_name)),
        source: None,
    }
}

/// Render an MDO field (custom attribute, standard attribute, tabular
/// section, or register part) as a completion item.
///
/// - `kind` is always `Field` regardless of origin — `Property` is reserved
///   for read-only platform properties on value-type receivers.
/// - `filter_text` carries both names so the user can type either the Russian
///   or English identifier to narrow the list.
/// - `sort_text` uses a short prefix (`"10_"`, `"20_"`, …) so MDO fields
///   sort before platform methods in the popup: user attributes first, then
///   tabular sections, then standard attributes, then register parts.
fn render_mdo_field(field: &Field, locale: ide_db::base_db::Locale) -> CompletionItem {
    let filter_text = format!("{} {}", field.name, field.english_name);
    CompletionItem {
        label: field.name.to_string(),
        detail: Some(render_field_detail(&field.ty, field.is_readonly, locale)),
        kind: CompletionItemKind::Field,
        insert_text: field.name.to_string(),
        documentation: None,
        sort_text: Some(sort_key_for_origin(field.origin).to_string()),
        filter_text: Some(filter_text),
        source: None,
    }
}

/// Build the `detail` string for an MDO field.
///
/// - TabularSection fields render with the locale-aware kind label
///   (`"ТабличнаяЧасть"` / `"TabularSection"`).
/// - Other `MetadataRef` fields render as `"<KindLabel>.<Name>"` via
///   [`hir::MetadataKind::display_label`] so hover and completion stay
///   aligned in either locale (no more silent `CatalogRef.Товары` leak
///   into a Russian IDE).
/// - Primitive types (`Число`, `Строка`, …) render via `Ty::display_name`
///   in the chosen locale.
/// - Appends `" [Только чтение]"` / `" [Read-only]"` for read-only
///   fields, mirroring the marker [`render_platform_property`] uses.
fn render_field_detail(ty: &Ty, is_readonly: bool, locale: ide_db::base_db::Locale) -> String {
    use hir::MetadataKind;
    let body = match ty {
        Ty::MetadataRef { kind, name } => {
            if matches!(kind, MetadataKind::TabularSection { .. }) {
                kind.display_label(locale).to_string()
            } else {
                format!("{}.{}", kind.display_label(locale), name.as_str())
            }
        }
        _ => ty.display_name(locale).to_string(),
    };
    if is_readonly {
        format!("{body} {}", read_only_marker(locale))
    } else {
        body
    }
}

/// Locale-aware "[Только чтение]" / "[Read-only]" marker shared by
/// platform-property and MDO-field completion details.
///
/// Centralised so the two renderers don't drift if either tweaks the
/// punctuation later.
fn read_only_marker(locale: ide_db::base_db::Locale) -> &'static str {
    match locale {
        ide_db::base_db::Locale::Ru => "[Только чтение]",
        ide_db::base_db::Locale::En => "[Read-only]",
    }
}

/// Sort-text prefix for MDO field origins.
///
/// Lower prefix → item appears higher in the sorted list.
/// Ordering: user attributes (most relevant) → tabular sections →
/// standard attributes → row columns → register parts →
/// platform properties (usually at the bottom).
fn sort_key_for_origin(origin: HirFieldOrigin) -> &'static str {
    match origin {
        HirFieldOrigin::UserAttribute => "10_",
        HirFieldOrigin::TabularSection => "20_",
        HirFieldOrigin::StandardAttribute => "30_",
        HirFieldOrigin::TabularSectionRowColumn => "40_",
        HirFieldOrigin::RegisterDimension
        | HirFieldOrigin::RegisterResource
        | HirFieldOrigin::RegisterAttribute => "50_",
        HirFieldOrigin::PlatformProperty => "60_",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_platform::{ContextAvailability, MethodParam, PlatformMethod};

    fn create_test_method() -> PlatformMethod {
        PlatformMethod {
            id: 999999, // Use invalid ID to ensure fallback to basic docs
            type_name: "Строка".into(),
            name: "ВРег".into(),
            english_name: "Upper".into(),
            return_type: Some("Строка".into()),
            parameters: vec![MethodParam {
                name: "Значение".into(),
                param_type: Some("Строка".into()),
                is_optional: false,
                is_variadic: false,
            }],
            variants: Vec::new(),
            min_version: Some("8.0".into()),
            context: Some(ContextAvailability {
                thick_client: true,
                thin_client: true,
                web_client: true,
                server: true,
                mobile_client: false,
                external_connection: true,
            }),
        }
    }

    #[test]
    fn test_render_platform_method() {
        // Public surface check: render produces a sensibly-shaped completion
        // item. Format details are tested in `symbol_info::presenters`.
        let method = create_test_method();
        let item = render_platform_method(&method);

        assert_eq!(item.label, "ВРег");
        assert_eq!(item.kind, CompletionItemKind::Method);
        assert!(item.insert_text.starts_with("ВРег("));
        assert!(item.insert_text.ends_with("$0)"));
        let detail = item.detail.as_deref().unwrap_or("");
        assert!(
            detail.contains("ВРег") && detail.contains("Строка"),
            "Detail should contain method name and parameter type, got: {detail}"
        );
    }

    #[test]
    fn test_end_to_end_platform_completion() {
        use bsl_platform::PlatformDataInner;
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use syntax::TextSize;
        use vfs::{FileId, FileSet, VfsPath};

        // Skip if no platform data available
        let data = PlatformDataInner::instance();
        if data.all_types().is_empty() || data.all_methods().is_empty() {
            println!("Skipping test: no platform data available");
            return;
        }

        // Create database and add file
        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);

        // BSL code with cursor after DOT
        // Use "XBase" type which has 30 methods in platform data
        let code = r#"Процедура Тест()
    Результат = XBase.
КонецПроцедуры"#;

        // Set file content + source root. The CommonModule fast-path in
        // `platform_completions` queries `module_index` via
        // `file_source_root_input`, so even tests that hit the "platform
        // type" path need a source root wired up.
        db.set_file_text(file_id, code);
        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
        db.set_file_source_root(file_id, SourceRootId(0));

        // Position is right after the DOT (end of "XBase.")
        // We want left_biased to catch the DOT token
        let dot_end = code.find("XBase.").unwrap() + "XBase.".len();
        let offset = TextSize::from(dot_end as u32);

        // Request completions at the DOT position
        let position = CompletionPosition {
            file_id,
            offset,
            workspace_root: None,
            locale: ide_db::base_db::Locale::Ru,
        };

        let items = platform_completions(&db, position);

        // Should have platform method completions
        assert!(items.is_some(), "Expected platform completions after DOT on XBase type");

        let items = items.unwrap();
        assert!(!items.is_empty(), "Expected at least one method completion");

        // Split by kind — the completion list now mixes methods (with
        // parenthesised snippets) and properties (bare labels). Each kind
        // is checked separately; both may be empty for rarely-used types,
        // but at least one must be present.
        let methods: Vec<&CompletionItem> =
            items.iter().filter(|i| i.kind == CompletionItemKind::Method).collect();
        let properties: Vec<&CompletionItem> =
            items.iter().filter(|i| i.kind == CompletionItemKind::Property).collect();
        assert_eq!(
            methods.len() + properties.len(),
            items.len(),
            "Unexpected CompletionItemKind in platform member completion"
        );
        println!("Found {} method + {} property completions", methods.len(), properties.len());

        // Methods must carry the paren-snippet with a tail-$0 cursor.
        for item in methods {
            assert!(
                item.insert_text.ends_with("$0)"),
                "Method snippet should end with $0): {}",
                item.insert_text
            );
            assert!(
                item.insert_text.contains('(') && item.insert_text.contains(')'),
                "Method snippet should have parentheses: {}",
                item.insert_text
            );
        }
        // Properties insert just the label — no parens, no snippet cursor.
        for item in properties {
            assert!(
                !item.insert_text.contains('('),
                "Property insert_text must not contain '(': {}",
                item.insert_text
            );
            assert_eq!(item.insert_text, item.label);
        }
    }

    #[test]
    fn test_completion_after_platform_global() {
        // `ОбработкаОшибок.|` — receiver is a platform global of type
        // `МенеджерОбработкиОшибок`. Without the global-scope fallback the
        // dispatcher would mistakenly treat `ОбработкаОшибок` as the type
        // name and surface zero methods. With the fix the receiver type
        // collapses to `Ty::PlatformObject("МенеджерОбработкиОшибок")` and
        // the manager's methods (e.g. `КраткоеПредставлениеОшибки`) appear.
        use bsl_platform::PlatformDataInner;
        use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
        use ide_db::RootDatabaseImpl;
        use syntax::TextSize;
        use vfs::{FileId, FileSet, VfsPath};

        let data = PlatformDataInner::instance();
        if data.all_global_properties().is_empty() {
            println!("Skipping: no platform global properties available");
            return;
        }

        let mut db = RootDatabaseImpl::new();
        let file_id = FileId(0);
        let code = r#"Процедура Тест()
    Текст = ОбработкаОшибок.
КонецПроцедуры"#;

        db.set_file_text(file_id, code);
        let mut file_set = FileSet::default();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
        db.set_file_source_root(file_id, SourceRootId(0));

        let dot_end = code.find("ОбработкаОшибок.").unwrap() + "ОбработкаОшибок.".len();
        let offset = TextSize::from(dot_end as u32);
        let position = CompletionPosition {
            file_id,
            offset,
            workspace_root: None,
            locale: ide_db::base_db::Locale::Ru,
        };

        let items = platform_completions(&db, position).expect(
            "platform_completions must surface МенеджерОбработкиОшибок methods after global property",
        );
        assert!(!items.is_empty(), "expected at least one method, got empty list");

        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"КраткоеПредставлениеОшибки"),
            "expected КраткоеПредставлениеОшибки in {:?}",
            labels
        );
    }
}
