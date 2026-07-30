//! CLI flags declaring the project source set — the main configuration root and
//! the extensions analyzed alongside it.
//!
//! These mirror the `[source]` section of the config file one-to-one, so that a
//! caller who cannot drop a config file into someone else's tree can still say
//! "this directory is an extension of that configuration". Mapping is
//! deliberately identical to the two shapes the config already accepts, so no
//! new project semantics enter through the command line.
//!
//! Where the CLI *is* stricter than the file: anything typed explicitly must
//! either work or fail loudly. A mistyped path in a config file may be a stale
//! entry someone else left behind, but a mistyped path in argv is this
//! invocation's mistake, and silently analyzing a different source set instead
//! is worse than refusing to start.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use clap::Args;
use project_model::{ExtensionDecl, SourceSetOverride, StructuredExtensionDecl};
use stdx::case::fold_lower_per_char;

/// Source-set flags shared by every command that builds a project from argv.
#[derive(Debug, Clone, Default, Args)]
pub struct SourceSetArgs {
    /// Root of the main configuration, relative to the project root (the
    /// `Configuration.xml`-bearing directory, e.g. `src/cf`).
    #[arg(long = "configuration-root", value_name = "PATH")]
    pub configuration_root: Option<String>,

    /// Extension to analyze alongside the main configuration. Repeat per
    /// extension. `NAME=PATH` declares a named extension that dependencies may
    /// refer to; a bare `PATH` behaves like a plain string entry in the config
    /// and may use a `*` glob in its final segment.
    #[arg(long = "extension", value_name = "NAME=PATH|PATH")]
    pub extensions: Vec<String>,

    /// Declare that an extension uses another extension's API:
    /// `NAME=DEP[,DEP...]`. Repeatable, and repeats for the same owner
    /// accumulate. Both sides must be names declared via `--extension NAME=PATH`.
    #[arg(long = "extension-depends-on", value_name = "NAME=DEP[,DEP...]")]
    pub extension_depends_on: Vec<String>,

    /// Analyze no extensions at all, overriding both a configured list and the
    /// conventional `src/cfe/*` discovery.
    #[arg(long = "no-extensions", conflicts_with = "extensions")]
    pub no_extensions: bool,
}

pub enum SourceSetArgsError {
    ConfigurationRootNotFound { value: String, expected: PathBuf },
    EmptyExtensionName { value: String },
    EmptyExtensionPath { value: String },
    DuplicateExtensionPath { first: String, second: String, path: PathBuf },
    MalformedDependsOn { value: String },
    UnknownDependsOnOwner { name: String },
    UnknownDependsOnTarget { owner: String, name: String },
    AmbiguousExtensionValue { value: String },
    EmptyDependsOnTarget { value: String },
}

impl fmt::Display for SourceSetArgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConfigurationRootNotFound { value, expected } => write!(
                f,
                "--configuration-root {value}: no Configuration.xml at {}",
                expected.display()
            ),
            Self::EmptyExtensionName { value } => {
                write!(f, "--extension {value}: the name before '=' is empty")
            }
            Self::EmptyExtensionPath { value } => {
                write!(f, "--extension {value}: the path is empty")
            }
            Self::DuplicateExtensionPath { first, second, path } => write!(
                f,
                "--extension {first} and --extension {second} name the same directory {}",
                path.display()
            ),
            Self::MalformedDependsOn { value } => write!(
                f,
                "--extension-depends-on {value}: expected NAME=DEP[,DEP...]"
            ),
            Self::UnknownDependsOnOwner { name } => write!(
                f,
                "--extension-depends-on {name}=...: {name} is not declared by any --extension NAME=PATH"
            ),
            Self::UnknownDependsOnTarget { owner, name } => write!(
                f,
                "--extension-depends-on {owner}={name}: {name} is not declared by any --extension NAME=PATH"
            ),
            Self::AmbiguousExtensionValue { value } => write!(
                f,
                "--extension {value}: reads both as a directory of that name and as NAME=PATH, \
                 and both exist; name it explicitly, as in --extension NAME={value}"
            ),
            Self::EmptyDependsOnTarget { value } => {
                write!(f, "--extension-depends-on {value}: a dependency name is empty")
            }
        }
    }
}

// Delegated rather than derived: `main` returns `Result`, and the runtime
// renders the error with `Debug`. A derived one would dump struct fields at a
// user who mistyped a path, when the whole point of failing fast here is to say
// what is wrong in a sentence.
impl fmt::Debug for SourceSetArgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Error for SourceSetArgsError {}

impl SourceSetArgs {
    /// Whether `--extension` or `--no-extensions` claimed the extension list.
    /// `--extension-depends-on` alone cannot: it only annotates entries the
    /// same invocation declared.
    pub fn declares_extensions(&self) -> bool {
        self.no_extensions || !self.extensions.is_empty()
    }

    /// Validates the flags and turns them into an override applied to the
    /// project config. `root` is the project root the relative paths resolve
    /// against — the same base the config file's own paths use.
    pub fn resolve(&self, root: &Path) -> Result<SourceSetOverride, SourceSetArgsError> {
        let configuration_root = self.resolve_configuration_root(root)?;
        let extensions = self.resolve_extensions(root)?;
        Ok(SourceSetOverride { configuration_root, extensions })
    }

    fn resolve_configuration_root(
        &self,
        root: &Path,
    ) -> Result<Option<String>, SourceSetArgsError> {
        let Some(value) = self.configuration_root.as_ref() else {
            return Ok(None);
        };
        let candidate = root.join(value);
        // `is_file`, not `exists`: a directory (or a fifo) named
        // `Configuration.xml` satisfies mere existence, and the root would be
        // accepted as a configuration it cannot possibly be.
        if !candidate.join("Configuration.xml").is_file() {
            // The config file's own key degrades to auto-discovery here; a flag
            // must not, or the command would quietly analyze whichever
            // configuration the search happens to find instead of the named one.
            return Err(SourceSetArgsError::ConfigurationRootNotFound {
                value: value.clone(),
                expected: candidate,
            });
        }
        Ok(Some(value.clone()))
    }

    fn resolve_extensions(
        &self,
        root: &Path,
    ) -> Result<Option<Vec<ExtensionDecl>>, SourceSetArgsError> {
        if !self.declares_extensions() || self.no_extensions {
            // Nothing here can own a dependency: either no extension was
            // declared at all, or the list is explicitly empty. Rejecting beats
            // silently dropping the edge the caller asked for.
            if let Some((owner, _)) = self.parse_dependencies()?.into_iter().next() {
                return Err(SourceSetArgsError::UnknownDependsOnOwner { name: owner });
            }
            return Ok(if self.no_extensions { Some(Vec::new()) } else { None });
        }

        let mut decls: Vec<ExtensionDecl> = Vec::with_capacity(self.extensions.len());
        // Folded name → index, matching how the topology compares names.
        let mut named: HashMap<String, usize> = HashMap::new();
        // Canonical path → the spelling that claimed it, for the duplicate check.
        let mut claimed: HashMap<PathBuf, String> = HashMap::new();

        for value in &self.extensions {
            let (name, path) = split_extension_value(value, root)?;
            if let Some(ref name) = name {
                named.insert(fold_lower_per_char(name), decls.len());
            }

            // Globs describe a set, so overlapping one with an explicit entry is
            // legitimate and left to the model's own de-duplication. Only
            // literal spellings are held to "name each directory once".
            if !path.contains('*') {
                let resolved = root.join(&path);
                let canonical = std::fs::canonicalize(&resolved).unwrap_or(resolved);
                if let Some(first) = claimed.get(&canonical) {
                    return Err(SourceSetArgsError::DuplicateExtensionPath {
                        first: first.clone(),
                        second: value.clone(),
                        path: canonical,
                    });
                }
                claimed.insert(canonical, value.clone());
            }

            decls.push(match name {
                Some(name) => ExtensionDecl::Structured(StructuredExtensionDecl {
                    name,
                    path,
                    depends_on: Vec::new(),
                }),
                None => ExtensionDecl::Path(path),
            });
        }

        for (owner, targets) in self.parse_dependencies()? {
            let Some(&index) = named.get(&fold_lower_per_char(&owner)) else {
                return Err(SourceSetArgsError::UnknownDependsOnOwner { name: owner });
            };
            // Targets are checked too, not just the owner. The topology would
            // happily resolve a target against the name a bare entry derives
            // from its directory, which is exactly the binding this flag must
            // not create: that name is incidental and collides with any other
            // directory spelled the same.
            for target in &targets {
                if !named.contains_key(&fold_lower_per_char(target)) {
                    return Err(SourceSetArgsError::UnknownDependsOnTarget {
                        owner,
                        name: target.clone(),
                    });
                }
            }
            match &mut decls[index] {
                ExtensionDecl::Structured(decl) => decl.depends_on.extend(targets),
                // Unreachable: `named` only ever holds indices of structured
                // entries, and nothing removes or reorders `decls`.
                ExtensionDecl::Path(_) => {
                    return Err(SourceSetArgsError::UnknownDependsOnOwner { name: owner })
                }
            }
        }

        Ok(Some(decls))
    }

    /// Owner → declared targets, in declaration order. Repeats of the same
    /// owner accumulate instead of the last flag winning, so
    /// `--extension-depends-on T=A --extension-depends-on T=B` and
    /// `--extension-depends-on T=A,B` describe the same edges.
    fn parse_dependencies(&self) -> Result<Vec<(String, Vec<String>)>, SourceSetArgsError> {
        let mut owners: Vec<(String, Vec<String>)> = Vec::new();

        for value in &self.extension_depends_on {
            let (owner, targets) = value
                .split_once('=')
                .ok_or_else(|| SourceSetArgsError::MalformedDependsOn { value: value.clone() })?;
            let owner = owner.trim();
            if owner.is_empty() {
                return Err(SourceSetArgsError::MalformedDependsOn { value: value.clone() });
            }
            let mut parsed = Vec::new();
            for target in targets.split(',') {
                let target = target.trim();
                if target.is_empty() {
                    return Err(SourceSetArgsError::EmptyDependsOnTarget { value: value.clone() });
                }
                parsed.push(target.to_string());
            }

            let key = fold_lower_per_char(owner);
            match owners.iter_mut().find(|(seen, _)| fold_lower_per_char(seen) == key) {
                Some((_, existing)) => existing.extend(parsed),
                None => owners.push((owner.to_string(), parsed)),
            }
        }

        Ok(owners)
    }
}

/// `NAME=PATH` splits on the first `=`; anything else is a bare path.
///
/// The split is unconditional, which costs one case: a directory whose own name
/// contains `=` cannot be given in the bare form, and combined with a glob it
/// is unreachable. That is deliberate. The two readings of `Named=cfe/*` — a
/// named entry with a glob, or a bare path under a directory called `Named=cfe`
/// — are textually identical, and deciding by whether a `*` is present picks
/// the bare one for `Named=cfe/*` too, which then matches nothing and drops the
/// extension without a word. A rare directory name that fails loudly beats a
/// common spelling that silently analyzes a different source set.
///
/// The name is trimmed and the path is not. A name is an identifier, where
/// surrounding blanks cannot be meaningful; a path is a filesystem path, where
/// they can — trimming it would quietly analyze a different directory than the
/// one that was passed.
fn split_extension_value(
    value: &str,
    root: &Path,
) -> Result<(Option<String>, String), SourceSetArgsError> {
    // The one case where both readings are live: the whole value names an
    // extension directory *and* the split names another one. Guessing here
    // would analyze a directory the caller did not ask for, so say so instead.
    if value.contains('=')
        && is_extension_dir(root, value)
        && value.split_once('=').is_some_and(|(_, path)| is_extension_dir(root, path))
    {
        return Err(SourceSetArgsError::AmbiguousExtensionValue { value: value.to_string() });
    }
    match value.split_once('=') {
        Some((name, path)) => {
            let name = name.trim();
            if name.is_empty() {
                return Err(SourceSetArgsError::EmptyExtensionName { value: value.to_string() });
            }
            if path.is_empty() {
                return Err(SourceSetArgsError::EmptyExtensionPath { value: value.to_string() });
            }
            Ok((Some(name.to_string()), path.to_string()))
        }
        _ => {
            if value.is_empty() {
                return Err(SourceSetArgsError::EmptyExtensionPath { value: value.to_string() });
            }
            Ok((None, value.to_string()))
        }
    }
}

fn is_extension_dir(root: &Path, rel: &str) -> bool {
    root.join(rel).join("Configuration.xml").is_file()
}

/// Where a resolved source-set field actually came from. Reported per field
/// because the root and the extension list are decided independently: a flag
/// may claim one while the other still comes from the file or from discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceProvider {
    Cli,
    ConfigFile,
    AutoDiscovery,
}

impl SourceProvider {
    pub fn label(self, config_file: &Path) -> String {
        match self {
            Self::Cli => "cli".to_string(),
            Self::ConfigFile => config_file.display().to_string(),
            Self::AutoDiscovery => "auto-discovery".to_string(),
        }
    }
}

/// Provider of the main configuration root. A configured root that does not
/// hold a `Configuration.xml` is not the answer: the model warns and falls
/// through to discovery, so reporting the file here would name a source that
/// did not decide anything.
pub fn configuration_root_provider(
    args: &SourceSetArgs,
    config_root: Option<&str>,
    project_root: &Path,
) -> SourceProvider {
    if args.configuration_root.is_some() {
        return SourceProvider::Cli;
    }
    match config_root {
        Some(value) if project_root.join(value).join("Configuration.xml").exists() => {
            SourceProvider::ConfigFile
        }
        _ => SourceProvider::AutoDiscovery,
    }
}

/// Provider of the extension list. Unlike the root, an explicit list is
/// authoritative whether or not any entry resolves, so presence decides.
pub fn extensions_provider(
    args: &SourceSetArgs,
    config_extensions: Option<&Vec<ExtensionDecl>>,
) -> SourceProvider {
    if args.declares_extensions() {
        SourceProvider::Cli
    } else if config_extensions.is_some() {
        SourceProvider::ConfigFile
    } else {
        SourceProvider::AutoDiscovery
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn args(flags: &[&str]) -> SourceSetArgs {
        use clap::Parser;

        #[derive(Parser)]
        struct Harness {
            #[command(flatten)]
            source_set: SourceSetArgs,
        }

        let mut argv = vec!["harness"];
        argv.extend_from_slice(flags);
        Harness::parse_from(argv).source_set
    }

    fn extension_dir(root: &Path, rel: &str) {
        let dir = root.join(rel);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Configuration.xml"), "<xml/>").unwrap();
    }

    fn names(decls: &[ExtensionDecl]) -> Vec<String> {
        decls
            .iter()
            .map(|decl| match decl {
                ExtensionDecl::Path(path) => path.clone(),
                ExtensionDecl::Structured(entry) => entry.name.clone(),
            })
            .collect()
    }

    fn depends_on(decls: &[ExtensionDecl], name: &str) -> Vec<String> {
        decls
            .iter()
            .find_map(|decl| match decl {
                ExtensionDecl::Structured(entry) if entry.name == name => {
                    Some(entry.depends_on.clone())
                }
                _ => None,
            })
            .unwrap_or_default()
    }

    #[test]
    fn named_and_bare_forms_map_to_the_two_config_shapes() {
        let dir = tempdir().unwrap();
        extension_dir(dir.path(), "ext");
        extension_dir(dir.path(), "vendor/flag");

        let resolved = args(&["--extension", "Flag=vendor/flag", "--extension", "ext"])
            .resolve(dir.path())
            .unwrap();

        let decls = resolved.extensions.unwrap();
        assert!(matches!(decls[0], ExtensionDecl::Structured(_)), "NAME=PATH is the strict shape");
        assert!(matches!(decls[1], ExtensionDecl::Path(_)), "a bare path is the lenient shape");
        assert_eq!(names(&decls), vec!["Flag", "ext"]);
    }

    #[test]
    fn configuration_root_without_configuration_xml_is_refused() {
        let dir = tempdir().unwrap();
        // Present so that the model's own search would have found *something*:
        // the flag must not degrade into that search.
        extension_dir(dir.path(), "src/cf");

        let err = args(&["--configuration-root", "nope"]).resolve(dir.path()).unwrap_err();

        assert!(matches!(err, SourceSetArgsError::ConfigurationRootNotFound { .. }), "got {err}");
    }

    #[test]
    fn configuration_root_that_exists_is_accepted() {
        let dir = tempdir().unwrap();
        extension_dir(dir.path(), "src/cf");

        let resolved = args(&["--configuration-root", "src/cf"]).resolve(dir.path()).unwrap();

        assert_eq!(resolved.configuration_root.as_deref(), Some("src/cf"));
    }

    #[test]
    fn no_extensions_yields_the_explicit_opt_out() {
        let dir = tempdir().unwrap();

        let resolved = args(&["--no-extensions"]).resolve(dir.path()).unwrap();

        assert_eq!(resolved.extensions, Some(Vec::new()), "must be the opt-out, not 'unset'");
    }

    #[test]
    fn no_source_set_flags_leave_the_config_alone() {
        let dir = tempdir().unwrap();

        let resolved = args(&[]).resolve(dir.path()).unwrap();

        assert_eq!(resolved.configuration_root, None);
        assert_eq!(resolved.extensions, None, "unset must stay unset, not become an empty list");
    }

    #[test]
    fn dependency_owner_is_matched_case_insensitively() {
        let dir = tempdir().unwrap();
        extension_dir(dir.path(), "b");
        extension_dir(dir.path(), "t");

        // The topology compares folded names on both sides, so the CLI barrier
        // must not reject spellings the same model accepts from a config file.
        let resolved = args(&[
            "--extension",
            "Base=b",
            "--extension",
            "Target=t",
            "--extension-depends-on",
            "TARGET=BASE",
        ])
        .resolve(dir.path())
        .unwrap();

        // Spelled differently from both the declaration and its folded form, so
        // a lookup that merely compares against the folded key still misses.
        assert_eq!(depends_on(&resolved.extensions.unwrap(), "Target"), vec!["BASE"]);
    }

    #[test]
    fn dependency_targets_survive_both_spellings() {
        let dir = tempdir().unwrap();
        for name in ["a", "b", "t"] {
            extension_dir(dir.path(), name);
        }
        let decl = ["--extension", "A=a", "--extension", "B=b", "--extension", "T=t"];

        let inline = args(&[&decl[..], &["--extension-depends-on", "T=A,B"]].concat())
            .resolve(dir.path())
            .unwrap();
        let repeated = args(
            &[&decl[..], &["--extension-depends-on", "T=A", "--extension-depends-on", "T=B"]]
                .concat(),
        )
        .resolve(dir.path())
        .unwrap();

        assert_eq!(depends_on(&inline.extensions.unwrap(), "T"), vec!["A", "B"]);
        assert_eq!(
            depends_on(&repeated.extensions.unwrap(), "T"),
            vec!["A", "B"],
            "repeats must accumulate rather than the last flag winning"
        );
    }

    #[test]
    fn dependency_on_a_bare_path_entry_is_refused() {
        let dir = tempdir().unwrap();
        extension_dir(dir.path(), "t");
        extension_dir(dir.path(), "legacy");

        // The topology would resolve `legacy` against the name that bare entry
        // derives from its directory. That name is incidental, so binding to it
        // is exactly what this flag must not do.
        let err = args(&[
            "--extension",
            "T=t",
            "--extension",
            "legacy",
            "--extension-depends-on",
            "T=legacy",
        ])
        .resolve(dir.path())
        .unwrap_err();

        assert!(matches!(err, SourceSetArgsError::UnknownDependsOnTarget { .. }), "got {err}");
    }

    #[test]
    fn a_value_that_reads_both_ways_is_refused() {
        let dir = tempdir().unwrap();
        extension_dir(dir.path(), "wanted=real");
        extension_dir(dir.path(), "real");

        // Both readings name a real extension directory. Picking one silently
        // would analyze a directory the caller never asked for.
        let err = args(&["--extension", "wanted=real"]).resolve(dir.path()).unwrap_err();

        assert!(matches!(err, SourceSetArgsError::AmbiguousExtensionValue { .. }), "got {err}");
    }

    #[test]
    fn the_escape_the_error_advertises_actually_works() {
        let dir = tempdir().unwrap();
        extension_dir(dir.path(), "wanted=real");
        extension_dir(dir.path(), "real");

        // The message tells the user to name it explicitly. Splitting on the
        // *first* `=` is what makes that work, so this pins the advice to the
        // parser rather than to the sentence.
        let resolved = args(&["--extension", "W=wanted=real"]).resolve(dir.path()).unwrap();

        assert_eq!(
            resolved.extensions.unwrap(),
            vec![ExtensionDecl::Structured(StructuredExtensionDecl {
                name: "W".to_string(),
                path: "wanted=real".to_string(),
                depends_on: Vec::new(),
            })]
        );
    }

    #[test]
    fn only_one_live_reading_needs_no_refusal() {
        let dir = tempdir().unwrap();
        extension_dir(dir.path(), "real");

        let resolved = args(&["--extension", "wanted=real"]).resolve(dir.path()).unwrap();

        assert_eq!(names(&resolved.extensions.unwrap()), vec!["wanted"]);
    }

    #[test]
    fn a_directory_named_configuration_xml_is_not_a_configuration() {
        let dir = tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("base/Configuration.xml")).unwrap();

        // Mere existence is satisfied by a directory or a fifo; the flag is
        // authoritative, so it has to insist on a file.
        let err = args(&["--configuration-root", "base"]).resolve(dir.path()).unwrap_err();

        assert!(matches!(err, SourceSetArgsError::ConfigurationRootNotFound { .. }), "got {err}");
    }

    #[test]
    fn a_named_entry_with_a_glob_stays_the_strict_shape() {
        let dir = tempdir().unwrap();
        extension_dir(dir.path(), "cfe/E");

        // The model refuses globs in a named entry, loudly. Re-reading the value
        // as a bare path to dodge that would produce a pattern matching nothing
        // and drop the extension in silence.
        let resolved = args(&["--extension", "Named=cfe/*"]).resolve(dir.path()).unwrap();

        assert_eq!(
            resolved.extensions.unwrap(),
            vec![ExtensionDecl::Structured(StructuredExtensionDecl {
                name: "Named".to_string(),
                path: "cfe/*".to_string(),
                depends_on: Vec::new(),
            })],
            "must reach the model as a named entry, which then rejects the glob"
        );
    }

    #[test]
    fn surrounding_blanks_are_kept_in_paths_and_dropped_in_names() {
        let dir = tempdir().unwrap();
        extension_dir(dir.path(), " ext ");

        let resolved =
            args(&["--extension", " ext ", "--extension", " Named = ext "]).resolve(dir.path());

        // The two spell the same directory, so the duplicate barrier fires —
        // proving the path kept its blanks while the name lost them.
        match resolved.unwrap_err() {
            SourceSetArgsError::DuplicateExtensionPath { .. } => {}
            other => panic!("got {other}"),
        }

        let single = args(&["--extension", " ext "]).resolve(dir.path()).unwrap();
        assert_eq!(
            single.extensions.unwrap(),
            vec![ExtensionDecl::Path(" ext ".to_string())],
            "a trimmed path would name a directory that was never passed"
        );
    }

    #[test]
    fn dependency_on_an_undeclared_owner_is_refused() {
        let dir = tempdir().unwrap();
        extension_dir(dir.path(), "y");

        let err = args(&["--extension", "Y=y", "--extension-depends-on", "Q=Y"])
            .resolve(dir.path())
            .unwrap_err();

        assert!(matches!(err, SourceSetArgsError::UnknownDependsOnOwner { .. }), "got {err}");
    }

    #[test]
    fn dependency_without_any_named_extension_is_refused() {
        let dir = tempdir().unwrap();

        let err = args(&["--extension-depends-on", "T=Y"]).resolve(dir.path()).unwrap_err();

        assert!(matches!(err, SourceSetArgsError::UnknownDependsOnOwner { .. }), "got {err}");
    }

    #[test]
    fn the_same_directory_named_twice_is_refused_in_either_order() {
        let dir = tempdir().unwrap();
        extension_dir(dir.path(), "ext");

        // The second order is the one the model itself accepts silently, so a
        // barrier that only covers the first would look fine on the model's own
        // tests and still let a duplicate through.
        for flags in [
            ["--extension", "ext", "--extension", "Named=ext"],
            ["--extension", "Named=ext", "--extension", "ext"],
        ] {
            let err = args(&flags).resolve(dir.path()).unwrap_err();
            assert!(
                matches!(err, SourceSetArgsError::DuplicateExtensionPath { .. }),
                "{flags:?} got {err}"
            );
        }
    }

    #[test]
    fn duplicate_detection_sees_through_path_spellings() {
        let dir = tempdir().unwrap();
        extension_dir(dir.path(), "ext");

        let err = args(&["--extension", "Named=ext", "--extension", "./ext"])
            .resolve(dir.path())
            .unwrap_err();

        assert!(matches!(err, SourceSetArgsError::DuplicateExtensionPath { .. }), "got {err}");
    }

    #[cfg(unix)]
    #[test]
    fn duplicate_detection_sees_through_symlinks() {
        let dir = tempdir().unwrap();
        extension_dir(dir.path(), "ext");
        std::os::unix::fs::symlink(dir.path().join("ext"), dir.path().join("alias")).unwrap();

        // A lexical normalization would call these two different directories;
        // the model canonicalizes, so the barrier has to as well.
        let err = args(&["--extension", "Named=ext", "--extension", "alias"])
            .resolve(dir.path())
            .unwrap_err();

        assert!(matches!(err, SourceSetArgsError::DuplicateExtensionPath { .. }), "got {err}");
    }

    #[test]
    fn a_glob_may_overlap_an_explicit_entry() {
        let dir = tempdir().unwrap();
        extension_dir(dir.path(), "cfe/base");

        // A glob describes a set; overlapping it with a named entry is ordinary
        // usage, and de-duplicating it belongs to the model, not to a refusal.
        let resolved =
            args(&["--extension", "cfe/*", "--extension", "cfe/base"]).resolve(dir.path()).unwrap();

        assert_eq!(resolved.extensions.unwrap().len(), 2, "both entries reach the model");
    }

    #[test]
    fn providers_are_reported_per_field() {
        let dir = tempdir().unwrap();
        extension_dir(dir.path(), "src/cf");
        let config_extensions = vec![ExtensionDecl::Path("ext".to_string())];

        let root_from_cli = args(&["--configuration-root", "src/cf"]);
        assert_eq!(
            configuration_root_provider(&root_from_cli, None, dir.path()),
            SourceProvider::Cli
        );
        assert_eq!(
            extensions_provider(&root_from_cli, Some(&config_extensions)),
            SourceProvider::ConfigFile,
            "a root flag must not claim the extension list too"
        );

        let extensions_from_cli = args(&["--extension", "ext"]);
        assert_eq!(
            configuration_root_provider(&extensions_from_cli, Some("src/cf"), dir.path()),
            SourceProvider::ConfigFile,
            "an extension flag must not claim the root too"
        );
        assert_eq!(
            extensions_provider(&extensions_from_cli, Some(&config_extensions)),
            SourceProvider::Cli
        );
    }

    #[test]
    fn a_configured_root_that_does_not_resolve_is_reported_as_discovery() {
        let dir = tempdir().unwrap();
        extension_dir(dir.path(), "src/cf");

        // The model warns and searches instead of using it, so naming the file
        // as the provider would credit a source that decided nothing.
        assert_eq!(
            configuration_root_provider(&args(&[]), Some("missing"), dir.path()),
            SourceProvider::AutoDiscovery
        );
        assert_eq!(
            configuration_root_provider(&args(&[]), Some("src/cf"), dir.path()),
            SourceProvider::ConfigFile
        );
        assert_eq!(
            configuration_root_provider(&args(&[]), None, dir.path()),
            SourceProvider::AutoDiscovery
        );
    }
}
