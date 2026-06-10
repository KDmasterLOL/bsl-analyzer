use std::hash::BuildHasherDefault;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;
use rustc_hash::FxHasher;

use base_db::{FileIdInput, Files, RootQueryDb, SourceDatabase, SourceRoot, SourceRootId};
use hir::{
    ConditionalTree, DefDatabase, ItemTree, ModuleBodies, ModuleData, ModuleId, RegionTree,
    SymbolTree,
};
use vfs::FileId;

use crate::features::FeaturesInput;
use crate::metadata::MetadataDb;
use crate::queries::{
    line_index_query, liveness_analysis_query, method_cfg_query, module_metadata_query,
    reaching_definitions_query,
};
use crate::type_kernel::{TypeKernelHandle, TypeKernelInner, TypeKernelInput};
use crate::{metadata, queries, vfs_helpers, RootDatabase, SdblHirEntries};
use hir::{all_sdbl_in_file_query, sdbl_hir_for_file_query};

#[salsa::db]
pub struct RootDatabaseImpl {
    storage: salsa::Storage<Self>,

    files: Files,

    workspace_configs_input: parking_lot::RwLock<Option<metadata::WorkspaceConfigsInput>>,

    /// Per-config-root revision inputs, keyed by the canonical root path. Shared
    /// across cloned database handles (snapshots) so every handle reads and bumps
    /// the same Salsa input for a root. A config-dependent query reads the input
    /// for its file's root (recording a Salsa dependency); bumping that root's
    /// revision invalidates only those queries, not every configuration.
    config_revisions:
        Arc<DashMap<String, metadata::ConfigRevisionInput, BuildHasherDefault<FxHasher>>>,

    /// Per-config-root metadata *structure* inputs, keyed by the canonical root
    /// path. Shared across cloned handles like [`config_revisions`]. Each holds the
    /// list of MDOs discovered in that root; a structure change re-sets it, driving
    /// `config_index`/`resolve_metadata_object` re-derivation for that root only.
    metadata_listings:
        Arc<DashMap<String, metadata::MetadataListingInput, BuildHasherDefault<FxHasher>>>,

    /// Fallback revision input for files that match no registered config root
    /// (e.g. a single file opened without a workspace). Such reads record a
    /// dependency here; coarse "everything changed" events bump it.
    global_config_revision: parking_lot::RwLock<Option<metadata::ConfigRevisionInput>>,

    features_input: parking_lot::RwLock<Option<FeaturesInput>>,

    type_kernel: Arc<TypeKernelInner>,

    type_kernel_input: parking_lot::RwLock<Option<TypeKernelInput>>,

    /// Optional process-side cache of loaded configurations, keyed by the interned
    /// config-root path string (stable and consistent across this build's batch
    /// databases — not necessarily filesystem-canonical, which does not matter since
    /// every batch interns the same root the same way). Set only by the batched
    /// graph build, which opens a fresh
    /// database per batch: without it each batch would reload the whole
    /// configuration via [`metadata::load_configuration`] (re-running
    /// `bsl_metadata::load_from_directory` over every metadata file). A single
    /// `Arc` is shared across all of one build's batch databases (and their per-job
    /// clones), so the load happens once. `None` for the long-lived LSP database,
    /// which already memoises `load_configuration` per revision — there the field is
    /// absent and loading is unchanged.
    graph_config_cache: Option<Arc<GraphConfigCache>>,
}

/// Build-scoped cache of loaded configurations by interned config-root path string.
/// A fresh instance per build keeps it a content snapshot — a later build (new
/// instance) never sees a stale entry — so no version key is needed.
pub type GraphConfigCache = dashmap::DashMap<PathBuf, Arc<bsl_metadata::Configuration>>;

impl Default for RootDatabaseImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for RootDatabaseImpl {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
            files: self.files.clone(),
            workspace_configs_input: parking_lot::RwLock::new(*self.workspace_configs_input.read()),
            config_revisions: Arc::clone(&self.config_revisions),
            metadata_listings: Arc::clone(&self.metadata_listings),
            global_config_revision: parking_lot::RwLock::new(*self.global_config_revision.read()),
            features_input: parking_lot::RwLock::new(*self.features_input.read()),
            type_kernel: Arc::clone(&self.type_kernel),
            type_kernel_input: parking_lot::RwLock::new(*self.type_kernel_input.read()),
            // Share the same cache across clones so a per-job db clone sees configs
            // loaded by its siblings.
            graph_config_cache: self.graph_config_cache.clone(),
        }
    }
}

impl RootDatabaseImpl {
    pub fn new() -> Self {
        let type_kernel = Arc::new(TypeKernelInner::new());
        let db = Self {
            storage: salsa::Storage::default(),
            files: Files::new(),
            workspace_configs_input: parking_lot::RwLock::new(None),
            config_revisions: Arc::new(DashMap::default()),
            metadata_listings: Arc::new(DashMap::default()),
            global_config_revision: parking_lot::RwLock::new(None),
            features_input: parking_lot::RwLock::new(None),
            type_kernel: Arc::clone(&type_kernel),
            type_kernel_input: parking_lot::RwLock::new(None),
            graph_config_cache: None,
        };
        let input = metadata::WorkspaceConfigsInput::new(&db, Vec::new());
        *db.workspace_configs_input.write() = Some(input);
        let global_config_revision = metadata::ConfigRevisionInput::new(&db, 0);
        *db.global_config_revision.write() = Some(global_config_revision);
        let defaults = project_model::FeaturesConfig::default();
        let features = FeaturesInput::new(&db, defaults.type_narrowing);
        *db.features_input.write() = Some(features);
        let type_kernel_input = TypeKernelInput::new(&db, TypeKernelHandle::new(type_kernel));
        *db.type_kernel_input.write() = Some(type_kernel_input);
        db
    }

    pub(crate) fn type_kernel_inner(&self) -> &Arc<TypeKernelInner> {
        &self.type_kernel
    }

    /// Run salsa's LRU trim now, dropping memoized values beyond each query's
    /// configured `lru` cap and releasing their heap. salsa only trims
    /// automatically at a revision boundary, so a single-revision batch (seed
    /// inputs once, never mutate) must call this explicitly — e.g. between
    /// chunks of files — to bound resident memory. Requires exclusive `&mut`
    /// access and blocks on any live snapshot, so never call it mid-`par_iter`.
    pub fn enforce_lru(&mut self) {
        salsa::Database::trigger_lru_eviction(self);
    }

    /// Per-ingredient memory snapshot for diagnostics tooling, via salsa's
    /// `memory_usage` introspection (the `salsa_unstable` feature). Each tuple is
    /// `(ingredient name, live entry count, salsa metadata bytes, field stack bytes,
    /// optional heap bytes)`. `heap` is `None` unless the ingredient implements a
    /// `heap_size` hook, so the strongest signal here is `count` (whether LRU is
    /// actually evicting) rather than absolute bytes.
    pub fn memory_report(&self) -> Vec<(&'static str, usize, usize, usize, Option<usize>)> {
        let db: &dyn salsa::Database = self;
        let info = db.memory_usage();
        info.structs
            .iter()
            .chain(info.queries.values())
            .map(|ing| {
                (
                    ing.debug_name(),
                    ing.count(),
                    ing.size_of_metadata(),
                    ing.size_of_fields(),
                    ing.heap_size_of_fields(),
                )
            })
            .collect()
    }

    /// Attach a build-scoped configuration cache shared across this build's batch
    /// databases. See [`GraphConfigCache`] and the `graph_config_cache` field.
    pub fn set_graph_config_cache(&mut self, cache: Arc<GraphConfigCache>) {
        self.graph_config_cache = Some(cache);
    }

    /// Warm body inference for every method of a large module in parallel, before
    /// the sequential `infer_query` fold reads them from cache. Each method infers
    /// its own body and pulls callee return types via `method_return_type`, which
    /// projects `infer_method` — so a method's inference can recurse into another
    /// `infer_method`. Both queries carry salsa Fixpoint recovery, so recursive and
    /// mutually recursive call SCCs converge rather than panic, and the warming
    /// stays cycle safe under cross-thread demand. The payoff is when this module is
    /// the straggler tail of its batch chunk: the inner `par_iter` reclaims the
    /// cores left idle by the finished files. Returns the number of methods warmed,
    /// or 0 if the module has fewer than `min_methods` (priming a small module is
    /// pure overhead).
    pub fn prime_module_inference(&self, file_id: FileId, min_methods: usize) -> usize {
        use hir::HirDatabase;
        use rayon::prelude::*;

        let module_id = ModuleId { file_id };
        let method_ids: Vec<hir::MethodId> = self
            .module_bodies(module_id)
            .iter_bodies()
            .map(|(local_id, _)| hir::MethodId { module: module_id, local_id })
            .collect();
        if method_ids.len() < min_methods {
            return 0;
        }
        let n = method_ids.len();
        method_ids.par_iter().for_each_with(self.clone(), |db, &method_id| {
            let method_input = hir::MethodIdInput::new(&*db, method_id);
            let _ = db.infer_method(method_input);
        });
        n
    }

    fn workspace_configs(&self) -> metadata::WorkspaceConfigsInput {
        self.workspace_configs_input
            .read()
            .expect("workspace_configs_input is initialized in RootDatabaseImpl::new")
    }

    pub fn workspace_configs_input(&self) -> metadata::WorkspaceConfigsInput {
        self.workspace_configs()
    }

    fn global_config_revision_input(&self) -> metadata::ConfigRevisionInput {
        self.global_config_revision
            .read()
            .expect("global_config_revision is initialized in RootDatabaseImpl::new")
    }

    /// The current revision for a registered config root, as recorded in its
    /// Salsa [`ConfigRevisionInput`](metadata::ConfigRevisionInput). Reading the
    /// input field here records a dependency on that specific root for the
    /// enclosing tracked query, so a later [`bump_config_revision`] invalidates
    /// only the queries that touched this root. Unregistered roots fall back to
    /// the global revision input, so they still record a dependency (coarse).
    pub fn config_revision(&self, root: &str) -> u32 {
        let key = metadata::canonicalize_configuration_path(root);
        match self.config_revisions.get(&key).map(|e| *e.value()) {
            Some(input) => input.revision(self),
            None => self.global_config_revision_input().revision(self),
        }
    }

    /// The longest registered config root that is a prefix of `path` (the same
    /// matching rule used by both reads and bumps, so their revision keys always
    /// agree). Reading the workspace config paths records a dependency, so a
    /// workspace reload invalidates every config-dependent query.
    fn longest_config_root_for_path(&self, path: &Path) -> Option<PathBuf> {
        self.all_config_paths()
            .into_iter()
            .map(|(_, p)| p)
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.as_os_str().len())
    }

    /// The revision token to fold into a config's interned key for any reader
    /// concerned with `path` (a source file or a config root). Derives the root
    /// by [`longest_config_root_for_path`] so file readers and per-root config
    /// readers share one key; unmatched paths use the global fallback revision.
    pub fn config_root_revision_for_path(&self, path: &Path) -> u32 {
        match self.longest_config_root_for_path(path) {
            Some(root) => self.config_revision(&root.to_string_lossy()),
            None => self.global_config_revision_input().revision(self),
        }
    }

    pub fn try_file_text(&self, file_id: vfs::FileId) -> Option<base_db::FileTextInput> {
        self.files.try_file_text(file_id)
    }

    /// Create the [`ConfigRevisionInput`](metadata::ConfigRevisionInput) for a
    /// root if it does not exist yet. Must be called outside any tracked query
    /// (Salsa forbids creating inputs during query execution). Idempotent: an
    /// existing root keeps its accumulated revision so previously recorded
    /// dependencies stay valid across workspace reloads.
    pub fn ensure_config_revision_input(&mut self, root: &str) {
        let key = metadata::canonicalize_configuration_path(root);
        if self.config_revisions.contains_key(&key) {
            return;
        }
        let input = metadata::ConfigRevisionInput::new(self, 0);
        self.config_revisions.insert(key, input);
    }

    /// Bump one config root's revision, invalidating only the queries that read
    /// that root's configuration. Creates the input first if needed.
    pub fn bump_config_revision(&mut self, root: &str) {
        use salsa::Setter;
        self.ensure_config_revision_input(root);
        let key = metadata::canonicalize_configuration_path(root);
        let input = match self.config_revisions.get(&key).map(|e| *e.value()) {
            Some(input) => input,
            None => return,
        };
        let current = input.revision(self);
        input.set_revision(self).to(current.wrapping_add(1));
    }

    /// Bump the revision for the config root that owns `path`, matched the same
    /// way reads derive their revision key. A path under no registered root bumps
    /// the global fallback instead.
    pub fn bump_config_for_path(&mut self, path: &Path) {
        use salsa::Setter;
        match self.longest_config_root_for_path(path) {
            Some(root) => self.bump_config_revision(&root.to_string_lossy()),
            None => {
                let input = self.global_config_revision_input();
                let current = input.revision(self);
                input.set_revision(self).to(current.wrapping_add(1));
            }
        }
    }

    /// Bump the revision for every config root that owns one of `paths`, each
    /// root at most once (so an N-file batch under one root is a single revision
    /// write, not N). Paths under no registered root bump the global fallback once.
    pub fn bump_config_for_paths<'a, I>(&mut self, paths: I)
    where
        I: IntoIterator<Item = &'a Path>,
    {
        use salsa::Setter;
        let mut roots: Vec<String> = Vec::new();
        let mut bump_global = false;
        for path in paths {
            match self.longest_config_root_for_path(path) {
                Some(root) => {
                    let key = metadata::canonicalize_configuration_path(&root.to_string_lossy());
                    if !roots.contains(&key) {
                        roots.push(key);
                    }
                }
                None => bump_global = true,
            }
        }
        for key in &roots {
            self.bump_config_revision(key);
        }
        if bump_global {
            let input = self.global_config_revision_input();
            let current = input.revision(self);
            input.set_revision(self).to(current.wrapping_add(1));
        }
    }

    /// Bump every config revision (all registered roots plus the global
    /// fallback). Used when the change is not attributable to a single root
    /// (e.g. metadata watch registration completing after the bootstrap load).
    pub fn bump_all_config_revisions(&mut self) {
        use salsa::Setter;
        let mut inputs: Vec<metadata::ConfigRevisionInput> =
            self.config_revisions.iter().map(|e| *e.value()).collect();
        inputs.push(self.global_config_revision_input());
        for input in inputs {
            let current = input.revision(self);
            input.set_revision(self).to(current.wrapping_add(1));
        }
    }

    pub fn set_all_config_paths(&mut self, paths: Vec<(Option<String>, std::path::PathBuf)>) {
        use salsa::Setter;
        for (_, path) in &paths {
            self.ensure_config_revision_input(&path.to_string_lossy());
        }
        let input = self.workspace_configs();
        input.set_paths(self).to(paths);
    }

    pub fn all_config_paths(&self) -> Vec<(Option<String>, std::path::PathBuf)> {
        self.workspace_configs().paths(self)
    }

    /// Set (or update) one config root's metadata structure listing. Must run
    /// outside any tracked query. On reload it updates the existing input in place
    /// so previously recorded `config_index`/`resolve_metadata_object` dependencies
    /// stay valid and re-derive from the new structure.
    pub fn set_metadata_listing(
        &mut self,
        root: &str,
        entries: Vec<metadata::MdoEntry>,
        defined_types: Vec<metadata::DefinedTypeEntry>,
        common_modules: Vec<metadata::CommonModuleEntry>,
    ) {
        use salsa::Setter;
        let key = metadata::canonicalize_configuration_path(root);
        let entries = Arc::new(entries);
        let defined_types = Arc::new(defined_types);
        let common_modules = Arc::new(common_modules);
        match self.metadata_listings.get(&key).map(|e| *e.value()) {
            Some(input) => {
                input.set_entries(self).to(entries);
                input.set_defined_types(self).to(defined_types);
                input.set_common_modules(self).to(common_modules);
            }
            None => {
                let input = metadata::MetadataListingInput::new(
                    self,
                    entries,
                    defined_types,
                    common_modules,
                );
                self.metadata_listings.insert(key, input);
            }
        }
    }

    /// The metadata structure-listing input for a config root, if one was set.
    /// Resolution consumers fold this through `resolve_metadata_object`.
    pub fn metadata_listing(&self, root: &str) -> Option<metadata::MetadataListingInput> {
        let key = metadata::canonicalize_configuration_path(root);
        self.metadata_listings.get(&key).map(|e| *e.value())
    }

    /// For `file_id`: the structure listings of the main config root and the file's
    /// applicable extension root (the longest extension path that is a prefix of the
    /// file), plus whether the per-MDO substrate is populated (the bootstrap ran for
    /// the roots this resolution touches). `None` if the file has no config root.
    /// Shared by the object and register per-file resolvers so they pick the same
    /// roots and make the same bootstrapped-vs-fallback decision.
    fn metadata_listings_for_file(
        &self,
        file_id: FileId,
    ) -> Option<(
        Option<metadata::MetadataListingInput>,
        Option<metadata::MetadataListingInput>,
        bool,
    )> {
        let file_path = vfs_helpers::get_file_path(self, file_id)?;
        let paths = RootDatabaseImpl::all_config_paths(self);

        let main_path = paths.iter().find_map(|(label, path)| label.is_none().then_some(path));
        let extension_path = paths
            .iter()
            .filter(|(label, path)| label.is_some() && file_path.starts_with(path))
            .max_by_key(|(_, path)| path.as_os_str().len())
            .map(|(_, path)| path);

        let main_listing = main_path.map(|p| self.metadata_listing(&p.to_string_lossy()));
        let ext_listing = extension_path.map(|p| self.metadata_listing(&p.to_string_lossy()));
        // "Bootstrapped" requires the main root's listing to actually be set. When
        // there is no main config root at all (`all_config_paths` empty — the batch
        // / CLI path that never calls `set_workspace_configs`), `main_listing` is
        // `None`, which must read as "not bootstrapped" so the caller falls back to
        // the whole-config lookup. A main root present but without a listing
        // (`Some(None)`, batch/graph/tests) is likewise not bootstrapped. Only a
        // listing actually present (`Some(Some(_))`, the LSP bootstrap) — with no
        // applicable extension root left unbootstrapped — enables the per-MDO path.
        let bootstrapped =
            matches!(main_listing, Some(Some(_))) && !matches!(ext_listing, Some(None));

        Some((main_listing.flatten(), ext_listing.flatten(), bootstrapped))
    }

    /// Resolve a single metadata object visible to `file_id` at per-MDO Salsa
    /// granularity, composing the main config with the file's applicable extension
    /// via [`MetadataObject::apply_extension_overlay`] (or whichever side alone
    /// exists) — exactly as `merged_visible_configuration` does per object.
    ///
    /// Replaces a `merged_visible_configuration().find_metadata_object` lookup for
    /// the migrated consumers. When the per-MDO substrate is populated (the LSP
    /// bootstrap ran), it resolves through the per-MDO queries so the caller depends
    /// on only that MDO — editing an unrelated MDO does not invalidate it. When the
    /// substrate is absent (batch analysis, the graph build's per-batch DBs, tests),
    /// it falls back to the whole-config merged lookup: identical result, no
    /// narrowing. Returns `None` when the file has no registered config root.
    pub fn resolve_metadata_object_for_file(
        &self,
        file_id: FileId,
        mdo_type: bsl_metadata::MdoType,
        name: &str,
    ) -> Option<Arc<bsl_metadata::MetadataObject>> {
        let (main_listing, ext_listing, bootstrapped) = self.metadata_listings_for_file(file_id)?;

        if !bootstrapped {
            use hir::ConfigsDatabase;
            return self
                .merged_visible_configuration(file_id)?
                .find_metadata_object(mdo_type, name)
                .cloned()
                .map(Arc::new);
        }

        let resolve_in = |listing: Option<metadata::MetadataListingInput>| {
            listing.and_then(|l| {
                metadata::resolve_metadata_object(self, l, mdo_type, name.to_string())
            })
        };
        match (resolve_in(main_listing), resolve_in(ext_listing)) {
            (Some(main), Some(ext)) => {
                let mut merged = (*main).clone();
                merged.apply_extension_overlay(&ext);
                Some(Arc::new(merged))
            }
            (Some(main), None) => Some(main),
            (None, Some(ext)) => Some(ext),
            (None, None) => None,
        }
    }

    /// The register counterpart of [`resolve_metadata_object_for_file`]: resolve a
    /// single register visible to `file_id`, composing main + the file's extension
    /// via [`bsl_metadata::Register::apply_extension_overlay`]. Per-MDO when the
    /// substrate is populated, falling back to
    /// `merged_visible_configuration().find_register_by_type_and_name` otherwise.
    pub fn resolve_register_for_file(
        &self,
        file_id: FileId,
        mdo_type: bsl_metadata::MdoType,
        name: &str,
    ) -> Option<Arc<bsl_metadata::Register>> {
        let (main_listing, ext_listing, bootstrapped) = self.metadata_listings_for_file(file_id)?;

        if !bootstrapped {
            use hir::ConfigsDatabase;
            return self
                .merged_visible_configuration(file_id)?
                .find_register_by_type_and_name(mdo_type, name)
                .cloned()
                .map(Arc::new);
        }

        let resolve_in = |listing: Option<metadata::MetadataListingInput>| {
            listing.and_then(|l| metadata::resolve_register(self, l, mdo_type, name.to_string()))
        };
        match (resolve_in(main_listing), resolve_in(ext_listing)) {
            (Some(main), Some(ext)) => {
                let mut merged = (*main).clone();
                merged.apply_extension_overlay(&ext);
                Some(Arc::new(merged))
            }
            (Some(main), None) => Some(main),
            (None, Some(ext)) => Some(ext),
            (None, None) => None,
        }
    }

    /// Resolve a register visible to `file_id` by NAME alone (its kind unknown to
    /// the caller). The by-name counterpart of [`resolve_register_for_file`], with
    /// the same main + file's-extension overlay and per-MDO-or-fallback split.
    pub fn resolve_register_by_name_for_file(
        &self,
        file_id: FileId,
        name: &str,
    ) -> Option<Arc<bsl_metadata::Register>> {
        let (main_listing, ext_listing, bootstrapped) = self.metadata_listings_for_file(file_id)?;

        if !bootstrapped {
            use hir::ConfigsDatabase;
            return self
                .merged_visible_configuration(file_id)?
                .find_register(name)
                .cloned()
                .map(Arc::new);
        }

        let resolve_in = |listing: Option<metadata::MetadataListingInput>| {
            listing.and_then(|l| metadata::resolve_register_by_name(self, l, name.to_string()))
        };
        match (resolve_in(main_listing), resolve_in(ext_listing)) {
            // Same name in base and extension: a borrowed register (same kind)
            // merges; a same-name register of a *different* kind is not a borrow,
            // so the extension's wins outright — mirroring `merge_extension_overlay`
            // (which adds rather than merges on a kind mismatch, and `find_register`
            // then returns the extension's).
            (Some(main), Some(ext)) if main.mdo_type() == ext.mdo_type() => {
                let mut merged = (*main).clone();
                merged.apply_extension_overlay(&ext);
                Some(Arc::new(merged))
            }
            (Some(_), Some(ext)) => Some(ext),
            (Some(main), None) => Some(main),
            (None, Some(ext)) => Some(ext),
            (None, None) => None,
        }
    }

    /// The defined-type counterpart of [`resolve_metadata_object_for_file`]:
    /// resolve a defined type's underlying type visible to `file_id`. A defined
    /// type's overlay replaces the underlying type wholesale, so the file's
    /// applicable extension wins outright over the base (no field merge). Per-
    /// defined-type when the substrate is populated, falling back to
    /// `merged_visible_configuration().resolve_defined_type` otherwise.
    pub fn resolve_defined_type_for_file(
        &self,
        file_id: FileId,
        name: &str,
    ) -> Option<bsl_metadata::AttributeType> {
        let (main_listing, ext_listing, bootstrapped) = self.metadata_listings_for_file(file_id)?;

        if !bootstrapped {
            use bsl_metadata::MetadataResolver;
            use hir::ConfigsDatabase;
            return self.merged_visible_configuration(file_id)?.resolve_defined_type(name);
        }

        let resolve_in = |listing: Option<metadata::MetadataListingInput>| {
            listing.and_then(|l| metadata::resolve_defined_type(self, l, name.to_string()))
        };
        resolve_in(ext_listing)
            .or_else(|| resolve_in(main_listing))
            .map(|underlying| (*underlying).clone())
    }

    /// The common-module counterpart of [`resolve_metadata_object_for_file`]:
    /// resolve a common module's metadata by name visible to `file_id` — the base
    /// config plus the file's own extension, with the extension winning. A main-config
    /// common module is visible everywhere; an extension's common module is visible
    /// only within that extension (a *sibling* extension's modules are not), the same
    /// scoping as metadata objects. Per-common-module when the substrate is populated,
    /// falling back to a per-config scan otherwise — `merge_extension_overlay` does
    /// not fold common modules into the merged configuration, so the fallback cannot
    /// go through `merged_visible_configuration`.
    pub fn resolve_common_module_for_file(
        &self,
        file_id: FileId,
        name: &str,
    ) -> Option<Arc<bsl_metadata::CommonModule>> {
        let (main_listing, ext_listing, bootstrapped) = self.metadata_listings_for_file(file_id)?;

        if bootstrapped {
            let resolve_in = |listing: Option<metadata::MetadataListingInput>| {
                listing.and_then(|l| metadata::resolve_common_module(self, l, name.to_string()))
            };
            return resolve_in(ext_listing).or_else(|| resolve_in(main_listing));
        }

        let file_path = vfs_helpers::get_file_path(self, file_id)?;
        let paths = RootDatabaseImpl::all_config_paths(self);

        let find_in = |root: &std::path::Path| -> Option<Arc<bsl_metadata::CommonModule>> {
            let path_input = metadata::intern_configuration_path(
                self,
                &root.to_string_lossy(),
                self.config_root_revision_for_path(root),
            );
            self.load_configuration(path_input).find_common_module(name).cloned().map(Arc::new)
        };

        if paths.is_empty() {
            let config_root = vfs_helpers::find_configuration_root(self, &file_path)?;
            return find_in(&config_root);
        }

        let main_path = paths.iter().find_map(|(label, path)| label.is_none().then_some(path));
        let extension_path = paths
            .iter()
            .filter(|(label, path)| label.is_some() && file_path.starts_with(path))
            .max_by_key(|(_, path)| path.as_os_str().len())
            .map(|(_, path)| path);

        extension_path.and_then(|p| find_in(p)).or_else(|| main_path.and_then(|p| find_in(p)))
    }

    /// Resolve the common module that owns the `Ext/Module.bsl` whose id is
    /// `module_file_id` (typically the file currently being analysed), composing the
    /// roots visible to it. Answers "is this `.bsl` a common module's source, and if
    /// so which?" via the per-root reverse index when the substrate is populated,
    /// falling back to a root-relative URI scan over the merged configuration's
    /// common modules otherwise.
    pub fn common_module_for_file_id(
        &self,
        module_file_id: FileId,
    ) -> Option<Arc<bsl_metadata::CommonModule>> {
        let (main_listing, ext_listing, bootstrapped) =
            self.metadata_listings_for_file(module_file_id)?;

        if bootstrapped {
            let resolve_in = |listing: Option<metadata::MetadataListingInput>| {
                listing
                    .and_then(|l| metadata::resolve_common_module_by_file(self, l, module_file_id))
            };
            return resolve_in(ext_listing).or_else(|| resolve_in(main_listing));
        }

        let file_path = vfs_helpers::get_file_path(self, module_file_id)?;
        let file_path_lower = file_path.to_string_lossy().to_lowercase();
        let paths = RootDatabaseImpl::all_config_paths(self);

        let load_at = |path: &std::path::Path| -> Arc<bsl_metadata::Configuration> {
            let path_input = metadata::intern_configuration_path(
                self,
                &path.to_string_lossy(),
                self.config_root_revision_for_path(path),
            );
            self.load_configuration(path_input)
        };

        let find_in = |root: &std::path::Path| -> Option<Arc<bsl_metadata::CommonModule>> {
            let config = load_at(root);
            // `root.join(uri).to_lowercase() == file_path_lower` reduces to a relative
            // lookup: strip the (lowercased) root prefix and match the module's folded
            // root-relative URI, so the O(all-modules) per-call Cyrillic re-fold is gone.
            // The separator between root and remainder is mandatory so a sibling whose
            // name merely starts with the root (`/cfg` vs `/cfgX`) is not a false match.
            let root_lower = root.to_string_lossy().to_lowercase();
            let root_lower = root_lower.strip_suffix(['/', '\\']).unwrap_or(&root_lower);
            let rel = file_path_lower.strip_prefix(root_lower)?.strip_prefix(['/', '\\'])?;
            config.find_common_module_by_uri_lower(rel).cloned().map(Arc::new)
        };

        if paths.is_empty() {
            let config_root = vfs_helpers::find_configuration_root(self, &file_path)?;
            return find_in(&config_root);
        }

        let main_path = paths.iter().find_map(|(name, path)| name.is_none().then_some(path));
        let extension_path = paths
            .iter()
            .filter(|(name, path)| name.is_some() && file_path.starts_with(path))
            .max_by_key(|(_, path)| path.as_os_str().len())
            .map(|(_, path)| path);

        main_path.and_then(|p| find_in(p)).or_else(|| extension_path.and_then(|p| find_in(p)))
    }

    /// The `Ext/Module.bsl` body file id(s) of the common module `name` visible to
    /// `file_id` — base + the file's own extension. A borrowed module has a base
    /// body and an extension body; both are returned so method/parameter validation
    /// sees the merged surface. The scoped counterpart of the former all-config
    /// `find_common_module_files_anywhere`: body ids come straight from the substrate
    /// when bootstrapped, otherwise from a scoped root-relative URI scan.
    pub fn resolve_common_module_files_for_file(&self, file_id: FileId, name: &str) -> Vec<FileId> {
        let Some((main_listing, ext_listing, bootstrapped)) =
            self.metadata_listings_for_file(file_id)
        else {
            return Vec::new();
        };

        let mut out: Vec<FileId> = Vec::new();

        if bootstrapped {
            for listing in [ext_listing, main_listing].into_iter().flatten() {
                if let Some(fid) =
                    metadata::common_module_index(self, listing).lookup_module_file(name)
                {
                    if !out.contains(&fid) {
                        out.push(fid);
                    }
                }
            }
            return out;
        }

        use bsl_metadata::traits::Module;

        let Some(file_path) = vfs_helpers::get_file_path(self, file_id) else {
            return out;
        };
        let paths = RootDatabaseImpl::all_config_paths(self);

        let body_in = |root: &std::path::Path| -> Option<FileId> {
            let path_input = metadata::intern_configuration_path(
                self,
                &root.to_string_lossy(),
                self.config_root_revision_for_path(root),
            );
            let config = self.load_configuration(path_input);
            let uri = config.find_common_module(name)?.uri()?;
            let vfs_path = vfs::VfsPath::new(root.join(uri).to_string_lossy().into_owned());
            self.resolve_vfs_path(SourceRootId(0), &vfs_path)
        };

        if paths.is_empty() {
            if let Some(root) = vfs_helpers::find_configuration_root(self, &file_path) {
                if let Some(fid) = body_in(&root) {
                    out.push(fid);
                }
            }
            return out;
        }

        let main_path = paths.iter().find_map(|(label, path)| label.is_none().then_some(path));
        let extension_path = paths
            .iter()
            .filter(|(label, path)| label.is_some() && file_path.starts_with(path))
            .max_by_key(|(_, path)| path.as_os_str().len())
            .map(|(_, path)| path);

        for root in [extension_path, main_path].into_iter().flatten() {
            if let Some(fid) = body_in(root) {
                if !out.contains(&fid) {
                    out.push(fid);
                }
            }
        }
        out
    }

    fn features(&self) -> FeaturesInput {
        self.features_input.read().expect("features_input is initialized in RootDatabaseImpl::new")
    }

    pub fn type_narrowing_enabled(&self) -> bool {
        self.features().type_narrowing(self)
    }

    pub fn set_type_narrowing_enabled(&mut self, enabled: bool) {
        use salsa::Setter;
        let input = self.features();
        input.set_type_narrowing(self).to(enabled);
    }

    pub(crate) fn get_file_path(&self, file_id: FileId) -> Option<PathBuf> {
        let source_root_input = self.file_source_root_input(file_id);
        let source_root_id = source_root_input.source_root_id(self);
        let source_root_input = self.source_root_input(source_root_id);
        let source_root = source_root_input.root(self);
        let file_set = source_root.file_set();
        let vfs_path = file_set.path_for_file(&file_id)?;
        Some(PathBuf::from(vfs_path.as_path()))
    }

    pub(crate) fn find_configuration_root(&self, file_path: &Path) -> Option<PathBuf> {
        let mut current = file_path.parent()?;

        loop {
            let common_modules = current.join("CommonModules");
            if common_modules.is_dir() {
                tracing::debug!(?current, "Found configuration root via CommonModules/");
                return Some(current.to_path_buf());
            }

            let config_xml = current.join("Configuration.xml");
            if config_xml.is_file() {
                tracing::debug!(?current, "Found configuration root via Configuration.xml");
                return Some(current.to_path_buf());
            }

            current = match current.parent() {
                Some(parent) if parent != current => parent,
                _ => return None,
            };
        }
    }
}

#[salsa::db]
impl salsa::Database for RootDatabaseImpl {}

#[salsa::db]
impl SourceDatabase for RootDatabaseImpl {
    fn file_text_input(&self, file_id: FileId) -> base_db::FileTextInput {
        self.files.file_text(file_id)
    }

    fn try_file_text_input(&self, file_id: FileId) -> Option<base_db::FileTextInput> {
        self.files.try_file_text(file_id)
    }

    fn file_revision_input(&self, file_id: FileId) -> base_db::FileRevisionInput {
        self.files.file_revision(file_id)
    }

    fn try_file_revision_input(&self, file_id: FileId) -> Option<base_db::FileRevisionInput> {
        self.files.try_file_revision(file_id)
    }

    fn file_text(&self, file_id: FileId) -> std::sync::Arc<str> {
        let input = base_db::FileIdInput::new(self, file_id);
        base_db::file_text_query(self, input)
    }

    fn set_file_revision_from_disk(&mut self, file_id: FileId, revision: u64) {
        let files = self.files.clone();
        files.set_file_revision_from_disk(self, file_id, revision);
    }

    fn source_root_input(&self, source_root_id: SourceRootId) -> base_db::SourceRootInput {
        self.files.source_root(source_root_id)
    }

    fn file_source_root_input(&self, file_id: FileId) -> base_db::FileSourceRootInput {
        self.files.file_source_root(file_id)
    }

    fn set_file_text(&mut self, file_id: FileId, text: &str) {
        let files = self.files.clone();
        files.set_file_text_smart(self, file_id, text);
    }

    fn set_file_source_root(&mut self, file_id: FileId, source_root_id: SourceRootId) {
        let files = self.files.clone();
        files.set_file_source_root(self, file_id, source_root_id);
    }

    fn set_source_root(&mut self, source_root_id: SourceRootId, source_root: SourceRoot) {
        let files = self.files.clone();
        files.set_source_root(self, source_root_id, source_root);
    }

    fn resolve_vfs_path(
        &self,
        source_root_id: SourceRootId,
        vfs_path: &vfs::VfsPath,
    ) -> Option<FileId> {
        let source_root_input = self.source_root_input(source_root_id);
        let vfs_path_str = vfs_path.as_path().to_string_lossy().to_string();
        base_db::resolve_vfs_path_query(self, source_root_input, vfs_path_str)
    }
}

#[salsa::db]
impl RootQueryDb for RootDatabaseImpl {
    fn parse(&self, file_id: FileId) -> syntax::Parse<syntax::SyntaxNode> {
        let input = base_db::FileIdInput::new(self, file_id);
        base_db::parse_query(self, input)
    }

    fn method_regions(
        &self,
        file_id: FileId,
    ) -> Arc<std::collections::HashMap<syntax::TextRange, String>> {
        let input = base_db::FileIdInput::new(self, file_id);
        base_db::method_regions_query(self, input)
    }
}

#[salsa::db]
impl DefDatabase for RootDatabaseImpl {
    fn item_tree(&self, file_id: FileId) -> Arc<ItemTree> {
        let file_id_input = base_db::FileIdInput::new(self, file_id);
        hir::item_tree_query(self, file_id_input)
    }

    fn region_tree(&self, file_id: FileId) -> Arc<RegionTree> {
        let file_id_input = base_db::FileIdInput::new(self, file_id);
        hir::region_tree_query(self, file_id_input)
    }

    fn conditional_tree(&self, file_id: FileId) -> Arc<ConditionalTree> {
        let file_id_input = base_db::FileIdInput::new(self, file_id);
        hir::conditional_tree_query(self, file_id_input)
    }

    fn module_data(&self, module_id: ModuleId) -> Arc<ModuleData> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir::module_data_query(self, file_id_input)
    }

    fn symbol_tree(&self, module_id: ModuleId) -> Arc<SymbolTree> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir::symbol_tree_query(self, file_id_input)
    }

    fn module_bodies(&self, module_id: ModuleId) -> Arc<ModuleBodies> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir::module_bodies_query(self, file_id_input)
    }

    fn method_body(&self, method: hir::MethodIdInput<'_>) -> Arc<hir::Body> {
        hir::method_body_query(self, method)
    }

    fn method_body_with_source_map(
        &self,
        method: hir::MethodIdInput<'_>,
    ) -> Arc<(hir::Body, hir::BodySourceMap)> {
        hir::method_body_with_source_map_query(self, method)
    }

    fn module_metadata(&self, module_id: ModuleId) -> Arc<hir::ModuleMetadata> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        module_metadata_query(self, file_id_input)
    }

    fn module_call_summary(&self, module_id: ModuleId) -> Arc<hir::ModuleCallSummary> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir::module_call_summary_query(self, file_id_input)
    }

    fn method_docs(&self, method: hir::MethodId) -> Option<Arc<hir::MethodDocs>> {
        let symbol_tree = self.symbol_tree(method.module);
        let method_symbol = symbol_tree.find_method_by_id(method)?;
        method_symbol.docs.clone()
    }

    fn variable_docs(&self, variable: hir::VariableId) -> Option<Arc<hir::VariableDocs>> {
        let symbol_tree = self.symbol_tree(variable.module);
        let variable_symbol = symbol_tree.find_variable_by_id(variable)?;
        variable_symbol.docs.clone()
    }

    fn workspace_symbols(
        &self,
        source_root_id: base_db::SourceRootId,
    ) -> Arc<hir::WorkspaceSymbols> {
        let source_root_input = self.source_root_input(source_root_id);
        hir::workspace_symbols_query(self, source_root_input)
    }

    fn workspace_index(&self, source_root_id: base_db::SourceRootId) -> Arc<hir::WorkspaceIndex> {
        let source_root_input = self.source_root_input(source_root_id);
        hir::workspace_index_query(self, source_root_input)
    }

    fn name_usage_index(
        &self,
        source_root_id: base_db::SourceRootId,
    ) -> Arc<hir::SourceRootNameUsage> {
        let source_root_input = self.source_root_input(source_root_id);
        hir::source_root_name_usage_query(self, source_root_input)
    }

    fn file_external_refs(&self, module_id: ModuleId) -> Arc<Vec<hir::ExternalRef>> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir::file_external_refs_query(self, file_id_input)
    }

    fn module_index(&self, source_root_id: base_db::SourceRootId) -> Arc<hir::ModuleIndex> {
        let source_root_input = self.source_root_input(source_root_id);
        hir::module_index_query(self, source_root_input)
    }

    fn file_dependencies(&self, module_id: ModuleId) -> Arc<Vec<FileId>> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir::file_dependencies_query(self, file_id_input)
    }
}

#[salsa::db]
impl hir::ConfigsDatabase for RootDatabaseImpl {
    fn configurations(&self, file_id: FileId) -> Vec<hir::VisibleConfig> {
        RootDatabase::get_all_configurations(self, file_id)
            .into_iter()
            .map(|(name, configuration)| hir::VisibleConfig { name, configuration })
            .collect()
    }

    fn resolve_metadata_object(
        &self,
        file_id: FileId,
        mdo_type: bsl_metadata::MdoType,
        name: &str,
    ) -> Option<Arc<bsl_metadata::MetadataObject>> {
        RootDatabaseImpl::resolve_metadata_object_for_file(self, file_id, mdo_type, name)
    }

    fn resolve_register(
        &self,
        file_id: FileId,
        mdo_type: bsl_metadata::MdoType,
        name: &str,
    ) -> Option<Arc<bsl_metadata::Register>> {
        RootDatabaseImpl::resolve_register_for_file(self, file_id, mdo_type, name)
    }

    fn resolve_register_by_name(
        &self,
        file_id: FileId,
        name: &str,
    ) -> Option<Arc<bsl_metadata::Register>> {
        RootDatabaseImpl::resolve_register_by_name_for_file(self, file_id, name)
    }

    fn resolve_defined_type(
        &self,
        file_id: FileId,
        name: &str,
    ) -> Option<bsl_metadata::AttributeType> {
        RootDatabaseImpl::resolve_defined_type_for_file(self, file_id, name)
    }

    fn resolve_common_module(
        &self,
        file_id: FileId,
        name: &str,
    ) -> Option<Arc<bsl_metadata::CommonModule>> {
        RootDatabaseImpl::resolve_common_module_for_file(self, file_id, name)
    }

    fn has_config_root(&self, file_id: FileId) -> bool {
        !RootDatabaseImpl::all_config_paths(self).is_empty()
            || RootDatabase::get_configuration(self, file_id).is_some()
    }

    fn file_has_visible_config(&self, file_id: FileId) -> bool {
        let Some(file_path) = vfs_helpers::get_file_path(self, file_id) else {
            return false;
        };

        let paths = RootDatabaseImpl::all_config_paths(self);
        if paths.is_empty() {
            return vfs_helpers::find_configuration_root(self, &file_path).is_some();
        }

        let has_main = paths.iter().any(|(name, _)| name.is_none());
        let has_applicable_extension =
            paths.iter().any(|(name, path)| name.is_some() && file_path.starts_with(path));

        has_main || has_applicable_extension
    }

    fn recorders_for_register(
        &self,
        file_id: FileId,
        parent: bsl_metadata::MdoType,
        register_name: &str,
    ) -> Vec<String> {
        // A reverse relation (all documents writing to the register) cannot narrow
        // to one MDO, so it reads the whole merged visible configuration for now; a
        // per-document reverse index is a follow-up.
        self.merged_visible_configuration(file_id)
            .map(|config| {
                config
                    .recorders_for_register(parent, register_name)
                    .iter()
                    .map(|n| n.as_str().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn merged_visible_configuration(
        &self,
        file_id: FileId,
    ) -> Option<Arc<bsl_metadata::Configuration>> {
        let file_path = vfs_helpers::get_file_path(self, file_id)?;
        let paths = RootDatabaseImpl::all_config_paths(self);

        let load_at = |path: &std::path::Path| -> Arc<bsl_metadata::Configuration> {
            let path_input = metadata::intern_configuration_path(
                self,
                &path.to_string_lossy(),
                self.config_root_revision_for_path(path),
            );
            self.load_configuration(path_input)
        };

        if paths.is_empty() {
            let config_root = vfs_helpers::find_configuration_root(self, &file_path)?;
            return Some(load_at(&config_root));
        }

        let main_path = paths.iter().find_map(|(name, path)| name.is_none().then_some(path));
        let extension_path = paths
            .iter()
            .filter(|(name, path)| name.is_some() && file_path.starts_with(path))
            .max_by_key(|(_, path)| path.as_os_str().len())
            .map(|(_, path)| path);

        match (main_path, extension_path) {
            (Some(main_path), Some(extension_path)) => {
                let main = load_at(main_path);
                let extension = load_at(extension_path);
                Some(Arc::new(main.merged_with_extension(&extension)))
            }
            (Some(main_path), None) => Some(load_at(main_path)),
            (None, Some(extension_path)) => Some(load_at(extension_path)),
            (None, None) => None,
        }
    }

    fn resolved_module_summary(
        &self,
        module_id: ModuleId,
    ) -> Arc<hir::call_graph::ResolvedModuleSummary> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        hir::resolved_module_summary_query(self, file_id_input)
    }

    fn workspace_call_graph(
        &self,
        source_root_id: base_db::SourceRootId,
    ) -> Arc<hir::call_graph::WorkspaceCallGraph> {
        let source_root_input = self.source_root_input(source_root_id);
        hir::workspace_call_graph_query(self, source_root_input)
    }
}

#[salsa::db]
impl hir::HirDatabase for RootDatabaseImpl {
    fn infer(&self, file_id: FileId) -> Arc<hir::InferenceResult> {
        let file_id_input = FileIdInput::new(self, file_id);
        hir::infer_query(self, file_id_input)
    }

    fn type_of_expr(
        &self,
        file_id: FileId,
        owner: hir::DefWithBodyId,
        expr: hir::ExprId,
    ) -> hir::TypeId {
        hir::type_of_expr_query(self, file_id, owner, expr)
    }

    fn narrow(
        &self,
        file_id: FileId,
        owner: hir::DefWithBodyId,
    ) -> Option<Arc<hir::dataflow::DataflowResult<hir::NarrowState>>> {
        hir::narrow_query(self, file_id, owner)
    }

    fn arg_diagnostics(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir::DefWithBodyId, hir::InferenceDiagnostic)>> {
        hir::arg_diagnostics_query(self, file_id)
    }

    fn type_narrowing_enabled(&self) -> bool {
        RootDatabaseImpl::type_narrowing_enabled(self)
    }

    fn proc_signature(
        &self,
        method_input: hir::MethodIdInput<'_>,
    ) -> Arc<hir::proc_signature::ProcSignature> {
        hir::proc_signature::proc_signature_query(self, method_input)
    }

    fn infer_method(&self, method: hir::MethodIdInput<'_>) -> Arc<hir::BodyInferenceResult> {
        hir::infer_method_query(self, method)
    }

    fn infer_module_code(&self, file_id: FileId) -> Arc<hir::ModuleCodeInferenceResult> {
        let file_id_input = FileIdInput::new(self, file_id);
        hir::infer_module_code_query(self, file_id_input)
    }

    fn module_reaching_definitions(
        &self,
        file_id: FileId,
    ) -> Arc<hir::dataflow::reaching_defs::ModuleReachingDefs> {
        let file_id_input = FileIdInput::new(self, file_id);
        queries::module_reaching_definitions_query(self, file_id_input)
    }
}

#[salsa::db]
impl RootDatabase for RootDatabaseImpl {
    fn get_configuration(&self, file_id: FileId) -> Option<Arc<bsl_metadata::Configuration>> {
        let file_path = vfs_helpers::get_file_path(self, file_id)?;
        let config_root = vfs_helpers::find_configuration_root(self, &file_path)?;
        let path_input = metadata::intern_configuration_path(
            self,
            &config_root.to_string_lossy(),
            self.config_root_revision_for_path(&file_path),
        );
        Some(self.load_configuration(path_input))
    }

    fn get_all_configurations(
        &self,
        file_id: FileId,
    ) -> Vec<(Option<String>, Arc<bsl_metadata::Configuration>)> {
        let all_paths = RootDatabaseImpl::all_config_paths(self);

        if all_paths.is_empty() {
            return self.get_configuration(file_id).into_iter().map(|c| (None, c)).collect();
        }

        all_paths
            .into_iter()
            .map(|(name, path)| {
                let path_input = metadata::intern_configuration_path(
                    self,
                    &path.to_string_lossy(),
                    self.config_root_revision_for_path(&path),
                );
                let config = self.load_configuration(path_input);
                (name, config)
            })
            .collect()
    }

    fn all_config_paths(&self) -> Vec<(Option<String>, std::path::PathBuf)> {
        RootDatabaseImpl::all_config_paths(self)
    }

    fn common_module_for_file_id(
        &self,
        module_file_id: FileId,
    ) -> Option<Arc<bsl_metadata::CommonModule>> {
        RootDatabaseImpl::common_module_for_file_id(self, module_file_id)
    }

    fn resolve_common_module_files(&self, file_id: FileId, name: &str) -> Vec<FileId> {
        RootDatabaseImpl::resolve_common_module_files_for_file(self, file_id, name)
    }

    fn all_sdbl_in_file(
        &self,
        file_id: FileId,
    ) -> Arc<Vec<(hir::SdblExprId, syntax::SdblQueryInfo)>> {
        let file_id_input = base_db::FileIdInput::new(self, file_id);
        all_sdbl_in_file_query(self, file_id_input)
    }

    fn sdbl_hir_in_file(&self, file_id: FileId) -> SdblHirEntries {
        let file_id_input = base_db::FileIdInput::new(self, file_id);
        sdbl_hir_for_file_query(self, file_id_input)
    }

    fn module_cfgs(&self, file_id_input: FileIdInput) -> Arc<hir::cfg::ModuleCfgs> {
        queries::module_cfgs_query(self, file_id_input)
    }

    fn module_reaching_definitions(
        &self,
        file_id_input: FileIdInput,
    ) -> Arc<hir::dataflow::reaching_defs::ModuleReachingDefs> {
        queries::module_reaching_definitions_query(self, file_id_input)
    }

    fn module_path_terminates(
        &self,
        file_id_input: FileIdInput,
    ) -> Arc<hir::dataflow::path_terminates::ModulePathTerminates> {
        queries::module_path_terminates_query(self, file_id_input)
    }

    fn module_liveness_analysis(
        &self,
        file_id_input: FileIdInput,
    ) -> Arc<hir::dataflow::liveness::ModuleLiveness> {
        queries::module_liveness_analysis_query(self, file_id_input)
    }

    fn reaching_definitions(
        &self,
        method_id: hir::MethodId,
    ) -> Option<Arc<hir::dataflow::reaching_defs::ReachingDefsResult>> {
        let method_id_input = hir::MethodIdInput::new(self, method_id);
        reaching_definitions_query(self, method_id_input)
    }

    fn liveness_analysis(
        &self,
        method_id: hir::MethodId,
    ) -> Option<Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>> {
        let method_id_input = hir::MethodIdInput::new(self, method_id);
        liveness_analysis_query(self, method_id_input)
    }

    fn method_cfg(&self, method_id: hir::MethodId) -> Arc<hir::cfg::ControlFlowGraph> {
        let method_id_input = hir::MethodIdInput::new(self, method_id);
        method_cfg_query(self, method_id_input)
    }

    fn module_level_cfg(&self, module_id: hir::ModuleId) -> Arc<hir::cfg::ControlFlowGraph> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        queries::module_level_cfg_query(self, file_id_input)
    }

    fn module_level_liveness_analysis(
        &self,
        module_id: hir::ModuleId,
    ) -> Option<Arc<hir::dataflow::DataflowResult<hir::dataflow::liveness::Liveness>>> {
        let file_id_input = base_db::FileIdInput::new(self, module_id.file_id);
        queries::module_level_liveness_analysis_query(self, file_id_input)
    }

    fn line_index(&self, file_id_input: base_db::FileIdInput) -> Arc<line_index::LineIndex> {
        line_index_query(self, file_id_input)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn config_root_revision_for_path(&self, path: &Path) -> u32 {
        RootDatabaseImpl::config_root_revision_for_path(self, path)
    }
}

#[salsa::db]
impl metadata::MetadataDb for RootDatabaseImpl {
    /// Override the default loader to consult the build-scoped cache when one is
    /// attached, so the whole-config metadata load runs once per config root per
    /// build instead of once per fresh batch database. This is the single chokepoint
    /// every config read funnels through (the resolver's `find_*`, `module_metadata`,
    /// `configurations`/`merged_visible_configuration`), so caching here covers them
    /// all. With no cache attached (the LSP database) it is the plain salsa query.
    fn load_configuration<'db>(
        &'db self,
        path_input: metadata::ConfigurationPathInput<'db>,
    ) -> Arc<bsl_metadata::Configuration> {
        let Some(cache) = &self.graph_config_cache else {
            return metadata::load_configuration(self, path_input);
        };
        let key = PathBuf::from(path_input.path(self));
        if let Some(config) = cache.get(&key) {
            return Arc::clone(&config);
        }
        // Miss: load (the build warms each root sequentially before its parallel
        // region, so concurrent first-loads of the same root do not occur; a rare
        // duplicate load would only repeat pure work, never corrupt the result).
        let config = metadata::load_configuration(self, path_input);
        cache.insert(key, Arc::clone(&config));
        config
    }
}

#[cfg(test)]
#[path = "database_impl_tests.rs"]
mod database_impl_tests;
