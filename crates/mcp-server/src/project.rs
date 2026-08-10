//! The one place this crate turns a workspace path into a [`Project`].
//!
//! A source set given on the command line is not on disk, so a call that only
//! receives a root path cannot see it. That matters here more than elsewhere:
//! the crate re-derives the project from a bare path in a dozen places — graph
//! scans, freshness checks, drift identity, the resident diagnostics host, the
//! broker key — and none of them can reach `SharedState`. Threading a spec
//! through every one of them would mean widening a dozen signatures whose
//! callers do not know about source sets either.
//!
//! Instead the override is process state, set once from argv before anything
//! builds a project, and consulted here. A server process serves exactly one
//! workspace with exactly one source set for its whole life, so there is nothing
//! for a second value to belong to. The scope stays inside this crate:
//! `project-model` gains no global, and `Project::new` keeps meaning what it
//! says everywhere else.

use std::path::Path;
use std::sync::OnceLock;

use project_model::{Project, ProjectConfig, ProjectError, SourceSetOverride};

static OVERRIDE: OnceLock<SourceSetOverride> = OnceLock::new();

/// Installs the source set this process was launched with.
///
/// Must run before anything builds a project — in particular before the broker
/// computes its backend key, which happens before a server exists at all.
/// Returns `false` if a source set was already installed, which only happens if
/// a caller violates that ordering; the first value wins, because a project
/// already built under it cannot be un-built.
pub fn set_source_set_override(source_set: SourceSetOverride) -> bool {
    OVERRIDE.set(source_set).is_ok()
}

/// The source set installed for this process, if any.
pub fn source_set_override() -> Option<&'static SourceSetOverride> {
    OVERRIDE.get()
}

/// Builds the project at `root` under this process's source set.
pub fn at(root: &Path) -> Result<Project, ProjectError> {
    let mut config = ProjectConfig::load(root)?.unwrap_or_default();
    if let Some(source_set) = OVERRIDE.get() {
        source_set.apply_to(&mut config);
    }
    Project::with_config(root, config)
}

/// The root table for one project: the configuration root plus every declared extension,
/// identified relative to the PROJECT directory — the identity [`bsl_search::WorkspaceRoots`]
/// documents and the one stored rows carry across restarts.
///
/// It lives beside [`at`] for the same reason [`at`] does: more than one subsystem needs the
/// table, and two of them — the search engine and the diagnostics resident — must agree on it
/// exactly. Agreement that follows from calling one constructor cannot drift; agreement between
/// two constructors is a claim someone has to keep true.
///
/// Rejected roots come back rather than being logged here. Whether a dropped root is worth a
/// line depends on how often the caller runs: the boot builds this once, while the resident
/// rebuilds on every config drift, and a warning repeated per rebuild buries the one that is
/// actually new. Callers that run once say it with [`warn_about_rejected_roots`].
pub fn workspace_roots(
    project: &Project,
) -> (bsl_search::WorkspaceRoots, Vec<bsl_search::RejectedRoot>) {
    let extensions: Vec<std::path::PathBuf> =
        project.extension_paths().iter().map(|(_, path)| path.clone()).collect();
    bsl_search::WorkspaceRoots::build(&project.root, project.source_path(), &extensions)
}

/// Name every root that did not make it into the table, with its reason: a silently dropped
/// root looks exactly like a tree nobody edited.
pub fn warn_about_rejected_roots(rejected: &[bsl_search::RejectedRoot]) {
    for rejection in rejected {
        tracing::warn!(
            path = ?rejection.path,
            reason = ?rejection.reason,
            "extension root is not registered in the search index",
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    fn rust_sources(dir: &Path, into: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read crate sources") {
            let path = entry.expect("read dir entry").path();
            if path.is_dir() {
                rust_sources(&path, into);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                into.push(path);
            }
        }
    }

    /// CALL sites of `name`: the name with a `(` next, whatever stands between them
    /// being whitespace. Both call syntaxes reach the same accessor and so must
    /// count the same — `x.name()` and `Type::name(x)` alike.
    ///
    /// Only the left side needs a boundary, and an ASCII one suffices: a longer
    /// identifier ending in this name continues leftwards through `_` or a letter.
    /// The right side needs none at all — the `(` is the boundary — which is what
    /// keeps this out of Rust's identifier grammar entirely. Approximating that
    /// grammar by hand is what three review rounds kept finding new holes in: first
    /// a needle that carried its own empty parentheses, then a boundary blind to
    /// combining marks, then one that swallowed typographic punctuation.
    fn call_occurrences(text: &str, name: &str) -> usize {
        let continues_leftwards = |c: char| c.is_ascii_alphanumeric() || c == '_';
        text.match_indices(name)
            .filter(|(at, _)| {
                let before = text[..*at].chars().next_back();
                let after = text[at + name.len()..].trim_start();
                !before.is_some_and(continues_leftwards) && after.starts_with('(')
            })
            .count()
    }

    /// Neutral names on purpose: the gate below counts the real ones.
    #[test]
    fn a_call_site_is_counted_and_a_mention_is_not() {
        assert_eq!(call_occurrences("x.bar()", "bar"), 1, "a method call");
        assert_eq!(call_occurrences("Type::bar(x)", "bar"), 1, "UFCS reaches the same accessor");
        assert_eq!(call_occurrences("bar (x)", "bar"), 1, "whitespace before the parenthesis");
        assert_eq!(
            call_occurrences("bar\u{0085}()", "bar"),
            1,
            "NEXT LINE is whitespace to Rust, so this is still a call"
        );
        assert_eq!(call_occurrences("«bar» в прозе", "bar"), 0, "a mention is not a call");
        assert_eq!(call_occurrences("foo_bar()", "bar"), 0, "a longer name is another name");
        assert_eq!(call_occurrences("bar_baz()", "bar"), 0, "so is a longer name after it");
    }

    /// Every root set in this crate comes from one derivation. The two accessors
    /// that hand back extension directories separately are what a second, hand-built
    /// set would have to reach for, so each gets exactly one place: the one that
    /// needs it for something other than a root set.
    ///
    /// COUNTED, not located. An allowlist by file says nothing about a second
    /// occurrence inside the allowed file, and two hand-built root tables in one file
    /// drift from each other exactly like two in two files.
    ///
    /// This gate covers unit tests too, and deliberately: the test that used to
    /// rebuild the set by hand also carried a comment claiming it armed "the SAME
    /// targets production arms" — a claim nothing held true.
    ///
    /// What is counted is CALLS, not mentions: naming an accessor in prose builds
    /// no root table, and a rule that forbade the name outright would be a rule
    /// about writing rather than about code.
    #[test]
    fn the_root_set_is_derived_in_exactly_one_place() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_sources(&src, &mut files);
        assert!(files.len() > 20, "the source scan found almost nothing: {}", files.len());

        let names = ["extension_paths", "extension_topology"];
        let mut sites: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for file in &files {
            let text = std::fs::read_to_string(file).expect("read crate source");
            let rel = file.strip_prefix(&src).expect("source under src").display().to_string();
            for name in names {
                for _ in 0..call_occurrences(&text, name) {
                    sites.entry(name).or_default().push(rel.replace('\\', "/"));
                }
            }
        }

        let expected: BTreeMap<&str, Vec<String>> = [
            (names[0], vec!["project.rs".to_string()]),
            (names[1], vec!["broker/name.rs".to_string()]),
        ]
        .into_iter()
        .collect();

        assert_eq!(
            sites, expected,
            "a root set must come from Project::source_roots(); these accessors rebuild it by hand"
        );
    }
}
