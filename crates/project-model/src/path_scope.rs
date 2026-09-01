//! One rule for "is this path inside a declared root, or inside a hole in one".
//!
//! The question is asked in three unrelated places — the file watcher deciding whether
//! an event is a workspace change, the walk deciding whether to descend, and the boot
//! check refusing a cache directory that swallows a root — and three independent
//! answers drifted apart twice before this type existed: one copy dropped a whole hole
//! where another carved out a single root, and two compared paths lexically while the
//! third canonicalised. Both divergences were silent, because each copy is correct on
//! the inputs its own tests use.
//!
//! The rule itself, in full:
//!
//! 1. every side has TWO spellings — the one it was declared by and the one the file
//!    system resolves it to — and they part company on a symlink, on `..` left in a
//!    config value (`root.join(config_root)` never resolves it) and on Windows, where
//!    `canonicalize` returns `\\?\C:\...`;
//! 2. a root declared INSIDE a hole wins over it — but only for itself, without
//!    re-opening the rest of the hole;
//! 3. a hole covering a root is a contradiction the caller must answer for, not
//!    something this type resolves quietly.
//!
//! Holes are always the caller's own statement about a subtree it owns. Nothing here
//! knows any directory NAME, and narrowing the file universe by name stays forbidden
//! (see `no_directory_is_excluded_from_the_walk` in [`crate::workspace_walk`]).

use std::path::{Path, PathBuf};

/// Every spelling one directory can appear under in a path handed to us.
///
/// Two, because both are real: the watcher arms and reports the DECLARED path while
/// topology decisions rank by the canonical one, and the walk yields entries spelled
/// the way its start was spelled. A predicate holding one spelling silently discards
/// every tree named by the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spellings {
    declared: PathBuf,
    canonical: PathBuf,
}

impl Spellings {
    /// Canonicalisation is best-effort by necessity: a path that does not exist yet is
    /// still a legitimate root (a cache directory is created after it is named), and
    /// falling back to the declared spelling keeps such a path decidable instead of
    /// dropping it.
    pub fn of(path: &Path) -> Self {
        Self {
            declared: path.to_path_buf(),
            canonical: path.canonicalize().unwrap_or_else(|_| path.to_path_buf()),
        }
    }

    pub fn declared(&self) -> &Path {
        &self.declared
    }

    pub fn canonical(&self) -> &Path {
        &self.canonical
    }

    /// Whether `path` lies at or under this directory, under either spelling.
    ///
    /// Matched on whole components — `Path::starts_with` never mistakes `.buildfoo`
    /// for `.build`.
    pub fn covers(&self, path: &Path) -> bool {
        path.starts_with(&self.declared) || path.starts_with(&self.canonical)
    }

    /// Whether `path` IS this directory, under either spelling. Distinct from
    /// [`Self::covers`]: a config file is decided by its parent being a watched
    /// directory exactly, not by lying somewhere beneath one.
    pub fn is(&self, path: &Path) -> bool {
        path == self.declared || path == self.canonical
    }

    /// Whether this directory covers `other` under either of `other`'s spellings.
    ///
    /// Both, for the same reason [`Self::covers`] takes both: the two part company at a
    /// symlink, and comparing one spelling against one other spelling would miss a
    /// containment that plainly holds.
    fn covers_dir(&self, other: &Spellings) -> bool {
        self.covers(&other.declared) || self.covers(&other.canonical)
    }
}

/// A subtree the caller has declared out of scope, with every spelling a path inside it
/// can arrive under.
#[derive(Debug, Clone)]
struct Hole {
    /// What the caller named it by — the spelling an error message should print, so the
    /// message says the thing the caller can go and change.
    named: Spellings,
    /// `named`'s two spellings PLUS one per root that contains this hole.
    ///
    /// The extra spellings are what makes the test that follows purely lexical. The walk
    /// hands us paths built by joining onto the root AS DECLARED, so a root carrying `..`
    /// or a symlink yields entries that match neither spelling of a hole named
    /// absolutely. Canonicalising each entry instead would be a system call per file;
    /// re-spelling the hole once per root, here, costs one call per hole.
    spellings: Vec<PathBuf>,
}

/// The set of roots a caller walks or watches, and the holes it has punched in them.
#[derive(Debug, Clone, Default)]
pub struct PathScope {
    roots: Vec<Spellings>,
    holes: Vec<Hole>,
    /// Roots that lie INSIDE a hole, and therefore win over it.
    ///
    /// A root is something the caller explicitly asked for, so the more specific
    /// statement holds. This is also what keeps the destructive case impossible: a hole
    /// that swallowed a root would leave the walk with NOTHING, and an empty walk has
    /// `unreadable == 0`, so [`crate::SourceSet::clean`] would call it a complete view
    /// of the tree — and a reconcile over that deletes every stored row. Carving the
    /// root back out keeps the walk non-empty without re-opening the rest of the hole,
    /// which simply dropping the hole would.
    carve_outs: Vec<Spellings>,
}

impl PathScope {
    pub fn new(roots: &[PathBuf], holes: &[PathBuf]) -> Self {
        let roots: Vec<Spellings> = roots.iter().map(|path| Spellings::of(path)).collect();
        let holes: Vec<Hole> = holes
            .iter()
            .map(|path| {
                let named = Spellings::of(path);
                let mut spellings = vec![named.declared.clone()];
                if named.canonical != named.declared {
                    spellings.push(named.canonical.clone());
                }
                for root in &roots {
                    let Ok(relative) = named.canonical.strip_prefix(&root.canonical) else {
                        continue;
                    };
                    let under_root = root.declared.join(relative);
                    if !spellings.contains(&under_root) {
                        spellings.push(under_root);
                    }
                }
                Hole { named, spellings }
            })
            .collect();
        let carve_outs = roots
            .iter()
            .filter(|root| holes.iter().any(|hole| hole.named.covers_dir(root)))
            .cloned()
            .collect();
        Self { roots, holes, carve_outs }
    }

    /// Whether `path` lies at or under a declared root. Holes are NOT consulted: a
    /// caller that needs both asks both, and the two questions have different answers
    /// for a path inside a carved-out root.
    pub fn covers(&self, path: &Path) -> bool {
        self.roots.iter().any(|root| root.covers(path))
    }

    /// Whether `path` lies in a hole — that is, in a subtree the caller declared out of
    /// scope and did not then declare a root inside.
    pub fn is_hole(&self, path: &Path) -> bool {
        self.holes.iter().any(|hole| hole.spellings.iter().any(|hole| path.starts_with(hole)))
            && !self.carve_outs.iter().any(|root| root.covers(path))
    }

    /// A hole that swallows a whole root, if there is one, as `(hole, root)` in the
    /// spellings the caller named them by.
    ///
    /// Reported rather than resolved. A caller that walks or watches such a root would
    /// be serving a tree it has stopped following, and from the outside a typo and a
    /// deliberate choice look identical — so the decision belongs to whoever chose the
    /// two paths, and the carve-out above exists only for the case where the root is
    /// declared later, after that decision was already made.
    pub fn hole_covering_a_root(&self) -> Option<(PathBuf, PathBuf)> {
        self.holes.iter().find_map(|hole| {
            self.roots
                .iter()
                .find(|root| hole.named.covers_dir(root))
                .map(|root| (hole.named.declared.clone(), root.declared.clone()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `..` survives into a scan root: `discover_source_path` builds it as
    /// `root.join(config_root)` (`lib.rs`) and never resolves the result. A refusal
    /// comparing the declared spellings alone is a no-op for exactly that root — it
    /// passes every test written with tidy absolute paths, and lets the cache swallow
    /// the sources in the one layout that reaches it.
    #[test]
    fn a_root_reached_through_dot_dot_is_seen_inside_the_hole() {
        let outer = tempfile::tempdir().unwrap();
        let sources = outer.path().join("elsewhere").join("sources");
        let workspace = outer.path().join("ws");
        std::fs::create_dir_all(&sources).unwrap();
        std::fs::create_dir_all(&workspace).unwrap();

        let through_dot_dot = workspace.join("..").join("elsewhere").join("sources");
        let hole = outer.path().join("elsewhere");
        let scope =
            PathScope::new(std::slice::from_ref(&through_dot_dot), std::slice::from_ref(&hole));

        let covering = scope.hole_covering_a_root();
        assert!(
            covering.is_some(),
            "a hole above the source root was not seen: {}",
            through_dot_dot.display()
        );

        // Positive control: the same root, spelled the same way, with the hole beside it
        // rather than above it. Without this the assertion above would hold on a build
        // that answers "covered" to everything.
        let beside = outer.path().join("cache");
        let accepted = PathScope::new(&[through_dot_dot], std::slice::from_ref(&beside));
        assert_eq!(accepted.hole_covering_a_root(), None, "a hole beside the root was refused");
    }

    /// A root declared through a symlink and a hole named absolutely: the walk yields
    /// entries spelled through the link, the watcher reports whichever the backend saw,
    /// and both must reach the same verdict.
    #[test]
    fn a_hole_under_a_linked_root_is_recognised_under_either_spelling() {
        let outer = tempfile::tempdir().unwrap();
        let real = outer.path().join("real");
        let cache = real.join(".build");
        std::fs::create_dir_all(&cache).unwrap();
        let link = outer.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&real, &link).unwrap();

        // The hole is named canonically while the root is named through the link, so
        // neither of the hole's own two spellings matches an entry the walk produces.
        let scope = PathScope::new(std::slice::from_ref(&link), std::slice::from_ref(&cache));

        assert!(scope.is_hole(&cache.join("bsl-graph.db")), "the canonical spelling missed");
        assert!(
            scope.is_hole(&link.join(".build").join("bsl-graph.db")),
            "the spelling the walk produces missed"
        );
        // Positive control: a sibling sharing the hole's name prefix stays in scope, so
        // the assertions above cannot be held by a predicate that answers "hole" always.
        assert!(!scope.is_hole(&link.join(".buildfoo").join("Module.bsl")), "a sibling was cut");
        assert!(!scope.is_hole(&link.join("src").join("Module.bsl")), "a source file was cut");
    }

    /// A root declared inside a hole wins over it — and takes only itself, leaving the
    /// rest of the hole shut.
    #[test]
    fn a_root_inside_a_hole_is_carved_out_of_it() {
        let outer = tempfile::tempdir().unwrap();
        let cache = outer.path().join(".build");
        let vendored = cache.join("vendor");
        std::fs::create_dir_all(&vendored).unwrap();

        let scope = PathScope::new(
            &[outer.path().to_path_buf(), vendored.clone()],
            std::slice::from_ref(&cache),
        );

        assert!(!scope.is_hole(&vendored.join("Module.bsl")), "the declared root stayed cut off");
        assert!(scope.is_hole(&cache.join("bsl-graph.db")), "the rest of the hole re-opened");
    }
}
