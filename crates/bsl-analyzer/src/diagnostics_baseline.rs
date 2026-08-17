use std::path::Path;

use ide::diagnostics_baseline::{
    classify_diagnostics, BaselineDiagnosticCandidate, DiagnosticsBaselineCoverage,
    DiagnosticsBaselineRange,
};
use ide_host_core::diagnostics_baseline::DiagnosticsBaselineSnapshot;

pub(crate) fn active_for_file(
    snapshot: &DiagnosticsBaselineSnapshot,
    project_root: &Path,
    path: &Path,
    text: &str,
    diagnostics: Vec<ide::Diagnostic>,
) -> Vec<ide::Diagnostic> {
    let Some((baseline, baseline_path)) = snapshot.ready() else { return diagnostics };
    let Ok(relative) = path.strip_prefix(project_root) else { return diagnostics };
    let relative = relative.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
    let source_lines: Vec<_> = text.lines().collect();
    let candidates: Vec<_> = diagnostics
        .iter()
        .enumerate()
        .map(|(index, diagnostic)| {
            let output = diagnostic.to_output(text);
            BaselineDiagnosticCandidate {
                diagnostic: index,
                path: relative.clone(),
                code: output.code,
                snippet: Some(ide::diagnostics_baseline::diagnostic_line_snippet(
                    &source_lines,
                    output.start_line,
                )),
                message: output.message,
                severity: output.severity,
                range: DiagnosticsBaselineRange {
                    start_line: output.start_line as u32,
                    start_column: output.start_column as u32,
                    end_line: output.end_line as u32,
                    end_column: output.end_column as u32,
                },
            }
        })
        .collect();
    let Ok(classified) = classify_diagnostics(
        baseline,
        baseline_path.to_owned(),
        candidates,
        &DiagnosticsBaselineCoverage::Partial {
            completed_files: std::collections::BTreeSet::from([relative]),
        },
    ) else {
        return diagnostics;
    };
    let active: std::collections::HashSet<_> =
        classified.new.into_iter().map(|item| item.diagnostic).collect();
    diagnostics
        .into_iter()
        .enumerate()
        .filter_map(|(index, diagnostic)| active.contains(&index).then_some(diagnostic))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide::diagnostics_baseline::{
        diagnostic_fingerprint, normalize_diagnostic_snippet, DiagnosticsBaseline,
        DiagnosticsBaselineEntry, DiagnosticsBaselineRange, DiagnosticsBaselineScope,
        DIAGNOSTICS_BASELINE_SCHEMA_VERSION,
    };
    use ide::{Diagnostic, DiagnosticCode, Severity, TextRange};

    fn diagnostic(code: DiagnosticCode) -> Diagnostic {
        Diagnostic {
            code,
            message: code.as_str().to_owned(),
            severity: Severity::Warning,
            range: TextRange::new(0.into(), 8.into()),
            tags: Vec::new(),
            fixes: Vec::new(),
        }
    }

    #[test]
    fn lsp_diagnostics_baseline_publish_keeps_new_and_protected_only() {
        let root = Path::new("/workspace");
        let path = root.join("Module.bsl");
        let text = "Процедура П()\nКонецПроцедуры\n";
        let snippet = normalize_diagnostic_snippet(text.lines().next().unwrap());
        let known = DiagnosticCode::EmptyCodeBlock;
        let baseline = DiagnosticsBaseline {
            schema_version: DIAGNOSTICS_BASELINE_SCHEMA_VERSION,
            scope: DiagnosticsBaselineScope { source_root: String::new(), extensions: vec![] },
            diagnostics: vec![DiagnosticsBaselineEntry {
                fingerprint: diagnostic_fingerprint("Module.bsl", known.as_str(), &snippet, 0),
                path: "Module.bsl".to_owned(),
                code: known.as_str().to_owned(),
                snippet,
                occurrence: 0,
                message: known.as_str().to_owned(),
                severity: "warning".to_owned(),
                range: DiagnosticsBaselineRange {
                    start_line: 0,
                    start_column: 0,
                    end_line: 0,
                    end_column: 8,
                },
            }],
        };
        let snapshot = DiagnosticsBaselineSnapshot::Ready {
            baseline,
            project_path: "baseline.json".to_owned(),
            path: root.join("baseline.json"),
            epoch: "test".to_owned(),
        };
        let active = active_for_file(
            &snapshot,
            root,
            &path,
            text,
            vec![
                diagnostic(known),
                diagnostic(DiagnosticCode::UnreachableCode),
                diagnostic(DiagnosticCode::UnknownSuppressionCode),
            ],
        );
        let codes: Vec<_> = active.iter().map(|item| item.code).collect();
        assert!(!codes.contains(&known));
        assert!(codes.contains(&DiagnosticCode::UnreachableCode));
        assert!(codes.contains(&DiagnosticCode::UnknownSuppressionCode));
    }
}
