//! The location contract: one shape for "where" and one shape for "how fresh and how
//! complete", shared by every agent-facing tool.
//!
//! Before this module each tool spelled a place its own way — 1-based lines here,
//! 0-based ranges there, a bare line number in a third, nothing at all in a fourth — and
//! a consumer had to know which tool it was talking to before it could read an answer.
//! The rule that keeps that from coming back: this module is the ONLY place that
//! serializes a location or a freshness envelope. A tool assembles the typed value and
//! hands it over; it never writes the keys itself.

use std::path::Path;

use bsl_search::WorkspaceRoots;
use line_index::LineColRange;
use serde_json::{json, Value};

/// Version of the location block itself, independent of any tool's response version:
/// the block travels across tools, so it cannot be versioned by one of them.
pub const LOCATION_SCHEMA_VERSION: &str = "1";

/// The position unit every published range is measured in. Declared in the payload
/// rather than assumed, because the same build serves both LSP (UTF-16) and older
/// fields counting bytes or code points.
pub const POSITION_ENCODING: &str = "utf-16";

/// A span inside a file: 0-based, end-exclusive, characters in UTF-16 code units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PositionRange {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

impl From<LineColRange> for PositionRange {
    fn from(r: LineColRange) -> Self {
        Self {
            start_line: r.start_line,
            start_character: r.start_character,
            end_line: r.end_line,
            end_character: r.end_character,
        }
    }
}

impl PositionRange {
    fn to_value(self) -> Value {
        json!({
            "start_line": self.start_line,
            "start_character": self.start_character,
            "end_line": self.end_line,
            "end_character": self.end_character,
        })
    }
}

/// The module a location belongs to, when the producer already holds both halves.
/// Never reconstructed by parsing a display string: a guess in a contract field is
/// worse than an absent field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleRef {
    pub kind: String,
    pub name: String,
}

/// A place in the workspace, addressable by the pair `(root_id, path)`.
///
/// `range` is the symbol's name (or, for a diagnostic, the finding's own span);
/// `enclosing_range` is the whole node. Both are optional: an answer that only knows
/// the file — a hit from a shared baseline, a card without a source — says so by
/// omitting them rather than inventing zeros.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub root_id: String,
    pub path: String,
    pub range: Option<PositionRange>,
    pub enclosing_range: Option<PositionRange>,
    pub module: Option<ModuleRef>,
}

/// Why a place could not be named. Always emitted in place of a location, so a
/// consumer never has to tell "absent because unknown" from "absent because none".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationUnavailable {
    /// The path lies outside every registered root, so no pair addresses it.
    PathOutsideRegisteredRoots,
    /// Only an absolute path is known and it is not relative to any root.
    PathNotRelativeToRoot,
    /// The entity has no source file by construction (a metadata object, a form item).
    NoSourceLocation,
    /// The root table is not available in this answer — a stale graph cache published
    /// as ready while the catch-up reload is still running.
    RootsUnavailable,
    /// The entity HAS a source file, but its path could not be named — a VFS entry that
    /// is not valid UTF-8, or one missing from the source root's file set. Distinct from
    /// `RootsUnavailable`: the table was there, the path was not.
    SourcePathUnavailable,
}

impl LocationUnavailable {
    pub fn code(self) -> &'static str {
        match self {
            Self::PathOutsideRegisteredRoots => "path_outside_registered_roots",
            Self::PathNotRelativeToRoot => "path_not_relative_to_root",
            Self::NoSourceLocation => "no_source_location",
            Self::RootsUnavailable => "roots_unavailable",
            Self::SourcePathUnavailable => "source_path_unavailable",
        }
    }
}

impl Location {
    /// A location from a pair the caller already holds.
    ///
    /// The root is NOT validated against any table: a hit from a shared baseline
    /// carries a root registered in the workspace that produced it, and replacing it
    /// with the configuration's empty id — or dropping the location — would lose an
    /// address its source knows and we do not.
    pub fn from_key(root_id: &str, rel_path: &str) -> Self {
        Self {
            root_id: root_id.to_string(),
            path: normalize_separators(rel_path),
            range: None,
            enclosing_range: None,
            module: None,
        }
    }

    /// A location for an absolute path, deriving the root from the root table.
    ///
    /// Only for callers that hold nothing but a path. A caller holding a pair must use
    /// [`Location::from_key`]: a relative path handed to the table is resolved against
    /// the CONFIGURATION root, so an extension file and a configuration file sharing a
    /// relative path would both come back as the configuration's.
    pub fn from_path(roots: &WorkspaceRoots, abs: &Path) -> Result<Self, LocationUnavailable> {
        if !abs.is_absolute() {
            return Err(LocationUnavailable::PathNotRelativeToRoot);
        }
        let key = roots.key_of_path(abs).ok_or(LocationUnavailable::PathOutsideRegisteredRoots)?;
        Ok(Self::from_key(&key.root_id, &key.path))
    }

    pub fn with_range(mut self, range: Option<PositionRange>) -> Self {
        self.range = range;
        self
    }

    pub fn with_enclosing_range(mut self, range: Option<PositionRange>) -> Self {
        self.enclosing_range = range;
        self
    }

    pub fn with_module(mut self, module: Option<ModuleRef>) -> Self {
        self.module = module;
        self
    }

    pub fn to_value(&self) -> Value {
        let mut body = json!({
            "root_id": self.root_id,
            "path": self.path,
            "position_encoding": POSITION_ENCODING,
            "schema_version": LOCATION_SCHEMA_VERSION,
        });
        let map = body.as_object_mut().expect("object literal");
        if let Some(range) = self.range {
            map.insert("range".into(), range.to_value());
        }
        if let Some(range) = self.enclosing_range {
            map.insert("enclosing_range".into(), range.to_value());
        }
        if let Some(module) = &self.module {
            map.insert("module".into(), json!({ "kind": module.kind, "name": module.name }));
        }
        body
    }
}

/// Windows spells a stored relative path with `\`, and the contract publishes `/`. On
/// UNIX a backslash is an ordinary character in a file name, so rewriting it there would
/// mint an address that looks right and points elsewhere — the exact failure this contract
/// exists to prevent.
fn normalize_separators(path: &str) -> String {
    if cfg!(windows) {
        path.replace('\\', "/")
    } else {
        path.to_owned()
    }
}

/// Why an answer is not the whole answer. A closed vocabulary: a new reason means a
/// new version of the contract, not a new free-form string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasonCode {
    /// The output budget cut the answer short.
    OutputBudget,
    /// A count limit (`max_findings`, `max_nodes`, a candidate cap) cut it short.
    ResultCap,
    /// An index needed for part of the answer is still being built.
    IndexBuilding,
    /// Some files could not be read.
    UnreadableFiles,
    /// Part of the request lies outside the configured analysis scope.
    OutOfAnalysisScope,
    /// A modality or subsystem the answer would normally use was unavailable.
    ModalityDegraded,
}

impl ReasonCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OutputBudget => "output_budget",
            Self::ResultCap => "result_cap",
            Self::IndexBuilding => "index_building",
            Self::UnreadableFiles => "unreadable_files",
            Self::OutOfAnalysisScope => "out_of_analysis_scope",
            Self::ModalityDegraded => "modality_degraded",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason {
    pub code: ReasonCode,
    pub detail: String,
}

/// Whether the answer is whole, and if not, why.
///
/// Built where the fact is known — the return of a trimming helper, a cap flag, an
/// unread counter — and never reconstructed by inspecting the rendered JSON: a reason
/// recovered from output is a guess about our own behaviour.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Completeness {
    reasons: Vec<Reason>,
}

impl Completeness {
    pub fn complete() -> Self {
        Self::default()
    }

    pub fn partial(code: ReasonCode, detail: impl Into<String>) -> Self {
        Self { reasons: vec![Reason { code, detail: detail.into() }] }
    }

    /// Record a reason when `present` holds. Takes the flag rather than being called
    /// under an `if` so the call site reads as "this fact, that code" and cannot
    /// silently skip a reason it computed.
    pub fn when(mut self, present: bool, code: ReasonCode, detail: impl Into<String>) -> Self {
        if present {
            self.reasons.push(Reason { code, detail: detail.into() });
        }
        self
    }

    pub fn is_complete(&self) -> bool {
        self.reasons.is_empty()
    }

    pub fn to_value(&self) -> Value {
        json!({
            "status": if self.is_complete() { "complete" } else { "partial" },
            "reasons": self
                .reasons
                .iter()
                .map(|r| json!({ "code": r.code.as_str(), "detail": r.detail }))
                .collect::<Vec<_>>(),
        })
    }
}

/// Which subsystem produced an answer. Named because the freshness fields below mean
/// different things — and are known to a different extent — per source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreshnessSource {
    Resident,
    Graph,
    SearchIndex,
}

impl FreshnessSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Resident => "resident",
            Self::Graph => "graph",
            Self::SearchIndex => "search-index",
        }
    }
}

/// The envelope every tool of the contract carries: what answered, at which revision
/// and topology, whether it had drifted, and whether the answer is whole.
///
/// `revision`, `topology_fingerprint` and `stale` are `Option` because a source may
/// genuinely have no such identity — the search index has no revision of its own, and
/// stamping the resident's or the graph's onto its answers is exactly the borrowed
/// identity this contract exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Freshness {
    pub source: FreshnessSource,
    pub revision: Option<u64>,
    pub topology_fingerprint: Option<u64>,
    pub stale: Option<bool>,
    pub completeness: Completeness,
}

impl Freshness {
    pub fn new(source: FreshnessSource, completeness: Completeness) -> Self {
        Self { source, revision: None, topology_fingerprint: None, stale: None, completeness }
    }

    pub fn with_revision(mut self, revision: u64) -> Self {
        self.revision = Some(revision);
        self
    }

    pub fn with_topology(mut self, topology: u64) -> Self {
        self.topology_fingerprint = Some(topology);
        self
    }

    pub fn with_stale(mut self, stale: bool) -> Self {
        self.stale = Some(stale);
        self
    }

    pub fn to_value(&self) -> Value {
        json!({
            "source": self.source.as_str(),
            "revision": self.revision,
            // Hex rather than a JSON number: the value is a u64 and a JS consumer
            // silently rounds anything above 2^53.
            "topology_fingerprint": self.topology_fingerprint.map(|fp| format!("{fp:016x}")),
            "stale": self.stale,
            "completeness": self.completeness.to_value(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A workspace with the configuration and one extension root beside it — the
    /// shape where one relative path exists in two roots at once.
    fn roots() -> WorkspaceRoots {
        let (roots, rejected) = WorkspaceRoots::build(
            Path::new("/ws"),
            Path::new("/ws/src/cf"),
            &[PathBuf::from("/ws/src/cfe/ext-a")],
        );
        assert!(rejected.is_empty(), "both roots must register for this stand to mean anything");
        roots
    }

    fn extension_root_id(roots: &WorkspaceRoots) -> String {
        roots.ids().find(|id| !id.is_empty()).expect("an extension root is registered").to_owned()
    }

    /// `ide` spells two of these codes itself, because its in-memory projection must emit
    /// the SAME string as the SQLite one for the byte-identity gates to keep comparing like
    /// with like — and it sits below this crate, so it cannot import the enum. That makes
    /// the two spellings a pair that can drift silently; this is the test that they cannot.
    #[test]
    fn the_reasons_ide_spells_itself_match_this_vocabulary() {
        assert_eq!(LocationUnavailable::NoSourceLocation.code(), ide::NO_SOURCE_LOCATION);
        assert_eq!(LocationUnavailable::RootsUnavailable.code(), ide::ROOTS_UNAVAILABLE);
    }

    #[test]
    fn a_location_names_its_encoding_and_version() {
        let value = Location::from_key("", "CommonModules/Сервер/Ext/Module.bsl")
            .with_range(Some(PositionRange {
                start_line: 119,
                start_character: 10,
                end_line: 119,
                end_character: 22,
            }))
            .to_value();

        assert_eq!(value["position_encoding"], POSITION_ENCODING);
        assert_eq!(value["schema_version"], LOCATION_SCHEMA_VERSION);
        assert_eq!(value["root_id"], "");
        assert_eq!(value["range"]["start_line"], 119);
        assert_eq!(value["range"]["end_character"], 22);
        // A location that knows only the file omits the ranges instead of zeroing them.
        assert!(value.get("enclosing_range").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn a_backslash_in_a_unix_name_is_a_name_and_not_a_separator() {
        // A path component may legally contain a backslash on UNIX; rewriting it would
        // address a different file, and `resolve` would hand back the wrong one.
        let location = Location::from_key("", "CommonModules/A\\B/Ext/Module.bsl");
        assert_eq!(location.path, "CommonModules/A\\B/Ext/Module.bsl");
    }

    #[test]
    fn a_pair_is_taken_as_given_and_not_re_derived() {
        // The same relative path in two roots: the pair-taking constructor keeps them
        // apart, which is the whole point of the pair being the key.
        let config = Location::from_key("", "CommonModules/Общий/Ext/Module.bsl");
        let extension = Location::from_key("ext-a", "CommonModules/Общий/Ext/Module.bsl");

        assert_eq!(config.path, extension.path);
        assert_ne!(config.root_id, extension.root_id);
        assert_eq!(extension.to_value()["root_id"], "ext-a");
    }

    #[test]
    fn a_path_derives_the_root_it_lies_in() {
        let roots = roots();
        let ext = extension_root_id(&roots);

        let in_extension = Location::from_path(
            &roots,
            Path::new("/ws/src/cfe/ext-a/CommonModules/М/Ext/Module.bsl"),
        )
        .expect("inside the extension root");
        assert_eq!(in_extension.root_id, ext);
        assert_eq!(in_extension.path, "CommonModules/М/Ext/Module.bsl");

        // Same relative path, other root: the pair keeps them apart.
        let in_configuration =
            Location::from_path(&roots, Path::new("/ws/src/cf/CommonModules/М/Ext/Module.bsl"))
                .expect("inside the configuration root");
        assert_eq!(in_configuration.root_id, "");
        assert_eq!(in_configuration.path, in_extension.path);
    }

    #[test]
    fn a_path_outside_every_root_names_the_reason() {
        let roots = roots();

        let err = Location::from_path(&roots, Path::new("/elsewhere/Module.bsl"))
            .expect_err("outside every root");

        assert_eq!(err.code(), "path_outside_registered_roots");
    }

    #[test]
    fn a_relative_path_is_refused_rather_than_attached_to_a_root() {
        let roots = roots();

        let err = Location::from_path(&roots, Path::new("CommonModules/М/Ext/Module.bsl"))
            .expect_err("a relative path addresses no root on its own");

        assert_eq!(err.code(), "path_not_relative_to_root");
    }

    #[test]
    fn completeness_defaults_to_whole_and_names_each_reason() {
        assert_eq!(Completeness::complete().to_value()["status"], "complete");
        assert_eq!(Completeness::complete().to_value()["reasons"].as_array().unwrap().len(), 0);

        let partial = Completeness::complete()
            .when(true, ReasonCode::OutputBudget, "raise max_output_tokens")
            .when(false, ReasonCode::ResultCap, "not this time");
        let value = partial.to_value();

        assert_eq!(value["status"], "partial");
        assert_eq!(value["reasons"].as_array().unwrap().len(), 1);
        assert_eq!(value["reasons"][0]["code"], "output_budget");
    }

    #[test]
    fn a_source_without_identity_says_null_rather_than_borrowing_one() {
        let value =
            Freshness::new(FreshnessSource::SearchIndex, Completeness::complete()).to_value();

        assert_eq!(value["source"], "search-index");
        assert!(value["revision"].is_null());
        assert!(value["topology_fingerprint"].is_null());
        assert!(value["stale"].is_null());
    }

    #[test]
    fn a_topology_fingerprint_survives_as_hex() {
        let value = Freshness::new(FreshnessSource::Graph, Completeness::complete())
            .with_revision(42)
            .with_topology(0x0a1b_2c3d_4e5f_6071)
            .with_stale(false)
            .to_value();

        assert_eq!(value["revision"], 42);
        assert_eq!(value["topology_fingerprint"], "0a1b2c3d4e5f6071");
        assert_eq!(value["stale"], false);
    }
}
