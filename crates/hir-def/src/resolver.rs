use std::sync::Arc;

use bsl_metadata::MdObject;

use crate::configs::ConfigsDatabase;
use crate::scope::{ExprScopes, ScopeId};
use crate::symbol_tree::SymbolTree;
use crate::{DefDatabase, MethodId, ModuleId, Name, PathResolution, QualifiedName, VariableId};

pub struct Resolver {
    #[doc(hidden)]
    pub scopes: Vec<Scope>,
}

#[doc(hidden)]
pub enum Scope {
    /// The enclosing module. `local_symbols` overrides same-module method/variable
    /// resolution with an explicit symbol tree (the *effective* module of an
    /// `&ИзменениеИКонтроль` extension); `None` resolves through
    /// `db.symbol_tree(module_id)` exactly as before — the byte-identical default for
    /// every ordinary module.
    ///
    /// `base_fallback` is the paired base module of a configuration-*extension* module
    /// (weaving, Phase 3): same-module method/variable lookups that miss in `module_id`'s
    /// own symbols retry against the base module's symbols, so an interceptor or new
    /// extension method that calls a base-module sibling resolves. The extension's own
    /// symbols are tried FIRST (extension shadows base); the fallback only fills misses, so
    /// it can never turn a resolved name unresolved. Mutually exclusive with `local_symbols`
    /// (the `&ИзменениеИКонтроль` effective module already contains the merged base body).
    ModuleScope {
        module_id: ModuleId,
        local_symbols: Option<Arc<SymbolTree>>,
        base_fallback: Option<ModuleId>,
    },

    ExprScope {
        scopes: Arc<ExprScopes>,
        scope_id: ScopeId,
    },

    WorkspaceScope,

    Builtins,
}

impl Resolver {
    pub fn for_module(module_id: ModuleId) -> Self {
        Resolver {
            scopes: vec![Scope::ModuleScope {
                module_id,
                local_symbols: None,
                base_fallback: None,
            }],
        }
    }

    pub fn with_workspace_scope(module_id: ModuleId) -> Self {
        Resolver {
            scopes: vec![
                Scope::WorkspaceScope,
                Scope::ModuleScope { module_id, local_symbols: None, base_fallback: None },
            ],
        }
    }

    pub fn with_builtins_and_workspace(module_id: ModuleId) -> Self {
        Resolver {
            scopes: vec![
                Scope::Builtins,
                Scope::WorkspaceScope,
                Scope::ModuleScope { module_id, local_symbols: None, base_fallback: None },
            ],
        }
    }

    /// Like [`Self::with_builtins_and_workspace`], but same-module method/variable
    /// lookups resolve against `local_symbols` (the effective module's symbol tree)
    /// instead of `db.symbol_tree(module_id)`. Cross-module / metadata resolution still
    /// keys on `module_id.file_id`, which is the base file — correct, because the
    /// effective module *is* the base module with the extension's edits applied.
    pub fn with_builtins_and_workspace_effective(
        module_id: ModuleId,
        local_symbols: Arc<SymbolTree>,
    ) -> Self {
        Resolver {
            scopes: vec![
                Scope::Builtins,
                Scope::WorkspaceScope,
                Scope::ModuleScope {
                    module_id,
                    local_symbols: Some(local_symbols),
                    base_fallback: None,
                },
            ],
        }
    }

    /// Like [`Self::with_builtins_and_workspace`], but same-module method/variable lookups
    /// that miss in `module_id`'s own symbols retry against `base_module_id` — the paired
    /// base module of a configuration-extension module (weaving, Phase 3). `module_id` stays
    /// the extension file (so configuration / metadata / cross-module resolution keys on the
    /// extension's own `file_id`, which is correct); only the bare same-module sibling lookup
    /// gains the base fallback. The extension shadows the base (own symbols tried first).
    pub fn with_builtins_and_workspace_weaving(
        module_id: ModuleId,
        base_module_id: ModuleId,
    ) -> Self {
        Resolver {
            scopes: vec![
                Scope::Builtins,
                Scope::WorkspaceScope,
                Scope::ModuleScope {
                    module_id,
                    local_symbols: None,
                    base_fallback: Some(base_module_id),
                },
            ],
        }
    }

    fn has_builtins(&self) -> bool {
        self.scopes.iter().any(|s| matches!(s, Scope::Builtins))
    }

    fn resolve_builtin(&self, name: &Name) -> Option<Name> {
        if !self.has_builtins() {
            return None;
        }

        if bsl_platform::PlatformDataInner::instance().get_global_function(name.as_str()).is_some()
        {
            Some(name.clone())
        } else {
            None
        }
    }

    fn mdo_visible(
        db: &dyn ConfigsDatabase,
        file_id: vfs::FileId,
        mdo_type: bsl_metadata::MdoType,
        mdo_name: &Name,
    ) -> bool {
        let needle = mdo_name.as_str();
        db.resolve_metadata_object(file_id, mdo_type, needle).is_some()
            || db.resolve_register(file_id, mdo_type, needle).is_some()
    }

    pub fn push_expr_scope(mut self, scopes: Arc<ExprScopes>, scope_id: ScopeId) -> Self {
        self.scopes.push(Scope::ExprScope { scopes, scope_id });
        self
    }

    pub fn module_id(&self) -> Option<ModuleId> {
        for scope in &self.scopes {
            if let Scope::ModuleScope { module_id, .. } = scope {
                return Some(*module_id);
            }
        }
        None
    }

    /// The effective module's symbol tree, when this resolver was built with one
    /// ([`Self::with_builtins_and_workspace_effective`]). `None` for every ordinary
    /// module, in which case same-module lookups fall back to `db.symbol_tree`.
    fn module_local_symbols(&self) -> Option<&Arc<SymbolTree>> {
        for scope in &self.scopes {
            if let Scope::ModuleScope { local_symbols, .. } = scope {
                return local_symbols.as_ref();
            }
        }
        None
    }

    /// The paired base module to retry same-module sibling lookups against, when this
    /// resolver was built for a configuration-extension module
    /// ([`Self::with_builtins_and_workspace_weaving`]). `None` for every other module.
    fn module_base_fallback(&self) -> Option<ModuleId> {
        for scope in &self.scopes {
            if let Scope::ModuleScope { base_fallback, .. } = scope {
                return *base_fallback;
            }
        }
        None
    }

    pub fn resolve_local(&self, name: &Name) -> Option<ResolvedLocal> {
        for scope in self.scopes.iter().rev() {
            if let Scope::ExprScope { scopes, scope_id } = scope {
                if let Some(def) = scopes.resolve_name(*scope_id, name) {
                    return Some(ResolvedLocal { def });
                }
            }
        }

        None
    }

    pub fn resolve_module_method(&self, db: &dyn DefDatabase, name: &Name) -> Option<MethodId> {
        if let Some(symbols) = self.module_local_symbols() {
            return symbols.find_method(name).map(|m| m.id);
        }
        let module_id = self.module_id()?;
        if let Some(m) = db.symbol_tree_ref(module_id).find_method(name) {
            return Some(m.id);
        }
        // Weaving: an extension method calling a base-module sibling resolves against the
        // paired base module. The returned `MethodId` carries the base module, so downstream
        // type inference uses the base method's real signature.
        let base = self.module_base_fallback()?;
        db.symbol_tree_ref(base).find_method(name).map(|m| m.id)
    }

    pub fn resolve_module_variable(&self, db: &dyn DefDatabase, name: &Name) -> Option<VariableId> {
        if let Some(symbols) = self.module_local_symbols() {
            return symbols.find_variable(name).map(|v| v.id);
        }
        let module_id = self.module_id()?;
        if let Some(v) = db.symbol_tree_ref(module_id).find_variable(name) {
            return Some(v.id);
        }
        let base = self.module_base_fallback()?;
        db.symbol_tree_ref(base).find_variable(name).map(|v| v.id)
    }

    pub fn user_common_module_exists(&self, db: &dyn ConfigsDatabase, module_name: &Name) -> bool {
        if !self.scopes.iter().any(|s| matches!(s, Scope::WorkspaceScope)) {
            return false;
        }
        let Some(module_id) = self.module_id() else { return false };
        let file_id = module_id.file_id;

        if db.has_config_root(file_id)
            && db.resolve_common_module(file_id, module_name.as_str()).is_none()
        {
            return false;
        }

        let source_root_id = db.file_source_root_input(file_id).source_root_id(db);
        db.module_index(source_root_id).resolve_common_module(module_name).is_some()
    }

    /// Resolve a bare register name (from a `Движения.<Регистр>` movement touch) to its
    /// metadata type and canonical name. The movement syntax carries only the register
    /// name; the configuration's register index supplies the type (Accumulation /
    /// Information / Accounting / Calculation). Returns `None` when no register of that
    /// name is visible, so an unresolved movement is surfaced honestly rather than guessed.
    pub fn resolve_register_by_name(
        &self,
        db: &dyn ConfigsDatabase,
        register_name: &Name,
    ) -> Option<(bsl_metadata::MdoType, Name)> {
        if !self.scopes.iter().any(|s| matches!(s, Scope::WorkspaceScope)) {
            return None;
        }
        let module_id = self.module_id()?;
        db.resolve_register_by_name(module_id.file_id, register_name.as_str())
            .map(|reg| (reg.mdo_type(), Name::new(reg.name())))
    }

    /// Names of the configuration's GLOBAL common modules — those whose exported
    /// procedures are callable unqualified. These are the candidate modules for a
    /// global-context `ПодключитьОбработчикОжидания` handler, which names the procedure
    /// without a module qualifier. Deduplicated across visible configs (extension overlays).
    ///
    /// Two deliberate scope choices, both in service of impact analysis:
    /// - Modules are NOT filtered by client/server dispatch. The help requires an idle
    ///   handler to be client-side, but the graph models the *named reference* — if such a
    ///   handler names a server-only method, renaming that method still breaks the
    ///   registration, so the edge must exist. Dispatch validity is a diagnostic concern.
    /// - The application module (`МодульПриложения`) — the help's other global-context host
    ///   — is not yet enumerated here; only global common modules are. A handler living in
    ///   the application module is a known, currently-unmodelled gap.
    pub fn global_common_module_names(&self, db: &dyn ConfigsDatabase) -> Vec<Name> {
        if !self.scopes.iter().any(|s| matches!(s, Scope::WorkspaceScope)) {
            return Vec::new();
        }
        let Some(module_id) = self.module_id() else { return Vec::new() };
        let mut names = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        for cfg in db.configurations(module_id.file_id).iter() {
            for cm in cfg.configuration.common_modules() {
                if cm.is_global() && seen.insert(intern::NormName::intern(cm.name())) {
                    names.push(Name::new(cm.name()));
                }
            }
        }
        names
    }

    /// Exported methods of the configuration's GLOBAL common modules — those callable
    /// without a module qualifier because a global common module extends the global
    /// context. Each entry is `(module_name, method_name, method_id)`.
    ///
    /// Both the global flag and the module body are taken from the module *effective* for
    /// this file: candidate names come from [`Self::global_common_module_names`], but each
    /// is confirmed global via [`ConfigsDatabase::resolve_common_module`] (base composed
    /// with the file's own extension, extension winning) before its bodies are located
    /// through [`Self::locate_common_module_candidates`] — the same visibility-correct path
    /// a qualified `Модуль.Метод()` call takes. So a bare call resolves to exactly the
    /// target the qualified form would, and an extension that turns a base-global module
    /// non-global (or vice versa) is honoured rather than leaking the base flag.
    ///
    /// Order is deterministic: global modules in configuration-iteration order, methods in
    /// symbol-tree order. Callers that need a single winner for a name keep the FIRST
    /// occurrence (a name exported by two global modules is a configuration-level ambiguity
    /// 1C itself forbids; we pick deterministically rather than guess).
    pub fn global_common_module_exports(
        &self,
        db: &dyn ConfigsDatabase,
    ) -> Vec<(Name, Name, MethodId)> {
        let Some(self_module) = self.module_id() else {
            return Vec::new();
        };
        let mut exports = Vec::new();
        for module_name in self.global_common_module_names(db) {
            let effective_is_global = db
                .resolve_common_module(self_module.file_id, module_name.as_str())
                .is_some_and(|cm| cm.is_global());
            if !effective_is_global {
                continue;
            }
            let Ok(candidates) = self.locate_common_module_candidates(db, &module_name) else {
                continue;
            };
            // Base first, then the caller's own extension body: a name declared
            // in both keeps the base declaration, an extension-added export is
            // appended after it.
            let mut seen = rustc_hash::FxHashSet::default();
            for module_id in candidates.readable() {
                let symbol_tree = db.symbol_tree_ref(module_id);
                for method in symbol_tree.exported_methods() {
                    if seen.insert(intern::NormName::intern(method.name.as_str())) {
                        exports.push((module_name.clone(), method.name.clone(), method.id));
                    }
                }
            }
        }
        exports
    }

    pub fn resolve_assignment_target(
        &self,
        db: &dyn ConfigsDatabase,
        name: &Name,
    ) -> AssignmentResolution {
        if let Some(local) = self.resolve_local(name) {
            return match local.def {
                crate::scope::ScopeDef::LocalVariable => AssignmentResolution::Local,
                crate::scope::ScopeDef::Parameter => AssignmentResolution::Param,
            };
        }
        if let Some(var_id) = self.resolve_module_variable(db, name) {
            return AssignmentResolution::ModuleVariable(var_id);
        }
        if let Some(module_id) = self.module_id() {
            if db.resolve_common_module(module_id.file_id, name.as_str()).is_some() {
                return AssignmentResolution::CommonModule(name.clone());
            }
        }
        AssignmentResolution::Unknown
    }

    pub fn resolve_name(&self, db: &dyn DefDatabase, name: &Name) -> Option<Resolution> {
        if let Some(builtin_name) = self.resolve_builtin(name) {
            return Some(Resolution::Builtin(builtin_name));
        }

        if let Some(local) = self.resolve_local(name) {
            return Some(Resolution::Local(local));
        }

        if let Some(method_id) = self.resolve_module_method(db, name) {
            return Some(Resolution::Method(method_id));
        }

        if let Some(variable_id) = self.resolve_module_variable(db, name) {
            return Some(Resolution::Variable(variable_id));
        }

        None
    }

    pub fn resolve_path(&self, db: &dyn ConfigsDatabase, path: &QualifiedName) -> PathResolution {
        let segments = path.segments();

        match segments.len() {
            0 => PathResolution::Unresolved(path.clone()),

            1 => {
                if let Some(resolution) = self.resolve_name(db, &segments[0]) {
                    return match resolution {
                        Resolution::Builtin(name) => PathResolution::Builtin(name),
                        Resolution::Method(id) => PathResolution::Method(id),
                        Resolution::Variable(id) => PathResolution::Variable(id),
                        Resolution::Local(_) => PathResolution::Unresolved(path.clone()),
                    };
                }
                PathResolution::Unresolved(path.clone())
            }

            2 => self.resolve_two_level(db, &segments[0], &segments[1]),

            3 => self.resolve_three_level(db, &segments[0], &segments[1], &segments[2]),

            _ => PathResolution::Unresolved(path.clone()),
        }
    }

    fn resolve_two_level(
        &self,
        db: &dyn ConfigsDatabase,
        module_name: &Name,
        method_name: &Name,
    ) -> PathResolution {
        for scope in &self.scopes {
            if let Scope::WorkspaceScope = scope {
                return self.resolve_cross_module(db, module_name, method_name);
            }
        }

        PathResolution::Unresolved(QualifiedName::from_segments([
            module_name.clone(),
            method_name.clone(),
        ]))
    }

    /// Every body file the common module `module_name` resolves to for this
    /// resolver's file (config visibility + body lookup), base first, without
    /// looking at any method. Shared by [`Self::resolve_qualified_method`] and
    /// the graph-index build, so both agree on which bodies a qualified call
    /// targets; only the method-lookup step (symbol tree vs. the resident graph
    /// index) differs.
    ///
    /// A module adopted by the caller's own
    /// extension has two bodies — the base one and the extension one that adds
    /// methods on top of it — and a method lookup that sees only one of them
    /// misreports the other half of the surface. Callers outside any extension
    /// get exactly the base body, so an extension-added method stays invisible
    /// to base-configuration code (calling it from there is a genuine error:
    /// the extension can be detached at any time).
    ///
    /// The returned vector is never empty.
    pub(crate) fn locate_common_module_candidates(
        &self,
        db: &dyn ConfigsDatabase,
        module_name: &Name,
    ) -> Result<CommonModuleCandidates, QualifiedMethodError> {
        if !self.scopes.iter().any(|s| matches!(s, Scope::WorkspaceScope)) {
            tracing::warn!(
                "locate_common_module_candidates called without workspace scope; refusing"
            );
            return Err(QualifiedMethodError::NotFound);
        }

        let module_id = self.module_id().ok_or_else(|| {
            tracing::warn!("locate_common_module_candidates called without module scope");
            QualifiedMethodError::NotFound
        })?;

        let file_id = module_id.file_id;

        if db.has_config_root(file_id) {
            if db.resolve_common_module(file_id, module_name.as_str()).is_none() {
                return Err(QualifiedMethodError::NotVisibleInConfigs);
            }

            if let Some(bodies) =
                db.resolve_common_module_file_candidates(file_id, module_name.as_str())
            {
                if !bodies.is_empty() {
                    // Composition, not a verdict: whether an empty `readable` is an
                    // answer depends on the method name, which this function does not
                    // have. Reporting the outcome here would also hide the unread ids
                    // from the graph index, which needs them to reproject on healing.
                    return Ok(CommonModuleCandidates {
                        bodies: bodies
                            .bodies
                            .into_iter()
                            .map(|b| (crate::ModuleId::new(b.file), b.unread))
                            .collect(),
                    });
                }
                // Configs see the module but no body file mapped (metadata-URI
                // drift): degrade to the path index below instead of reporting
                // a module the visibility gate just confirmed as missing.
            }
        }

        let source_root_id = db.file_source_root_input(file_id).source_root_id(db);
        let module_index = db.module_index(source_root_id);

        let target_file_id = module_index
            .resolve_common_module(module_name)
            .ok_or(QualifiedMethodError::NotFound)?;

        // The path index is built from source-root paths, which hold an unread body
        // exactly like any other file — its entry cannot be dropped, because the first
        // query through a dropped id panics resolving its path. So the sorting happens
        // here, on the way out.
        Ok(CommonModuleCandidates {
            bodies: vec![(crate::ModuleId::new(target_file_id), db.file_is_unread(target_file_id))],
        })
    }

    pub fn resolve_qualified_method(
        &self,
        db: &dyn ConfigsDatabase,
        module_name: &Name,
        method_name: &Name,
    ) -> Result<QualifiedMethodResolution, QualifiedMethodError> {
        let _span =
            tracing::info_span!("resolve_qualified_method", %module_name, %method_name).entered();

        let candidates = self.locate_common_module_candidates(db, module_name)?;
        for (target_module_id, unread) in candidates.bodies {
            // Walking in priority order and stopping at the first unread body is what
            // keeps a lower-priority body from answering for a higher-priority one
            // whose surface is unknown: the unread base may well declare this method,
            // and its declaration would have won.
            if unread {
                return Err(QualifiedMethodError::BodyUnread);
            }
            let symbol_tree = db.symbol_tree_ref(target_module_id);
            if let Some(method_symbol) = symbol_tree.find_method(method_name) {
                return Ok(QualifiedMethodResolution {
                    method_id: method_symbol.id,
                    is_export: method_symbol.is_export,
                });
            }
        }
        Err(QualifiedMethodError::NotFound)
    }

    fn resolve_cross_module(
        &self,
        db: &dyn ConfigsDatabase,
        module_name: &Name,
        method_name: &Name,
    ) -> PathResolution {
        let unresolved = || {
            PathResolution::Unresolved(QualifiedName::from_segments([
                module_name.clone(),
                method_name.clone(),
            ]))
        };
        match self.resolve_qualified_method(db, module_name, method_name) {
            Ok(r) if r.is_export => PathResolution::Method(r.method_id),
            Ok(_) | Err(_) => unresolved(),
        }
    }

    pub fn resolve_three_level_method(
        &self,
        db: &dyn ConfigsDatabase,
        mdo_type_plural: &Name,
        mdo_name: &Name,
        method_name: &Name,
    ) -> Result<QualifiedMethodResolution, QualifiedMethodError> {
        let mdo_type =
            bsl_metadata::MdoType::from_plural(mdo_type_plural.as_str()).ok_or_else(|| {
                tracing::debug!("Unknown MDO type plural: {}", mdo_type_plural);
                QualifiedMethodError::NotFound
            })?;

        let manager_type = crate::body::ManagerType::from_mdo_type(mdo_type).ok_or_else(|| {
            tracing::debug!("MdoType {:?} does not have manager module", mdo_type);
            QualifiedMethodError::NotFound
        })?;

        self.resolve_manager_method(db, manager_type, mdo_name, method_name)
    }

    /// Resolve a user-defined method on the manager module of a metadata object,
    /// addressed by an already-parsed [`ManagerType`] (e.g.
    /// `Справочники.Контрагенты.Метод`). Platform manager methods (e.g.
    /// `СоздатьЭлемент`) are not user methods and resolve to `NotFound`.
    /// Locate the manager module of `manager_type`/`mdo_name` (config visibility +
    /// path index), without looking at any method. Shared by
    /// [`Self::resolve_manager_method`] and the graph-index build.
    pub(crate) fn locate_manager_module(
        &self,
        db: &dyn ConfigsDatabase,
        manager_type: crate::body::ManagerType,
        mdo_name: &Name,
    ) -> Result<ManagerModuleTarget, QualifiedMethodError> {
        let mdo_type = manager_type.to_mdo_type();

        let current_module_id = self.module_id().ok_or_else(|| {
            tracing::warn!("locate_manager_module called without module scope");
            QualifiedMethodError::NotFound
        })?;

        let current_file_id = current_module_id.file_id;
        if db.file_has_visible_config(current_file_id)
            && !Self::mdo_visible(db, current_file_id, mdo_type, mdo_name)
        {
            return Err(QualifiedMethodError::NotVisibleInConfigs);
        }

        let source_root_id = db.file_source_root_input(current_file_id).source_root_id(db);
        let module_index = db.module_index(source_root_id);

        let target_file_id = module_index
            .resolve_manager(manager_type, mdo_name)
            .ok_or(QualifiedMethodError::NotFound)?;

        // Composition, not a verdict — same reason as the common-module route. Callers
        // that know the method name turn "not found in an unread body" into
        // `BodyUnread`; the graph index needs the id either way, or healing the body
        // reprojects nobody.
        Ok(ManagerModuleTarget {
            module: crate::ModuleId::new(target_file_id),
            unread: db.file_is_unread(target_file_id),
        })
    }

    pub fn resolve_manager_method(
        &self,
        db: &dyn ConfigsDatabase,
        manager_type: crate::body::ManagerType,
        mdo_name: &Name,
        method_name: &Name,
    ) -> Result<QualifiedMethodResolution, QualifiedMethodError> {
        let _span = tracing::info_span!(
            "resolve_manager_method",
            ?manager_type,
            mdo_name = %mdo_name,
            method = %method_name
        )
        .entered();

        let target = self.locate_manager_module(db, manager_type, mdo_name)?;
        let symbol_tree = db.symbol_tree_ref(target.module);

        let method_symbol = symbol_tree.find_method(method_name).ok_or(if target.unread {
            QualifiedMethodError::BodyUnread
        } else {
            QualifiedMethodError::NotFound
        })?;

        Ok(QualifiedMethodResolution {
            method_id: method_symbol.id,
            is_export: method_symbol.is_export,
        })
    }

    pub fn resolve_aliased_manager_method(
        &self,
        db: &dyn ConfigsDatabase,
        mdo_type: bsl_metadata::MdoType,
        mdo_name: &Name,
        method_name: &Name,
    ) -> Result<QualifiedMethodResolution, QualifiedMethodError> {
        let _span = tracing::info_span!(
            "resolve_aliased_manager_method",
            ?mdo_type,
            mdo_name = %mdo_name,
            method = %method_name
        )
        .entered();

        let manager_type = crate::body::ManagerType::from_mdo_type(mdo_type).ok_or_else(|| {
            tracing::debug!("MdoType {:?} does not have manager module", mdo_type);
            QualifiedMethodError::NotFound
        })?;

        self.resolve_manager_method(db, manager_type, mdo_name, method_name)
    }

    pub fn resolve_object_module_method(
        &self,
        db: &dyn ConfigsDatabase,
        mdo_type: bsl_metadata::MdoType,
        mdo_name: &Name,
        method_name: &Name,
    ) -> Result<QualifiedMethodResolution, QualifiedMethodError> {
        let _span = tracing::info_span!(
            "resolve_object_module_method",
            ?mdo_type,
            mdo_name = %mdo_name,
            method = %method_name
        )
        .entered();

        let current_module_id = self.module_id().ok_or_else(|| {
            tracing::warn!("resolve_object_module_method called without module scope");
            QualifiedMethodError::NotFound
        })?;

        let current_file_id = current_module_id.file_id;
        if db.file_has_visible_config(current_file_id)
            && !Self::mdo_visible(db, current_file_id, mdo_type, mdo_name)
        {
            tracing::debug!(
                "resolve_object_module_method: {:?} '{}' not declared in any visible config",
                mdo_type,
                mdo_name
            );
            return Err(QualifiedMethodError::NotVisibleInConfigs);
        }

        let source_root_id = db.file_source_root_input(current_file_id).source_root_id(db);
        let module_index = db.module_index(source_root_id);

        let target_file_id =
            module_index.resolve_object_module(mdo_type, mdo_name).ok_or_else(|| {
                tracing::debug!("Object module not found: {:?} / {}", mdo_type, mdo_name);
                QualifiedMethodError::NotFound
            })?;

        let target_module_id = crate::ModuleId::new(target_file_id);
        let symbol_tree = db.symbol_tree_ref(target_module_id);

        let method_symbol = symbol_tree.find_method(method_name).ok_or_else(|| {
            tracing::debug!(
                "Object module '{:?}/{}' found but method '{}' NOT found",
                mdo_type,
                mdo_name,
                method_name
            );
            // Not finding it in a body nobody could read is not a finding.
            if db.file_is_unread(target_file_id) {
                QualifiedMethodError::BodyUnread
            } else {
                QualifiedMethodError::NotFound
            }
        })?;

        tracing::info!(
            "SUCCESS - object module method '{}' in '{:?}/{}' (is_export={})",
            method_name,
            mdo_type,
            mdo_name,
            method_symbol.is_export
        );

        Ok(QualifiedMethodResolution {
            method_id: method_symbol.id,
            is_export: method_symbol.is_export,
        })
    }

    pub fn resolve_record_set_module_method(
        &self,
        db: &dyn ConfigsDatabase,
        mdo_type: bsl_metadata::MdoType,
        mdo_name: &Name,
        method_name: &Name,
    ) -> Result<QualifiedMethodResolution, QualifiedMethodError> {
        let _span = tracing::info_span!(
            "resolve_record_set_module_method",
            ?mdo_type,
            mdo_name = %mdo_name,
            method = %method_name
        )
        .entered();

        let current_module_id = self.module_id().ok_or_else(|| {
            tracing::warn!("resolve_record_set_module_method called without module scope");
            QualifiedMethodError::NotFound
        })?;

        let current_file_id = current_module_id.file_id;
        if db.file_has_visible_config(current_file_id)
            && !Self::mdo_visible(db, current_file_id, mdo_type, mdo_name)
        {
            tracing::debug!(
                "resolve_record_set_module_method: {:?} '{}' not declared in any visible config",
                mdo_type,
                mdo_name
            );
            return Err(QualifiedMethodError::NotVisibleInConfigs);
        }

        let source_root_id = db.file_source_root_input(current_file_id).source_root_id(db);
        let module_index = db.module_index(source_root_id);

        let target_file_id =
            module_index.resolve_record_set_module(mdo_type, mdo_name).ok_or_else(|| {
                tracing::debug!("Record-set module not found: {:?} / {}", mdo_type, mdo_name);
                QualifiedMethodError::NotFound
            })?;

        let target_module_id = crate::ModuleId::new(target_file_id);
        let symbol_tree = db.symbol_tree_ref(target_module_id);

        let method_symbol = symbol_tree.find_method(method_name).ok_or_else(|| {
            tracing::debug!(
                "Record-set module '{:?}/{}' found but method '{}' NOT found",
                mdo_type,
                mdo_name,
                method_name
            );
            // Not finding it in a body nobody could read is not a finding.
            if db.file_is_unread(target_file_id) {
                QualifiedMethodError::BodyUnread
            } else {
                QualifiedMethodError::NotFound
            }
        })?;

        tracing::info!(
            "SUCCESS - record-set module method '{}' in '{:?}/{}' (is_export={})",
            method_name,
            mdo_type,
            mdo_name,
            method_symbol.is_export
        );

        Ok(QualifiedMethodResolution {
            method_id: method_symbol.id,
            is_export: method_symbol.is_export,
        })
    }

    fn resolve_three_level(
        &self,
        db: &dyn ConfigsDatabase,
        mdo_type_plural: &Name,
        mdo_name: &Name,
        method_name: &Name,
    ) -> PathResolution {
        let unresolved = || {
            PathResolution::Unresolved(QualifiedName::from_segments([
                mdo_type_plural.clone(),
                mdo_name.clone(),
                method_name.clone(),
            ]))
        };
        match self.resolve_three_level_method(db, mdo_type_plural, mdo_name, method_name) {
            Ok(r) if r.is_export => PathResolution::Method(r.method_id),
            Ok(_) | Err(_) => unresolved(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedLocal {
    pub def: crate::scope::ScopeDef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Builtin(Name),

    Local(ResolvedLocal),

    Method(MethodId),

    Variable(VariableId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssignmentResolution {
    Local,
    Param,
    ModuleVariable(VariableId),
    CommonModule(Name),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualifiedMethodResolution {
    pub method_id: MethodId,
    pub is_export: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualifiedMethodError {
    NotVisibleInConfigs,
    NotFound,
    /// The module exists, but a body of it could not be read, so nothing can be
    /// concluded about the call — least of all against the file making it.
    BodyUnread,
}

/// A manager module the path index named, and whether its bytes could be read.
/// Reported rather than judged for the same reason as [`CommonModuleCandidates`]:
/// the verdict needs the method name, and the graph index needs the id regardless.
pub(crate) struct ManagerModuleTarget {
    pub(crate) module: ModuleId,
    pub(crate) unread: bool,
}

/// The bodies of a common module that name resolution may look a method up in,
/// plus whether looking there is the whole story.
pub(crate) struct CommonModuleCandidates {
    /// Every body in PRIORITY ORDER with its readability. Order is semantic — the
    /// base declaration wins over an extension's — so a hit in a later body is the
    /// answer only when every earlier body was readable and did not have it.
    pub(crate) bodies: Vec<(ModuleId, bool)>,
}

impl CommonModuleCandidates {
    pub(crate) fn readable(&self) -> impl Iterator<Item = ModuleId> + '_ {
        self.bodies.iter().filter(|(_, unread)| !unread).map(|(m, _)| *m)
    }

    pub(crate) fn unread(&self) -> impl Iterator<Item = ModuleId> + '_ {
        self.bodies.iter().filter(|(_, unread)| *unread).map(|(m, _)| *m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::{ExprScopes, ScopeDef};
    use crate::ModuleId;
    use vfs::FileId;

    #[test]
    fn test_module_resolver() {
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);
        let resolver = Resolver::for_module(module_id);

        assert_eq!(resolver.module_id(), Some(module_id));
    }

    #[test]
    fn test_local_resolution() {
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let mut scopes = ExprScopes::new();
        scopes.add_parameter(Name::new("Параметр"));
        scopes.add_local_variable(scopes.root_scope(), Name::new("Переменная"));

        let root_scope = scopes.root_scope();
        let resolver =
            Resolver::for_module(module_id).push_expr_scope(Arc::new(scopes), root_scope);

        let resolved = resolver.resolve_local(&Name::new("Параметр"));
        assert_eq!(resolved.map(|r| r.def), Some(ScopeDef::Parameter));

        let resolved = resolver.resolve_local(&Name::new("Переменная"));
        assert_eq!(resolved.map(|r| r.def), Some(ScopeDef::LocalVariable));

        let resolved = resolver.resolve_local(&Name::new("НеСуществует"));
        assert_eq!(resolved, None);
    }

    #[test]
    fn test_case_insensitive_resolution() {
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let mut scopes = ExprScopes::new();
        scopes.add_parameter(Name::new("МойПараметр"));

        let root_scope = scopes.root_scope();
        let resolver =
            Resolver::for_module(module_id).push_expr_scope(Arc::new(scopes), root_scope);

        let resolved = resolver.resolve_local(&Name::new("мойпараметр"));
        assert_eq!(resolved.map(|r| r.def), Some(ScopeDef::Parameter));

        let resolved = resolver.resolve_local(&Name::new("МОЙПАРАМЕТР"));
        assert_eq!(resolved.map(|r| r.def), Some(ScopeDef::Parameter));
    }

    #[test]
    fn test_resolve_path_single_segment() {
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);
        let _resolver = Resolver::for_module(module_id);

        let _path = QualifiedName::from_segments([Name::new("Переменная")]);
    }

    #[test]
    fn test_resolve_path_two_segments() {
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);
        let _resolver = Resolver::with_workspace_scope(module_id);

        let _path = QualifiedName::from_segments([Name::new("ОбщийМодуль"), Name::new("Метод")]);
    }

    #[test]
    fn test_resolve_path_three_segments() {
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);
        let _resolver = Resolver::with_workspace_scope(module_id);

        let _path = QualifiedName::from_segments([
            Name::new("Документы"),
            Name::new("ПКО"),
            Name::new("Создать"),
        ]);
    }

    #[test]
    fn test_builtins_scope_guard_is_opt_in() {
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let plain = Resolver::for_module(module_id);
        assert!(plain.resolve_builtin(&Name::new("Сообщить")).is_none());

        let with_workspace = Resolver::with_workspace_scope(module_id);
        assert!(with_workspace.resolve_builtin(&Name::new("Сообщить")).is_none());

        let with_all = Resolver::with_builtins_and_workspace(module_id);
        assert!(with_all.has_builtins());
    }

    #[test]
    fn test_builtins_scope_resolves_platform_global() {
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let resolver = Resolver::with_builtins_and_workspace(module_id);

        assert!(
            bsl_platform::PlatformDataInner::instance().get_global_function("Сообщить").is_some(),
            "bundled platform data must include `Сообщить`; missing data would mean the \
             loader regressed — guard this assumption loudly"
        );

        let resolved = resolver.resolve_builtin(&Name::new("сообщить"));
        assert_eq!(
            resolved.as_ref().map(|n| n.as_str()),
            Some("сообщить"),
            "case-insensitive platform global lookup must succeed"
        );

        assert!(resolver.resolve_builtin(&Name::new("НетТакогоBuiltin")).is_none());
    }
}
