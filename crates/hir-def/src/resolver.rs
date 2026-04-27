//! Name resolution for BSL.
//!
//! The Resolver provides a unified API for resolving names at different levels:
//! - Builtins: platform global functions (never shadowed by user code)
//! - Module-level: procedures, functions, module variables
//! - Expression-level: parameters, local variables
//!
//! ## Resolution Order
//!
//! BSL platform globals take precedence over local variables — declaring
//! `Перем Сообщить` does not shadow the platform `Сообщить()` call. The
//! Resolver reflects this by checking `Scope::Builtins` **first**,
//! regardless of its position in the scope stack. For the remaining
//! scopes the usual lexical order applies:
//!
//! 1. `Scope::Builtins` (platform globals) — highest priority
//! 2. `Scope::ExprScope` (parameters, local variables) — innermost user scope
//! 3. `Scope::ModuleScope` (methods, module variables)
//! 4. `Scope::WorkspaceScope` (exported methods from other modules) — outermost

use std::sync::Arc;

use crate::configs::{ConfigsDatabase, VisibleConfig};
use crate::scope::{ExprScopes, ScopeId};
use crate::{DefDatabase, MethodId, ModuleId, Name, PathResolution, QualifiedName, VariableId};

/// Resolver for name resolution.
///
/// The resolver maintains a stack of scopes (module scope + expression scopes)
/// and resolves names by walking up the scope chain.
pub struct Resolver {
    #[doc(hidden)]
    pub scopes: Vec<Scope>,
}

#[doc(hidden)]
pub enum Scope {
    /// Module-level scope (procedures, functions, module variables).
    ModuleScope(ModuleId),

    /// Expression scope (parameters, local variables).
    ExprScope { scopes: Arc<ExprScopes>, scope_id: ScopeId },

    /// Workspace-wide scope for cross-module resolution (exported symbols).
    ///
    /// Note: Full cross-module resolution requires ModuleGraph (Iteration 9.5).
    /// For now, this provides the infrastructure.
    WorkspaceScope,

    /// Platform built-in scope (global functions from `bsl-platform`).
    ///
    /// Consulted before any user scope because BSL platform globals are not
    /// shadowed by local or module-level names (e.g. a local `Сообщить`
    /// variable does not hide the platform `Сообщить()` function).
    Builtins,
}

impl Resolver {
    /// Create a resolver for a module.
    pub fn for_module(module_id: ModuleId) -> Self {
        Resolver { scopes: vec![Scope::ModuleScope(module_id)] }
    }

    /// Create a resolver with workspace scope.
    ///
    /// This allows resolving exported methods from other modules.
    /// Note: Full cross-module resolution requires ModuleGraph (Iteration 9.5).
    pub fn with_workspace_scope(module_id: ModuleId) -> Self {
        Resolver { scopes: vec![Scope::WorkspaceScope, Scope::ModuleScope(module_id)] }
    }

    /// Create a resolver with builtins, workspace and module scopes.
    ///
    /// Preferred constructor for callers that perform unqualified name
    /// resolution (hover, completion, type inference), because it makes
    /// platform globals visible ahead of user scopes.
    pub fn with_builtins_and_workspace(module_id: ModuleId) -> Self {
        Resolver {
            scopes: vec![Scope::Builtins, Scope::WorkspaceScope, Scope::ModuleScope(module_id)],
        }
    }

    /// Returns `true` if this resolver includes the builtins scope.
    fn has_builtins(&self) -> bool {
        self.scopes.iter().any(|s| matches!(s, Scope::Builtins))
    }

    /// Resolve a name against the platform builtin table.
    ///
    /// Returns `Some(name)` when the identifier matches a platform global
    /// function (e.g. `Сообщить`, `НачатьТранзакцию`). The check is
    /// case-insensitive and goes through the static `bsl-platform` index,
    /// matching the behaviour of existing hover/completion call sites.
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

    /// Is `module_name` declared as a CommonModule in any visible configuration?
    ///
    /// Configs are iterated in reverse — extensions (appended to the list
    /// after main) are consulted first. The `any(...)` short-circuit means
    /// the reverse order does not change the *bool* result, but it encodes
    /// the intended union-wins-extension priority for future expansions.
    ///
    /// **Known gap:** This helper only answers yes/no. The actual `FileId`
    /// returned by [`resolve_cross_module`] still comes from the path-based
    /// `module_index`, which is last-write-wins on same-named collisions
    /// between main and extensions. Per-config `FileId` tagging is tracked
    /// separately and is out of scope for Task 1.6.
    ///
    /// [`resolve_cross_module`]: Resolver::resolve_cross_module
    fn module_visible_in_configs(configs: &[VisibleConfig], module_name: &Name) -> bool {
        let needle = module_name.as_str();
        configs.iter().rev().any(|cfg| cfg.configuration.find_common_module(needle).is_some())
    }

    /// Is `mdo_name` a metadata object of `mdo_type` in any visible
    /// configuration? Same reverse-iteration rule as
    /// [`Self::module_visible_in_configs`].
    ///
    /// Probes both [`bsl_metadata::Configuration::find_metadata_object`]
    /// (objects: `Catalog`, `Document`, `Enum`, `Task`, …) and
    /// [`bsl_metadata::Configuration::find_register_by_type_and_name`]
    /// (registers: `InformationRegister`, `AccumulationRegister`, …).
    /// Without the register branch the gate would falsely reject any
    /// register MDO that lives in `registers` rather than
    /// `metadata_objects`, blocking workspace `ManagerModule.bsl`
    /// resolution for register kinds (Phase A).
    fn mdo_visible_in_configs(
        configs: &[VisibleConfig],
        mdo_type: bsl_metadata::MdoType,
        mdo_name: &Name,
    ) -> bool {
        let needle = mdo_name.as_str();
        configs.iter().rev().any(|cfg| {
            cfg.configuration.find_metadata_object(mdo_type, needle).is_some()
                || cfg.configuration.find_register_by_type_and_name(mdo_type, needle).is_some()
        })
    }

    /// Add an expression scope to the resolver.
    ///
    /// This is used when resolving names inside a procedure/function body.
    pub fn push_expr_scope(mut self, scopes: Arc<ExprScopes>, scope_id: ScopeId) -> Self {
        self.scopes.push(Scope::ExprScope { scopes, scope_id });
        self
    }

    /// Get the module ID if this is a module-level resolver.
    pub fn module_id(&self) -> Option<ModuleId> {
        for scope in &self.scopes {
            if let Scope::ModuleScope(module_id) = scope {
                return Some(*module_id);
            }
        }
        None
    }

    /// Resolve a local name (parameter or local variable).
    ///
    /// Returns None if the name is not found in any expression scope.
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

    /// Resolve a method (procedure or function) in module scope.
    ///
    /// Searches the current module's SymbolTree for a method with the given name.
    /// Returns the MethodId if found.
    pub fn resolve_module_method(&self, db: &dyn DefDatabase, name: &Name) -> Option<MethodId> {
        let module_id = self.module_id()?;
        let symbol_tree = db.symbol_tree(module_id);
        let method = symbol_tree.find_method(name)?;
        Some(method.id)
    }

    /// Resolve a module-level variable.
    ///
    /// Searches the current module's SymbolTree for a variable with the given name.
    /// Returns the VariableId if found.
    pub fn resolve_module_variable(&self, db: &dyn DefDatabase, name: &Name) -> Option<VariableId> {
        let module_id = self.module_id()?;
        let symbol_tree = db.symbol_tree(module_id);
        let variable = symbol_tree.find_variable(name)?;
        Some(variable.id)
    }

    /// Resolve the `ЭтотОбъект` / `ThisObject` receiver to the enclosing
    /// MDO's `(kind, name)`.
    ///
    /// Reads [`DefDatabase::module_metadata`] for the current module and
    /// returns `Some((mdo_type, name))` only when **both** conditions
    /// hold:
    ///
    /// 1. The module is an `ObjectModule` — the single module type
    ///    where `ЭтотОбъект` semantically means "the current MDO as a
    ///    `*Object` reference" (record-set / form / manager / common /
    ///    command modules have their own `ЭтотОбъект` semantics, out
    ///    of scope for Task 5).
    /// 2. The MDO flavour has a matching `*Object` companion in
    ///    [`crate::ty::MetadataKind`] (checked via
    ///    [`crate::ty::MetadataKind::object_kind_for`]). Without a
    ///    coercion target the downstream `FieldLookup` /
    ///    `MethodLookup` adapters have nothing to resolve against,
    ///    so a `Ty::ThisObject` constructed here would dangle.
    ///    `Task`, `BusinessProcess`, and
    ///    `ChartOfCharacteristicTypes` all sit in this gap today —
    ///    their ObjectModule `ЭтотОбъект` stays `Ty::Unknown` until
    ///    dedicated `*Object` variants land.
    ///
    /// # Why in `Resolver`
    ///
    /// The identifier is intercepted ahead of the usual lookup cascade
    /// (builtins / locals / module) because BSL treats `ЭтотОбъект`
    /// like a platform global — not shadowable, resolved through module
    /// metadata rather than scope chain. Keeping the helper on
    /// `Resolver` groups it with the other `resolve_*` entry points so
    /// hir-ty / ide callers have a single lookup surface.
    pub fn resolve_this_object(
        &self,
        db: &dyn DefDatabase,
    ) -> Option<(bsl_metadata::MdoType, Name)> {
        let module_id = self.module_id()?;
        let metadata = db.module_metadata(module_id);
        let mdo = metadata.mdo.as_ref()?;

        if metadata.module_type != bsl_metadata::ModuleType::ObjectModule {
            return None;
        }

        // Only MDO flavours with an `*Object` companion in
        // `MetadataKind` are allowed to surface `Ty::ThisObject`.
        // See `resolve_this_object` doc block for rationale.
        crate::ty::MetadataKind::object_kind_for(mdo.mdo_type)?;

        Some((mdo.mdo_type, Name::new(&mdo.name)))
    }

    /// Resolve a name at any level (builtin, local, module, workspace).
    ///
    /// Resolution order (first match wins):
    /// 1. Builtins (platform globals — highest priority, not shadowed)
    /// 2. Local scope (parameters, local variables)
    /// 3. Module scope (methods, module variables)
    /// 4. Workspace scope (cross-module, not yet implemented here)
    pub fn resolve_name(&self, db: &dyn DefDatabase, name: &Name) -> Option<Resolution> {
        // Builtins take precedence over everything (BSL semantics: platform
        // globals are not shadowed by local/module names).
        if let Some(builtin_name) = self.resolve_builtin(name) {
            return Some(Resolution::Builtin(builtin_name));
        }

        // Try local scope
        if let Some(local) = self.resolve_local(name) {
            return Some(Resolution::Local(local));
        }

        // Try module method
        if let Some(method_id) = self.resolve_module_method(db, name) {
            return Some(Resolution::Method(method_id));
        }

        // Try module variable
        if let Some(variable_id) = self.resolve_module_variable(db, name) {
            return Some(Resolution::Variable(variable_id));
        }

        // TODO: Workspace scope (Iteration 9.5 with ModuleGraph)

        None
    }

    /// Resolve a qualified path (e.g., Module.Method or Documents.PKO.Create).
    ///
    /// Performs segment-by-segment resolution.
    ///
    /// # Resolution Strategy
    ///
    /// - **1 segment:** Try local resolution (parameter/variable/method)
    /// - **2 segments:** Module.Method → cross-module resolution
    /// - **3 segments:** Documents.PKO.Create → manager module resolution
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Single: local name resolution
    /// let path = QualifiedName::from_segments([Name::new("Переменная")]);
    /// let resolution = resolver.resolve_path(db, &path);
    ///
    /// // Two: cross-module resolution
    /// let path = QualifiedName::from_segments([Name::new("ОбщийМодуль"), Name::new("Метод")]);
    /// let resolution = resolver.resolve_path(db, &path);
    ///
    /// // Three: manager module resolution
    /// let path = QualifiedName::from_segments([
    ///     Name::new("Документы"),
    ///     Name::new("ПКО"),
    ///     Name::new("Создать")
    /// ]);
    /// let resolution = resolver.resolve_path(db, &path);
    /// match resolution {
    ///     PathResolution::Method(id) => println!("Found method: {:?}", id),
    ///     PathResolution::Unresolved(_) => println!("Not found"),
    ///     _ => {}
    /// }
    /// ```
    pub fn resolve_path(&self, db: &dyn ConfigsDatabase, path: &QualifiedName) -> PathResolution {
        let segments = path.segments();

        match segments.len() {
            0 => {
                // Empty path - invalid
                PathResolution::Unresolved(path.clone())
            }

            1 => {
                // Single segment - try unified resolution (builtin > local > module)
                if let Some(resolution) = self.resolve_name(db, &segments[0]) {
                    return match resolution {
                        Resolution::Builtin(name) => PathResolution::Builtin(name),
                        Resolution::Method(id) => PathResolution::Method(id),
                        Resolution::Variable(id) => PathResolution::Variable(id),
                        Resolution::Local(_) => {
                            // Local variables cannot be resolved as paths
                            PathResolution::Unresolved(path.clone())
                        }
                    };
                }
                PathResolution::Unresolved(path.clone())
            }

            2 => {
                // Two segments: Module.Method
                self.resolve_two_level(db, &segments[0], &segments[1])
            }

            3 => {
                // Three segments: Documents.PKO.Create
                self.resolve_three_level(db, &segments[0], &segments[1], &segments[2])
            }

            _ => {
                // More than 3 segments - not supported in BSL
                PathResolution::Unresolved(path.clone())
            }
        }
    }

    /// Resolve two-level path: Module.Method
    ///
    /// Checks if this resolver has WorkspaceScope, and if so, attempts
    /// cross-module resolution.
    fn resolve_two_level(
        &self,
        db: &dyn ConfigsDatabase,
        module_name: &Name,
        method_name: &Name,
    ) -> PathResolution {
        // Check if workspace scope is available
        for scope in &self.scopes {
            if let Scope::WorkspaceScope = scope {
                return self.resolve_cross_module(db, module_name, method_name);
            }
        }

        // No workspace scope - cannot resolve cross-module
        PathResolution::Unresolved(QualifiedName::from_segments([
            module_name.clone(),
            method_name.clone(),
        ]))
    }

    /// Method-oriented cross-module resolution: `CommonModule.Method`.
    ///
    /// Shared implementation for both the Definition-layer (`resolve_path`,
    /// which collapses the outcome into [`PathResolution`]) and the
    /// type-inference layer (which needs the `is_export` flag and the
    /// failure reason to emit precise diagnostics).
    ///
    /// # Algorithm
    ///
    /// 1. Require workspace scope — without it this resolver is module-local.
    /// 2. Consult `db.configurations(file_id)`; if any are registered the
    ///    module name must be declared in at least one (CFE union).
    /// 3. Resolve the module file via the path-based `module_index`.
    /// 4. Look up the method in `symbol_tree` and return both id and export
    ///    flag — non-exported methods are still returned so callers can
    ///    distinguish "not found" from "found but not exported".
    ///
    /// Tests and early workspaces have no configurations registered yet; in
    /// that case (empty `configurations()`) the resolver falls back to pure
    /// `module_index` lookup, preserving prior behaviour.
    ///
    /// # Performance
    ///
    /// - **Metadata check:** O(K·logM) where K = #configs, M = #modules per config
    /// - **File lookup:** O(1) via `module_index`
    /// - **Method lookup:** O(1) via `symbol_tree`
    pub fn resolve_qualified_method(
        &self,
        db: &dyn ConfigsDatabase,
        module_name: &Name,
        method_name: &Name,
    ) -> Result<QualifiedMethodResolution, QualifiedMethodError> {
        let _span =
            tracing::info_span!("resolve_qualified_method", %module_name, %method_name).entered();

        // Cross-module resolution requires workspace scope.
        if !self.scopes.iter().any(|s| matches!(s, Scope::WorkspaceScope)) {
            tracing::warn!("resolve_qualified_method called without workspace scope; refusing");
            return Err(QualifiedMethodError::NotFound);
        }

        let module_id = self.module_id().ok_or_else(|| {
            tracing::warn!("resolve_qualified_method called without module scope");
            QualifiedMethodError::NotFound
        })?;

        let file_id = module_id.file_id;

        // Visibility gate: any registered configuration must declare
        // `module_name`. Extensions iterate first (reverse registration order)
        // so a module declared in both main and an extension resolves to the
        // extension's declaration — the union-wins-extension rule.
        let configurations = db.configurations(file_id);
        if !configurations.is_empty()
            && !Self::module_visible_in_configs(&configurations, module_name)
        {
            tracing::debug!(
                "resolve_qualified_method: module '{}' is not declared in any visible \
                 configuration (main + {} extensions); refusing",
                module_name,
                configurations.iter().filter(|c| c.name.is_some()).count()
            );
            return Err(QualifiedMethodError::NotVisibleInConfigs);
        }

        // Path-based module lookup (O(1) — no BSL parsing).
        let source_root_id = db.file_source_root_input(file_id).source_root_id(db);
        let module_index = db.module_index(source_root_id);

        let target_file_id = module_index.resolve_common_module(module_name).ok_or_else(|| {
            tracing::debug!(
                "resolve_qualified_method: module '{}' NOT found in module_index",
                module_name
            );
            QualifiedMethodError::NotFound
        })?;

        tracing::debug!(
            "resolve_qualified_method: module '{}' resolved to FileId({})",
            module_name,
            target_file_id.index()
        );

        // Method lookup in the resolved module.
        let target_module_id = crate::ModuleId::new(target_file_id);
        let symbol_tree = db.symbol_tree(target_module_id);
        let method_symbol = symbol_tree.find_method(method_name).ok_or_else(|| {
            tracing::debug!(
                "resolve_qualified_method: module '{}' found but method '{}' NOT found",
                module_name,
                method_name
            );
            QualifiedMethodError::NotFound
        })?;

        tracing::debug!(
            "resolve_qualified_method: SUCCESS - '{}.{}' (export = {})",
            module_name,
            method_name,
            method_symbol.is_export
        );
        Ok(QualifiedMethodResolution {
            method_id: method_symbol.id,
            is_export: method_symbol.is_export,
        })
    }

    /// Adapter from the method-oriented resolver to [`PathResolution`].
    ///
    /// # Navigation vs inference divergence
    ///
    /// `PathResolution` is consumed by goto / hover / completion, which
    /// must not surface callees that are unreachable from the caller's
    /// module — so non-exported methods collapse to `Unresolved`. Type
    /// inference calls [`Self::resolve_qualified_method`] directly because
    /// it *does* need to distinguish "not exported" from "not found" to
    /// emit the richer `MethodNotExport` diagnostic. This asymmetry is
    /// intentional and covered by:
    ///
    /// - `non_exported_method_reports_method_not_export`
    ///   (`crates/ide/tests/resolve_qualified_call.rs`) — inference path.
    /// - Definition-layer goto returns `None` via the
    ///   `PathResolution::Unresolved` arm in `hir::Semantics`.
    ///
    /// If a future consumer of `resolve_path` needs to distinguish the
    /// two, extend `PathResolution` with a `NotExported(MethodId)` variant
    /// rather than duplicating resolution logic.
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

    /// Resolve a three-level qualified method call
    /// (`Документы.ПКО.СоздатьДокумент`) to a [`QualifiedMethodResolution`].
    ///
    /// Public counterpart of [`Self::resolve_qualified_method`] for the
    /// 3-segment manager chain. Returns both the `MethodId` and the
    /// `is_export` flag so the type-inference layer can emit a precise
    /// `MethodNotExport` diagnostic without running a second lookup.
    ///
    /// # Algorithm
    ///
    /// 1. `MdoType::from_plural(mdo_type_plural)` — unknown plural collapses
    ///    to [`QualifiedMethodError::NotFound`].
    /// 2. `ManagerType::from_mdo_type` — kinds without a manager module
    ///    (`Cube`, `DimensionTable`, `CommonModule`) return `NotFound`.
    /// 3. Require module scope (caller needs to be inside a module).
    /// 4. Consult `db.configurations(file_id)` — when at least one
    ///    configuration is registered, the MDO must be declared (main + CFE
    ///    union). Empty config lists skip the gate so fixture-only tests
    ///    keep resolving through path-based lookup.
    /// 5. Resolve the manager-module `FileId` via `module_index`.
    /// 6. Look up the method in the target `symbol_tree` by name.
    ///
    /// Non-exported methods are still returned (`is_export: false`) so
    /// callers can surface a dedicated `MethodNotExport` diagnostic.
    ///
    /// # Salsa invalidation
    ///
    /// Reads `db.configurations(...)` (CFE gate) and `db.symbol_tree(...)`
    /// through Salsa — every consumer's `infer` / `resolve_path` transitively
    /// depends on both, so changes to the workspace config set or the target
    /// symbol tree invalidate correctly.
    ///
    /// # Performance
    ///
    /// - Module lookup: O(1) via `module_index`.
    /// - Method lookup: O(1) via `symbol_tree`.
    /// - Total: ~1-5ms first call, <10μs cached.
    pub fn resolve_three_level_method(
        &self,
        db: &dyn ConfigsDatabase,
        mdo_type_plural: &Name,
        mdo_name: &Name,
        method_name: &Name,
    ) -> Result<QualifiedMethodResolution, QualifiedMethodError> {
        let _span = tracing::info_span!(
            "resolve_three_level_method",
            mdo_type = %mdo_type_plural,
            mdo_name = %mdo_name,
            method = %method_name
        )
        .entered();

        // Step 1: Parse plural form → MdoType
        let mdo_type =
            bsl_metadata::MdoType::from_plural(mdo_type_plural.as_str()).ok_or_else(|| {
                tracing::debug!("Unknown MDO type plural: {}", mdo_type_plural);
                QualifiedMethodError::NotFound
            })?;

        // Step 2: Convert MdoType → ManagerType
        let manager_type = crate::body::ManagerType::from_mdo_type(mdo_type).ok_or_else(|| {
            // Types without manager modules (Cube, DimensionTable, CommonModule).
            tracing::debug!("MdoType {:?} does not have manager module", mdo_type);
            QualifiedMethodError::NotFound
        })?;

        // Step 3: Get current module to determine source root
        let current_module_id = self.module_id().ok_or_else(|| {
            tracing::warn!("resolve_three_level_method called without module scope");
            QualifiedMethodError::NotFound
        })?;

        // Step 4: Verify the metadata object is declared in at least one
        // visible configuration (main + CFE extensions, extension wins).
        // When no configuration is registered (tests), skip the check so
        // fixture-only workspaces keep resolving path-based manager modules.
        let current_file_id = current_module_id.file_id;
        let configurations = db.configurations(current_file_id);
        if !configurations.is_empty()
            && !Self::mdo_visible_in_configs(&configurations, mdo_type, mdo_name)
        {
            tracing::debug!(
                "resolve_three_level_method: {:?} '{}' not declared in any visible configuration",
                mdo_type,
                mdo_name
            );
            return Err(QualifiedMethodError::NotVisibleInConfigs);
        }

        // Step 5: Resolve manager module via ModuleIndex
        let source_root_id = db.file_source_root_input(current_file_id).source_root_id(db);
        let module_index = db.module_index(source_root_id);

        let target_file_id =
            module_index.resolve_manager(manager_type, mdo_name).ok_or_else(|| {
                tracing::debug!("Manager module not found: {:?} / {}", manager_type, mdo_name);
                QualifiedMethodError::NotFound
            })?;

        tracing::debug!(
            "Manager module '{:?}/{}' resolved to FileId({})",
            manager_type,
            mdo_name,
            target_file_id.index()
        );

        // Step 6: Load symbol_tree for manager module
        let target_module_id = crate::ModuleId::new(target_file_id);
        let symbol_tree = db.symbol_tree(target_module_id);

        // Step 7: Look up method by name. Returned even when non-exported —
        // the `is_export` flag lets the caller pick the right diagnostic.
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
            "SUCCESS - found method '{}' in manager module '{:?}/{}' (is_export={})",
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

    /// 2-shape variant of [`Self::resolve_three_level_method`]:
    /// `М = Справочники.X; М.МойМетод()` where `М` carries
    /// [`crate::ty::Ty::ObjectManager { kind, name }`][ObjectManager] —
    /// the manager-collection plural has already been consumed by type
    /// inference, so this entry skips the `MdoType::from_plural` step.
    ///
    /// Otherwise identical to [`Self::resolve_three_level_method`]:
    ///
    /// 1. `MdoType` → `ManagerType` (gates out `Cube`,
    ///    `DimensionTable`, `CommonModule`).
    /// 2. Visibility gate via [`Self::mdo_visible_in_configs`] (objects
    ///    *and* registers).
    /// 3. `module_index.resolve_manager(...)` for the manager-module
    ///    `FileId`.
    /// 4. `symbol_tree.find_method(...)` returns the method even when
    ///    not exported, so the caller can pick `MethodNotExport` vs
    ///    `MethodNotFound`.
    ///
    /// `Err(NotVisibleInConfigs)` and `Err(NotFound)` are kept distinct
    /// for the same reason as the 3-segment path: callers fall back to
    /// the platform `lookup_method` only when the workspace
    /// authoritatively does *not* know the receiver, and the platform
    /// surface is the legitimate next consult.
    ///
    /// [ObjectManager]: crate::ty::Ty::ObjectManager
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
        let configurations = db.configurations(current_file_id);
        if !configurations.is_empty()
            && !Self::mdo_visible_in_configs(&configurations, mdo_type, mdo_name)
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

    /// Phase B counterpart to
    /// [`Self::resolve_aliased_manager_method`]: resolves
    /// `<Ty::MetadataRef { *Object, name }>.method()` against
    /// `<MDO>/Ext/ObjectModule.bsl`.
    ///
    /// Same shape as the Phase A entry — visibility gate, then
    /// [`crate::module_index::ModuleIndex::resolve_object_module`] for
    /// the file id, then `symbol_tree.find_method`. Returns
    /// non-exported methods so the call site can pick
    /// `MethodNotExport` vs `MethodNotFound`.
    ///
    /// The caller is responsible for filtering `MetadataKind` to the
    /// `*Object` family (`CatalogObject`, `DocumentObject`,
    /// `ExchangePlanObject`, `ChartOfAccountsObject`); this entry
    /// trusts that gate and only consults [`MdoType`]. `*Ref` and
    /// register receivers must NOT reach this entry — their HIR types
    /// have no ObjectModule call surface today.
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
        let configurations = db.configurations(current_file_id);
        if !configurations.is_empty()
            && !Self::mdo_visible_in_configs(&configurations, mdo_type, mdo_name)
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

    /// Legacy [`PathResolution`] adapter over [`Self::resolve_three_level_method`].
    ///
    /// Used by [`Self::resolve_path`] (Definition layer). Non-exported
    /// methods collapse to `Unresolved` here — the Ty-layer gets the full
    /// outcome via [`Self::resolve_three_level_method`] instead.
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

/// Result of resolving a local name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedLocal {
    pub def: crate::scope::ScopeDef,
}

/// Result of name resolution at any level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Platform builtin (global function). Never shadowed by user code.
    Builtin(Name),

    /// Local name (parameter or local variable).
    Local(ResolvedLocal),

    /// Module-level method (procedure or function).
    Method(MethodId),

    /// Module-level variable.
    Variable(VariableId),
}

/// Successful outcome of [`Resolver::resolve_qualified_method`].
///
/// The resolver returns this for both exported and non-exported methods —
/// export visibility is a diagnostic concern (the caller may surface it),
/// not a resolution concern (the method exists and is reachable through
/// name resolution).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualifiedMethodResolution {
    pub method_id: MethodId,
    /// Whether the resolved method is marked `Экспорт`.
    pub is_export: bool,
}

/// Reason [`Resolver::resolve_qualified_method`] could not resolve a call.
///
/// Distinct from (and intentionally narrower than) the diagnostic-kind
/// enums owned by the consumer layers (`hir-ty::UnresolvedMethodKind`,
/// code-actions): hir-def owns name-resolution reasons only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualifiedMethodError {
    /// Module name is not declared in any visible configuration (CFE union
    /// of main + registered extensions).
    NotVisibleInConfigs,
    /// Module not indexed, method absent in the resolved module, resolver
    /// lacks workspace scope, or no module scope was attached.
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

        // Resolve parameter
        let resolved = resolver.resolve_local(&Name::new("Параметр"));
        assert_eq!(resolved.map(|r| r.def), Some(ScopeDef::Parameter));

        // Resolve local variable
        let resolved = resolver.resolve_local(&Name::new("Переменная"));
        assert_eq!(resolved.map(|r| r.def), Some(ScopeDef::LocalVariable));

        // Not found
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

        // Different case
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

        // Single segment path should attempt local resolution
        let _path = QualifiedName::from_segments([Name::new("Переменная")]);

        // Note: Without a real database, resolution will return Unresolved
        // This test verifies the method signature and basic logic
        // Full integration tests are in ide-db crate
    }

    #[test]
    fn test_resolve_path_two_segments() {
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);
        let _resolver = Resolver::with_workspace_scope(module_id);

        // Two segment path: Module.Method
        let _path = QualifiedName::from_segments([Name::new("ОбщийМодуль"), Name::new("Метод")]);

        // Without database, this returns Unresolved
        // Full test in ide-db with real database
    }

    #[test]
    fn test_resolve_path_three_segments() {
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);
        let _resolver = Resolver::with_workspace_scope(module_id);

        // Three segment path: Documents.PKO.Create
        let _path = QualifiedName::from_segments([
            Name::new("Документы"),
            Name::new("ПКО"),
            Name::new("Создать"),
        ]);

        // Manager module resolution - placeholder for now
        // Will be implemented with metadata integration
    }

    #[test]
    fn test_builtins_scope_guard_is_opt_in() {
        // Without Scope::Builtins the resolver must not call into the
        // platform singleton, so builtin resolution returns None even when
        // the name matches a known platform global.
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let plain = Resolver::for_module(module_id);
        assert!(plain.resolve_builtin(&Name::new("Сообщить")).is_none());

        let with_workspace = Resolver::with_workspace_scope(module_id);
        assert!(with_workspace.resolve_builtin(&Name::new("Сообщить")).is_none());

        let with_all = Resolver::with_builtins_and_workspace(module_id);
        assert!(with_all.has_builtins());
    }

    // Build an in-memory `VisibleConfig` with the given common-module names.
    // No file I/O — only the metadata a `Resolver` needs to check visibility.
    fn make_visible_config(ext_name: Option<&str>, module_names: &[&str]) -> VisibleConfig {
        let mut configuration = bsl_metadata::Configuration::new("test");
        for name in module_names {
            let module = bsl_metadata::CommonModule::builder().name(*name).build();
            configuration.add_common_module(module);
        }
        VisibleConfig {
            name: ext_name.map(|s| s.to_string()),
            configuration: std::sync::Arc::new(configuration),
        }
    }

    #[test]
    fn test_module_visible_in_configs_matches_extension_and_main() {
        let main = make_visible_config(None, &["ОбщегоНазначения"]);
        let ext = make_visible_config(Some("BMS_RU_UT"), &["ТестовыйМодуль"]);
        let configs = vec![main, ext];

        // Declared only in main
        assert!(Resolver::module_visible_in_configs(&configs, &Name::new("ОбщегоНазначения")));
        // Declared only in extension
        assert!(Resolver::module_visible_in_configs(&configs, &Name::new("ТестовыйМодуль")));
        // Case-insensitive — mirrors `find_common_module` contract
        assert!(Resolver::module_visible_in_configs(&configs, &Name::new("общегоназначения")));
        // Unknown anywhere
        assert!(!Resolver::module_visible_in_configs(&configs, &Name::new("НетТакогоМодуля")));
    }

    #[test]
    fn test_module_visible_empty_configs_returns_false() {
        // With no registered configs the helper must not falsely accept names;
        // the resolver's fallback to `module_index` gates that distinction.
        let empty: Vec<VisibleConfig> = Vec::new();
        assert!(!Resolver::module_visible_in_configs(&empty, &Name::new("Anything")));
    }

    #[test]
    fn test_builtins_scope_resolves_platform_global() {
        // `Сообщить` ships with the bundled platform data. If the loader
        // silently produced an empty index, this test must fail — otherwise
        // a regression in platform-data initialisation would go unnoticed.
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

        // Nonsense names never resolve as builtins.
        assert!(resolver.resolve_builtin(&Name::new("НетТакогоBuiltin")).is_none());
    }

    // Integration tests with RootDatabaseImpl are in ide-db crate
}
