use std::sync::Arc;

use bsl_metadata::MdObject;

use crate::configs::ConfigsDatabase;
use crate::scope::{ExprScopes, ScopeId};
use crate::{DefDatabase, MethodId, ModuleId, Name, PathResolution, QualifiedName, VariableId};

pub struct Resolver {
    #[doc(hidden)]
    pub scopes: Vec<Scope>,
}

#[doc(hidden)]
pub enum Scope {
    ModuleScope(ModuleId),

    ExprScope { scopes: Arc<ExprScopes>, scope_id: ScopeId },

    WorkspaceScope,

    Builtins,
}

impl Resolver {
    pub fn for_module(module_id: ModuleId) -> Self {
        Resolver { scopes: vec![Scope::ModuleScope(module_id)] }
    }

    pub fn with_workspace_scope(module_id: ModuleId) -> Self {
        Resolver { scopes: vec![Scope::WorkspaceScope, Scope::ModuleScope(module_id)] }
    }

    pub fn with_builtins_and_workspace(module_id: ModuleId) -> Self {
        Resolver {
            scopes: vec![Scope::Builtins, Scope::WorkspaceScope, Scope::ModuleScope(module_id)],
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
            if let Scope::ModuleScope(module_id) = scope {
                return Some(*module_id);
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
        let module_id = self.module_id()?;
        let symbol_tree = db.symbol_tree(module_id);
        let method = symbol_tree.find_method(name)?;
        Some(method.id)
    }

    pub fn resolve_module_variable(&self, db: &dyn DefDatabase, name: &Name) -> Option<VariableId> {
        let module_id = self.module_id()?;
        let symbol_tree = db.symbol_tree(module_id);
        let variable = symbol_tree.find_variable(name)?;
        Some(variable.id)
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
        let needle = register_name.as_str();
        db.configurations(module_id.file_id).iter().rev().find_map(|cfg| {
            cfg.configuration
                .find_register(needle)
                .map(|reg| (reg.mdo_type(), Name::new(reg.name())))
        })
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
        let mut seen = std::collections::HashSet::new();
        for cfg in db.configurations(module_id.file_id).iter() {
            for cm in cfg.configuration.common_modules() {
                if cm.is_global() && seen.insert(cm.name().to_lowercase()) {
                    names.push(Name::new(cm.name()));
                }
            }
        }
        names
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

    /// Locate the common module `module_name` is declared in (config visibility +
    /// path index), without looking at any method. Shared by
    /// [`Self::resolve_qualified_method`] and the graph-index build, so both agree
    /// on which module a qualified call targets; only the method-lookup step
    /// (symbol tree vs. the resident graph index) differs.
    pub(crate) fn locate_common_module(
        &self,
        db: &dyn ConfigsDatabase,
        module_name: &Name,
    ) -> Result<ModuleId, QualifiedMethodError> {
        if !self.scopes.iter().any(|s| matches!(s, Scope::WorkspaceScope)) {
            tracing::warn!("locate_common_module called without workspace scope; refusing");
            return Err(QualifiedMethodError::NotFound);
        }

        let module_id = self.module_id().ok_or_else(|| {
            tracing::warn!("locate_common_module called without module scope");
            QualifiedMethodError::NotFound
        })?;

        let file_id = module_id.file_id;

        if db.has_config_root(file_id)
            && db.resolve_common_module(file_id, module_name.as_str()).is_none()
        {
            return Err(QualifiedMethodError::NotVisibleInConfigs);
        }

        let source_root_id = db.file_source_root_input(file_id).source_root_id(db);
        let module_index = db.module_index(source_root_id);

        let target_file_id = module_index
            .resolve_common_module(module_name)
            .ok_or(QualifiedMethodError::NotFound)?;

        Ok(crate::ModuleId::new(target_file_id))
    }

    pub fn resolve_qualified_method(
        &self,
        db: &dyn ConfigsDatabase,
        module_name: &Name,
        method_name: &Name,
    ) -> Result<QualifiedMethodResolution, QualifiedMethodError> {
        let _span =
            tracing::info_span!("resolve_qualified_method", %module_name, %method_name).entered();

        let target_module_id = self.locate_common_module(db, module_name)?;
        let symbol_tree = db.symbol_tree(target_module_id);
        let method_symbol =
            symbol_tree.find_method(method_name).ok_or(QualifiedMethodError::NotFound)?;

        Ok(QualifiedMethodResolution {
            method_id: method_symbol.id,
            is_export: method_symbol.is_export,
        })
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
    ) -> Result<ModuleId, QualifiedMethodError> {
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

        Ok(crate::ModuleId::new(target_file_id))
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

        let target_module_id = self.locate_manager_module(db, manager_type, mdo_name)?;
        let symbol_tree = db.symbol_tree(target_module_id);

        let method_symbol =
            symbol_tree.find_method(method_name).ok_or(QualifiedMethodError::NotFound)?;

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

        let current_module_id = self.module_id().ok_or_else(|| {
            tracing::warn!("resolve_aliased_manager_method called without module scope");
            QualifiedMethodError::NotFound
        })?;

        let current_file_id = current_module_id.file_id;
        if db.file_has_visible_config(current_file_id)
            && !Self::mdo_visible(db, current_file_id, mdo_type, mdo_name)
        {
            tracing::debug!(
                "resolve_aliased_manager_method: {:?} '{}' not declared in any visible config",
                mdo_type,
                mdo_name
            );
            return Err(QualifiedMethodError::NotVisibleInConfigs);
        }

        let source_root_id = db.file_source_root_input(current_file_id).source_root_id(db);
        let module_index = db.module_index(source_root_id);

        let target_file_id =
            module_index.resolve_manager(manager_type, mdo_name).ok_or_else(|| {
                tracing::debug!("Manager module not found: {:?} / {}", manager_type, mdo_name);
                QualifiedMethodError::NotFound
            })?;

        let target_module_id = crate::ModuleId::new(target_file_id);
        let symbol_tree = db.symbol_tree(target_module_id);

        let method_symbol = symbol_tree.find_method(method_name).ok_or_else(|| {
            tracing::debug!(
                "Manager module '{:?}/{}' found but method '{}' NOT found",
                manager_type,
                mdo_name,
                method_name
            );
            QualifiedMethodError::NotFound
        })?;

        tracing::info!(
            "SUCCESS - aliased manager method '{}' in '{:?}/{}' (is_export={})",
            method_name,
            manager_type,
            mdo_name,
            method_symbol.is_export
        );

        Ok(QualifiedMethodResolution {
            method_id: method_symbol.id,
            is_export: method_symbol.is_export,
        })
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
        let symbol_tree = db.symbol_tree(target_module_id);

        let method_symbol = symbol_tree.find_method(method_name).ok_or_else(|| {
            tracing::debug!(
                "Object module '{:?}/{}' found but method '{}' NOT found",
                mdo_type,
                mdo_name,
                method_name
            );
            QualifiedMethodError::NotFound
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
        let symbol_tree = db.symbol_tree(target_module_id);

        let method_symbol = symbol_tree.find_method(method_name).ok_or_else(|| {
            tracing::debug!(
                "Record-set module '{:?}/{}' found but method '{}' NOT found",
                mdo_type,
                mdo_name,
                method_name
            );
            QualifiedMethodError::NotFound
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
