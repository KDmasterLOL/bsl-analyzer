use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};

pub const DIAGNOSTICS_BASELINE_SCHEMA_VERSION: u32 = 1;

pub fn normalize_diagnostic_snippet(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn diagnostic_line_snippet(lines: &[&str], line: usize) -> String {
    normalize_diagnostic_snippet(lines.get(line).copied().unwrap_or_default())
}

pub fn diagnostic_fingerprint(path: &str, code: &str, snippet: &str, occurrence: u32) -> String {
    let mut hasher = blake3::Hasher::new();
    for part in [path.as_bytes(), code.as_bytes(), snippet.as_bytes()] {
        hasher.update(part);
        hasher.update(&[0]);
    }
    hasher.update(&occurrence.to_le_bytes());
    hasher.finalize().to_hex().to_string()
}

#[derive(Debug, thiserror::Error)]
#[error("diagnostic source snippet is unavailable")]
pub struct MissingDiagnosticSnippet;

pub fn strict_diagnostic_fingerprint(
    path: &str,
    code: &str,
    snippet: Option<&str>,
    occurrence: u32,
) -> Result<String, MissingDiagnosticSnippet> {
    snippet
        .map(|snippet| diagnostic_fingerprint(path, code, snippet, occurrence))
        .ok_or(MissingDiagnosticSnippet)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsBaseline {
    pub schema_version: u32,
    pub scope: DiagnosticsBaselineScope,
    pub diagnostics: Vec<DiagnosticsBaselineEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsBaselineScope {
    pub source_root: String,
    pub extensions: Vec<DiagnosticsBaselineExtension>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsBaselineExtension {
    pub name: String,
    pub path: String,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsBaselineEntry {
    pub fingerprint: String,
    pub path: String,
    pub code: String,
    pub snippet: String,
    pub occurrence: u32,
    pub message: String,
    pub severity: String,
    pub range: DiagnosticsBaselineRange,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsBaselineRange {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticsBaselineCoverage {
    Full,
    Partial { completed_files: BTreeSet<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedDiagnostics<T> {
    pub new: Vec<ClassifiedDiagnostic<T>>,
    pub known: Vec<ClassifiedDiagnostic<T>>,
    pub resolved: Vec<DiagnosticsBaselineEntry>,
    pub summary: DiagnosticsBaselineSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedDiagnostic<T> {
    pub diagnostic: T,
    pub entry: DiagnosticsBaselineEntry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineDiagnosticCandidate<T> {
    pub diagnostic: T,
    pub path: String,
    pub code: String,
    pub snippet: Option<String>,
    pub message: String,
    pub severity: String,
    pub range: DiagnosticsBaselineRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticsBaselineState {
    Disabled,
    Full,
    Partial,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsBaselineSummary {
    pub state: DiagnosticsBaselineState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub known: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
    pub complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl DiagnosticsBaselineSummary {
    pub fn disabled() -> Self {
        Self {
            state: DiagnosticsBaselineState::Disabled,
            new: None,
            known: None,
            resolved: None,
            path: None,
            schema_version: None,
            complete: true,
            error_code: None,
            detail: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DiagnosticsBaselineError {
    #[error("invalid diagnostics baseline JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported diagnostics baseline schema version {found}; expected {expected}")]
    UnsupportedSchema { found: u32, expected: u32 },
    #[error("diagnostics baseline scope does not match the current project")]
    ScopeMismatch,
    #[error("diagnostics baseline contains invalid relative path: {0}")]
    InvalidPath(String),
    #[error("diagnostics baseline fingerprint does not match its fields: {0}")]
    FingerprintMismatch(String),
    #[error("diagnostics baseline contains duplicate entry: {0}")]
    Duplicate(String),
    #[error("diagnostics baseline cannot contain protected diagnostic: {0}")]
    ProtectedDiagnostic(String),
}

pub fn parse_diagnostics_baseline(
    bytes: &[u8],
    expected_scope: &DiagnosticsBaselineScope,
) -> Result<DiagnosticsBaseline, DiagnosticsBaselineError> {
    let baseline: DiagnosticsBaseline = serde_json::from_slice(bytes)?;
    validate_diagnostics_baseline(&baseline, expected_scope)?;
    Ok(baseline)
}

pub fn diagnostics_baseline_json(
    baseline: &DiagnosticsBaseline,
) -> Result<Vec<u8>, DiagnosticsBaselineError> {
    validate_diagnostics_baseline(baseline, &baseline.scope)?;
    let mut baseline = baseline.clone();
    baseline.diagnostics.sort_by(|a, b| {
        (&a.path, &a.code, &a.snippet, a.occurrence).cmp(&(
            &b.path,
            &b.code,
            &b.snippet,
            b.occurrence,
        ))
    });
    let mut bytes = serde_json::to_vec_pretty(&baseline)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn validate_diagnostics_baseline(
    baseline: &DiagnosticsBaseline,
    expected_scope: &DiagnosticsBaselineScope,
) -> Result<(), DiagnosticsBaselineError> {
    if baseline.schema_version != DIAGNOSTICS_BASELINE_SCHEMA_VERSION {
        return Err(DiagnosticsBaselineError::UnsupportedSchema {
            found: baseline.schema_version,
            expected: DIAGNOSTICS_BASELINE_SCHEMA_VERSION,
        });
    }
    if &baseline.scope != expected_scope {
        return Err(DiagnosticsBaselineError::ScopeMismatch);
    }
    validate_relative_path(&baseline.scope.source_root, true)?;
    for extension in &baseline.scope.extensions {
        validate_relative_path(&extension.path, false)?;
    }

    let mut fingerprints = HashSet::new();
    let mut identities = HashSet::new();
    for entry in &baseline.diagnostics {
        validate_relative_path(&entry.path, false)?;
        if is_protected_diagnostic(&entry.code) {
            return Err(DiagnosticsBaselineError::ProtectedDiagnostic(entry.code.clone()));
        }
        let expected =
            diagnostic_fingerprint(&entry.path, &entry.code, &entry.snippet, entry.occurrence);
        if entry.fingerprint != expected {
            return Err(DiagnosticsBaselineError::FingerprintMismatch(entry.fingerprint.clone()));
        }
        if !fingerprints.insert(&entry.fingerprint) {
            return Err(DiagnosticsBaselineError::Duplicate(entry.fingerprint.clone()));
        }
        let identity = (&entry.path, &entry.code, &entry.snippet, entry.occurrence);
        if !identities.insert(identity) {
            return Err(DiagnosticsBaselineError::Duplicate(entry.fingerprint.clone()));
        }
    }
    Ok(())
}

fn validate_relative_path(path: &str, allow_empty: bool) -> Result<(), DiagnosticsBaselineError> {
    let valid = (allow_empty && path.is_empty())
        || (!path.is_empty()
            && !path.starts_with('/')
            && !path.contains('\\')
            && path
                .split('/')
                .all(|component| !component.is_empty() && component != "." && component != ".."));
    if valid {
        Ok(())
    } else {
        Err(DiagnosticsBaselineError::InvalidPath(path.to_owned()))
    }
}

pub fn classify_diagnostics<T>(
    baseline: &DiagnosticsBaseline,
    baseline_path: String,
    mut current: Vec<BaselineDiagnosticCandidate<T>>,
    coverage: &DiagnosticsBaselineCoverage,
) -> Result<ClassifiedDiagnostics<T>, MissingDiagnosticSnippet> {
    current.sort_by(|a, b| {
        (&a.path, &a.range, &a.code, &a.message).cmp(&(&b.path, &b.range, &b.code, &b.message))
    });

    let baseline_fingerprints: HashSet<&str> =
        baseline.diagnostics.iter().map(|entry| entry.fingerprint.as_str()).collect();
    let mut matched = HashSet::new();
    let mut occurrences: HashMap<(String, String, String), u32> = HashMap::new();
    let mut new = Vec::new();
    let mut known = Vec::new();

    for candidate in current {
        let snippet = normalize_diagnostic_snippet(
            candidate.snippet.as_deref().ok_or(MissingDiagnosticSnippet)?,
        );
        let occurrence = occurrences
            .entry((candidate.path.clone(), candidate.code.clone(), snippet.clone()))
            .or_default();
        let occurrence_index = *occurrence;
        let fingerprint =
            diagnostic_fingerprint(&candidate.path, &candidate.code, &snippet, occurrence_index);
        *occurrence += 1;

        let classified = ClassifiedDiagnostic {
            diagnostic: candidate.diagnostic,
            entry: DiagnosticsBaselineEntry {
                fingerprint: fingerprint.clone(),
                path: candidate.path,
                code: candidate.code,
                snippet,
                occurrence: occurrence_index,
                message: candidate.message,
                severity: candidate.severity,
                range: candidate.range,
            },
        };
        if !is_protected_diagnostic(&classified.entry.code)
            && baseline_fingerprints.contains(fingerprint.as_str())
        {
            matched.insert(fingerprint);
            known.push(classified);
        } else {
            new.push(classified);
        }
    }

    let resolved: Vec<_> = baseline
        .diagnostics
        .iter()
        .filter(|entry| {
            !matched.contains(&entry.fingerprint)
                && match coverage {
                    DiagnosticsBaselineCoverage::Full => true,
                    DiagnosticsBaselineCoverage::Partial { completed_files } => {
                        completed_files.contains(&entry.path)
                    }
                }
        })
        .cloned()
        .collect();
    let complete = matches!(coverage, DiagnosticsBaselineCoverage::Full);
    let summary = DiagnosticsBaselineSummary {
        state: if complete {
            DiagnosticsBaselineState::Full
        } else {
            DiagnosticsBaselineState::Partial
        },
        new: Some(new.len()),
        known: Some(known.len()),
        resolved: Some(resolved.len()),
        path: Some(baseline_path),
        schema_version: Some(baseline.schema_version),
        complete,
        error_code: None,
        detail: None,
    };

    Ok(ClassifiedDiagnostics { new, known, resolved, summary })
}

fn is_protected_diagnostic(code: &str) -> bool {
    matches!(code, "UnknownSuppressionCode" | "SuppressionWithoutCode")
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASELINE_JSON: &str = r#"{
        "schema_version": 1,
        "scope": {
            "source_root": "src",
            "extensions": [{
                "name": "Extension",
                "path": "extensions/Extension",
                "depends_on": []
            }]
        },
        "diagnostics": [{
            "fingerprint": "abc",
            "path": "CommonModules/Module/Ext/Module.bsl",
            "code": "LineLength",
            "snippet": "Message(\"long line\");",
            "occurrence": 0,
            "message": "Line is too long",
            "severity": "Warning",
            "range": {
                "start_line": 1,
                "start_column": 0,
                "end_line": 1,
                "end_column": 21
            }
        }]
    }"#;

    #[test]
    fn diagnostics_baseline_schema_round_trips() {
        let baseline: DiagnosticsBaseline = serde_json::from_str(BASELINE_JSON).unwrap();
        assert_eq!(baseline.schema_version, DIAGNOSTICS_BASELINE_SCHEMA_VERSION);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&serde_json::to_string(&baseline).unwrap())
                .unwrap(),
            serde_json::from_str::<serde_json::Value>(BASELINE_JSON).unwrap()
        );
    }

    #[test]
    fn diagnostics_baseline_schema_rejects_unknown_fields() {
        let json = BASELINE_JSON
            .replace("\"schema_version\": 1,", "\"schema_version\": 1, \"extra\": true,");
        assert!(serde_json::from_str::<DiagnosticsBaseline>(&json).is_err());
    }

    #[test]
    fn diagnostics_baseline_scope_distinguishes_full_and_partial() {
        assert_eq!(DiagnosticsBaselineCoverage::Full, DiagnosticsBaselineCoverage::Full);
        let completed_files = BTreeSet::from(["Module.bsl".to_owned()]);
        assert_eq!(
            DiagnosticsBaselineCoverage::Partial { completed_files: completed_files.clone() },
            DiagnosticsBaselineCoverage::Partial { completed_files }
        );
    }

    #[test]
    fn diagnostics_baseline_scope_serializes_summary_states() {
        for state in [
            DiagnosticsBaselineState::Disabled,
            DiagnosticsBaselineState::Full,
            DiagnosticsBaselineState::Partial,
            DiagnosticsBaselineState::Error,
        ] {
            let summary = DiagnosticsBaselineSummary {
                state,
                new: None,
                known: None,
                resolved: None,
                path: None,
                schema_version: None,
                complete: state == DiagnosticsBaselineState::Full,
                error_code: None,
                detail: None,
            };
            let value = serde_json::to_value(summary).unwrap();
            assert_eq!(value["state"], state_string(state));
            assert!(value.get("new").is_none());
        }
    }

    #[test]
    fn diagnostics_fingerprint_normalizes_and_preserves_recipe() {
        let snippet = normalize_diagnostic_snippet("  Message(  A ); ");
        assert_eq!(snippet, "Message( A );");
        assert_eq!(
            diagnostic_fingerprint("a.bsl", "Rule", &snippet, 0),
            "55a27325281b7f3d47e95d1123a1c09590908e5b3378b73168372a0e665264cd"
        );
    }

    #[test]
    fn diagnostics_fingerprint_requires_snippet() {
        assert!(strict_diagnostic_fingerprint("a.bsl", "Rule", None, 0).is_err());
        assert_eq!(
            strict_diagnostic_fingerprint("a.bsl", "Rule", Some(""), 0).unwrap(),
            diagnostic_fingerprint("a.bsl", "Rule", "", 0)
        );
    }

    #[test]
    fn diagnostics_baseline_classify_survives_line_shift() {
        let baseline = baseline(vec![entry("A();", 0, 1)]);
        let result = classify_diagnostics(
            &baseline,
            "baseline.json".to_owned(),
            vec![candidate("A();", 9, "current")],
            &DiagnosticsBaselineCoverage::Full,
        )
        .unwrap();

        assert!(result.new.is_empty());
        assert_eq!(result.known.len(), 1);
        assert!(result.resolved.is_empty());
        assert_eq!(result.known[0].entry.range.start_line, 9);
    }

    #[test]
    fn diagnostics_baseline_classify_changed_expression_is_new_and_resolved() {
        let baseline = baseline(vec![entry("A();", 0, 1)]);
        let result = classify_diagnostics(
            &baseline,
            "baseline.json".to_owned(),
            vec![candidate("B();", 1, "current")],
            &DiagnosticsBaselineCoverage::Full,
        )
        .unwrap();

        assert_eq!(result.new.len(), 1);
        assert!(result.known.is_empty());
        assert_eq!(result.resolved.len(), 1);
    }

    #[test]
    fn diagnostics_baseline_classify_numbers_identical_lines() {
        let baseline = baseline(vec![entry("A();", 0, 1), entry("A();", 1, 2)]);
        let result = classify_diagnostics(
            &baseline,
            "baseline.json".to_owned(),
            vec![candidate("A();", 4, "first"), candidate("A();", 8, "second")],
            &DiagnosticsBaselineCoverage::Full,
        )
        .unwrap();

        assert_eq!(result.known.len(), 2);
        assert_eq!(result.known[0].entry.occurrence, 0);
        assert_eq!(result.known[1].entry.occurrence, 1);
    }

    #[test]
    fn diagnostics_baseline_io_is_byte_deterministic() {
        let baseline = baseline(vec![entry("B();", 0, 2), entry("A();", 0, 1)]);
        let first = diagnostics_baseline_json(&baseline).unwrap();
        let second = diagnostics_baseline_json(&baseline).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.last(), Some(&b'\n'));

        let parsed = parse_diagnostics_baseline(&first, &baseline.scope).unwrap();
        assert_eq!(parsed.diagnostics[0].snippet, "A();");
        assert_eq!(parsed.diagnostics[1].snippet, "B();");
    }

    #[test]
    fn diagnostics_baseline_io_recalculates_fingerprint() {
        let baseline = baseline(vec![entry("A();", 0, 1)]);
        let mut value = serde_json::to_value(&baseline).unwrap();
        value["diagnostics"][0]["fingerprint"] = "forged".into();
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(matches!(
            parse_diagnostics_baseline(&bytes, &baseline.scope),
            Err(DiagnosticsBaselineError::FingerprintMismatch(_))
        ));
    }

    #[test]
    fn diagnostics_baseline_io_rejects_bad_schema_and_json() {
        let mut baseline = baseline(vec![]);
        baseline.schema_version = 99;
        let bytes = serde_json::to_vec(&baseline).unwrap();
        assert!(matches!(
            parse_diagnostics_baseline(&bytes, &baseline.scope),
            Err(DiagnosticsBaselineError::UnsupportedSchema { .. })
        ));
        assert!(matches!(
            parse_diagnostics_baseline(b"{", &baseline.scope),
            Err(DiagnosticsBaselineError::Json(_))
        ));
    }

    #[test]
    fn diagnostics_baseline_io_rejects_duplicates() {
        let baseline = baseline(vec![entry("A();", 0, 1), entry("A();", 0, 2)]);
        let bytes = serde_json::to_vec(&baseline).unwrap();
        assert!(matches!(
            parse_diagnostics_baseline(&bytes, &baseline.scope),
            Err(DiagnosticsBaselineError::Duplicate(_))
        ));
    }

    #[test]
    fn diagnostics_baseline_io_rejects_incompatible_scope_and_paths() {
        let baseline = baseline(vec![]);
        let other_scope =
            DiagnosticsBaselineScope { source_root: "other".to_owned(), extensions: vec![] };
        let bytes = serde_json::to_vec(&baseline).unwrap();
        assert!(matches!(
            parse_diagnostics_baseline(&bytes, &other_scope),
            Err(DiagnosticsBaselineError::ScopeMismatch)
        ));

        let invalid = DiagnosticsBaseline {
            scope: DiagnosticsBaselineScope {
                source_root: "/absolute".to_owned(),
                extensions: vec![],
            },
            ..baseline
        };
        assert!(matches!(
            diagnostics_baseline_json(&invalid),
            Err(DiagnosticsBaselineError::InvalidPath(_))
        ));
    }

    #[test]
    fn diagnostics_baseline_protected_diagnostics_remain_active() {
        for code in ["UnknownSuppressionCode", "SuppressionWithoutCode"] {
            let mut protected = entry("A();", 0, 1);
            protected.code = code.to_owned();
            protected.fingerprint = diagnostic_fingerprint("a.bsl", code, "A();", 0);
            let baseline = baseline(vec![protected]);
            assert!(matches!(
                diagnostics_baseline_json(&baseline),
                Err(DiagnosticsBaselineError::ProtectedDiagnostic(found)) if found == code
            ));

            let mut current = candidate("A();", 1, "current");
            current.code = code.to_owned();
            let result = classify_diagnostics(
                &baseline,
                "baseline.json".to_owned(),
                vec![current],
                &DiagnosticsBaselineCoverage::Full,
            )
            .unwrap();
            assert_eq!(result.new.len(), 1);
            assert!(result.known.is_empty());
        }
    }

    fn baseline(diagnostics: Vec<DiagnosticsBaselineEntry>) -> DiagnosticsBaseline {
        DiagnosticsBaseline {
            schema_version: DIAGNOSTICS_BASELINE_SCHEMA_VERSION,
            scope: DiagnosticsBaselineScope { source_root: "src".to_owned(), extensions: vec![] },
            diagnostics,
        }
    }

    fn entry(snippet: &str, occurrence: u32, line: u32) -> DiagnosticsBaselineEntry {
        DiagnosticsBaselineEntry {
            fingerprint: diagnostic_fingerprint("a.bsl", "Rule", snippet, occurrence),
            path: "a.bsl".to_owned(),
            code: "Rule".to_owned(),
            snippet: snippet.to_owned(),
            occurrence,
            message: "old".to_owned(),
            severity: "Warning".to_owned(),
            range: range(line),
        }
    }

    fn candidate(
        snippet: &str,
        line: u32,
        diagnostic: &'static str,
    ) -> BaselineDiagnosticCandidate<&'static str> {
        BaselineDiagnosticCandidate {
            diagnostic,
            path: "a.bsl".to_owned(),
            code: "Rule".to_owned(),
            snippet: Some(snippet.to_owned()),
            message: "current".to_owned(),
            severity: "Warning".to_owned(),
            range: range(line),
        }
    }

    fn range(line: u32) -> DiagnosticsBaselineRange {
        DiagnosticsBaselineRange {
            start_line: line,
            start_column: 0,
            end_line: line,
            end_column: 1,
        }
    }

    fn state_string(state: DiagnosticsBaselineState) -> &'static str {
        match state {
            DiagnosticsBaselineState::Disabled => "disabled",
            DiagnosticsBaselineState::Full => "full",
            DiagnosticsBaselineState::Partial => "partial",
            DiagnosticsBaselineState::Error => "error",
        }
    }
}
