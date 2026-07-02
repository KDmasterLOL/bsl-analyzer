//! Shared workspace-drift classification.
//!
//! One implementation of "what did these drained hub entries change" so diagnostics and
//! search agree on the taxonomy instead of each re-deriving it. Events are hints: this
//! re-stats every path ([`file_fingerprint`]) so a coalesced create/delete settles on
//! what is actually on disk now — the "stats are truth" discipline the diagnostics drift
//! path has always used.
//!
//! The taxonomy (`.xml` metadata / `.bsl` body modified / `.bsl` removed / subtree
//! rescan / config) is shared; baseline-relative policy is the caller's, selected by the
//! `baseline` argument:
//!
//! - With a baseline (diagnostics), the classification reproduces the old inline rules
//!   exactly, including EXTENSION MATCHING: an exact lowercase suffix (`ends_with(".bsl")`
//!   / `ends_with(".xml")`), so `Module.BSL` is ignored just as before. A new `.bsl` or a
//!   removed resident `.bsl` folds into `structural_rescan` (a full rebuild),
//!   content-unchanged touches are dropped, and `new_fp`/`removed_keys` carry the stats
//!   delta. `bsl_removed` stays empty (a removed body is structural).
//! - Without a baseline (search), EXTENSION MATCHING is case-INSENSITIVE (as the old
//!   search sink's `extension().eq_ignore_ascii_case`), so `Module.BSL` is classified.
//!   Every present `.bsl` is `bsl_modified`, every gone `.bsl` is `bsl_removed`, and every
//!   `.xml` (present or gone) is a resolver input; `structural_rescan` reduces to a real
//!   subtree removal.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::change_hub::{ChangeEntry, ChangeKind};
use crate::graph::file_fingerprint;

/// A changed path in both spellings drift consumers need. `key` is the scan-universe
/// canonical string (diagnostics stats, resolves, and re-keys by it); `raw` is the
/// watcher's spelling (search strips its possibly-symlinked source root against it, so a
/// canonicalised key would fail to match).
pub(crate) struct DriftPath {
    pub key: String,
    pub raw: PathBuf,
}

/// Drained hub entries bucketed by on-disk truth and file class. See the module docs for
/// the baseline policy split.
#[derive(Default)]
pub(crate) struct DriftClassification {
    /// `.xml` metadata that changed (added / edited / removed). Diagnostics point-refreshes
    /// the substrate for these; search resolves them to affected documents.
    pub xml_paths: Vec<DriftPath>,
    /// `.bsl` bodies present on disk with changed content. Diagnostics re-keys them; search
    /// marks them dirty.
    pub bsl_modified: Vec<DriftPath>,
    /// `.bsl` files gone from disk. Search tombstones them. Empty with a baseline
    /// (diagnostics folds a removed resident body into `structural_rescan`).
    pub bsl_removed: Vec<DriftPath>,
    /// A change no point-refresh can express: a removed directory subtree, or — with a
    /// baseline — a new/removed resident `.bsl` that moves the file universe. Diagnostics
    /// full-rebuilds; search re-walks.
    pub structural_rescan: bool,
    /// An analyzer config file at the workspace root changed.
    pub config_changed: bool,
    /// On-disk fingerprints for the added/edited `.xml` and modified `.bsl`, for the
    /// diagnostics stats delta. Empty without a baseline.
    pub new_fp: HashMap<String, u64>,
    /// Baseline keys now gone (a removed tracked `.xml`), for the diagnostics stats delta.
    /// Empty without a baseline.
    pub removed_keys: Vec<String>,
}

/// Whether `key` carries the file class `dot_ext` (e.g. `".bsl"`), under the caller's
/// matching policy. The two consumers had different historical rules and this preserves
/// both exactly:
/// - baseline policy (diagnostics, `case_insensitive == false`): an exact lowercase
///   suffix, the old inline `key.ends_with(".bsl")` / `ends_with(".xml")`. `Module.BSL`
///   does NOT match — a change diagnostics has always ignored.
/// - search policy (`case_insensitive == true`): a case-insensitive extension match, as
///   the old search sink's `extension().eq_ignore_ascii_case`. `Module.BSL` DOES match.
fn is_ext(key: &str, dot_ext: &str, case_insensitive: bool) -> bool {
    if case_insensitive {
        let bare = &dot_ext[1..];
        Path::new(key).extension().is_some_and(|e| e.eq_ignore_ascii_case(bare))
    } else {
        key.ends_with(dot_ext)
    }
}

/// Classify drained hub `entries`. `config_paths` are the canonical analyzer-config file
/// paths at the workspace root (a match forces `config_changed`); `baseline` is the
/// per-path fingerprint map to diff against, or `None` for the stateless (search) policy.
pub(crate) fn classify_drift(
    entries: &[ChangeEntry],
    config_paths: &HashSet<PathBuf>,
    baseline: Option<&HashMap<String, u64>>,
) -> DriftClassification {
    let mut out = DriftClassification::default();
    let mut seen: HashSet<String> = HashSet::new();
    // Search (no baseline) matches extensions case-insensitively, as its old sink did;
    // diagnostics (baseline) matches the exact lowercase suffix, as its old inline rules
    // did. See [`is_ext`].
    let case_insensitive = baseline.is_none();

    for entry in entries {
        // A vanished directory subtree expands into removed descendants the drain could
        // not enumerate — structural, so let a full scan/rebuild (or re-walk) sort it out.
        if entry.kind == ChangeKind::SubtreeRemoved {
            out.structural_rescan = true;
            continue;
        }
        // Only a file at THIS exact location is config drift; an identically-named file
        // elsewhere in the tree is not (parity with the scan path).
        if config_paths.contains(&entry.canonical) {
            out.config_changed = true;
            continue;
        }
        // `canonical` already carries the scan-universe key spelling.
        let key = entry.canonical.to_string_lossy().into_owned();
        if !seen.insert(key.clone()) {
            continue;
        }
        let is_xml = is_ext(&key, ".xml", case_insensitive);
        let is_bsl = is_ext(&key, ".bsl", case_insensitive);
        if !is_xml && !is_bsl {
            continue;
        }
        let drift_path = |k: &str| DriftPath { key: k.to_owned(), raw: entry.raw.clone() };

        match file_fingerprint(&entry.canonical) {
            Some(fp) => match baseline {
                // Search: any present file is a body edit / metadata edit to act on.
                None => {
                    if is_xml {
                        out.xml_paths.push(drift_path(&key));
                    } else {
                        out.bsl_modified.push(drift_path(&key));
                    }
                }
                Some(base) => match base.get(&key) {
                    // Unchanged content (a spurious event or a mtime-only touch): no-op.
                    Some(&old) if old == fp => {}
                    Some(_) => {
                        if is_xml {
                            out.xml_paths.push(drift_path(&key));
                        } else {
                            out.bsl_modified.push(drift_path(&key));
                        }
                        out.new_fp.insert(key.clone(), fp);
                    }
                    None => {
                        // A brand-new `.xml` is a substrate re-discovery; a brand-new
                        // `.bsl` moves the file universe → structural.
                        if is_xml {
                            out.xml_paths.push(drift_path(&key));
                            out.new_fp.insert(key.clone(), fp);
                        } else {
                            out.structural_rescan = true;
                        }
                    }
                },
            },
            None => match baseline {
                // Search: a gone file is a tombstone (`.bsl`) or a metadata removal (`.xml`).
                None => {
                    if is_xml {
                        out.xml_paths.push(drift_path(&key));
                    } else {
                        out.bsl_removed.push(drift_path(&key));
                    }
                }
                Some(base) => {
                    // Only act on the removal of a file that was tracked; an untracked
                    // removal is not drift for the resident.
                    if base.contains_key(&key) {
                        if is_xml {
                            out.xml_paths.push(drift_path(&key));
                            out.removed_keys.push(key.clone());
                        } else {
                            out.structural_rescan = true;
                        }
                    }
                }
            },
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(canonical: &str, kind: ChangeKind) -> ChangeEntry {
        ChangeEntry {
            canonical: PathBuf::from(canonical),
            raw: PathBuf::from(canonical),
            kind,
            seq: 0,
        }
    }

    /// Without a baseline (search), on-disk truth drives the taxonomy: a present `.bsl`
    /// is a body edit, a gone `.bsl` a tombstone, any `.xml` a resolver input, and a
    /// subtree removal a re-walk. No fingerprints are computed.
    #[test]
    fn stateless_policy_buckets_by_kind_and_extension() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let present_bsl = root.join("Present.bsl");
        std::fs::write(&present_bsl, "Процедура П() КонецПроцедуры").unwrap();
        let present_xml = root.join("Present.xml");
        std::fs::write(&present_xml, "<x/>").unwrap();
        let gone_bsl = root.join("Gone.bsl");
        let gone_xml = root.join("Gone.xml");

        let entries = [
            entry(present_bsl.to_str().unwrap(), ChangeKind::MaybeChanged),
            entry(present_xml.to_str().unwrap(), ChangeKind::MaybeChanged),
            entry(gone_bsl.to_str().unwrap(), ChangeKind::MaybeRemoved),
            entry(gone_xml.to_str().unwrap(), ChangeKind::MaybeRemoved),
            entry(&root.join("Catalogs").to_string_lossy(), ChangeKind::SubtreeRemoved),
        ];

        let class = classify_drift(&entries, &HashSet::new(), None);

        assert_eq!(class.bsl_modified.len(), 1);
        assert_eq!(class.bsl_modified[0].key, present_bsl.to_string_lossy());
        assert_eq!(class.bsl_removed.len(), 1);
        assert_eq!(class.bsl_removed[0].key, gone_bsl.to_string_lossy());
        // A present and a removed xml both feed the resolver.
        assert_eq!(class.xml_paths.len(), 2);
        assert!(class.structural_rescan, "a subtree removal forces a re-walk");
        assert!(class.new_fp.is_empty(), "no fingerprints without a baseline");
        assert!(class.removed_keys.is_empty());
    }

    /// With a baseline (diagnostics), content-unchanged touches drop out, a modified body
    /// is a re-key with a fresh fingerprint, and a new `.bsl` folds into a structural
    /// rebuild — the old inline rules.
    #[test]
    fn baseline_policy_matches_inline_rules() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let modified = root.join("Modified.bsl");
        std::fs::write(&modified, "Процедура П() КонецПроцедуры").unwrap();
        let unchanged = root.join("Unchanged.bsl");
        std::fs::write(&unchanged, "Процедура Н() КонецПроцедуры").unwrap();
        let new_bsl = root.join("New.bsl");
        std::fs::write(&new_bsl, "Процедура М() КонецПроцедуры").unwrap();
        let edited_xml = root.join("Edited.xml");
        std::fs::write(&edited_xml, "<x/>").unwrap();

        let mut baseline = HashMap::new();
        // `Unchanged.bsl` carries its true current fingerprint (so it is skipped);
        // `Modified.bsl` a stale one (so it is re-keyed). `New.bsl`/`Edited.xml` absent.
        baseline.insert(
            unchanged.to_string_lossy().into_owned(),
            file_fingerprint(&unchanged).unwrap(),
        );
        baseline.insert(modified.to_string_lossy().into_owned(), 0);

        let entries = [
            entry(modified.to_str().unwrap(), ChangeKind::MaybeChanged),
            entry(unchanged.to_str().unwrap(), ChangeKind::MaybeChanged),
            entry(new_bsl.to_str().unwrap(), ChangeKind::MaybeChanged),
            entry(edited_xml.to_str().unwrap(), ChangeKind::MaybeChanged),
        ];

        let class = classify_drift(&entries, &HashSet::new(), Some(&baseline));

        assert_eq!(class.bsl_modified.len(), 1, "only the content-changed body");
        assert_eq!(class.bsl_modified[0].key, modified.to_string_lossy());
        assert!(class.new_fp.contains_key(&modified.to_string_lossy().into_owned()));
        assert_eq!(class.xml_paths.len(), 1, "the new xml is a substrate re-discovery");
        assert!(class.structural_rescan, "a new .bsl moves the file universe");
        assert!(class.bsl_removed.is_empty(), "diagnostics never populates bsl_removed");
    }

    /// A tracked `.bsl` removal is structural for diagnostics; an untracked removal is a
    /// no-op. A tracked `.xml` removal is a resolver input carrying a stats-delete key.
    #[test]
    fn baseline_policy_gates_removals_on_the_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let tracked_bsl = root.join("Tracked.bsl");
        let untracked_bsl = root.join("Untracked.bsl");
        let tracked_xml = root.join("Tracked.xml");

        let mut baseline = HashMap::new();
        baseline.insert(tracked_bsl.to_string_lossy().into_owned(), 1);
        baseline.insert(tracked_xml.to_string_lossy().into_owned(), 2);

        let entries = [
            entry(tracked_bsl.to_str().unwrap(), ChangeKind::MaybeRemoved),
            entry(untracked_bsl.to_str().unwrap(), ChangeKind::MaybeRemoved),
            entry(tracked_xml.to_str().unwrap(), ChangeKind::MaybeRemoved),
        ];

        let class = classify_drift(&entries, &HashSet::new(), Some(&baseline));

        assert!(class.structural_rescan, "a tracked body removal rebuilds");
        assert_eq!(class.xml_paths.len(), 1, "the tracked xml removal feeds the resolver");
        assert_eq!(class.removed_keys, vec![tracked_xml.to_string_lossy().into_owned()]);
    }

    /// Extension matching is policy-dependent: the baseline (diagnostics) policy keeps the
    /// old exact lowercase suffix, so an uppercase `Module.BSL` is ignored; the stateless
    /// (search) policy matches case-insensitively, so the same path is classified.
    #[test]
    fn uppercase_extension_follows_the_matching_policy() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let upper = root.join("Module.BSL");
        std::fs::write(&upper, "Процедура П() КонецПроцедуры").unwrap();
        let key = upper.to_string_lossy().into_owned();

        let entries = [entry(&key, ChangeKind::MaybeChanged)];

        // Baseline policy: the exact `ends_with(".bsl")` rule ignores the uppercase file,
        // exactly as the old inline diagnostics classification did.
        let mut baseline = HashMap::new();
        baseline.insert(key.clone(), 0u64);
        let diag = classify_drift(&entries, &HashSet::new(), Some(&baseline));
        assert!(
            diag.bsl_modified.is_empty() && !diag.structural_rescan,
            "baseline policy ignores an uppercase .BSL extension",
        );

        // Search policy: the case-insensitive rule classifies it as a body edit.
        let search = classify_drift(&entries, &HashSet::new(), None);
        assert_eq!(search.bsl_modified.len(), 1, "search policy matches .BSL case-insensitively");
        assert_eq!(search.bsl_modified[0].key, key);
    }

    /// An analyzer-config file at the workspace root is config drift; anything else at a
    /// non-config location is not.
    #[test]
    fn config_paths_flag_config_drift() {
        let root = PathBuf::from("/ws");
        let toml = root.join("bsl-analyzer.toml");
        let mut config_paths = HashSet::new();
        config_paths.insert(toml.clone());

        let entries = [entry(toml.to_str().unwrap(), ChangeKind::MaybeChanged)];
        let class = classify_drift(&entries, &config_paths, Some(&HashMap::new()));
        assert!(class.config_changed);
        assert!(class.xml_paths.is_empty() && class.bsl_modified.is_empty());
    }
}
