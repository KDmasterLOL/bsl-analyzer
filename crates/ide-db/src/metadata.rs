use bsl_metadata::Configuration;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use stdx::case::CaseExt;

#[salsa::interned(debug)]
pub struct ConfigurationPathInput {
    pub path: String,
    pub root_revision: u32,
}

pub fn intern_configuration_path<'db>(
    db: &'db dyn salsa::Database,
    raw_path: &str,
    root_revision: u32,
) -> ConfigurationPathInput<'db> {
    let canonical = canonicalize_configuration_path(raw_path);
    ConfigurationPathInput::new(db, canonical, root_revision)
}

/// Per-config-root revision counter, as a Salsa input so that config-dependent
/// queries which read it (via [`intern_configuration_path`] callers running
/// inside a tracked query) record a dependency on the specific root. Bumping one
/// root's revision then invalidates only the queries that touched that root,
/// instead of a single global counter invalidating every configuration.
#[salsa::input(debug)]
pub struct ConfigRevisionInput {
    pub revision: u32,
}

pub(crate) fn canonicalize_configuration_path(raw_path: &str) -> String {
    if cfg!(windows) {
        let trimmed = raw_path.strip_prefix(r"\\?\").unwrap_or(raw_path);
        let mut s = trimmed.replace('\\', "/");
        s.make_ascii_lowercase();
        s
    } else if raw_path.contains('\\') {
        raw_path.replace('\\', "/")
    } else {
        raw_path.to_owned()
    }
}

#[salsa::input(debug)]
pub struct WorkspaceConfigsInput {
    pub paths: Vec<(Option<String>, PathBuf)>,
}

/// Whether the host's initial workspace load has completed. Defaults to `true`
/// (batch analysis, the graph build, MCP and tests have no boot window); the
/// LSP server sets it to `false` while the initial VFS scan streams in and back
/// to `true` in its finalize, right before the metadata bootstrap and warm-up.
///
/// The whole-configuration loader consults it so that nothing computed during
/// the boot window can trigger the full-config XML parse — minutes of
/// non-cancellable work inside a single query, against a workspace that is not
/// even fully on disk in the VFS yet. The flag is a Salsa input (not a plain
/// field) so the read inside a calling query records a dependency: the finalize
/// flip then invalidates anything that resolved against the gated stub.
#[salsa::input(debug)]
pub struct WorkspaceLoadStateInput {
    pub complete: bool,
}

// Keyed by config root (base config + each extension), so the cache holds one entry
// per root, not per file/module — its size tracks the number of configurations, which
// is small. The cap must exceed the realistic number of extension roots: the graph
// build pre-warms every root before its parallel region (a per-root reload there would
// re-enter the metadata loader's `rayon::scope` inside a worker thread), so an eviction
// under the cap would let that load run in parallel and break the build's concurrency
// invariant. 1024 is far above any real extension count while still bounded.
#[salsa::tracked(lru = 1024)]
pub fn load_configuration<'db>(
    db: &'db dyn salsa::Database,
    path_input: ConfigurationPathInput<'db>,
) -> Arc<Configuration> {
    let _span = tracing::info_span!("load_configuration").entered();

    let path_str = path_input.path(db);
    let path = PathBuf::from(path_str);

    tracing::warn!(?path, "METADATA LOAD: loading configuration from directory");

    let config = bsl_metadata::load_from_directory(&path).unwrap_or_else(|e| {
        tracing::error!(error = %e, ?path, "failed to load configuration");
        Configuration::new("Configuration")
    });

    tracing::warn!(
        common_modules = config.common_modules().len(),
        metadata_objects = config.metadata_objects().len(),
        "METADATA LOAD: configuration loaded successfully"
    );

    Arc::new(config)
}

/// The base configuration merged with one extension's overlay, memoised per
/// (base, extension) path pair. `Configuration::merged_with_extension` deep-clones the
/// whole base configuration, so without this every metadata lookup of every extension
/// file re-cloned the entire configuration — the dominant cost when analysing a
/// heavily-extended project. Keyed on the two path inputs (which carry the config-root
/// revisions), so the merge runs once per extension and re-runs only when a config
/// actually changes. The result is identical to the inline merge, just shared.
#[salsa::tracked(lru = 1024)]
pub fn merged_configuration<'db>(
    db: &'db dyn salsa::Database,
    main_input: ConfigurationPathInput<'db>,
    extension_input: ConfigurationPathInput<'db>,
) -> Arc<Configuration> {
    let _span = tracing::info_span!("merged_configuration").entered();
    let main = load_configuration(db, main_input);
    let extension = load_configuration(db, extension_input);
    Arc::new(main.merged_with_extension(&extension))
}

/// The composing files of a single metadata object: the main `<Name>.xml` and an
/// optional `Ext/Predefined.xml`, plus the kind that selects the parser. Interned
/// so [`parse_mdo_query`] keys on the file identities; the per-file content
/// revisions drive invalidation, so editing one MDO's XML re-parses only it.
#[salsa::interned(debug)]
pub struct MdoFiles<'db> {
    pub mdo_type: bsl_metadata::MdoType,
    pub main: vfs::FileId,
    pub predefined: Option<vfs::FileId>,
}

/// Parse one metadata object from its composing files, read through the versioned
/// VFS (`file_text`). Memoised per MDO and backdated on an unchanged object, so a
/// reload re-parses only the files that actually changed and only that object's
/// consumers are invalidated.
#[salsa::tracked(lru = 8192)]
pub fn parse_mdo_query<'db>(
    db: &'db dyn base_db::SourceDatabase,
    files: MdoFiles<'db>,
) -> Option<Arc<bsl_metadata::MetadataObject>> {
    let _span = tracing::info_span!("parse_mdo").entered();

    let main_text = db.file_text(files.main(db));
    let predefined_text = files.predefined(db).map(|fid| db.file_text(fid));

    bsl_metadata::parse_metadata_object_from_texts(
        files.mdo_type(db),
        &main_text,
        predefined_text.as_deref(),
    )
    .map(Arc::new)
}

/// One discovered metadata object in a config root's *structure* listing: which
/// kind, its name, and the [`vfs::FileId`]s of its composing files. Carries no
/// parsed content — only identities — so the listing changes on add/remove/rename
/// of an MDO, never on a content edit within one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MdoEntry {
    pub kind: bsl_metadata::MdoType,
    pub name: String,
    pub main: vfs::FileId,
    pub predefined: Option<vfs::FileId>,
}

/// One discovered defined type in a config root's *structure* listing: its name
/// and the [`vfs::FileId`] of its main XML. Defined types are global (keyed by
/// name, no kind, no predefined sidecar), so they ride a separate field of the
/// listing rather than [`MdoEntry`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DefinedTypeEntry {
    pub name: String,
    pub main: vfs::FileId,
}

/// One discovered common module in a config root's *structure* listing: its name,
/// the [`vfs::FileId`] of its metadata XML, and the [`vfs::FileId`] of its
/// `Ext/Module.bsl` (the module source) when present. The module-file id backs the
/// reverse "which common module owns this `.bsl`" lookup ([`CommonModuleIndex`]'s
/// `by_module_file`); it is `None` for protected/binary modules with no source.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CommonModuleEntry {
    pub name: String,
    pub main: vfs::FileId,
    pub module_file: Option<vfs::FileId>,
}

/// One discovered event subscription in a config root's *structure* listing: its
/// name and the [`vfs::FileId`] of its main XML. Event subscriptions are global
/// flat metadata objects, keyed by name, so they ride a dedicated listing field.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EventSubscriptionEntry {
    pub name: String,
    pub main: vfs::FileId,
}

/// The per-config-root *structure* input: the MDOs and defined types that exist in
/// one config root (base config or one extension). Set out-of-query by the
/// bootstrap; re-set only when the directory structure changes. Keeping it per-root
/// means an extension's MDOs never collide with the base config's in
/// [`config_index`], and a structure change in one root does not invalidate
/// another. `entries` (MDOs + registers), `defined_types`, and `common_modules` are
/// separate fields, so a structure change to one family does not invalidate the
/// indexes derived from the others.
#[salsa::input(debug)]
pub struct MetadataListingInput {
    pub entries: Arc<Vec<MdoEntry>>,
    pub defined_types: Arc<Vec<DefinedTypeEntry>>,
    pub common_modules: Arc<Vec<CommonModuleEntry>>,
    pub event_subscriptions: Arc<Vec<EventSubscriptionEntry>>,
}

/// The composing-file identities for one MDO, as held in a [`ConfigIndex`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MdoFileIds {
    pub main: vfs::FileId,
    pub predefined: Option<vfs::FileId>,
}

/// A config root's `(kind, lowercased-name) -> files` lookup, derived from its
/// [`MetadataListingInput`]. Built by [`config_index`]; depends only on the
/// structure input, so a content edit leaves it memoised.
#[derive(Default, PartialEq, Eq, Debug)]
pub struct ConfigIndex {
    by_name: std::collections::HashMap<(bsl_metadata::MdoType, String), MdoFileIds>,
    /// Registers keyed by name alone (no kind), for callers that know only the
    /// register name (e.g. a `Движения.<Register>` movement touch). A register
    /// name is unique within a config root, so this carries its kind alongside.
    register_by_name: std::collections::HashMap<String, (bsl_metadata::MdoType, MdoFileIds)>,
}

impl ConfigIndex {
    pub fn lookup(&self, kind: bsl_metadata::MdoType, name: &str) -> Option<MdoFileIds> {
        self.by_name.get(&(kind, name.fold_lower())).copied()
    }

    pub fn lookup_register_by_name(
        &self,
        name: &str,
    ) -> Option<(bsl_metadata::MdoType, MdoFileIds)> {
        self.register_by_name.get(&name.fold_lower()).copied()
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}

/// Build a config root's name lookup from its structure listing. Tracked on the
/// listing input alone, so it re-runs only on a structure change (add/remove/
/// rename), not on a content edit — those flow through [`parse_mdo_query`].
#[salsa::tracked]
pub fn config_index(
    db: &dyn base_db::SourceDatabase,
    listing: MetadataListingInput,
) -> Arc<ConfigIndex> {
    let _span = tracing::info_span!("config_index").entered();

    let entries = listing.entries(db);
    let mut by_name = std::collections::HashMap::with_capacity(entries.len());
    let mut register_by_name = std::collections::HashMap::new();
    for entry in entries.iter() {
        let ids = MdoFileIds { main: entry.main, predefined: entry.predefined };
        by_name.insert((entry.kind, entry.name.fold_lower()), ids);
        if entry.kind.is_register() {
            register_by_name.insert(entry.name.fold_lower(), (entry.kind, ids));
        }
    }
    Arc::new(ConfigIndex { by_name, register_by_name })
}

/// Resolve a single metadata object within one config root, at per-MDO Salsa
/// granularity. Depends on [`config_index`] (structure) to map the name to its
/// files, then on [`parse_mdo_query`] (content) for that one MDO. A content edit
/// re-parses only the edited MDO and re-runs only this resolution for it; sibling
/// resolutions in the same root stay memoised. An add/remove re-runs
/// `config_index`, so an absent-name miss correctly invalidates when the MDO later
/// appears. Extension overlay across roots is composed by callers, not here.
#[salsa::tracked]
pub fn resolve_metadata_object(
    db: &dyn base_db::SourceDatabase,
    listing: MetadataListingInput,
    mdo_type: bsl_metadata::MdoType,
    name: String,
) -> Option<Arc<bsl_metadata::MetadataObject>> {
    let _span = tracing::info_span!("resolve_metadata_object").entered();

    let index = config_index(db, listing);
    let ids = index.lookup(mdo_type, &name)?;
    let files = MdoFiles::new(db, mdo_type, ids.main, ids.predefined);
    parse_mdo_query(db, files)
}

/// Parse one register from its main XML, read through the versioned VFS. The
/// register counterpart of [`parse_mdo_query`]; keyed on the same interned
/// [`MdoFiles`] (registers have no predefined sidecar) but a separate tracked fn,
/// so a register and an object never share a memo. Backdates on an unchanged
/// register.
#[salsa::tracked(lru = 8192)]
pub fn parse_register_query(
    db: &dyn base_db::SourceDatabase,
    files: MdoFiles<'_>,
) -> Option<Arc<bsl_metadata::Register>> {
    let _span = tracing::info_span!("parse_register").entered();

    let main_text = db.file_text(files.main(db));
    bsl_metadata::parse_register_from_text(files.mdo_type(db), &main_text).map(Arc::new)
}

/// Resolve a single register within one config root, the register counterpart of
/// [`resolve_metadata_object`]. Shares [`config_index`] (the listing carries
/// register entries too) but parses via [`parse_register_query`]. Extension
/// overlay across roots is composed by callers.
#[salsa::tracked]
pub fn resolve_register(
    db: &dyn base_db::SourceDatabase,
    listing: MetadataListingInput,
    mdo_type: bsl_metadata::MdoType,
    name: String,
) -> Option<Arc<bsl_metadata::Register>> {
    let _span = tracing::info_span!("resolve_register").entered();

    let index = config_index(db, listing);
    let ids = index.lookup(mdo_type, &name)?;
    let files = MdoFiles::new(db, mdo_type, ids.main, ids.predefined);
    parse_register_query(db, files)
}

/// Resolve a register within one config root by NAME alone (its kind is unknown
/// to the caller — e.g. a `Движения.<Register>` movement touch). Shares
/// [`config_index`]'s name-only register map, then parses the one register via
/// [`parse_register_query`]. Same per-MDO granularity and absent-name
/// invalidation as [`resolve_register`]; extension overlay across roots is
/// composed by callers.
#[salsa::tracked]
pub fn resolve_register_by_name(
    db: &dyn base_db::SourceDatabase,
    listing: MetadataListingInput,
    name: String,
) -> Option<Arc<bsl_metadata::Register>> {
    let _span = tracing::info_span!("resolve_register_by_name").entered();

    let index = config_index(db, listing);
    let (kind, ids) = index.lookup_register_by_name(&name)?;
    let files = MdoFiles::new(db, kind, ids.main, ids.predefined);
    parse_register_query(db, files)
}

/// The main XML file of a single defined type, interned so
/// [`parse_defined_type_query`] keys on the file identity; its content revision
/// drives invalidation, so editing one defined type re-parses only it.
#[salsa::interned(debug)]
pub struct DefinedTypeFile<'db> {
    pub main: vfs::FileId,
}

/// Parse one defined type from its main XML, read through the versioned VFS, and
/// return its underlying type (the resolution unit; an extension overlay replaces
/// it wholesale). The defined-type counterpart of [`parse_register_query`].
/// Backdates on an unchanged underlying type.
#[salsa::tracked(lru = 8192)]
pub fn parse_defined_type_query(
    db: &dyn base_db::SourceDatabase,
    file: DefinedTypeFile<'_>,
) -> Option<Arc<bsl_metadata::AttributeType>> {
    let _span = tracing::info_span!("parse_defined_type").entered();

    let main_text = db.file_text(file.main(db));
    bsl_metadata::parse_defined_type_from_text(&main_text)
        .map(|dt| Arc::new(dt.underlying_type().clone()))
}

/// A config root's `lowercased-name -> defined-type file` lookup, derived from its
/// [`MetadataListingInput`]'s `defined_types` field. Tracked on that field alone,
/// so a content edit leaves it memoised and an MDO structure change does not
/// invalidate it.
#[derive(Default, PartialEq, Eq, Debug)]
pub struct DefinedTypeIndex {
    by_name: std::collections::HashMap<String, vfs::FileId>,
}

impl DefinedTypeIndex {
    pub fn lookup(&self, name: &str) -> Option<vfs::FileId> {
        self.by_name.get(&name.fold_lower()).copied()
    }
}

/// Build a config root's defined-type name lookup from its structure listing.
#[salsa::tracked]
pub fn defined_type_index(
    db: &dyn base_db::SourceDatabase,
    listing: MetadataListingInput,
) -> Arc<DefinedTypeIndex> {
    let _span = tracing::info_span!("defined_type_index").entered();

    let entries = listing.defined_types(db);
    let mut by_name = std::collections::HashMap::with_capacity(entries.len());
    for entry in entries.iter() {
        by_name.insert(entry.name.fold_lower(), entry.main);
    }
    Arc::new(DefinedTypeIndex { by_name })
}

/// Resolve a single defined type's underlying type within one config root, at
/// per-defined-type Salsa granularity. The defined-type counterpart of
/// [`resolve_metadata_object`]; extension overlay across roots is composed by
/// callers (an extension replaces the underlying type wholesale).
#[salsa::tracked]
pub fn resolve_defined_type(
    db: &dyn base_db::SourceDatabase,
    listing: MetadataListingInput,
    name: String,
) -> Option<Arc<bsl_metadata::AttributeType>> {
    let _span = tracing::info_span!("resolve_defined_type").entered();

    let index = defined_type_index(db, listing);
    let main = index.lookup(&name)?;
    let file = DefinedTypeFile::new(db, main);
    parse_defined_type_query(db, file)
}

/// The main XML file of a single common module, interned so
/// [`parse_common_module_query`] keys on the file identity; its content revision
/// drives invalidation, so editing one common module re-parses only it.
#[salsa::interned(debug)]
pub struct CommonModuleFile<'db> {
    pub main: vfs::FileId,
}

/// Parse one common module's metadata from its main XML, read through the versioned
/// VFS. The common-module counterpart of [`parse_defined_type_query`]; only metadata
/// (flags + name) is read — the module body is resolved through the symbol tree.
/// Backdates on unchanged metadata.
#[salsa::tracked(lru = 8192)]
pub fn parse_common_module_query(
    db: &dyn base_db::SourceDatabase,
    file: CommonModuleFile<'_>,
) -> Option<Arc<bsl_metadata::CommonModule>> {
    let _span = tracing::info_span!("parse_common_module").entered();

    let main_text = db.file_text(file.main(db));
    bsl_metadata::parse_common_module_from_text(&main_text).map(Arc::new)
}

/// A config root's common-module lookup, derived from its [`MetadataListingInput`]'s
/// `common_modules` field: `lowercased-name -> main XML` for the by-name resolution,
/// and `module-file id -> name` for the reverse "which common module owns this
/// `.bsl`" lookup. Tracked on that field alone, so a content edit leaves it memoised.
#[derive(Default, PartialEq, Eq, Debug)]
pub struct CommonModuleIndex {
    by_name: std::collections::HashMap<String, vfs::FileId>,
    by_module_file: std::collections::HashMap<vfs::FileId, String>,
    /// `lowercased-name -> Ext/Module.bsl id`, for resolving a common module's
    /// body file scoped to this root (method/parameter validation needs the body,
    /// not the metadata XML). Absent for modules whose body was not enrolled.
    module_file_by_name: std::collections::HashMap<String, vfs::FileId>,
}

impl CommonModuleIndex {
    pub fn lookup(&self, name: &str) -> Option<vfs::FileId> {
        self.by_name.get(&name.fold_lower()).copied()
    }

    /// The `Ext/Module.bsl` id of the common module `name` in this root, if known.
    pub fn lookup_module_file(&self, name: &str) -> Option<vfs::FileId> {
        self.module_file_by_name.get(&name.fold_lower()).copied()
    }

    /// The lowercased name of the common module whose `Ext/Module.bsl` is
    /// `module_file`, if any.
    pub fn name_for_module_file(&self, module_file: vfs::FileId) -> Option<&str> {
        self.by_module_file.get(&module_file).map(String::as_str)
    }
}

/// Build a config root's common-module lookup from its structure listing.
#[salsa::tracked]
pub fn common_module_index(
    db: &dyn base_db::SourceDatabase,
    listing: MetadataListingInput,
) -> Arc<CommonModuleIndex> {
    let _span = tracing::info_span!("common_module_index").entered();

    let entries = listing.common_modules(db);
    let mut by_name = std::collections::HashMap::with_capacity(entries.len());
    let mut by_module_file = std::collections::HashMap::new();
    let mut module_file_by_name = std::collections::HashMap::new();
    for entry in entries.iter() {
        by_name.insert(entry.name.fold_lower(), entry.main);
        if let Some(module_file) = entry.module_file {
            by_module_file.insert(module_file, entry.name.fold_lower());
            module_file_by_name.insert(entry.name.fold_lower(), module_file);
        }
    }
    Arc::new(CommonModuleIndex { by_name, by_module_file, module_file_by_name })
}

/// Resolve a single common module's metadata by name within one config root, at
/// per-common-module Salsa granularity. The common-module counterpart of
/// [`resolve_defined_type`]; extension overlay across roots is composed by callers
/// (an extension replaces the module wholesale).
#[salsa::tracked]
pub fn resolve_common_module(
    db: &dyn base_db::SourceDatabase,
    listing: MetadataListingInput,
    name: String,
) -> Option<Arc<bsl_metadata::CommonModule>> {
    let _span = tracing::info_span!("resolve_common_module").entered();

    let index = common_module_index(db, listing);
    let main = index.lookup(&name)?;
    let file = CommonModuleFile::new(db, main);
    parse_common_module_query(db, file)
}

/// Resolve the common module whose `Ext/Module.bsl` is `module_file` within one
/// config root. Answers "which common module owns this `.bsl`?" via the reverse
/// index, then parses that module's metadata at per-common-module granularity.
#[salsa::tracked]
pub fn resolve_common_module_by_file(
    db: &dyn base_db::SourceDatabase,
    listing: MetadataListingInput,
    module_file: vfs::FileId,
) -> Option<Arc<bsl_metadata::CommonModule>> {
    let _span = tracing::info_span!("resolve_common_module_by_file").entered();

    let index = common_module_index(db, listing);
    let name = index.name_for_module_file(module_file)?;
    let main = index.lookup(name)?;
    let file = CommonModuleFile::new(db, main);
    parse_common_module_query(db, file)
}

/// The main XML file of a single event subscription, interned so
/// [`parse_event_subscription_query`] keys on the file identity; its content
/// revision drives invalidation, so editing one subscription re-parses only it.
#[salsa::interned(debug)]
pub struct EventSubscriptionFile<'db> {
    pub main: vfs::FileId,
}

/// Parse one event subscription from its main XML, read through the versioned VFS.
/// Backdates on unchanged metadata.
#[salsa::tracked(lru = 8192)]
pub fn parse_event_subscription_query(
    db: &dyn base_db::SourceDatabase,
    file: EventSubscriptionFile<'_>,
) -> Option<Arc<bsl_metadata::EventSubscription>> {
    let _span = tracing::info_span!("parse_event_subscription").entered();

    let main_text = db.file_text(file.main(db));
    bsl_metadata::parse_event_subscription_from_text(&main_text).map(Arc::new)
}

/// A config root's event-subscription lookup, derived from its
/// [`MetadataListingInput`]'s `event_subscriptions` field. Tracked on that field
/// alone, so a content edit leaves it memoised and unrelated MDO structure changes
/// do not invalidate it.
#[derive(Default, PartialEq, Eq, Debug)]
pub struct EventSubscriptionIndex {
    by_name: std::collections::HashMap<String, vfs::FileId>,
}

impl EventSubscriptionIndex {
    pub fn lookup(&self, name: &str) -> Option<vfs::FileId> {
        self.by_name.get(&name.fold_lower()).copied()
    }
}

/// Build a config root's event-subscription name lookup from its structure listing.
#[salsa::tracked]
pub fn event_subscription_index(
    db: &dyn base_db::SourceDatabase,
    listing: MetadataListingInput,
) -> Arc<EventSubscriptionIndex> {
    let _span = tracing::info_span!("event_subscription_index").entered();

    let entries = listing.event_subscriptions(db);
    let mut by_name = std::collections::HashMap::with_capacity(entries.len());
    for entry in entries.iter() {
        by_name.insert(entry.name.fold_lower(), entry.main);
    }
    Arc::new(EventSubscriptionIndex { by_name })
}

/// Resolve a single event subscription's metadata by name within one config root,
/// at per-event-subscription Salsa granularity. Extension overlay across roots is
/// composed by callers (an extension replaces the subscription wholesale).
#[salsa::tracked]
pub fn resolve_event_subscription(
    db: &dyn base_db::SourceDatabase,
    listing: MetadataListingInput,
    name: String,
) -> Option<Arc<bsl_metadata::EventSubscription>> {
    let _span = tracing::info_span!("resolve_event_subscription").entered();

    let index = event_subscription_index(db, listing);
    let main = index.lookup(&name)?;
    let file = EventSubscriptionFile::new(db, main);
    parse_event_subscription_query(db, file)
}

#[salsa::db]
pub trait MetadataDb: salsa::Database {
    fn load_configuration<'db>(
        &'db self,
        path_input: ConfigurationPathInput<'db>,
    ) -> Arc<Configuration>
    where
        Self: Sized,
    {
        load_configuration(self, path_input)
    }
}

pub fn get_module_type_from_uri(file_uri: &str) -> Option<bsl_metadata::ModuleType> {
    let parts: Vec<&str> = file_uri.split('/').collect();

    if parts.is_empty() {
        return None;
    }

    if parts.len() >= 2 && parts[parts.len() - 2] == "Ext" {
        match parts[parts.len() - 1] {
            "ManagedApplicationModule.bsl" => {
                return Some(bsl_metadata::ModuleType::ManagedApplicationModule);
            }
            "OrdinaryApplicationModule.bsl" => {
                return Some(bsl_metadata::ModuleType::OrdinaryApplicationModule);
            }
            "ApplicationModule.bsl" => {
                return Some(bsl_metadata::ModuleType::ApplicationModule);
            }
            "SessionModule.bsl" => return Some(bsl_metadata::ModuleType::SessionModule),
            "ExternalConnectionModule.bsl" => {
                return Some(bsl_metadata::ModuleType::ExternalConnectionModule);
            }
            _ => {}
        }
    }

    if let Some(cm_idx) = parts.iter().position(|&p| p == "CommonModules") {
        if parts.len() >= cm_idx + 4 {
            return Some(bsl_metadata::ModuleType::CommonModule);
        }
    }

    if let Some(idx) = parts.iter().position(|&p| p == "HTTPServices") {
        if parts.len() >= idx + 4 {
            return Some(bsl_metadata::ModuleType::HTTPServiceModule);
        }
    }

    if let Some(idx) = parts.iter().position(|&p| p == "WebServices") {
        if parts.len() >= idx + 4 {
            return Some(bsl_metadata::ModuleType::WebServiceModule);
        }
    }

    if let Some(idx) = parts.iter().position(|&p| p == "IntegrationServices") {
        if parts.len() >= idx + 4 {
            return Some(bsl_metadata::ModuleType::IntegrationServiceModule);
        }
    }

    if let Some(idx) = parts.iter().position(|&p| p == "CommonCommands" || p == "ОбщиеКоманды")
    {
        if parts.len() >= idx + 4 && parts[parts.len() - 1] == "CommandModule.bsl" {
            return Some(bsl_metadata::ModuleType::CommandModule);
        }
    }

    if let Some(cmd_idx) = parts.iter().position(|&p| p == "Commands") {
        if parts.len() >= cmd_idx + 4 && parts[parts.len() - 1].ends_with("CommandModule.bsl") {
            return Some(bsl_metadata::ModuleType::CommandModule);
        }
    }

    if let Some(idx) = parts.iter().rposition(|&p| p == "CommonForms" || p == "ОбщиеФормы")
    {
        if parts.len() == idx + 5
            && parts[parts.len() - 1] == "Module.bsl"
            && parts[parts.len() - 2] == "Form"
            && parts[parts.len() - 3] == "Ext"
        {
            return Some(bsl_metadata::ModuleType::FormModule);
        }
    }

    if let Some(forms_idx) = parts.iter().position(|&p| p == "Forms") {
        if parts.len() >= forms_idx + 5
            && parts[parts.len() - 1] == "Module.bsl"
            && parts[parts.len() - 2] == "Form"
            && parts[parts.len() - 3] == "Ext"
        {
            return Some(bsl_metadata::ModuleType::FormModule);
        }
    }

    if parts.len() >= 4 {
        let module_file = parts[parts.len() - 1];
        return match module_file {
            "ObjectModule.bsl" => Some(bsl_metadata::ModuleType::ObjectModule),
            "ManagerModule.bsl" => Some(bsl_metadata::ModuleType::ManagerModule),
            "RecordSetModule.bsl" => Some(bsl_metadata::ModuleType::RecordSetModule),
            _ => None,
        };
    }

    None
}

#[derive(Debug, Clone)]
pub struct ModulePathInfo {
    pub mdo_type: Option<bsl_metadata::MdoType>,
    pub name: Option<String>,
    pub module_type: bsl_metadata::ModuleType,
}

pub fn parse_module_path(file_uri: &str) -> Option<ModulePathInfo> {
    let parts: Vec<&str> = file_uri.split('/').collect();

    if parts.len() < 4 {
        return None;
    }

    let type_idx =
        parts.iter().rposition(|&p| mdo_type_from_plural(p).is_some() || p == "CommonModules")?;

    if parts.len() < type_idx + 4 {
        return None;
    }

    let type_plural = parts[type_idx];
    let name = parts[type_idx + 1].to_string();

    let mdo_type = mdo_type_from_plural(type_plural);

    let module_file = parts[parts.len() - 1];
    let module_type = match module_file {
        "ObjectModule.bsl" => bsl_metadata::ModuleType::ObjectModule,
        "ManagerModule.bsl" => bsl_metadata::ModuleType::ManagerModule,
        "RecordSetModule.bsl" => bsl_metadata::ModuleType::RecordSetModule,
        "Module.bsl" if type_plural == "CommonModules" => bsl_metadata::ModuleType::CommonModule,
        _ => bsl_metadata::ModuleType::Unknown,
    };

    Some(ModulePathInfo { mdo_type, name: Some(name), module_type })
}

fn mdo_type_from_plural(type_plural: &str) -> Option<bsl_metadata::MdoType> {
    match type_plural {
        "Catalogs" | "Справочники" => Some(bsl_metadata::MdoType::Catalog),
        "Documents" | "Документы" => Some(bsl_metadata::MdoType::Document),
        "BusinessProcesses" | "БизнесПроцессы" => {
            Some(bsl_metadata::MdoType::BusinessProcess)
        }
        "Tasks" | "Задачи" => Some(bsl_metadata::MdoType::Task),
        "ExchangePlans" | "ПланыОбмена" => Some(bsl_metadata::MdoType::ExchangePlan),
        "ChartsOfAccounts" | "ПланыСчетов" => {
            Some(bsl_metadata::MdoType::ChartOfAccounts)
        }
        "ChartsOfCalculationTypes" | "ПланыВидовРасчета" => {
            Some(bsl_metadata::MdoType::ChartOfCalculationTypes)
        }
        "ChartsOfCharacteristicTypes" | "ПланыВидовХарактеристик" => {
            Some(bsl_metadata::MdoType::ChartOfCharacteristicTypes)
        }
        "InformationRegisters" | "РегистрыСведений" => {
            Some(bsl_metadata::MdoType::InformationRegister)
        }
        "AccumulationRegisters" | "РегистрыНакопления" => {
            Some(bsl_metadata::MdoType::AccumulationRegister)
        }
        "AccountingRegisters" | "РегистрыБухгалтерии" => {
            Some(bsl_metadata::MdoType::AccountingRegister)
        }
        "CalculationRegisters" | "РегистрыРасчета" => {
            Some(bsl_metadata::MdoType::CalculationRegister)
        }
        "DataProcessors" | "Обработки" => Some(bsl_metadata::MdoType::DataProcessor),
        "Reports" | "Отчеты" => Some(bsl_metadata::MdoType::Report),
        _ => None,
    }
}

pub fn find_metadata_object<DB: MetadataDb>(
    db: &DB,
    path_input: ConfigurationPathInput,
    mdo_type: bsl_metadata::MdoType,
    name: &str,
) -> Option<bsl_metadata::MetadataObject> {
    let config = db.load_configuration(path_input);

    if let Some(mdo) =
        config.metadata_objects().iter().find(|mdo| mdo.mdo_type == mdo_type && mdo.name == name)
    {
        return Some(mdo.clone());
    }

    use bsl_metadata::MdoType;
    if matches!(
        mdo_type,
        MdoType::InformationRegister
            | MdoType::AccumulationRegister
            | MdoType::AccountingRegister
            | MdoType::CalculationRegister
    ) {
        #[allow(unused_imports)]
        use bsl_metadata::traits::MdObject;
        config
            .registers()
            .iter()
            .find(|reg| reg.mdo_type() == mdo_type && reg.name() == name)
            .map(|reg| bsl_metadata::MetadataObject::new(mdo_type, reg.name()))
    } else {
        None
    }
}

pub fn find_common_module<DB: MetadataDb>(
    db: &DB,
    path_input: ConfigurationPathInput,
    name: &str,
) -> Option<bsl_metadata::CommonModule> {
    let config = db.load_configuration(path_input);
    config.find_common_module(name).cloned()
}

pub fn get_module_owner<DB: MetadataDb>(
    db: &DB,
    path_input: ConfigurationPathInput,
    file_uri: &str,
) -> Option<ModuleOwner> {
    let _span = tracing::debug_span!("get_module_owner", file_uri).entered();

    let parts: Vec<&str> = file_uri.split('/').collect();

    if parts.len() < 3 {
        tracing::debug!("URI too short, expected at least 3 parts");
        return None;
    }

    let type_plural = parts[0];
    let name = parts[1];

    if type_plural == "CommonModules" {
        return find_common_module(db, path_input, name).map(ModuleOwner::CommonModule);
    }

    let mdo_type = match type_plural {
        "Catalogs" | "Справочники" => bsl_metadata::MdoType::Catalog,
        "Documents" | "Документы" => bsl_metadata::MdoType::Document,
        "InformationRegisters" | "РегистрыСведений" => {
            bsl_metadata::MdoType::InformationRegister
        }
        "AccumulationRegisters" | "РегистрыНакопления" => {
            bsl_metadata::MdoType::AccumulationRegister
        }
        "AccountingRegisters" | "РегистрыБухгалтерии" => {
            bsl_metadata::MdoType::AccountingRegister
        }
        "CalculationRegisters" | "РегистрыРасчета" => {
            bsl_metadata::MdoType::CalculationRegister
        }
        _ => {
            tracing::debug!(?type_plural, "Unknown metadata type");
            return None;
        }
    };

    find_metadata_object(db, path_input, mdo_type, name).map(ModuleOwner::MetadataObject)
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModuleOwner {
    CommonModule(bsl_metadata::CommonModule),
    MetadataObject(bsl_metadata::MetadataObject),
}

impl ModuleOwner {
    pub fn name(&self) -> &str {
        match self {
            ModuleOwner::CommonModule(m) => {
                use bsl_metadata::traits::MdObject;
                m.name()
            }
            ModuleOwner::MetadataObject(m) => &m.name,
        }
    }
}

pub(crate) fn find_common_module_by_path(
    configuration: &bsl_metadata::Configuration,
    file_path: &Path,
) -> Option<bsl_metadata::CommonModule> {
    // The configuration stores each common module's `uri` relative to the config root
    // (`CommonModules/<Имя>/Ext/Module.bsl`), while the analyzed `file_path` is absolute
    // (or scan-relative). Matching the two by full-string equality never holds, so the
    // module's metadata stayed `None` and every metadata diagnostic over it was silent.
    // Resolve by the name segment instead, the way the service matchers above already do.
    //
    // The segment after `CommonModules/` is the module's directory name, which 1C keeps
    // identical to the metadata `<Name>` (the designer enforces it) — so `find_common_module`
    // (keyed on the parsed name) resolves it. `rposition` takes the module-level
    // `CommonModules`, not an accidental one in the workspace prefix above the config root.
    let file_str = file_path.to_string_lossy().replace('\\', "/");
    let parts: Vec<&str> = file_str.split('/').collect();
    let cm_idx = parts.iter().rposition(|&p| p == "CommonModules")?;
    let name = parts.get(cm_idx + 1)?;

    configuration.find_common_module(name).cloned()
}

pub(crate) fn load_form_from_path(file_path: &Path) -> Option<Arc<bsl_metadata::Form>> {
    use bsl_metadata::xml_parser::parse_form_from_bsl_path;

    tracing::debug!(path = %file_path.display(), "Attempting to load form metadata");

    match parse_form_from_bsl_path(file_path) {
        Ok(form) => {
            tracing::debug!(
                form_name = %form.name(),
                form_type = ?form.form_type(),
                event_handlers = form.event_handlers().len(),
                command_handlers = form.command_handlers().len(),
                "Loaded form metadata"
            );
            Some(Arc::new(form))
        }
        Err(e) => {
            tracing::debug!(?e, path = %file_path.display(), "Could not load form metadata");
            None
        }
    }
}

pub fn build_module_metadata(
    file_path: &Path,
    configuration: Option<&bsl_metadata::Configuration>,
) -> hir::ModuleMetadata {
    let uri = file_path.to_string_lossy().to_string();

    let path_info = parse_module_path(&uri);

    let module_type = get_module_type_from_uri(&uri).unwrap_or(bsl_metadata::ModuleType::Unknown);

    tracing::debug!(uri = %uri, module_type = ?module_type, "build_module_metadata");

    let mut execution_context = None;
    let mut common_module = None;
    let mut mdo = None;
    let mut register = None;
    let mut form = None;
    let mut http_service = None;
    let mut web_service = None;
    let mut integration_service = None;

    if let Some(config) = configuration {
        match module_type {
            bsl_metadata::ModuleType::CommonModule => {
                if let Some(cm) = find_common_module_by_path(config, file_path) {
                    execution_context = Some(hir::compute_execution_context(&cm));
                    common_module = Some(Arc::new(cm));
                }
            }
            bsl_metadata::ModuleType::ManagerModule
            | bsl_metadata::ModuleType::ObjectModule
            | bsl_metadata::ModuleType::RecordSetModule => {
                if let Some(ref info) = path_info {
                    if let (Some(mdo_type), Some(ref name)) = (info.mdo_type, &info.name) {
                        if matches!(
                            mdo_type,
                            bsl_metadata::MdoType::InformationRegister
                                | bsl_metadata::MdoType::AccumulationRegister
                                | bsl_metadata::MdoType::AccountingRegister
                                | bsl_metadata::MdoType::CalculationRegister
                        ) {
                            if let Some(reg) = config.find_register_by_type_and_name(mdo_type, name)
                            {
                                register = Some(Arc::new(reg.clone()));
                            }
                        } else {
                            if let Some(obj) = config.find_metadata_object(mdo_type, name) {
                                mdo = Some(Arc::new(obj.clone()));
                            }
                        }
                    }
                }
            }
            bsl_metadata::ModuleType::FormModule => {
                form = load_form_from_path(file_path);
            }
            bsl_metadata::ModuleType::HTTPServiceModule => {
                http_service = find_http_service_by_path(config, file_path);
            }
            bsl_metadata::ModuleType::WebServiceModule => {
                web_service = find_web_service_by_path(config, file_path);
            }
            bsl_metadata::ModuleType::IntegrationServiceModule => {
                integration_service = find_integration_service_by_path(config, file_path);
            }
            _ => {}
        }
    }

    if module_type == bsl_metadata::ModuleType::FormModule && form.is_none() {
        form = load_form_from_path(file_path);
    }

    hir::ModuleMetadata {
        module_type,
        execution_context,
        common_module,
        mdo,
        register,
        form,
        http_service,
        web_service,
        integration_service,
    }
}

pub(crate) fn find_http_service_by_path(
    configuration: &bsl_metadata::Configuration,
    file_path: &Path,
) -> Option<Arc<bsl_metadata::HTTPService>> {
    let file_str = file_path.to_string_lossy().replace('\\', "/");

    let parts: Vec<&str> = file_str.split('/').collect();

    let http_idx = parts.iter().position(|&p| p == "HTTPServices")?;

    let name = parts.get(http_idx + 1)?;

    configuration.find_http_service(name).map(|hs| Arc::new(hs.clone()))
}

pub(crate) fn find_web_service_by_path(
    configuration: &bsl_metadata::Configuration,
    file_path: &Path,
) -> Option<Arc<bsl_metadata::WebService>> {
    let file_str = file_path.to_string_lossy().replace('\\', "/");

    let parts: Vec<&str> = file_str.split('/').collect();

    let ws_idx = parts.iter().position(|&p| p == "WebServices")?;

    let name = parts.get(ws_idx + 1)?;

    configuration.find_web_service(name).map(|ws| Arc::new(ws.clone()))
}

pub(crate) fn find_integration_service_by_path(
    configuration: &bsl_metadata::Configuration,
    file_path: &Path,
) -> Option<Arc<bsl_metadata::IntegrationService>> {
    let file_str = file_path.to_string_lossy().replace('\\', "/");

    let parts: Vec<&str> = file_str.split('/').collect();

    let is_idx = parts.iter().position(|&p| p == "IntegrationServices")?;

    let name = parts.get(is_idx + 1)?;

    configuration.find_integration_service(name).map(|svc| Arc::new(svc.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[salsa::db]
    #[derive(Default, Clone)]
    struct TestDatabase {
        storage: salsa::Storage<Self>,
    }

    #[salsa::db]
    impl salsa::Database for TestDatabase {}

    #[salsa::db]
    impl MetadataDb for TestDatabase {}

    #[test]
    fn test_load_configuration_caching() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let config1 = db.load_configuration(path_input);
        let config2 = db.load_configuration(path_input);

        assert!(Arc::ptr_eq(&config1, &config2), "Salsa should cache configuration");

        assert!(!config1.common_modules().is_empty(), "Should load common modules");
    }

    #[test]
    fn test_load_configuration_different_paths() {
        let db = TestDatabase::default();

        let input1 = ConfigurationPathInput::new(&db, "/path/to/config1".to_string(), 0);
        let input2 = ConfigurationPathInput::new(&db, "/path/to/config2".to_string(), 0);

        assert_ne!(input1, input2, "Different paths should create different inputs");
    }

    #[test]
    fn intern_configuration_path_collapses_separator_variants() {
        let db = TestDatabase::default();
        let backslash = intern_configuration_path(&db, r"C:\foo\bar", 0);
        let forward = intern_configuration_path(&db, "C:/foo/bar", 0);
        assert_eq!(
            backslash, forward,
            "backslash and forward-slash paths must intern as the same Salsa key",
        );
    }

    #[cfg(windows)]
    #[test]
    fn intern_configuration_path_is_case_insensitive_on_windows() {
        let db = TestDatabase::default();
        let upper = intern_configuration_path(&db, r"C:\Foo\Bar", 0);
        let lower = intern_configuration_path(&db, r"c:\foo\bar", 0);
        assert_eq!(upper, lower);
    }

    #[cfg(windows)]
    #[test]
    fn intern_configuration_path_strips_extended_prefix_on_windows() {
        let db = TestDatabase::default();
        let extended = intern_configuration_path(&db, r"\\?\C:\foo\bar", 0);
        let plain = intern_configuration_path(&db, r"C:\foo\bar", 0);
        assert_eq!(extended, plain);
    }

    #[cfg(not(windows))]
    #[test]
    fn intern_configuration_path_preserves_case_on_posix() {
        let db = TestDatabase::default();
        let upper = intern_configuration_path(&db, "/Foo/Bar", 0);
        let lower = intern_configuration_path(&db, "/foo/bar", 0);
        assert_ne!(upper, lower, "POSIX file systems are case-sensitive");
    }

    #[test]
    fn test_find_metadata_object() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let catalog =
            find_metadata_object(&db, path_input, bsl_metadata::MdoType::Catalog, "Справочник1");
        assert!(catalog.is_some(), "Should find Справочник1");
        assert_eq!(catalog.unwrap().name, "Справочник1");

        let not_found =
            find_metadata_object(&db, path_input, bsl_metadata::MdoType::Catalog, "NonExistent");
        assert!(not_found.is_none(), "Should not find non-existent object");
    }

    #[test]
    fn test_find_common_module() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let module = find_common_module(&db, path_input, "ГлобальныйСерверныйМодуль");
        assert!(module.is_some(), "Should find ГлобальныйСерверныйМодуль");

        use bsl_metadata::traits::MdObject;
        assert_eq!(module.unwrap().name(), "ГлобальныйСерверныйМодуль");

        let not_found = find_common_module(&db, path_input, "NonExistent");
        assert!(not_found.is_none(), "Should not find non-existent module");
    }

    #[test]
    fn test_get_module_owner_common_module() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let owner = get_module_owner(
            &db,
            path_input,
            "CommonModules/ГлобальныйСерверныйМодуль/Ext/Module.bsl",
        );

        assert!(owner.is_some(), "Should find module owner");
        let owner = owner.unwrap();

        match &owner {
            ModuleOwner::CommonModule(m) => {
                use bsl_metadata::traits::MdObject;
                assert_eq!(m.name(), "ГлобальныйСерверныйМодуль");
            }
            _ => panic!("Should be CommonModule"),
        }

        assert_eq!(owner.name(), "ГлобальныйСерверныйМодуль");
    }

    #[test]
    fn test_get_module_owner_catalog_english() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let owner = get_module_owner(&db, path_input, "Catalogs/Справочник1/Ext/ObjectModule.bsl");

        assert!(owner.is_some(), "Should find catalog owner");
        let owner = owner.unwrap();

        match &owner {
            ModuleOwner::MetadataObject(m) => {
                assert_eq!(m.name, "Справочник1");
                assert_eq!(m.mdo_type, bsl_metadata::MdoType::Catalog);
            }
            _ => panic!("Should be MetadataObject"),
        }

        assert_eq!(owner.name(), "Справочник1");
    }

    #[test]
    fn test_get_module_owner_catalog_russian() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let owner =
            get_module_owner(&db, path_input, "Справочники/Справочник1/Ext/ManagerModule.bsl");

        assert!(owner.is_some(), "Should find catalog owner with Russian plural");
        assert_eq!(owner.unwrap().name(), "Справочник1");
    }

    #[test]
    fn test_get_module_owner_register() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let owner = get_module_owner(
            &db,
            path_input,
            "InformationRegisters/РегистрСведений1/Ext/ManagerModule.bsl",
        );

        assert!(owner.is_some(), "Should find register owner");
        let owner = owner.unwrap();

        match &owner {
            ModuleOwner::MetadataObject(m) => {
                assert_eq!(m.name, "РегистрСведений1");
                assert_eq!(m.mdo_type, bsl_metadata::MdoType::InformationRegister);
            }
            _ => panic!("Should be MetadataObject"),
        }
    }

    #[test]
    fn test_get_module_owner_invalid_uri() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let owner = get_module_owner(&db, path_input, "CommonModules/Module.bsl");
        assert!(owner.is_none(), "Should return None for URI too short");

        let owner = get_module_owner(&db, path_input, "UnknownType/Object/Ext/Module.bsl");
        assert!(owner.is_none(), "Should return None for unknown type");

        let owner = get_module_owner(&db, path_input, "Catalogs/NonExistent/Ext/ObjectModule.bsl");
        assert!(owner.is_none(), "Should return None for non-existent object");
    }

    #[test]
    fn test_module_owner_clone_and_eq() {
        let db = TestDatabase::default();

        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../bsl-metadata/fixtures/designer").to_string();
        let path_input = ConfigurationPathInput::new(&db, path, 0);

        let owner1 = get_module_owner(
            &db,
            path_input,
            "CommonModules/ГлобальныйСерверныйМодуль/Ext/Module.bsl",
        );
        let owner2 = get_module_owner(
            &db,
            path_input,
            "CommonModules/ГлобальныйСерверныйМодуль/Ext/Module.bsl",
        );

        assert!(owner1.is_some() && owner2.is_some());

        let owner1_clone = owner1.clone();
        assert_eq!(owner1, owner1_clone, "Cloned owner should be equal");

        assert_eq!(owner1, owner2, "Same module owner should be equal");
    }

    #[test]
    fn test_get_module_type_command_module() {
        let uri = "Catalogs/Справочник1/Commands/Команда1/Ext/CommandModule.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::CommandModule));
    }

    #[test]
    fn test_get_module_type_common_command_module() {
        let uri = "CommonCommands/АвтономнаяРабота/Ext/CommandModule.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::CommandModule));

        let uri = "src/cf/CommonCommands/АвтономнаяРабота/Ext/CommandModule.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::CommandModule));

        let uri = "ОбщиеКоманды/ВыполнитьДействие/Ext/CommandModule.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::CommandModule));
    }

    #[test]
    fn test_get_module_type_common_module() {
        let uri = "CommonModules/ГлобальныйМодуль/Ext/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::CommonModule));

        let uri = "/home/user/project/src/cf/CommonModules/ГлобальныйМодуль/Ext/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::CommonModule));
    }

    #[test]
    fn test_get_module_type_form_module() {
        let uri = "Catalogs/Номенклатура/Forms/ФормаЭлемента/Ext/Form/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::FormModule));

        let uri = "/home/user/project/src/cf/BusinessProcesses/Исполнение/Forms/ВводОписанияЗадачиИсполнителя/Ext/Form/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::FormModule));
    }

    #[test]
    fn test_get_module_type_common_form_module() {
        let uri = "CommonForms/ТестоваяФорма/Ext/Form/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::FormModule));

        let uri = "/home/user/project/src/cf/CommonForms/ТестоваяФорма/Ext/Form/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::FormModule));

        let uri = "ОбщиеФормы/ТестоваяФорма/Ext/Form/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::FormModule));

        let uri = "CommonForms/ТестоваяФорма/Ext/Module.bsl";
        assert_eq!(get_module_type_from_uri(uri), None);
    }

    #[test]
    fn test_build_module_metadata_loads_common_form_without_configuration() {
        let fixture_root = std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../bsl-metadata/fixtures/designer"
        ));
        let bsl_path = fixture_root.join("CommonForms/ТестоваяФорма/Ext/Form/Module.bsl");

        let metadata = build_module_metadata(&bsl_path, None);

        assert_eq!(metadata.module_type, bsl_metadata::ModuleType::FormModule);
        let form = metadata.form.as_ref().expect("common form metadata should be loaded");
        assert_eq!(form.name(), "ТестоваяФорма");
        assert!(form.is_handler("ПриСозданииНаСервере"));
    }

    #[test]
    fn test_build_module_metadata_populates_integration_service() {
        let fixture_root = std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../bsl-metadata/fixtures/designer"
        ));
        let config = bsl_metadata::load_from_directory(&fixture_root).unwrap();

        let bsl_path = fixture_root.join("IntegrationServices/ОбменСообщениями/Ext/Module.bsl");
        let metadata = build_module_metadata(&bsl_path, Some(&config));

        assert_eq!(metadata.module_type, bsl_metadata::ModuleType::IntegrationServiceModule);
        let service =
            metadata.integration_service.as_ref().expect("integration service should be populated");
        let handlers: Vec<_> = service.receive_handlers().collect();
        assert_eq!(handlers, vec!["ОбработатьСообщениеОбычныйПриоритет"]);
    }

    #[test]
    fn test_build_module_metadata_populates_common_module_from_absolute_path() {
        // The config stores common-module `uri` relative to the config root, but analysis
        // passes an absolute path. Resolution must still find the module (by name segment),
        // otherwise every common-module metadata diagnostic stays silent.
        let fixture_root = std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../bsl-metadata/fixtures/designer"
        ));
        let config = bsl_metadata::load_from_directory(&fixture_root).unwrap();

        let bsl_path = fixture_root.join("CommonModules/КлиентскийОбщийМодуль/Ext/Module.bsl");
        let metadata = build_module_metadata(&bsl_path, Some(&config));

        assert_eq!(metadata.module_type, bsl_metadata::ModuleType::CommonModule);
        use bsl_metadata::traits::MdObject;
        let cm = metadata.common_module.as_ref().expect("common module should be populated");
        assert_eq!(cm.name(), "КлиентскийОбщийМодуль");
        assert!(cm.is_client_managed_application(), "client flag must be parsed through");
        assert!(metadata.execution_context.is_some(), "execution context must be derived");
    }

    #[test]
    fn test_get_module_type_object_module() {
        let uri = "Catalogs/Номенклатура/Ext/ObjectModule.bsl";
        assert_eq!(get_module_type_from_uri(uri), Some(bsl_metadata::ModuleType::ObjectModule));
    }

    #[test]
    fn test_get_module_type_unknown() {
        let uri = "SomeRandomPath/File.bsl";
        assert_eq!(get_module_type_from_uri(uri), None);
    }

    #[test]
    fn test_get_module_type_managed_application_module() {
        let uri = "Ext/ManagedApplicationModule.bsl";
        assert_eq!(
            get_module_type_from_uri(uri),
            Some(bsl_metadata::ModuleType::ManagedApplicationModule)
        );

        let uri = "Configuration/Ext/ManagedApplicationModule.bsl";
        assert_eq!(
            get_module_type_from_uri(uri),
            Some(bsl_metadata::ModuleType::ManagedApplicationModule)
        );
    }

    #[test]
    fn test_get_module_type_application_family() {
        assert_eq!(
            get_module_type_from_uri("Ext/SessionModule.bsl"),
            Some(bsl_metadata::ModuleType::SessionModule)
        );
        assert_eq!(
            get_module_type_from_uri("Configuration/Ext/SessionModule.bsl"),
            Some(bsl_metadata::ModuleType::SessionModule)
        );
        assert_eq!(
            get_module_type_from_uri("Ext/OrdinaryApplicationModule.bsl"),
            Some(bsl_metadata::ModuleType::OrdinaryApplicationModule)
        );
        assert_eq!(
            get_module_type_from_uri("Ext/ExternalConnectionModule.bsl"),
            Some(bsl_metadata::ModuleType::ExternalConnectionModule)
        );
        // A common module's Ext/Module.bsl must NOT be swallowed by the Ext arm.
        assert_eq!(
            get_module_type_from_uri("CommonModules/Foo/Ext/Module.bsl"),
            Some(bsl_metadata::ModuleType::CommonModule)
        );
    }

    #[test]
    fn test_parse_module_path_simple() {
        let info = parse_module_path("Catalogs/Справочник1/Ext/ObjectModule.bsl").unwrap();
        assert_eq!(info.mdo_type, Some(bsl_metadata::MdoType::Catalog));
        assert_eq!(info.name.as_deref(), Some("Справочник1"));
        assert_eq!(info.module_type, bsl_metadata::ModuleType::ObjectModule);
    }

    #[test]
    fn test_parse_module_path_with_prefix() {
        let info = parse_module_path("./src/cf/Catalogs/ДействияСогласования/Ext/ObjectModule.bsl")
            .unwrap();
        assert_eq!(info.mdo_type, Some(bsl_metadata::MdoType::Catalog));
        assert_eq!(info.name.as_deref(), Some("ДействияСогласования"));
        assert_eq!(info.module_type, bsl_metadata::ModuleType::ObjectModule);
    }

    #[test]
    fn test_parse_module_path_with_absolute_documents_prefix() {
        let info = parse_module_path(
            "/Users/test/Documents/git/project/Catalogs/Справочник1/Ext/ObjectModule.bsl",
        )
        .unwrap();
        assert_eq!(info.mdo_type, Some(bsl_metadata::MdoType::Catalog));
        assert_eq!(info.name.as_deref(), Some("Справочник1"));
        assert_eq!(info.module_type, bsl_metadata::ModuleType::ObjectModule);
    }

    #[test]
    fn test_parse_module_path_document() {
        let info =
            parse_module_path("src/cf/Documents/ПриходнаяНакладная/Ext/ObjectModule.bsl").unwrap();
        assert_eq!(info.mdo_type, Some(bsl_metadata::MdoType::Document));
        assert_eq!(info.name.as_deref(), Some("ПриходнаяНакладная"));
    }

    #[test]
    fn test_parse_module_path_register() {
        let info = parse_module_path(
            "src/cf/InformationRegisters/НастройкиОбмена/Ext/RecordSetModule.bsl",
        )
        .unwrap();
        assert_eq!(info.mdo_type, Some(bsl_metadata::MdoType::InformationRegister));
        assert_eq!(info.name.as_deref(), Some("НастройкиОбмена"));
        assert_eq!(info.module_type, bsl_metadata::ModuleType::RecordSetModule);
    }

    #[test]
    fn test_parse_module_path_data_processor() {
        let info = parse_module_path("DataProcessors/ЗагрузкаДанных/Ext/ObjectModule.bsl").unwrap();
        assert_eq!(info.mdo_type, Some(bsl_metadata::MdoType::DataProcessor));
        assert_eq!(info.name.as_deref(), Some("ЗагрузкаДанных"));
    }
}
