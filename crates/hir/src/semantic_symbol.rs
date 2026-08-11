use std::cell::{OnceCell, RefCell};
use std::sync::Arc;

use crate::{Definition, Name, NameClass, ReferenceScope, Semantics};
use bsl_types::builders::Builders;
use bsl_types::kind::{TypeId, TypeKind};
use hir_def::scope::{ExprScopes, ScopeDef};
use hir_def::{
    BindingId, DefDatabase, DefWithBodyId, ExprId, MethodId, ModuleBodies, ModuleId, VariableId,
};
use hir_ty::narrow::{NarrowExprIndex, NarrowState};
use hir_ty::{db::HirDatabase, ImplicitLocalInfo};
use rustc_hash::FxHashMap;
use stdx::case::{fold_lower_per_char, CaseExt};
use syntax::ast::{self, AstNode};
use syntax::{SyntaxKind, TextRange, TextSize};
use vfs::FileId;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SemanticSymbolKey {
    Definition(Definition),
    BodyLocal { file_id: FileId, owner: DefWithBodyId, name_lower: String },
    ImplicitLocal { file_id: FileId, owner: DefWithBodyId, name_lower: String, declaration: ExprId },
    TypedMember { file_id: FileId, range: TextRange },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticSymbolKind {
    Function,
    Method,
    Parameter,
    Variable,
    Property,
    Type,
    Namespace,
    Class,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticSymbol {
    pub key: SemanticSymbolKey,
    pub name: Name,
    pub kind: SemanticSymbolKind,
    pub definition: Option<Definition>,
    pub declaration: Option<SymbolDeclaration>,
    pub ty: Option<TypeId>,
}

impl SemanticSymbol {
    pub fn reference_scope(&self, db: &dyn DefDatabase) -> ReferenceScope {
        if let Some(def) = self.definition.as_ref() {
            return def.reference_scope(db);
        }
        match &self.key {
            SemanticSymbolKey::BodyLocal { .. } | SemanticSymbolKey::ImplicitLocal { .. } => {
                ReferenceScope::FileLocal
            }
            SemanticSymbolKey::TypedMember { .. } | SemanticSymbolKey::Definition(_) => {
                ReferenceScope::Unknown
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolDeclaration {
    pub file_id: FileId,
    pub range: TextRange,
    pub name: Name,
    pub kind: SemanticSymbolKind,
}

impl<'db, DB: HirDatabase + base_db::RootQueryDb> Semantics<'db, DB> {
    pub fn symbol_at(&self, file_id: FileId, offset: TextSize) -> Option<SemanticSymbol> {
        let parse = self.db.parse(file_id);
        let root = parse.syntax_node();
        let token = root.token_at_offset(offset).right_biased()?;
        self.symbol_for_token(file_id, &token)
    }

    pub fn symbol_for_token(
        &self,
        file_id: FileId,
        token: &syntax::SyntaxToken,
    ) -> Option<SemanticSymbol> {
        FileSymbolCtx::new(self.db, file_id).symbol_for_token(token)
    }
}

/// Per-file symbol resolution that shares lookup state across tokens.
///
/// `Semantics::symbol_for_token` re-derives everything from the database on
/// every call, which is fine for a single hover but quadratic when a caller
/// resolves every name token of a file (semantic highlighting): each token
/// re-scanned all method bodies (deep-cloning the matched one), re-built
/// `ExprScopes`, and re-collected the global common-module exports. This
/// context is built once per file and reuses those structures across tokens.
/// The one-shot `Semantics` entry points delegate here, so both paths resolve
/// identically.
pub struct FileSymbolCtx<'db, DB: HirDatabase + base_db::RootQueryDb> {
    sema: Semantics<'db, DB>,
    file_id: FileId,
    module_id: ModuleId,
    module_bodies: Arc<ModuleBodies>,
    /// Method-def syntax range → item-tree top-level index, which is also the
    /// lowered body's local id (both count every top-level item in document
    /// order).
    method_ranges: FxHashMap<TextRange, u32>,
    /// When false, explicit binding symbols skip type inference: highlighting
    /// never reads `SemanticSymbol::ty`, and the binding type is the only
    /// symbol field that forces inference for otherwise-syntactic tokens.
    binding_types: bool,
    /// Per-owner binding buckets keyed by `fold_lower_per_char` — the key
    /// equality that matches `eq_ignore_case` — keeping the first binding in
    /// iteration order, like the linear `find` it replaces.
    owner_bindings: RefCell<FxHashMap<DefWithBodyId, Arc<FxHashMap<String, BindingId>>>>,
    expr_scopes: RefCell<FxHashMap<u32, Arc<ExprScopes>>>,
    /// Memoized scope resolutions keyed by the token text as written, so a
    /// cached `Definition` always embeds the requested casing.
    local_defs: RefCell<FxHashMap<(u32, String), Option<Definition>>>,
    module_methods: RefCell<FxHashMap<String, Option<MethodId>>>,
    module_vars: RefCell<FxHashMap<String, Option<VariableId>>>,
    global_exports: OnceCell<FxHashMap<String, MethodId>>,
    /// Narrowing dataflow per owner. `narrow_or_base` re-runs the whole
    /// dataflow solve on every call (`db.narrow` is not a tracked query), so
    /// resolving it per path token is quadratic in the body size; the paired
    /// index replaces `containing_vertex`'s per-lookup CFG scan.
    narrow_cache: RefCell<FxHashMap<DefWithBodyId, NarrowEntry>>,
}

type NarrowEntry = Option<(Arc<dataflow::DataflowResult<NarrowState>>, Arc<NarrowExprIndex>)>;

impl<'db, DB: HirDatabase + base_db::RootQueryDb> FileSymbolCtx<'db, DB> {
    pub fn new(db: &'db DB, file_id: FileId) -> Self {
        let module_id = ModuleId::new(file_id);
        let module_bodies = db.module_bodies(module_id);
        let tree = db.item_tree(file_id);
        let mut method_ranges = FxHashMap::default();
        for (idx, item) in tree.top_level_items().iter().enumerate() {
            let source_range = match item {
                hir_def::item_tree::ModItem::Procedure(proc_idx) => {
                    tree.procedure(*proc_idx).source_range
                }
                hir_def::item_tree::ModItem::Function(func_idx) => {
                    tree.function(*func_idx).source_range
                }
                _ => continue,
            };
            method_ranges.entry(source_range).or_insert(idx as u32);
        }
        Self {
            sema: Semantics::new(db),
            file_id,
            module_id,
            module_bodies,
            method_ranges,
            binding_types: true,
            owner_bindings: RefCell::new(FxHashMap::default()),
            expr_scopes: RefCell::new(FxHashMap::default()),
            local_defs: RefCell::new(FxHashMap::default()),
            module_methods: RefCell::new(FxHashMap::default()),
            module_vars: RefCell::new(FxHashMap::default()),
            global_exports: OnceCell::new(),
            narrow_cache: RefCell::new(FxHashMap::default()),
        }
    }

    /// Skip inferring explicit binding types (`SemanticSymbol::ty` stays
    /// `None` for them). For callers that never read the type.
    pub fn without_binding_types(mut self) -> Self {
        self.binding_types = false;
        self
    }

    fn db(&self) -> &'db DB {
        self.sema.db
    }

    pub fn symbol_for_token(&self, token: &syntax::SyntaxToken) -> Option<SemanticSymbol> {
        match crate::classify_token(token) {
            NameClass::FreeName { token } => self.symbol_for_free_name(&token),
            NameClass::FieldName { receiver, token, is_call } => {
                self.symbol_for_field_name(&receiver, &token, is_call)
            }
            NameClass::TypeRef { token } => self.symbol_for_type_ref(&token),
            NameClass::Literal { .. } | NameClass::Keyword { .. } | NameClass::Other => None,
        }
    }

    fn symbol_for_free_name(&self, token: &syntax::SyntaxToken) -> Option<SemanticSymbol> {
        // A global common module export shadows a same-named platform global, but a local or
        // same-module symbol wins — the helper gates on those. Checked before the builtin
        // short-circuit so Local → Module → Global-CM → Platform holds for goto/hover/refs too.
        if let Some(definition) = self.global_export_definition(token) {
            return Some(symbol_from_definition(self.db(), definition, None));
        }

        if let Some(definition) = self.sema.try_resolve_builtin(token.text()) {
            return Some(symbol_from_definition(self.db(), definition, None));
        }

        if let Some(symbol) = self.symbol_for_body_local(token) {
            return Some(symbol);
        }

        let definition = self.resolve_name_to_definition(token)?;
        Some(symbol_from_definition(self.db(), definition, None))
    }

    fn symbol_for_field_name(
        &self,
        receiver: &syntax::SyntaxNode,
        token: &syntax::SyntaxToken,
        is_call: bool,
    ) -> Option<SemanticSymbol> {
        let method = || {
            self.resolve_method_call_to_definition(token)
                .map(|definition| symbol_from_definition(self.db(), definition, None))
        };
        let property = || self.symbol_for_typed_property(receiver, token);

        if is_call {
            method().or_else(property).or_else(|| {
                self.resolve_name_to_definition(token)
                    .map(|definition| symbol_from_definition(self.db(), definition, None))
            })
        } else {
            property().or_else(method).or_else(|| {
                self.resolve_name_to_definition(token)
                    .map(|definition| symbol_from_definition(self.db(), definition, None))
            })
        }
    }

    fn symbol_for_typed_property(
        &self,
        receiver: &syntax::SyntaxNode,
        token: &syntax::SyntaxToken,
    ) -> Option<SemanticSymbol> {
        let receiver_id = self.type_of_expr(receiver);
        if matches!(self.db().lookup_type(receiver_id), TypeKind::Unknown) {
            return None;
        }

        let obj_resolver = hir_ty::DbObjectResolver::new(self.db(), self.file_id);
        let name = Name::new(token.text());
        let field = hir_ty::lookup_field(self.db(), &obj_resolver, receiver_id, &name)?;
        Some(SemanticSymbol {
            key: SemanticSymbolKey::TypedMember {
                file_id: self.file_id,
                range: token.text_range(),
            },
            name: field.name,
            kind: SemanticSymbolKind::Property,
            definition: None,
            declaration: None,
            ty: Some(field.ty),
        })
    }

    fn symbol_for_type_ref(&self, token: &syntax::SyntaxToken) -> Option<SemanticSymbol> {
        let definition = self.resolve_name_to_definition(token);
        definition.map(|definition| symbol_from_definition(self.db(), definition, None))
    }

    pub fn resolve_method_call_to_definition(
        &self,
        token: &syntax::SyntaxToken,
    ) -> Option<Definition> {
        if !token.kind().is_name_token() {
            return None;
        }

        let receiver_node = crate::field_name_receiver(token)?;
        let receiver_id = self.type_of_expr(&receiver_node);
        if matches!(self.db().lookup_type(receiver_id), TypeKind::Unknown) {
            return None;
        }

        let method_name = Name::new(token.text());
        let resolution = hir_ty::resolve_method(self.db(), receiver_id, &method_name)?;

        Some(Definition::BuiltinMethodHandle { handle: resolution.handle, method_name })
    }

    pub fn resolve_name_to_definition(&self, token: &syntax::SyntaxToken) -> Option<Definition> {
        let _span = tracing::info_span!("resolve_name_to_definition").entered();

        if token.kind() != SyntaxKind::IDENT && crate::field_name_receiver(token).is_none() {
            return None;
        }

        let token_text = token.text();
        let name = Name::new(token_text);

        if let Some(def) = self.sema.try_resolve_qualified_name_for_token(self.file_id, token) {
            tracing::debug!(?def, "resolved as qualified name");
            return Some(def);
        }

        if crate::field_name_receiver(token).is_some() {
            tracing::debug!("skipping free-name resolution: token is field-name in FIELD_EXPR");
            return None;
        }

        // A global common module export extends the global context and so shadows a
        // same-named platform global. Resolved before builtins to keep Local → Module →
        // Global-CM → Platform consistent with name inference and signature help; the helper
        // gates on nearer scopes (local/parameter, same-module method/variable) missing.
        if let Some(def) = self.global_export_definition(token) {
            tracing::debug!(?def, "resolved as global common module export");
            return Some(def);
        }

        if let Some(def) = self.sema.try_resolve_builtin(token_text) {
            tracing::debug!(?def, "resolved as builtin");
            return Some(def);
        }

        if let Some(def) = self.resolve_local_to_definition(token) {
            tracing::debug!(?def, "resolved as local symbol");
            return Some(def);
        }

        if let Some(method_id) = self.module_method(&name) {
            tracing::debug!(?method_id, "resolved as module method");
            return Some(Definition::Method(method_id));
        }

        if let Some(var_id) = self.module_variable(&name) {
            tracing::debug!(?var_id, "resolved as module variable");
            return Some(Definition::Variable(var_id));
        }

        // Last, because it is a PLATFORM global: a module method or variable named
        // like a metadata plural holds the name, and asking the collection first
        // made its own declaration unreachable from its uses. Same Local → Module →
        // Global order the comment above states for global common-module exports.
        if bsl_metadata::MdoType::is_plural_form(token_text) {
            if let Some(mdo_type) = bsl_metadata::MdoType::from_plural(token_text) {
                tracing::debug!(?mdo_type, "resolved as MDO collection");
                return Some(Definition::MdoCollectionType(mdo_type));
            }
        }

        tracing::debug!("unresolved identifier: {}", token_text);
        None
    }

    /// See `Semantics::global_export_definition`'s shadow contract: a nearer
    /// scope (local/parameter, same-module method or variable) wins over the
    /// global common-module export.
    fn global_export_definition(&self, token: &syntax::SyntaxToken) -> Option<Definition> {
        let name = Name::new(token.text());
        let shadowed = self.resolve_local_to_definition(token).is_some()
            || self.module_method(&name).is_some()
            || self.module_variable(&name).is_some();
        if shadowed {
            return None;
        }

        self.global_exports()
            .get(&fold_lower_per_char(token.text()))
            .map(|method_id| Definition::Method(*method_id))
    }

    fn global_exports(&self) -> &FxHashMap<String, MethodId> {
        self.global_exports.get_or_init(|| {
            let mut map = FxHashMap::default();
            let exports = hir_def::resolver::Resolver::with_workspace_scope(self.module_id)
                .global_common_module_exports(self.db());
            // First occurrence wins, matching the linear `find` this replaces
            // (and the deterministic-winner contract of the export list).
            for entry in exports.entries {
                if let hir_def::resolver::GlobalExportDefinition::Method(method_id) =
                    entry.definition
                {
                    if entry.capabilities.callable == Some(true) {
                        map.entry(fold_lower_per_char(entry.name.as_str())).or_insert(method_id);
                    }
                }
            }
            map
        })
    }

    fn module_method(&self, name: &Name) -> Option<MethodId> {
        if let Some(hit) = self.module_methods.borrow().get(name.as_str()) {
            return *hit;
        }
        let resolver = hir_def::resolver::Resolver::for_module(self.module_id);
        let result = resolver.resolve_module_method(self.db(), name);
        self.module_methods.borrow_mut().insert(name.as_str().to_string(), result);
        result
    }

    fn module_variable(&self, name: &Name) -> Option<VariableId> {
        if let Some(hit) = self.module_vars.borrow().get(name.as_str()) {
            return *hit;
        }
        let resolver = hir_def::resolver::Resolver::for_module(self.module_id);
        let result = resolver.resolve_module_variable(self.db(), name);
        self.module_vars.borrow_mut().insert(name.as_str().to_string(), result);
        result
    }

    fn resolve_local_to_definition(&self, token: &syntax::SyntaxToken) -> Option<Definition> {
        let (method_node, local_id) = self.enclosing_method(token)?;
        let key = (local_id, token.text().to_string());
        if let Some(hit) = self.local_defs.borrow().get(&key) {
            return hit.clone();
        }
        let result = self.resolve_local_uncached(&method_node, local_id, &Name::new(token.text()));
        self.local_defs.borrow_mut().insert(key, result.clone());
        result
    }

    fn resolve_local_uncached(
        &self,
        method_node: &syntax::SyntaxNode,
        local_id: u32,
        name: &Name,
    ) -> Option<Definition> {
        let scopes = self.scopes_for(method_node, local_id)?;
        let scope_def = scopes.resolve_name(scopes.root_scope(), name)?;

        let tree = self.db().item_tree(self.file_id);
        let params = match tree.top_level_items().get(local_id as usize)? {
            hir_def::item_tree::ModItem::Procedure(proc_idx) => &tree.procedure(*proc_idx).params,
            hir_def::item_tree::ModItem::Function(func_idx) => &tree.function(*func_idx).params,
            _ => return None,
        };
        let method_id = MethodId { module: self.module_id, local_id };
        Some(match scope_def {
            ScopeDef::Parameter => {
                let param_index =
                    params.iter().position(|p| p.name.eq_ignore_case(name)).unwrap_or(0) as u32;
                Definition::Parameter { method_id, param_name: name.clone(), param_index }
            }
            ScopeDef::LocalVariable => Definition::Local { method_id, var_name: name.clone() },
        })
    }

    fn scopes_for(
        &self,
        method_node: &syntax::SyntaxNode,
        local_id: u32,
    ) -> Option<Arc<ExprScopes>> {
        if let Some(scopes) = self.expr_scopes.borrow().get(&local_id) {
            return Some(scopes.clone());
        }
        let scopes = if let Some(proc_def) = ast::ProcedureDef::cast(method_node.clone()) {
            ExprScopes::from_procedure(&proc_def)
        } else {
            let func_def = ast::FunctionDef::cast(method_node.clone())?;
            ExprScopes::from_function(&func_def)
        };
        let scopes = Arc::new(scopes);
        self.expr_scopes.borrow_mut().insert(local_id, scopes.clone());
        Some(scopes)
    }

    /// The nearest enclosing method definition, if the item tree knows it.
    /// Outer definitions are not consulted: a name scoped to an unknown inner
    /// definition must not resolve against an enclosing one.
    fn enclosing_method(&self, token: &syntax::SyntaxToken) -> Option<(syntax::SyntaxNode, u32)> {
        let mut node = token.parent()?;
        loop {
            if matches!(node.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF) {
                let local_id = *self.method_ranges.get(&node.text_range())?;
                return Some((node, local_id));
            }
            node = node.parent()?;
        }
    }

    fn symbol_for_body_local(&self, token: &syntax::SyntaxToken) -> Option<SemanticSymbol> {
        let file_id = self.file_id;
        let (owner, body, source_map) = self.body_for_token(token)?;
        let name = Name::new(token.text());
        let name_lower = name.as_str().fold_lower();

        if let Some(binding_id) =
            self.owner_bindings(owner, body).get(&fold_lower_per_char(token.text())).copied()
        {
            let binding = body.binding(binding_id);
            let range = source_map.binding_range(binding_id)?;
            let is_param = body.params().any(|param_id| param_id == binding_id);
            let kind =
                if is_param { SemanticSymbolKind::Parameter } else { SemanticSymbolKind::Variable };
            let ty = if self.binding_types {
                crate::infer_owner(self.db(), file_id, owner).type_id_of_binding(binding_id)
            } else {
                None
            };
            return Some(SemanticSymbol {
                key: SemanticSymbolKey::BodyLocal { file_id, owner, name_lower },
                name: binding.name.clone(),
                kind,
                definition: None,
                declaration: Some(SymbolDeclaration {
                    file_id,
                    range,
                    name: binding.name.clone(),
                    kind,
                }),
                ty,
            });
        }

        let occurrence_expr = source_map.expr_at_range(token.text_range())?;
        let routed = crate::infer_owner(self.db(), file_id, owner);
        let implicit = routed.implicit_locals().get(&name_lower)?;
        let unknown = self.db().unknown();
        let occurrence_ty = routed.type_id_of_expr(occurrence_expr).unwrap_or(unknown);
        let (declaration, range, ty) = select_implicit_local_declaration(
            source_map,
            implicit,
            token.text_range(),
            occurrence_ty,
            unknown,
        )?;
        Some(SemanticSymbol {
            key: SemanticSymbolKey::ImplicitLocal { file_id, owner, name_lower, declaration },
            name: implicit.name.clone(),
            kind: SemanticSymbolKind::Variable,
            definition: None,
            declaration: Some(SymbolDeclaration {
                file_id,
                range,
                name: implicit.name.clone(),
                kind: SemanticSymbolKind::Variable,
            }),
            ty: Some(ty),
        })
    }

    fn owner_bindings(
        &self,
        owner: DefWithBodyId,
        body: &hir_def::Body,
    ) -> Arc<FxHashMap<String, BindingId>> {
        if let Some(bindings) = self.owner_bindings.borrow().get(&owner) {
            return bindings.clone();
        }
        let mut map = FxHashMap::default();
        for (binding_id, binding) in body.bindings_iter() {
            map.entry(fold_lower_per_char(binding.name.as_str())).or_insert(binding_id);
        }
        let map = Arc::new(map);
        self.owner_bindings.borrow_mut().insert(owner, map.clone());
        map
    }

    fn type_of_expr(&self, node: &syntax::SyntaxNode) -> TypeId {
        let range = node.text_range();
        if let Some((owner, body, source_map)) = self.body_for_node(node.clone()) {
            if let Some(expr_id) = source_map.expr_at_range(range) {
                let routed = crate::infer_owner(self.db(), self.file_id, owner);
                let base_id =
                    routed.type_id_of_expr(expr_id).unwrap_or_else(|| self.db().unknown());
                return self.narrow_or_base_cached(owner, body, expr_id, base_id);
            }
        }
        self.db().unknown()
    }

    /// `narrow_or_base` through the per-owner cache: one dataflow solve and
    /// one expression index per body, however many tokens resolve against it.
    fn narrow_or_base_cached(
        &self,
        owner: DefWithBodyId,
        body: &hir_def::Body,
        expr_id: ExprId,
        base: TypeId,
    ) -> TypeId {
        if !self.db().type_narrowing_enabled() {
            return base;
        }
        if !matches!(body.expr(expr_id), hir_def::hir::Expr::Path(_)) {
            return base;
        }
        let Some((result, index)) = self.narrow_for(owner, body) else {
            return base;
        };
        crate::narrow_or_base_indexed(self.db(), body, &result, &index, expr_id, base)
    }

    fn narrow_for(&self, owner: DefWithBodyId, body: &hir_def::Body) -> NarrowEntry {
        if let Some(hit) = self.narrow_cache.borrow().get(&owner) {
            return hit.clone();
        }
        let entry = self.db().narrow(self.file_id, owner).map(|result| {
            let index = Arc::new(NarrowExprIndex::build(body, result.cfg()));
            (result, index)
        });
        self.narrow_cache.borrow_mut().insert(owner, entry.clone());
        entry
    }

    fn body_for_token(
        &self,
        token: &syntax::SyntaxToken,
    ) -> Option<(DefWithBodyId, &hir_def::Body, &hir_def::BodySourceMap)> {
        self.body_for(token.text_range(), token.parent()?)
    }

    fn body_for_node(
        &self,
        node: syntax::SyntaxNode,
    ) -> Option<(DefWithBodyId, &hir_def::Body, &hir_def::BodySourceMap)> {
        self.body_for(node.text_range(), node)
    }

    /// The lowered body a source range participates in.
    ///
    /// Module code is checked first, matching the legacy all-bodies scan
    /// order; the remaining candidates are the syntactically enclosing method
    /// definitions (method ranges are disjoint, so no other body can contain
    /// the range). A range that participates in no body — a callee name, a
    /// token in a dead preprocessor branch — resolves to `None`, as it did
    /// when every body was scanned.
    fn body_for(
        &self,
        range: TextRange,
        start: syntax::SyntaxNode,
    ) -> Option<(DefWithBodyId, &hir_def::Body, &hir_def::BodySourceMap)> {
        if let Some(result) = self.module_bodies.module_code_result() {
            if result.source_map.expr_at_range(range).is_some()
                || result.source_map.binding_at_range(range).is_some()
            {
                return Some((DefWithBodyId::ModuleCode, &result.body, &result.source_map));
            }
        }

        let mut node = Some(start);
        while let Some(current) = node {
            if matches!(current.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF) {
                if let Some(local_id) = self.method_ranges.get(&current.text_range()) {
                    if let Some(result) = self.module_bodies.lower_result(*local_id) {
                        if result.source_map.expr_at_range(range).is_some()
                            || result.source_map.binding_at_range(range).is_some()
                        {
                            return Some((
                                DefWithBodyId::Method(*local_id),
                                &result.body,
                                &result.source_map,
                            ));
                        }
                    }
                }
            }
            node = current.parent();
        }

        None
    }
}

fn select_implicit_local_declaration(
    source_map: &hir_def::BodySourceMap,
    implicit: &ImplicitLocalInfo,
    occurrence_range: TextRange,
    occurrence_ty: TypeId,
    unknown: TypeId,
) -> Option<(ExprId, TextRange, TypeId)> {
    if occurrence_ty != unknown {
        let typed_preceding = implicit
            .assignments
            .iter()
            .filter_map(|assignment| {
                if assignment.ty != occurrence_ty {
                    return None;
                }
                let range = source_map.expr_range(assignment.target)?;
                (range.start() <= occurrence_range.start()).then_some((assignment, range))
            })
            .next_back();

        if let Some((assignment, range)) = typed_preceding {
            return Some((assignment.target, range, assignment.ty));
        }
    }

    let preceding = implicit
        .assignments
        .iter()
        .filter_map(|assignment| {
            let range = source_map.expr_range(assignment.target)?;
            (range.start() <= occurrence_range.start()).then_some((assignment, range))
        })
        .next_back();

    if let Some((assignment, range)) = preceding {
        return Some((assignment.target, range, assignment.ty));
    }

    let range = source_map.expr_range(implicit.first_assignment)?;
    Some((implicit.first_assignment, range, implicit.ty))
}

fn symbol_from_definition(
    db: &dyn hir_def::DefDatabase,
    definition: Definition,
    ty: Option<TypeId>,
) -> SemanticSymbol {
    let name = definition.name(db).unwrap_or_else(Name::missing);
    let kind = kind_for_definition(&definition);
    let declaration = declaration_for_definition(db, &definition, kind);
    SemanticSymbol {
        key: SemanticSymbolKey::Definition(definition.clone()),
        name,
        kind,
        definition: Some(definition),
        declaration,
        ty,
    }
}

fn kind_for_definition(definition: &Definition) -> SemanticSymbolKind {
    match definition {
        Definition::Method(_) | Definition::BuiltinFunction(_) => SemanticSymbolKind::Function,
        Definition::BuiltinMethodHandle { .. } => SemanticSymbolKind::Method,
        Definition::Variable(_) | Definition::Local { .. } => SemanticSymbolKind::Variable,
        Definition::Parameter { .. } => SemanticSymbolKind::Parameter,
        Definition::VirtualTableField { .. } => SemanticSymbolKind::Property,
        Definition::MdoCollectionType(_) => SemanticSymbolKind::Class,
        Definition::MdoObject { .. } => SemanticSymbolKind::Type,
        Definition::MdoManagerModule { .. } | Definition::Module(_) => {
            SemanticSymbolKind::Namespace
        }
        Definition::Unresolved => SemanticSymbolKind::Variable,
    }
}

fn declaration_for_definition(
    db: &dyn hir_def::DefDatabase,
    definition: &Definition,
    kind: SemanticSymbolKind,
) -> Option<SymbolDeclaration> {
    match definition {
        Definition::Method(method_id) => declaration_for_method(db, *method_id, kind),
        Definition::Variable(var_id) => Some(SymbolDeclaration {
            file_id: var_id.module.file_id,
            range: definition.source_range(db)?,
            name: definition.name(db)?,
            kind,
        }),
        _ => None,
    }
}

fn declaration_for_method(
    db: &dyn hir_def::DefDatabase,
    method_id: MethodId,
    kind: SemanticSymbolKind,
) -> Option<SymbolDeclaration> {
    Some(SymbolDeclaration {
        file_id: method_id.module.file_id,
        range: definition_source_range(db, method_id)?,
        name: Definition::Method(method_id).name(db)?,
        kind,
    })
}

fn definition_source_range(
    db: &dyn hir_def::DefDatabase,
    method_id: MethodId,
) -> Option<TextRange> {
    Definition::Method(method_id).source_range(db)
}
