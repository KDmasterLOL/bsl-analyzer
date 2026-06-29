//! Metadata-substrate boot and incremental refresh, shared by every consumer.
//!
//! The metadata substrate is the per-MDO Salsa representation (`set_metadata_listing`
//! plus per-file revisions feeding `parse_mdo_query` / `resolve_metadata_object`). Building
//! and refreshing it is a *policy* about the analysis substrate, not about any one
//! frontend — so it lives here on [`AnalysisHost`], and the LSP server, MCP server, and
//! CLI drive it through the same code. Each frontend owns its own VFS and its own change
//! *trigger* (a push watcher, a pull drift-scan, or a one-shot batch load); only the
//! substrate-mutation policy is shared.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use base_db::{content_revision, read_disk_text, SourceDatabase, SourceRoot, METADATA_SOURCE_ROOT};
use ide_db::metadata::{CommonModuleEntry, DefinedTypeEntry, MdoEntry};
use vfs::{file_set::FileSet, FileId, Vfs, VfsPath};

use crate::AnalysisHost;

/// A lock-flavour-neutral handle to the consumer's VFS. The metadata policy does its
/// expensive discovery and disk reads *outside* this, then acquires a mutable [`Vfs`]
/// only for the short interning critical section. Keeping the lock type behind this
/// trait stops `ide-host-core` from imposing a lock flavour (the LSP uses
/// `parking_lot::RwLock`, the MCP server `tokio::sync::RwLock`) on its consumers.
pub trait VfsWrite {
    fn with_write<R>(&self, f: impl FnOnce(&mut Vfs) -> R) -> R;
}

/// One config root's discovered structure listing as built during bootstrap /
/// refresh: the root path plus its MDOs, defined types, and common modules, ready
/// to hand to `RootDatabaseImpl::set_metadata_listing`.
type RootStructureListing = (String, Vec<MdoEntry>, Vec<DefinedTypeEntry>, Vec<CommonModuleEntry>);

impl AnalysisHost {
    /// Build the metadata substrate for every config root the database knows about.
    /// `alloc_file_id` is idempotent, so it reuses the FileId already interned for a
    /// watched path. Runs after the source root is initialised and re-runs on reload.
    pub fn bootstrap_metadata_substrate(&mut self, vfs: &impl VfsWrite) {
        let start = Instant::now();

        let config_paths = self.raw_database().all_config_paths();
        if config_paths.is_empty() {
            return;
        }

        // One config root's discovered (stat-only) structure, before any file is
        // read or interned.
        struct RootDiscovery {
            root_string: String,
            mdos: Vec<bsl_metadata::DiscoveredMdo>,
            defined_types: Vec<bsl_metadata::DiscoveredDefinedType>,
            common_modules: Vec<bsl_metadata::DiscoveredCommonModule>,
        }

        // Discover every root's structure WITHOUT the vfs lock — discovery walks
        // and stats the filesystem but never touches the vfs.
        let discoveries: Vec<RootDiscovery> = config_paths
            .iter()
            .map(|(_, root_path)| {
                let mut mdos = bsl_metadata::discover_metadata_structure(root_path);
                mdos.extend(bsl_metadata::discover_register_structure(root_path));
                RootDiscovery {
                    root_string: root_path.to_string_lossy().to_string(),
                    mdos,
                    defined_types: bsl_metadata::discover_defined_type_structure(root_path),
                    common_modules: bsl_metadata::discover_common_module_structure(root_path),
                }
            })
            .collect();

        // Gather every composing file that needs its content revision, then read
        // and hash them in parallel OFF the vfs lock. The text itself is not
        // retained — only `(path, revision)`. (`module_file` is BSL source owned by
        // root(0), read elsewhere — not hashed here.)
        let mut to_read: Vec<PathBuf> = Vec::new();
        for d in &discoveries {
            for m in &d.mdos {
                to_read.push(m.main.clone());
                if let Some(p) = &m.predefined {
                    to_read.push(p.clone());
                }
            }
            to_read.extend(d.defined_types.iter().map(|t| t.main.clone()));
            to_read.extend(d.common_modules.iter().map(|c| c.main.clone()));
        }
        let revisions_by_path: HashMap<PathBuf, u64> = {
            use rayon::prelude::*;
            to_read
                .par_iter()
                .filter_map(|path| {
                    let text = read_disk_text(path).ok()?;
                    Some((path.clone(), content_revision(&text)))
                })
                .collect()
        };

        let mut metadata_file_set = FileSet::new();
        let mut revisions: Vec<(FileId, u64)> = Vec::new();
        let mut listings: Vec<RootStructureListing> = Vec::new();

        // Intern under a single short vfs write: allocate FileIds and grow the
        // metadata file set. This is the only lock-held work; the expensive reads
        // already happened above. A file whose read failed (vanished between
        // discovery and the read pass) is absent from `revisions_by_path`, so
        // `intern_metadata_file` returns `None` and the MDO is dropped.
        vfs.with_write(|vfs| {
            for d in discoveries {
                let mut entries = Vec::with_capacity(d.mdos.len());
                for m in d.mdos {
                    let Some(main) = intern_metadata_file(
                        vfs,
                        &m.main,
                        &revisions_by_path,
                        &mut metadata_file_set,
                        &mut revisions,
                    ) else {
                        continue;
                    };
                    let predefined = m.predefined.as_ref().and_then(|p| {
                        intern_metadata_file(
                            vfs,
                            p,
                            &revisions_by_path,
                            &mut metadata_file_set,
                            &mut revisions,
                        )
                    });
                    entries.push(MdoEntry { kind: m.mdo_type, name: m.name, main, predefined });
                }
                let mut defined_types = Vec::new();
                for t in d.defined_types {
                    let Some(main) = intern_metadata_file(
                        vfs,
                        &t.main,
                        &revisions_by_path,
                        &mut metadata_file_set,
                        &mut revisions,
                    ) else {
                        continue;
                    };
                    defined_types.push(DefinedTypeEntry { name: t.name, main });
                }
                let mut common_modules = Vec::new();
                for c in d.common_modules {
                    let Some(main) = intern_metadata_file(
                        vfs,
                        &c.main,
                        &revisions_by_path,
                        &mut metadata_file_set,
                        &mut revisions,
                    ) else {
                        continue;
                    };
                    // The module's `Ext/Module.bsl` is BSL source owned by root(0),
                    // not a metadata file — look up the analyzer's existing FileId for
                    // it (bootstrap runs after the source root is interned) rather than
                    // enrolling a duplicate. `None` when the path is absent or unloaded;
                    // the reverse lookup then misses, which is correct.
                    let module_file = c
                        .module_file
                        .as_ref()
                        .and_then(|p| vfs.file_id(&VfsPath::new(p.to_path_buf())));
                    common_modules.push(CommonModuleEntry { name: c.name, main, module_file });
                }
                listings.push((d.root_string, entries, defined_types, common_modules));
            }
        });

        let mdo_count: usize = listings.iter().map(|(_, e, _, _)| e.len()).sum();
        let file_count = revisions.len();

        let db = self.raw_database_mut();
        db.set_source_root(METADATA_SOURCE_ROOT, SourceRoot::new_local(metadata_file_set));
        for (fid, _) in &revisions {
            db.set_file_source_root(*fid, METADATA_SOURCE_ROOT);
        }
        for (fid, revision) in &revisions {
            db.set_file_revision_from_disk(*fid, *revision);
        }
        for (root, entries, defined_types, common_modules) in listings {
            db.set_metadata_listing(&root, entries, defined_types, common_modules);
        }

        tracing::info!(
            mdo_count,
            file_count,
            elapsed_ms = start.elapsed().as_millis() as u64,
            "bootstrapped metadata substrate",
        );
    }

    /// Incrementally refresh the metadata substrate for the config roots that own
    /// any of `changed_paths` (a post-boot metadata batch: content edits, adds,
    /// removes, renames). Re-discovers each affected root's structure (stat-only, no
    /// content read), then:
    /// - reads on disk **only** the changed or brand-new composing files, bumping
    ///   their revision so `parse_mdo_query` re-parses just those MDOs; unchanged
    ///   MDOs keep their revision and stay memoised;
    /// - augments root(1) with any new composing files (removed files linger
    ///   harmlessly — nothing in a listing references them);
    /// - re-sets a root's structure listing **only** when its entries actually
    ///   changed (add / remove / rename), so a pure content edit does not churn
    ///   `config_index`.
    ///
    /// Vanished mains drop out of the re-discovered structure (and so out of the
    /// listing), tombstoning them: `resolve_metadata_object` then returns `None`.
    /// Runs after the boot bootstrap (root(1) already exists). Returns whether any
    /// substrate input actually changed, so callers can gate a diagnostics refresh.
    pub fn refresh_metadata_substrate(
        &mut self,
        vfs: &impl VfsWrite,
        changed_paths: &[PathBuf],
    ) -> bool {
        if changed_paths.is_empty() {
            return false;
        }

        let config_paths = self.raw_database().all_config_paths();
        let mut affected: Vec<PathBuf> = Vec::new();
        for (_, root) in &config_paths {
            if !affected.iter().any(|r| r == root)
                && changed_paths.iter().any(|p| p.starts_with(root))
            {
                affected.push(root.clone());
            }
        }
        if affected.is_empty() {
            return false;
        }

        let changed_set: HashSet<&Path> = changed_paths.iter().map(|p| p.as_path()).collect();

        let mut metadata_file_set = {
            let db = self.raw_database();
            db.source_root_input(METADATA_SOURCE_ROOT).root(db).file_set().clone()
        };
        let files_before = metadata_file_set.len();

        let mut new_file_ids: Vec<FileId> = Vec::new();
        let mut revisions: Vec<(FileId, u64)> = Vec::new();
        let mut listings: Vec<RootStructureListing> = Vec::new();

        vfs.with_write(|vfs| {
            for root in &affected {
                let mut discovered = bsl_metadata::discover_metadata_structure(root);
                discovered.extend(bsl_metadata::discover_register_structure(root));
                let mut entries = Vec::with_capacity(discovered.len());
                for d in discovered {
                    let Some(main) = enroll_refresh(
                        vfs,
                        &d.main,
                        &changed_set,
                        &mut metadata_file_set,
                        &mut new_file_ids,
                        &mut revisions,
                    ) else {
                        continue;
                    };
                    let predefined = d.predefined.as_ref().and_then(|p| {
                        enroll_refresh(
                            vfs,
                            p,
                            &changed_set,
                            &mut metadata_file_set,
                            &mut new_file_ids,
                            &mut revisions,
                        )
                    });
                    entries.push(MdoEntry { kind: d.mdo_type, name: d.name, main, predefined });
                }
                let mut defined_types = Vec::new();
                for d in bsl_metadata::discover_defined_type_structure(root) {
                    let Some(main) = enroll_refresh(
                        vfs,
                        &d.main,
                        &changed_set,
                        &mut metadata_file_set,
                        &mut new_file_ids,
                        &mut revisions,
                    ) else {
                        continue;
                    };
                    defined_types.push(DefinedTypeEntry { name: d.name, main });
                }
                let mut common_modules = Vec::new();
                for d in bsl_metadata::discover_common_module_structure(root) {
                    let Some(main) = enroll_refresh(
                        vfs,
                        &d.main,
                        &changed_set,
                        &mut metadata_file_set,
                        &mut new_file_ids,
                        &mut revisions,
                    ) else {
                        continue;
                    };
                    // The module's `Ext/Module.bsl` is BSL source owned by root(0),
                    // not a metadata file — reuse the analyzer's existing FileId for
                    // it rather than enrolling a duplicate (see `bootstrap_metadata_substrate`).
                    let module_file = d
                        .module_file
                        .as_ref()
                        .and_then(|p| vfs.file_id(&VfsPath::new(p.to_path_buf())));
                    common_modules.push(CommonModuleEntry { name: d.name, main, module_file });
                }
                listings.push((
                    root.to_string_lossy().to_string(),
                    entries,
                    defined_types,
                    common_modules,
                ));
            }
        });

        let reread = revisions.len();
        let added = new_file_ids.len();

        let db = self.raw_database_mut();
        let mut changed = false;
        if metadata_file_set.len() != files_before {
            db.set_source_root(METADATA_SOURCE_ROOT, SourceRoot::new_local(metadata_file_set));
            changed = true;
        }
        for fid in &new_file_ids {
            db.set_file_source_root(*fid, METADATA_SOURCE_ROOT);
        }
        for (fid, revision) in &revisions {
            db.set_file_revision_from_disk(*fid, *revision);
            changed = true;
        }
        for (root, entries, defined_types, common_modules) in listings {
            let structure_changed = match db.metadata_listing(&root) {
                Some(input) => {
                    *input.entries(db) != entries
                        || *input.defined_types(db) != defined_types
                        || *input.common_modules(db) != common_modules
                }
                None => true,
            };
            if structure_changed {
                db.set_metadata_listing(&root, entries, defined_types, common_modules);
                changed = true;
            }
        }

        tracing::debug!(
            roots = affected.len(),
            reread,
            added,
            changed,
            "refreshed metadata substrate incrementally",
        );
        changed
    }
}

/// Intern a metadata composing file (already read and hashed in the parallel pass)
/// to a stable [`FileId`] and add it to the metadata file set, recording its
/// pre-computed content revision. Returns `None` when the file is absent from
/// `revisions_by_path` — it could not be read (discovered then vanished) — so the
/// caller drops it from the MDO. `alloc_file_id` is idempotent: an already-watched
/// path keeps its id. This runs under the vfs lock and does no I/O.
fn intern_metadata_file(
    vfs: &mut Vfs,
    path: &Path,
    revisions_by_path: &HashMap<PathBuf, u64>,
    file_set: &mut FileSet,
    revisions: &mut Vec<(FileId, u64)>,
) -> Option<FileId> {
    let revision = *revisions_by_path.get(path)?;
    let vfs_path = VfsPath::new(path.to_path_buf());
    let file_id = vfs.alloc_file_id(vfs_path.clone());
    file_set.insert(file_id, vfs_path);
    revisions.push((file_id, revision));
    Some(file_id)
}

/// Enroll a composing file during an incremental refresh: intern it, ensure it is
/// in the metadata file set, and (re)read its revision only if it changed or is
/// brand-new — an unchanged, already-enrolled file keeps its boot revision and is
/// not read. A newly added file is recorded in `new_file_ids` so the caller maps
/// its source root. Returns `None` only when a changed/new file cannot be read
/// (vanished), so the caller drops that MDO.
fn enroll_refresh(
    vfs: &mut Vfs,
    path: &Path,
    changed: &HashSet<&Path>,
    file_set: &mut FileSet,
    new_file_ids: &mut Vec<FileId>,
    revisions: &mut Vec<(FileId, u64)>,
) -> Option<FileId> {
    let vfs_path = VfsPath::new(path.to_path_buf());
    let is_new = file_set.file_for_path(&vfs_path).is_none();

    if changed.contains(path) || is_new {
        let revision = content_revision(&read_disk_text(path).ok()?);
        let file_id = vfs.alloc_file_id(vfs_path.clone());
        file_set.insert(file_id, vfs_path);
        if is_new {
            new_file_ids.push(file_id);
        }
        revisions.push((file_id, revision));
        Some(file_id)
    } else {
        file_set.file_for_path(&vfs_path).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::Arc;

    use ide_db::metadata::resolve_metadata_object;

    /// A caller-owned VFS for tests: a single-threaded `RefCell` is enough since the
    /// host's metadata policy acquires the write only for the interning critical
    /// section and never holds it across a yield.
    struct TestVfs(RefCell<Vfs>);
    impl VfsWrite for TestVfs {
        fn with_write<R>(&self, f: impl FnOnce(&mut Vfs) -> R) -> R {
            f(&mut self.0.borrow_mut())
        }
    }

    fn catalog_xml(name: &str, uuid: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<MetaDataObject xmlns="http://v8.1c.ru/8.3/MDClasses" version="2.10">
    <Catalog uuid="{uuid}">
        <Properties><Name>{name}</Name><CodeLength>9</CodeLength></Properties>
    </Catalog>
</MetaDataObject>"#
        )
    }

    /// The shared policy works on a bare `AnalysisHost` with a caller-owned VFS — the
    /// reuse contract for the MCP server and CLI. Mirrors the LSP-side incremental test:
    /// a content edit re-parses only the touched MDO, a structural add resolves and
    /// invalidates the prior absent-key miss, and a removal tombstones the object.
    #[test]
    fn refresh_is_incremental_and_tracks_structure_on_bare_host() {
        let cat = bsl_metadata::MdoType::Catalog;
        let root = std::env::temp_dir().join(format!(
            "ihc_refresh_meta_{}_{}",
            std::process::id(),
            line!()
        ));
        let catalogs = root.join("Catalogs");
        std::fs::create_dir_all(&catalogs).unwrap();
        let write = |name: &str, uuid: &str| {
            std::fs::write(catalogs.join(format!("{name}.xml")), catalog_xml(name, uuid)).unwrap()
        };
        write("Справочник1", "00000000-0000-0000-0000-000000000001");
        write("Товары", "00000000-0000-0000-0000-000000000002");

        let mut host = AnalysisHost::default();
        host.raw_database_mut().set_all_config_paths(vec![(None, root.clone())]);
        let vfs = TestVfs(RefCell::new(Vfs::default()));
        host.bootstrap_metadata_substrate(&vfs);

        let root_key = root.to_string_lossy().to_string();
        let resolve = |host: &AnalysisHost, name: &str| {
            let db = host.raw_database();
            let listing = db.metadata_listing(&root_key).unwrap();
            resolve_metadata_object(db, listing, cat, name.to_string())
        };

        let c1 = resolve(&host, "Справочник1").expect("Справочник1");
        let tovary = resolve(&host, "Товары").expect("Товары");

        // CONTENT edit to Товары: re-read only it.
        write("Товары", "00000000-0000-0000-0000-0000000000ff");
        assert!(host.refresh_metadata_substrate(&vfs, &[catalogs.join("Товары.xml")]));
        assert!(
            Arc::ptr_eq(&c1, &resolve(&host, "Справочник1").unwrap()),
            "a content edit to Товары must not re-resolve the sibling"
        );
        assert!(
            !Arc::ptr_eq(&tovary, &resolve(&host, "Товары").unwrap()),
            "Товары re-parses after its content changed"
        );
        let c1 = resolve(&host, "Справочник1").unwrap();

        // STRUCTURE add: a brand-new catalog appears and resolves.
        assert!(resolve(&host, "Услуги").is_none(), "Услуги absent before the add");
        write("Услуги", "00000000-0000-0000-0000-000000000003");
        assert!(host.refresh_metadata_substrate(&vfs, &[catalogs.join("Услуги.xml")]));
        assert_eq!(resolve(&host, "Услуги").expect("Услуги after add").name, "Услуги");
        assert!(
            Arc::ptr_eq(&c1, &resolve(&host, "Справочник1").unwrap()),
            "a structure add must not re-resolve an untouched sibling"
        );

        // STRUCTURE remove: deleting a catalog tombstones it.
        std::fs::remove_file(catalogs.join("Товары.xml")).unwrap();
        assert!(host.refresh_metadata_substrate(&vfs, &[catalogs.join("Товары.xml")]));
        assert!(resolve(&host, "Товары").is_none(), "removed catalog resolves to None");

        std::fs::remove_dir_all(&root).ok();
    }
}
