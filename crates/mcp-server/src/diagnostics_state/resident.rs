use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ide::{Analysis, RootDatabaseImpl};
use vfs::{FileId, Vfs, VfsPath};

use super::workspace_sweep::{CodeAggregate, SweepCancel, SweepOptions, WorkspaceSweep};

/// Adapts the resident's owned [`Vfs`] to the lock-neutral [`ide_host_core::VfsWrite`]
/// the shared metadata policy expects. The resident is only ever touched while the
/// caller holds the state mutex (the db is `!Sync`), so a single-threaded `RefCell`
/// gives the interning critical section its interior mutability without a second lock —
/// the same discipline the LSP's `parking_lot`-locked adapter has, minus the lock.
pub(super) struct ResidentVfs(pub(super) RefCell<Vfs>);

impl ide_host_core::VfsWrite for ResidentVfs {
    fn with_write<R>(&self, f: impl FnOnce(&mut Vfs) -> R) -> R {
        f(&mut self.0.borrow_mut())
    }
}

/// Retract everything a path's registration owns: its text input, its file-set entry
/// and its `by_path` back-link. Returns whether the file set moved.
///
/// Where the id lives depends on whether the path was still serving, and the two
/// removal routes disagree about that: the retry list owns paths that left `by_path`
/// the moment they stopped serving, while the drift classifier hands over paths that
/// may still be in it. Keying on `by_path` alone therefore reads an `Admitted` hole as
/// "never indexed" and retracts nothing — the file set keeps mapping the deleted file's
/// id, so `module_index_query` holds it as an empty module while the hole count drops
/// to zero and the workspace calls itself fresh.
///
/// `registered` is what makes the interner safe to ask: `Vfs` keeps a `FileId` forever,
/// so a path it merely REMEMBERS from an earlier life looks exactly like one this
/// resident registered. Only the caller knows which it is.
fn retire_registration(
    resident: &mut DiagnosticsResident,
    file_set: &mut vfs::FileSet,
    key: &str,
    registered: bool,
) -> bool {
    use ide_host_core::{set_file_text_source, FileTextSource, VfsWrite};

    let file_id = match resident.by_path.get(key) {
        Some(&file_id) => Some(file_id),
        None if registered => {
            resident.vfs.with_write(|vfs| vfs.file_id(&VfsPath::new(PathBuf::from(key))))
        }
        None => None,
    };
    let Some(file_id) = file_id else { return false };
    set_file_text_source(&mut resident.db, file_id, FileTextSource::Tombstone);
    resident.by_path.remove(key);
    if file_set.path_for_file(&file_id).is_some() {
        file_set.remove(file_id);
        return true;
    }
    false
}

/// Re-read every held hole. Returns `(healed, vanished)` by canonical key, so the
/// caller can move each one's baseline entry and bump what a baseline move obliges.
///
/// The full add sequence runs for EVERY heal, never a shortened "it was registered
/// before, just re-register the text" path. `Vfs` keeps a `FileId` forever while a
/// removal drops the file-set entry, so a deleted-then-recreated path is
/// indistinguishable from one that never left — and the short path would leave it
/// with an id no `path_for_file` can resolve. Every step is idempotent, so paying the
/// whole sequence costs nothing but is safe on the case that cannot be detected.
pub(super) fn retry_resident_holes(
    resident: &mut DiagnosticsResident,
    config_is_current: bool,
) -> (Vec<(String, Option<u64>)>, Vec<String>) {
    use base_db::{SourceDatabase, SourceRoot};
    use ide_host_core::{set_file_text_source, FileTextSource, VfsWrite};

    let mut healed = Vec::new();
    let mut vanished = Vec::new();
    let candidates: Vec<(String, HoleOrigin)> =
        resident.holes.iter().map(|(k, o)| (k.clone(), *o)).collect();

    let mut file_set = {
        let db = &resident.db;
        db.source_root_input(crate::graph::input::GRAPH_SOURCE_ROOT).root(db).file_set().clone()
    };
    let mut file_set_modified = false;

    for (key, origin) in candidates {
        let path = Path::new(&key);
        // Stat BEFORE the read, the same order every other applier uses. The baseline
        // must describe the bytes actually applied: stat-after-read would record a
        // write that landed between the two, so the baseline would match a disk state
        // whose text was never served, no scan would ever see drift again, and the
        // file would serve the older text at `stale: false` forever.
        let fp_before = crate::graph::scan::file_fingerprint(path);
        // Absence is established by trying to open the file, not by its absence from
        // someone else's listing — an incomplete walk must not retire a hole.
        match base_db::read_disk_text(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Gone. For a path that was serving, this is a removal and owes
                // everything the ordinary removal branch does — otherwise the
                // metadata back-link keeps pointing at a tombstoned FileId.
                file_set_modified |= retire_registration(
                    resident,
                    &mut file_set,
                    &key,
                    origin == HoleOrigin::Admitted,
                );
                resident.holes.remove(&key);
                vanished.push(key);
            }
            Err(_) => {} // still unreadable → stays a hole
            Ok(text) => {
                // Returning a NEW path to service asserts it belongs to the
                // configuration being served, which is the one thing the retry list
                // must not assume: it is memory, and the gate is deliberately asked
                // of the disk. A path that was already serving is not re-admitted.
                if origin == HoleOrigin::Pending && !config_is_current {
                    continue;
                }
                let vfs_path = VfsPath::new(path.to_path_buf());
                let file_id = resident.vfs.with_write(|vfs| vfs.alloc_file_id(vfs_path.clone()));
                resident.db.set_file_source_root(file_id, crate::graph::input::GRAPH_SOURCE_ROOT);
                set_file_text_source(&mut resident.db, file_id, FileTextSource::Disk(&text));
                if file_set.path_for_file(&file_id).is_none() {
                    file_set.insert(file_id, vfs_path);
                    file_set_modified = true;
                }
                resident.by_path.insert(key.clone(), file_id);
                resident.holes.remove(&key);
                healed.push((key, fp_before));
            }
        }
    }

    // The clone is published ONCE, after the loop — the same shape the add/remove
    // branches use. An insert that never reaches the db leaves `path_for_file` empty
    // and the first query panics.
    if file_set_modified {
        resident.db.set_source_root(
            crate::graph::input::GRAPH_SOURCE_ROOT,
            SourceRoot::new_local(file_set),
        );
    }

    // The substrate is re-issued for every transition, not just for paths that were
    // never registered. Whether a module's back-link is currently `None` depends on
    // whether a NEIGHBOUR in the same config root drifted while the hole was held —
    // which no per-hole flag can know. Skipping it would leave a healed common module
    // serving ordinary findings forever while its module-level diagnostics stay mute.
    let touched: Vec<PathBuf> = healed
        .iter()
        .map(|(key, _)| key)
        .chain(&vanished)
        .map(PathBuf::from)
        .filter(|p| project_model::is_substrate_listed_body_path(p))
        .collect();
    if !touched.is_empty() {
        let unread_bodies = resident.unread_bodies();
        ide_host_core::refresh_metadata_substrate(
            &mut resident.db,
            &resident.vfs,
            &touched,
            &unread_bodies,
        );
    }

    (healed, vanished)
}

/// Whether a hole's path was already admitted into the resident that owns it.
///
/// Healing an `Admitted` hole returns a file whose membership in this configuration
/// was already asserted; healing a `Pending` one asserts it for the first time and is
/// therefore an admission, gated on the configuration still being current. The
/// distinction is RECORDED when the hole is created because it cannot be derived
/// later: `Vfs` keeps a `FileId` forever, so a deleted-then-recreated path looks
/// exactly like one that was never removed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum HoleOrigin {
    Admitted,
    Pending,
}

/// The built resident database plus the path→FileId index needed to resolve a request
/// path to the Salsa input it set. Held behind the [`std::sync::Mutex`]; reads borrow it,
/// a reload mutates `db` in place.
pub(crate) struct DiagnosticsResident {
    pub(super) db: RootDatabaseImpl,
    /// The VFS pre-seeded with the resident's `.bsl` FileIds and grown by the metadata
    /// bootstrap with the metadata-XML ids. Kept alongside the db so a drift-driven
    /// substrate refresh can intern new composing files onto the same id space without
    /// rebuilding it.
    pub(super) vfs: ResidentVfs,
    /// Canonical-path string → FileId for every SERVED resident `.bsl`. A file whose
    /// bytes could not be read is absent here even though it exists on disk — see
    /// `holes`.
    pub(super) by_path: HashMap<String, FileId>,
    /// Workspace `.bsl` files that exist but could not be read, by the same canonical
    /// key as `by_path`. Doubles as the RETRY LIST: every reconciliation window tries
    /// to re-read them, which is what makes healing independent of both the drift
    /// fingerprint (only `(mtime, len)`) and hub health (a healthy hub runs no scan).
    /// The value records whether the path was ever admitted into THIS resident —
    /// re-admission has to ask the configuration gate, a return to service does not,
    /// and the VFS interner cannot tell the two apart because it never forgets an id.
    pub(super) holes: HashMap<String, HoleOrigin>,
    /// The project's effective diagnostics settings, loaded from `bsl-analyzer.toml` /
    /// `.bsl-analyzer.json` the same way LSP and CLI do — so `file`/`workspace` honour
    /// the project's disabled rules and thresholds, not analyzer defaults.
    pub(super) config: ide::DiagnosticsConfig,
    /// The workspace root the resident was built against — the SAME root the graph build
    /// uses (`source_dir`), so an absolute finding path strips to the graph encoder's rel
    /// and the `method/file/<rel>::<name>` graph bridge resolves.
    pub(super) workspace_root: PathBuf,
    /// `[analysis].diff_base` from the project config; drives the drift-time
    /// rescope so the vendor-diff filter tracks the moving working copy.
    pub(super) diff_base: Option<String>,
    /// Resolved (base, HEAD) OIDs the current scope was built against; the
    /// drift poll rebuilds when the live refs no longer match (ref-only moves).
    pub(super) scope_identity: Option<(String, String)>,
    /// `[analysis].ignored_authors` from the project config, kept for the
    /// drift-time filter rebuild when HEAD moves.
    pub(super) ignored_authors: Vec<String>,
    /// Blame-backed line filter pinned to one HEAD state; `None` when not
    /// configured or when the repository cannot support it (fail-open).
    pub(super) author_filter: Option<std::sync::Arc<vcs::AuthorFilter>>,
}

impl DiagnosticsResident {
    /// Resolve a request path to the resident FileId, canonicalising it the same way
    /// the loader did. A relative path is resolved against the workspace root (not the
    /// process CWD), so `diagnostics file` works regardless of where the server was
    /// started. `None` when the path is not a resident workspace `.bsl`.
    pub(crate) fn file_id_for(&self, path: &Path) -> Option<FileId> {
        let resolved;
        let abs: &Path = if path.is_absolute() {
            path
        } else {
            resolved = self.workspace_root.join(path);
            &resolved
        };
        self.by_path.get(&canonical_key(abs)).copied()
    }

    /// Whether `path` is a workspace `.bsl` that exists but could not be read.
    ///
    /// Callers that get `None` from [`Self::file_id_for`] must ask this before
    /// answering "not a workspace file": for a hole that answer is a lie about an
    /// existing file, and replacing the old lie ("the file is clean") with a new one
    /// is not the point of holding it out of service.
    pub(crate) fn is_unread(&self, path: &Path) -> bool {
        let resolved;
        let abs: &Path = if path.is_absolute() {
            path
        } else {
            resolved = self.workspace_root.join(path);
            &resolved
        };
        self.holes.contains_key(&canonical_key(abs))
    }

    /// How many workspace `.bsl` files exist but could not be read.
    pub(crate) fn unread_count(&self) -> usize {
        self.holes.len()
    }

    /// Whether the VFS interner already holds an id for `path`.
    ///
    /// Test-only, and deliberately so: it is the ONLY way to tell "not registered"
    /// from "registered but filtered out downstream", and those two states differ by
    /// whether a later query panics.
    #[cfg(test)]
    pub(super) fn vfs_file_id_for_test(&self, path: &Path) -> Option<FileId> {
        use ide_host_core::VfsWrite;
        self.vfs.with_write(|vfs| vfs.file_id(&VfsPath::new(path.to_path_buf())))
    }

    /// Whether the source root still maps `file_id` to a path.
    ///
    /// Test-only. The file-set entry is what a removal must actually retract: the
    /// interner keeps the id regardless, and a deleted file disappears from metadata
    /// discovery on its own — so neither of those can tell a complete removal from a
    /// partial one.
    #[cfg(test)]
    pub(super) fn file_set_has_for_test(&self, file_id: FileId) -> bool {
        use base_db::SourceDatabase;
        let db = &self.db;
        db.source_root_input(crate::graph::input::GRAPH_SOURCE_ROOT)
            .root(db)
            .file_set()
            .path_for_file(&file_id)
            .is_some()
    }

    /// The hole paths, in the shape the metadata substrate keys module bodies with.
    pub(super) fn unread_bodies(&self) -> ide_host_core::UnreadBodies {
        self.holes.keys().map(PathBuf::from).collect()
    }

    /// The workspace root the resident was built against (the graph's `source_dir`),
    /// used to bridge findings to durable `method/file/<rel>::<name>` graph ids.
    pub(crate) fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// An `Analysis` view over a cloned db handle. The clone shares the Salsa storage
    /// (memo/LRU cache), and is dropped before the read guard is released.
    pub(crate) fn analysis(&self) -> Analysis {
        Analysis::from_database(self.db.clone())
    }

    /// The resident Salsa database, for the `metadata` tool's root-scoped metadata
    /// reads (`resolve_*_across_roots` point-lookups and the Channel-2
    /// `configuration_for_root` header/enumeration). Borrowed under the state lock, so
    /// the borrow cannot outlive the read and a reload can never alias it.
    pub(crate) fn db(&self) -> &RootDatabaseImpl {
        &self.db
    }

    pub(crate) fn file_count(&self) -> usize {
        self.by_path.len()
    }

    /// The project's effective diagnostics config, the single source of truth shared
    /// with LSP and CLI. `file` and `workspace` analyse against this, never defaults.
    pub(crate) fn config(&self) -> &ide::DiagnosticsConfig {
        &self.config
    }

    /// Whether `path` has any line in the vendor-diff analysis scope. Resolves
    /// the path the same way [`Self::file_id_for`] does (relative → workspace
    /// root, canonicalised). `true` when no scope is configured.
    pub(crate) fn path_in_scope(&self, path: &Path) -> bool {
        let Some(scope) = self.config.scope.as_ref() else { return true };
        scope.is_file_in_scope(&self.abs_path_for(path))
    }

    /// The blame-backed `ignored_authors` filter, when active.
    pub(crate) fn author_filter(&self) -> Option<&std::sync::Arc<vcs::AuthorFilter>> {
        self.author_filter.as_ref()
    }

    /// Resolve a request path to the absolute canonical form the resident and
    /// the git workdir agree on.
    pub(crate) fn abs_path_for(&self, path: &Path) -> PathBuf {
        let abs =
            if path.is_absolute() { path.to_path_buf() } else { self.workspace_root.join(path) };
        abs.canonicalize().unwrap_or(abs)
    }
}

/// Build the `ignored_authors` blame filter for `root`. `None` — with a
/// warning — when the repository cannot support attribution (missing, bare,
/// shallow, unborn HEAD): MCP fails open and reports everything, matching the
/// scope policy; only the CLI treats these as hard errors.
pub(crate) fn build_author_filter(
    root: &Path,
    authors: &[String],
) -> Option<std::sync::Arc<vcs::AuthorFilter>> {
    if authors.is_empty() {
        return None;
    }
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    match vcs::AuthorFilter::new(&root, authors.to_vec()) {
        Ok(filter) => {
            tracing::info!(
                authors = authors.len(),
                head = %filter.head_identity(),
                "ignored-authors filter active"
            );
            Some(std::sync::Arc::new(filter))
        }
        Err(error) => {
            tracing::warn!(%error, "ignored-authors filter unavailable; reporting all findings");
            None
        }
    }
}

/// Whether a diagnostic on `range` survives the author filter: any covered
/// line kept → survives. Uses the scope-gate line mapping (half-open range →
/// last line from `end - 1`, empty range anchors at `start`).
pub(crate) fn diagnostic_survives_authors(
    keep: &vcs::LineKeep,
    index: &line_index::LineIndex,
    range: syntax::TextRange,
) -> bool {
    let start = index.line_col(range.start()).line;
    let end_offset =
        if range.is_empty() { range.start() } else { range.end() - line_index::TextSize::from(1) };
    let end = index.line_col(end_offset).line;
    keep.range_survives(start + 1, end + 1)
}

/// Drop diagnostics attributed to ignored authors, counting what was dropped.
/// Any blame failure keeps the file's findings intact (fail-open) — MCP must
/// degrade to noise, never to silence.
fn filter_by_author(
    analysis: &Analysis,
    file_id: FileId,
    path: Option<&Path>,
    filter: &vcs::AuthorFilter,
    diagnostics: Vec<ide::Diagnostic>,
    ignored: &std::sync::atomic::AtomicUsize,
) -> Vec<ide::Diagnostic> {
    let Some(path) = path else { return diagnostics };
    let text = analysis.file_text(file_id);
    match filter.lines_kept_cached(path, text.as_bytes()) {
        Ok(keep) => {
            if keep.ignored_line_count() == 0 {
                return diagnostics;
            }
            let index = line_index::LineIndex::new(&text);
            let before = diagnostics.len();
            let kept: Vec<_> = diagnostics
                .into_iter()
                .filter(|d| diagnostic_survives_authors(&keep, &index, d.range))
                .collect();
            ignored.fetch_add(before - kept.len(), std::sync::atomic::Ordering::Relaxed);
            kept
        }
        Err(error) => {
            tracing::warn!(%error, "blame failed; keeping every finding for the file");
            diagnostics
        }
    }
}

/// Compute the vendor-diff scope for `root` against `base` (workdir mode, so
/// uncommitted and untracked edits count as changed), plus the resolved
/// (base, HEAD) identity the drift poll compares against. Scope `None` — and
/// a warning — when the repo or ref cannot be resolved: MCP fails open,
/// matching LSP.
pub(crate) type ScopeBuild =
    (Option<std::sync::Arc<base_db::AnalysisScope>>, Option<(String, String)>);

pub(crate) fn build_scope(root: &Path, base: &str) -> ScopeBuild {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let identity = vcs::scope_ref_identity(&root, base).ok();
    match vcs::generate_workdir_diff_report(&root, base, true) {
        Ok(diff) => {
            let scope = std::sync::Arc::new(base_db::AnalysisScope::from_report(
                diff.report.base_ref,
                &diff.workdir,
                diff.report.files.into_iter().map(|(path, change)| (path, change.hunks)),
            ));
            tracing::info!(
                base,
                files_in_scope = scope.in_scope_file_count(),
                "vendor-diff analysis scope active"
            );
            (Some(scope), identity)
        }
        Err(error) => {
            tracing::warn!(base, %error, "vendor-diff scope unavailable; analyzing everything");
            (None, identity)
        }
    }
}

impl DiagnosticsResident {
    /// Workspace-wide diagnostics aggregated per code (the `workspace` action). Runs
    /// rayon over per-worker db clones (shared Salsa storage, the CLI `analyze`
    /// discipline). The caller MUST hold the state lock for the whole sweep so no
    /// reload mutates the master db mid-flight — that would cancel the cloned queries.
    /// Bounded by `opts.max_files` over a stable FileId order, so a cap is deterministic.
    ///
    /// `cancel` is the sweep's cancellation bridge: each worker registers its clone's
    /// salsa token before its first query, so `cancel_all` unwinds in-flight queries at
    /// their next salsa boundary and the file-boundary check skips the rest. Only
    /// worker-clone tokens are ever cancelled — the master db handle stays untouched,
    /// so concurrent `diagnostics` calls and later sweeps are unaffected.
    pub(crate) fn workspace_aggregates(
        &self,
        config: &ide::DiagnosticsConfig,
        opts: &SweepOptions,
        cancel: &SweepCancel,
    ) -> WorkspaceSweep {
        use rayon::prelude::*;
        use std::collections::HashSet;
        use std::panic::AssertUnwindSafe;

        // Vendor-diff file-gate: unchanged-vs-base files are excluded up front so the
        // sweep never walks thousands of files whose report is guaranteed empty;
        // `files_out_of_scope` keeps the coverage bookkeeping honest about the gap.
        let mut files: Vec<FileId> = Vec::with_capacity(self.by_path.len());
        let mut files_out_of_scope = 0usize;
        for (path, file_id) in &self.by_path {
            if config.scope.as_ref().is_none_or(|s| s.is_file_in_scope(Path::new(path))) {
                files.push(*file_id);
            } else {
                files_out_of_scope += 1;
            }
        }
        files.sort_by_key(|f| f.0);
        // Holes stay in the DENOMINATOR. They are not served, so they cannot be swept,
        // but shrinking the total would make an existing workspace file simply absent
        // from the coverage bookkeeping — the one thing `files_out_of_scope` exists to
        // prevent for the skips beside it.
        let files_total = self.by_path.len() + self.holes.len();
        let in_scope = files.len();
        let truncated = in_scope > opts.max_files;
        let swept = &files[..opts.max_files.min(in_scope)];

        // Author filter: blame runs inside the sweep workers (the sweep holds
        // the state lock for its whole duration anyway), keyed back to paths.
        let author_filter = self.author_filter.as_ref();
        let path_of: HashMap<FileId, &str> = if author_filter.is_some() {
            self.by_path.iter().map(|(path, id)| (*id, path.as_str())).collect()
        } else {
            HashMap::new()
        };
        let author_ignored = std::sync::atomic::AtomicUsize::new(0);

        // Per file: the (code, bucket) of each diagnostic, `None` for a file skipped or
        // unwound by cancellation. Each rayon worker owns an `Analysis` over a db clone;
        // queries run in parallel on the shared, unmutated Salsa storage. The salsa
        // token is per-handle, so it must be taken from the exact handle the worker
        // queries — registered lazily on the worker's first file, and re-registered
        // after every rayon split (`SweepWorker::clone` resets the flag). The catch
        // keeps a cancellation unwind inside the worker: the sweep degrades to skipped
        // files instead of a panic crossing rayon into the state lock.
        let per_file: Vec<Option<Vec<(String, ide::SeverityBucket)>>> = swept
            .par_iter()
            .map_with(SweepWorker::new(self.db.clone()), |worker, &file_id| {
                if !worker.registered {
                    cancel
                        .register(salsa::Database::cancellation_token(worker.analysis.database()));
                    worker.registered = true;
                }
                if cancel.is_cancelled() {
                    return None;
                }
                let caught = salsa::Cancelled::catch(AssertUnwindSafe(|| {
                    let diagnostics = worker.analysis.diagnostics(file_id, config);
                    let diagnostics = match author_filter {
                        Some(filter) if !diagnostics.is_empty() => {
                            let path = path_of.get(&file_id).map(Path::new);
                            filter_by_author(
                                &worker.analysis,
                                file_id,
                                path,
                                filter,
                                diagnostics,
                                &author_ignored,
                            )
                        }
                        _ => diagnostics,
                    };
                    diagnostics
                        .iter()
                        .map(|d| {
                            (d.code.as_str().to_string(), ide::SeverityBucket::from(d.severity))
                        })
                        .collect()
                }));
                match caught {
                    Ok(diags) => Some(diags),
                    // Only the request's own cancellation may degrade to a skipped
                    // file. A pending write cannot exist under the resident mutex
                    // and a propagated panic is a real defect in a sibling worker —
                    // re-raise both instead of hiding them behind valid aggregates.
                    Err(salsa::Cancelled::Local) => None,
                    Err(other) => std::panic::resume_unwind(Box::new(other)),
                }
            })
            .collect();

        let cancelled = cancel.is_cancelled();
        let files_swept = per_file.iter().filter(|r| r.is_some()).count();

        // Fold: code -> (bucket, total count, files-affected). All occurrences of a code
        // share a bucket under one config, so first-seen is representative.
        let mut map: HashMap<String, (ide::SeverityBucket, usize, usize)> = HashMap::new();
        for file_diags in per_file.iter().flatten() {
            let mut seen_here: HashSet<&str> = HashSet::new();
            for (code, bucket) in file_diags {
                let entry = map.entry(code.clone()).or_insert((*bucket, 0, 0));
                entry.1 += 1;
                if seen_here.insert(code.as_str()) {
                    entry.2 += 1;
                }
            }
        }

        let mut aggregates: Vec<CodeAggregate> = map
            .into_iter()
            .filter(|(_, (bucket, _, _))| *bucket >= opts.min_severity)
            .filter(|(code, _)| opts.codes.is_empty() || opts.codes.iter().any(|c| c == code))
            .map(|(code, (severity, count, files_affected))| CodeAggregate {
                code,
                severity,
                count,
                files_affected,
            })
            .collect();
        // Most-severe first, then most-frequent, then code for a stable order.
        aggregates.sort_by(|a, b| {
            b.severity.cmp(&a.severity).then(b.count.cmp(&a.count)).then(a.code.cmp(&b.code))
        });

        WorkspaceSweep {
            aggregates,
            files_swept,
            files_total,
            files_out_of_scope,
            files_unread: self.holes.len(),
            findings_ignored_by_author: author_ignored.load(std::sync::atomic::Ordering::Relaxed),
            author_head: author_filter.map(|f| f.short_identity()),
            truncated,
            cancelled,
        }
    }
}

/// Per-rayon-worker sweep state: an [`Analysis`] over an owned db clone plus whether
/// that clone's salsa cancellation token has been registered with the sweep's
/// [`SweepCancel`]. A rayon split clones the worker; the fresh db handle carries a
/// FRESH token, so `Clone` resets `registered` and the split re-registers before its
/// first query.
struct SweepWorker {
    analysis: Analysis,
    registered: bool,
}

impl SweepWorker {
    fn new(db: RootDatabaseImpl) -> Self {
        Self { analysis: Analysis::from_database(db), registered: false }
    }
}

impl Clone for SweepWorker {
    fn clone(&self) -> Self {
        Self::new(self.analysis.database().clone())
    }
}

/// Canonicalise a path to the same key the loader indexed by (`enumerate_bsl_files`
/// canonicalises, falling back to the raw path). Lets a request path in any form
/// resolve to the resident FileId.
pub(super) fn canonical_key(path: &Path) -> String {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().into_owned()
}

/// Apply drifted XML metadata + modified BSL bodies to the resident under an
/// already-held lock, shared by the scan and event-driven drift paths so both mutate
/// the resident identically. Returns `(needs_rebuild, moved)`: a full rebuild is
/// needed when an XML path resolves outside every config root (a symlink the
/// point-refresh cannot express) or a modified `.bsl` has no resident FileId (the file
/// universe moved); `moved` is whether any Salsa input actually changed. `fp_of` yields
/// the on-disk fingerprint of a `modified_bsl` path so an already-current body is
/// skipped. The caller owns the drift-baseline update (full rebase vs incremental).
pub(super) fn apply_resident_changes(
    resident: &mut DiagnosticsResident,
    xml_paths: &[PathBuf],
    added_bsl: &[String],
    modified_bsl: &[String],
    removed_bsl: &[String],
    fp_of: impl Fn(&str) -> Option<u64>,
    stats: &HashMap<String, u64>,
) -> (bool, bool) {
    use base_db::{SourceDatabase, SourceRoot};
    use ide_host_core::{set_file_text_source, FileTextSource, VfsWrite};

    // Pre-classification: an XML path resolving outside every registered config root is
    // drift the point-refresh cannot express — `refresh_metadata_substrate` gates its
    // re-discovery on `changed.starts_with(root)`, so it would silently no-op. Bail to a
    // full rebuild, which re-reads through the discovery joins, symlinks and all.
    let config_roots = resident.db.all_config_paths();
    let xml_outside_roots =
        xml_paths.iter().any(|p| !config_roots.iter().any(|(_, root)| p.starts_with(root)));
    if xml_outside_roots {
        return (true, false);
    }

    let mut moved = false;

    // (1) Reconcile the file universe FIRST — mirrors the LSP's `process_changes`
    // discipline (one FileSet clone, per-file inputs, one `set_source_root`), so the
    // substrate refresh below resolves module back-links through an up-to-date VFS and
    // root. Per-file ordering matters: the source-root + content-revision inputs are
    // registered BEFORE the file becomes visible through the FileSet
    // (`file_text_query` panics on a visible file with no revision).
    let mut file_set_modified = false;
    let mut file_set = {
        let db = &resident.db;
        db.source_root_input(crate::graph::input::GRAPH_SOURCE_ROOT).root(db).file_set().clone()
    };
    for path in added_bsl {
        // Vanished again before we got here (create+delete coalesced apart): the
        // removal pass — or the next drift window — settles it.
        if fp_of(path).is_none() && !Path::new(path).is_file() {
            continue;
        }
        // Read BEFORE interning. A file that cannot be read is not registered at all,
        // and "at all" has to include the VFS: `alloc_file_id` used to run first, so
        // skipping only from the read onwards would leave a `FileId` with no file-set
        // entry, which the next query resolves into a `path_for_file` panic.
        let text = match base_db::read_disk_text(Path::new(path)) {
            Ok(text) => text,
            Err(_) => {
                // Never admitted into this resident, so healing it later must ask the
                // configuration gate first.
                resident.holes.insert(path.clone(), HoleOrigin::Pending);
                moved = true;
                continue;
            }
        };
        let vfs_path = VfsPath::new(path.clone());
        let file_id = resident.vfs.with_write(|vfs| vfs.alloc_file_id(vfs_path.clone()));
        if let Some(&known) = resident.by_path.get(path.as_str()) {
            if known != file_id {
                // The path is already registered under a different id — an aliasing
                // (symlink/canonicalisation) case registration cannot express safely.
                return (true, moved);
            }
        }
        resident.db.set_file_source_root(file_id, crate::graph::input::GRAPH_SOURCE_ROOT);
        set_file_text_source(&mut resident.db, file_id, FileTextSource::Disk(&text));
        if file_set.path_for_file(&file_id).is_none() {
            file_set.insert(file_id, vfs_path);
            file_set_modified = true;
        }
        // The classifier's `key` IS the canonical by_path spelling (both come from the
        // scan-universe canonicalisation), so insert it verbatim — re-canonicalising
        // here could diverge on a path that vanished between classify and apply.
        resident.by_path.insert(path.clone(), file_id);
        // A path returning to service leaves the hole list, whichever branch brings it
        // back. `by_path` and `holes` must not intersect: the workspace denominator is
        // their sum, and `unread_bodies()` would hand the substrate a path it is
        // serving, blanking the back-link of a module that reads perfectly well.
        resident.holes.remove(path.as_str());
        moved = true;
    }
    for path in removed_bsl {
        // A deleted file is no longer a hole either: the retry list must not keep
        // probing a path the workspace no longer has, and leaving it there would
        // report a phantom in `unread_files` for as long as it is never recreated.
        // The origin decides whether anything was ever registered under this path —
        // a hole is not always the retry list's to retire, because the retry window
        // is throttled and `reconcile_tick` never opens it at all.
        let was_hole = resident.holes.remove(path.as_str());
        let registered = was_hole == Some(HoleOrigin::Admitted);
        // The resident moved if the removal changed anything the workspace REPORTS,
        // and the hole list is reported: it is `unread_files`, it is half of
        // `files_total`, and a non-empty one is what makes the answer stale. A
        // `Pending` hole has no registration by construction, so counting only
        // registrations would let all three change under an unmoved generation — the
        // same `result_id` answering "unreadable" and then "not in workspace". The
        // creation of that hole bumps the generation; its removal owes the same.
        // Never indexed and never a hole → nothing moved (an untracked removal is not
        // drift).
        let moved_here = was_hole.is_some() || resident.by_path.contains_key(path.as_str());
        file_set_modified |= retire_registration(resident, &mut file_set, path, registered);
        moved |= moved_here;
    }
    if file_set_modified {
        resident.db.set_source_root(
            crate::graph::input::GRAPH_SOURCE_ROOT,
            SourceRoot::new_local(file_set),
        );
    }

    // (2) Refresh the per-MDO substrate. Beside the drifted `.xml`, a created or
    // deleted common-module/service body changes its listing's `module_file`
    // reverse-index entry (the body is ordinary source, so it never flows through the
    // metadata-XML path) — include those bodies in the same re-discovery, exactly as
    // the LSP does. The config-revision bump stays `.xml`-only: a body add/remove does
    // not change the whole-config metadata content.
    let structural_listing_bodies: Vec<PathBuf> = added_bsl
        .iter()
        .chain(removed_bsl)
        .map(PathBuf::from)
        .filter(|p| project_model::is_substrate_listed_body_path(p))
        .filter(|p| config_roots.iter().any(|(_, root)| p.starts_with(root)))
        .collect();
    if !xml_paths.is_empty() || !structural_listing_bodies.is_empty() {
        let unread_bodies = resident.unread_bodies();
        let mut refresh: Vec<PathBuf> = xml_paths.to_vec();
        refresh.extend(structural_listing_bodies);
        ide_host_core::refresh_metadata_substrate(
            &mut resident.db,
            &resident.vfs,
            &refresh,
            &unread_bodies,
        );
        if !xml_paths.is_empty() {
            resident.db.bump_config_for_paths(xml_paths.iter().map(|p| p.as_path()));
        }
        moved = true;
    }

    // `.bsl` bodies: disk-backed re-key. A body already at its on-disk fingerprint (a
    // racing caller beat us) is skipped.
    let mut became_holes: Vec<PathBuf> = Vec::new();
    for path in modified_bsl {
        let Some(fp) = fp_of(path) else { continue };
        if stats.get(path).copied() == Some(fp) {
            continue;
        }
        let Some(&file_id) = resident.by_path.get(path) else {
            // A path already held as a hole is not "never indexed": the retry cycle
            // owns it, and healing it HERE would be wrong twice over — the file set
            // was already published above, so the insert would not reach the db, and
            // an admission would slip past the configuration gate.
            if resident.holes.contains_key(path) {
                continue;
            }
            return (true, moved); // a modified `.bsl` we never indexed → structural
        };
        match base_db::read_disk_text(Path::new(path)) {
            Ok(text) => {
                set_file_text_source(&mut resident.db, file_id, FileTextSource::Disk(&text))
            }
            Err(_) => {
                // Unreadable now. The tombstone is mandatory (a disk-backed re-read
                // would panic), but it is no longer passed off as content: the file
                // leaves service and joins the retry list, so consumers see "known
                // but unreadable" instead of an empty module. `Admitted` — it was
                // already serving under this configuration.
                set_file_text_source(&mut resident.db, file_id, FileTextSource::Tombstone);
                resident.by_path.remove(path);
                resident.holes.insert(path.clone(), HoleOrigin::Admitted);
                became_holes.push(PathBuf::from(path));
            }
        }
        moved = true;
    }

    // A body that just became a hole has to LOSE its `module_file` back-link, and the
    // substrate pass above already ran — it keys off `added_bsl`/`removed_bsl`, and
    // this transition is neither. Without re-issuing it here, the same disk state
    // answers differently depending on WHEN the file became unreadable: at build time
    // the back-link is `None`, at drift time it still points at the tombstoned FileId.
    // Consumers of a non-empty back-link over an empty symbol tree conclude the module
    // has no API and emit blocking findings ("create procedure …") against innocent
    // files, where `None` makes them return silently.
    let became_holes: Vec<PathBuf> = became_holes
        .into_iter()
        .filter(|p| project_model::is_substrate_listed_body_path(p))
        .filter(|p| config_roots.iter().any(|(_, root)| p.starts_with(root)))
        .collect();
    if !became_holes.is_empty() {
        let unread_bodies = resident.unread_bodies();
        ide_host_core::refresh_metadata_substrate(
            &mut resident.db,
            &resident.vfs,
            &became_holes,
            &unread_bodies,
        );
    }

    (false, moved)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{module_path, sample_workspace, wait_ready, write};
    use super::super::{DiagnosticsState, ResidentOutcome};
    use super::*;
    use ide::DiagnosticsConfig;

    /// First use builds the resident db over the workspace and resolves a request
    /// path to a FileId, then computes diagnostics for it.
    #[test]
    fn builds_resident_and_serves_file_diagnostics() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let path = module_path(root, "Сервер");
        let out = state.read(|resident, _| {
            let file_id = resident.file_id_for(&path).expect("path resolves to a resident FileId");
            resident.analysis().diagnostics(file_id, &DiagnosticsConfig::default()).len()
        });
        match out {
            ResidentOutcome::Ready(_, _) => {}
            _ => panic!("expected Ready outcome from a loaded db"),
        }
    }

    /// The resident is disk-backed: a workspace file is registered by content revision,
    /// not pinned as a `FileTextInput` overlay, so `file_text_query` re-reads it from disk
    /// under the LRU cap. This is what keeps a whole-workspace resident from OOMing. The
    /// file's text must still be queryable (diagnostics ran above), it just must not be
    /// held resident as a salsa input.
    #[test]
    fn resident_text_is_disk_backed_not_pinned() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let path = module_path(root, "Сервер");
        let out = state.read(|resident, _| {
            let file_id = resident.file_id_for(&path).expect("path resolves to a resident FileId");
            let pinned = resident.db.try_file_text(file_id).is_some();
            let len = resident.analysis().file_text(file_id).len();
            (pinned, len)
        });
        match out {
            ResidentOutcome::Ready((pinned, len), _) => {
                assert!(!pinned, "workspace file must be disk-backed, not pinned as an overlay");
                assert!(len > 0, "disk-backed text must still be readable on demand");
            }
            _ => panic!("expected Ready outcome"),
        }
    }

    /// The resident loads the project's `bsl-analyzer.toml` and exposes it as the
    /// effective config, so `file`/`workspace` honour the same disabled rules and tuned
    /// thresholds as LSP and CLI — not analyzer defaults.
    #[test]
    fn resident_config_reflects_project_toml() {
        use ide::DiagnosticCode;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        write(
            root,
            "bsl-analyzer.toml",
            "[source]\nroot = \".\"\n\n\
             [diagnostics.parameters]\n\
             Typo = false\n\n\
             [diagnostics.parameters.LineLength]\n\
             maxLineLength = 200\n",
        );

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let out = state.read(|resident, _| {
            let config = resident.config();
            (
                config.is_disabled(DiagnosticCode::Typo),
                config.get_int(DiagnosticCode::LineLength, "maxLineLength"),
            )
        });
        match out {
            ResidentOutcome::Ready((typo_disabled, line_len), _) => {
                assert!(typo_disabled, "project toml disables Typo");
                assert_eq!(line_len, Some(200), "project toml sets the LineLength threshold");
            }
            _ => panic!("expected Ready outcome"),
        }
    }

    /// A `diagnostics file` request may pass a workspace-relative path; it must resolve
    /// against the workspace root, not the process CWD.
    #[test]
    fn file_id_resolves_relative_path_against_workspace_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let rel = Path::new("CommonModules/Сервер/Ext/Module.bsl");
        let abs = module_path(root, "Сервер");
        let found = state.read(|resident, _| {
            (resident.file_id_for(rel).is_some(), resident.file_id_for(&abs).is_some())
        });
        match found {
            ResidentOutcome::Ready((rel_ok, abs_ok), _) => {
                assert!(rel_ok, "relative path resolves against the workspace root");
                assert!(abs_ok, "absolute path still resolves");
            }
            _ => panic!("expected Ready"),
        }
    }

    /// The resident's metadata substrate resolves a common module's `Ext/Module.bsl`
    /// back to the SAME FileId the resident indexed for it. This guards the seeding
    /// invariant: the VFS is pre-seeded with the resident's `.bsl` ids before the
    /// bootstrap interns the metadata XML on top, so the reverse index carries the
    /// resident's own id. Were the ids unseeded, the bootstrap would drop the back-link
    /// and `common_module_for_file_id` would return `None`.
    #[test]
    fn resident_substrate_backlinks_common_module_to_its_own_file_id() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let module = module_path(root, "Сервер");
        let project = project_model::Project::new(root).expect("valid test project");
        let config_root = project
            .source_path()
            .canonicalize()
            .unwrap_or_else(|_| project.source_path().to_path_buf());
        let root_key = config_root.to_string_lossy().into_owned();

        let out = state.read(|resident, _| {
            let file_id = resident.file_id_for(&module).expect("module .bsl resolves to a FileId");
            let listing_present = resident.db.metadata_listing(&root_key).is_some();
            let resolved = resident.db.common_module_for_file_id(file_id).is_some();
            (listing_present, resolved)
        });
        match out {
            ResidentOutcome::Ready((listing_present, resolved), _) => {
                assert!(
                    listing_present,
                    "the metadata substrate must be bootstrapped for the config root"
                );
                assert!(
                    resolved,
                    "the substrate must resolve the common module through the resident's own id"
                );
            }
            _ => panic!("expected Ready outcome from a loaded db"),
        }
    }

    fn sweep_opts() -> super::super::SweepOptions {
        super::super::SweepOptions {
            min_severity: ide::SeverityBucket::Hint,
            codes: Vec::new(),
            max_files: 1000,
        }
    }

    /// Under a vendor-diff scope the sweep analyses only files with changed lines;
    /// the excluded rest is counted so the coverage bookkeeping stays honest.
    #[test]
    fn sweep_under_scope_excludes_unchanged_files_and_counts_them() {
        use std::sync::Arc;

        use super::super::SweepCancel;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);
        // A second module, so the scope has something to exclude.
        super::super::test_support::write_common_module(
            root,
            "Клиент",
            false,
            "&НаКлиенте\nПроцедура Показать() Экспорт КонецПроцедуры",
        );

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let out = state.read(|resident, _| {
            let workdir =
                resident.workspace_root().canonicalize().expect("workspace root canonicalizes");
            let module = module_path(root, "Сервер").canonicalize().expect("module exists");
            let rel = module
                .strip_prefix(&workdir)
                .expect("module under workspace root")
                .to_string_lossy()
                .into_owned();
            let scope =
                Arc::new(base_db::AnalysisScope::from_report("vendor", &workdir, [(rel, None)]));

            let mut config = resident.config().clone();
            config.scope = Some(scope);
            let sweep =
                resident.workspace_aggregates(&config, &sweep_opts(), &SweepCancel::default());
            (resident.file_count(), sweep)
        });
        match out {
            ResidentOutcome::Ready((file_count, sweep), _) => {
                assert!(file_count > 1, "the fixture must contain more than one .bsl");
                assert_eq!(sweep.files_total, file_count);
                assert_eq!(
                    sweep.files_out_of_scope,
                    file_count - 1,
                    "everything except the admitted module must be excluded"
                );
                assert_eq!(sweep.files_swept, 1, "only the in-scope module is analysed");
            }
            _ => panic!("expected Ready"),
        }
    }

    /// A sweep whose cancellation was requested before it started produces an honest
    /// partial result (no files, `cancelled` set) and leaves the resident fully
    /// usable: a follow-up sweep with a fresh token registry completes normally —
    /// cancellation touches only per-worker clone tokens, never the master db.
    #[test]
    fn pre_cancelled_sweep_is_partial_and_leaves_the_resident_usable() {
        use super::super::SweepCancel;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let cancelled_sweep = SweepCancel::default();
        cancelled_sweep.cancel_all();
        let out = state.read(|resident, _| {
            resident.workspace_aggregates(resident.config(), &sweep_opts(), &cancelled_sweep)
        });
        match out {
            ResidentOutcome::Ready(sweep, _) => {
                assert!(sweep.cancelled, "the sweep must report the cancellation");
                assert_eq!(sweep.files_swept, 0, "no file completes under a pre-cancelled sweep");
                assert!(sweep.aggregates.is_empty());
                assert_eq!(sweep.files_total, 1, "coverage bookkeeping still describes the config");
            }
            _ => panic!("expected Ready outcome"),
        }

        let out = state.read(|resident, _| {
            resident.workspace_aggregates(resident.config(), &sweep_opts(), &SweepCancel::default())
        });
        match out {
            ResidentOutcome::Ready(sweep, _) => {
                assert!(!sweep.cancelled);
                assert_eq!(sweep.files_swept, 1, "a fresh sweep over the same resident completes");
            }
            _ => panic!("expected Ready outcome"),
        }
    }

    /// The core mechanism the sweep relies on, exercised deterministically: a
    /// cancelled salsa token of a worker-style db clone unwinds an in-flight
    /// diagnostics computation with `Cancelled::Local` (mid-file, not just at the
    /// file-boundary check), the catch contains the unwind, and the master handle
    /// keeps serving queries afterwards.
    #[test]
    fn cancelled_clone_token_unwinds_a_diagnostics_query() {
        use ide::DiagnosticsConfig;
        use std::panic::AssertUnwindSafe;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let path = module_path(root, "Сервер");
        let out = state.read(|resident, _| {
            let file_id = resident.file_id_for(&path).expect("path resolves to a resident FileId");

            let analysis = resident.analysis();
            salsa::Database::cancellation_token(analysis.database()).cancel();
            let caught = salsa::Cancelled::catch(AssertUnwindSafe(|| {
                analysis.diagnostics(file_id, &DiagnosticsConfig::default()).len()
            }));
            let unwound = matches!(caught, Err(salsa::Cancelled::Local));

            // A fresh clone is a different salsa handle with its own token: the
            // same query must complete normally after the first clone's cancel —
            // reaching the return proves it did not unwind.
            let _ = resident.analysis().diagnostics(file_id, &DiagnosticsConfig::default());
            unwound
        });
        match out {
            ResidentOutcome::Ready(unwound, _) => {
                assert!(unwound, "a cancelled clone token must unwind the query with Local");
            }
            _ => panic!("expected Ready outcome"),
        }
    }

    /// A cancel arriving after the sweep completed is a no-op: the result is already
    /// final and the resident keeps serving per-file diagnostics.
    #[test]
    fn late_cancel_after_sweep_completion_is_a_noop() {
        use super::super::SweepCancel;
        use ide::DiagnosticsConfig;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        sample_workspace(root);

        let state = DiagnosticsState::for_workspace(root.to_path_buf());
        state.ensure_loading();
        wait_ready(&state);

        let cancel = SweepCancel::default();
        let out = state.read(|resident, _| {
            resident.workspace_aggregates(resident.config(), &sweep_opts(), &cancel)
        });
        let sweep = match out {
            ResidentOutcome::Ready(sweep, _) => sweep,
            _ => panic!("expected Ready outcome"),
        };
        assert!(!sweep.cancelled);
        assert_eq!(sweep.files_swept, 1);

        cancel.cancel_all();

        let path = module_path(root, "Сервер");
        let out = state.read(|resident, _| {
            let file_id = resident.file_id_for(&path).expect("path resolves to a resident FileId");
            resident.analysis().diagnostics(file_id, &DiagnosticsConfig::default()).len()
        });
        assert!(
            matches!(out, ResidentOutcome::Ready(_, _)),
            "the resident must keep serving per-file diagnostics after a late cancel"
        );
    }

    /// A symlink inside the config tree must not drop the common module's back-link.
    #[cfg(unix)]
    #[test]
    fn resident_substrate_backlinks_common_module_through_symlinked_dir() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let root = base.join("ws");
        std::fs::create_dir_all(&root).unwrap();
        let real = base.join("real");
        super::super::test_support::write_common_module(
            &real,
            "Сервер",
            true,
            "&НаСервере\nФункция Ч() Экспорт КонецФункции",
        );
        std::os::unix::fs::symlink(real.join("CommonModules"), root.join("CommonModules")).unwrap();

        let state = DiagnosticsState::for_workspace(root.clone());
        state.ensure_loading();
        wait_ready(&state);

        let module = root.join("CommonModules/Сервер/Ext/Module.bsl");
        let out = state.read(|resident, _| {
            let file_id = resident.file_id_for(&module).expect("module .bsl resolves to a FileId");
            resident.db.common_module_for_file_id(file_id).is_some()
        });
        match out {
            ResidentOutcome::Ready(resolved, _) => assert!(
                resolved,
                "back-link must resolve through a symlinked config subtree via the canonicalising fallback"
            ),
            _ => panic!("expected Ready outcome from a loaded db"),
        }
    }
}
