use std::path::Path;

use ide::diagnostics_baseline::{
    BaselineDiagnosticCandidate, DiagnosticsBaselineCoverage, DiagnosticsBaselineRange,
};
use ide_host_core::diagnostics_baseline::DiagnosticsBaselineSnapshot;

pub mod transaction;

pub(crate) fn active_for_file(
    snapshot: &DiagnosticsBaselineSnapshot,
    project_root: &Path,
    path: &Path,
    text: &str,
    diagnostics: Vec<ide::Diagnostic>,
) -> Vec<ide::Diagnostic> {
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
    let coverage = DiagnosticsBaselineCoverage::Partial {
        completed_files: std::collections::BTreeSet::from([relative.clone()]),
    };
    let active: std::collections::HashSet<_> = match snapshot {
        DiagnosticsBaselineSnapshot::Ready { baseline, project_path, .. } => {
            let Ok(classified) = ide::diagnostics_baseline::classify_diagnostics_with(
                baseline,
                project_path.clone(),
                candidates,
                &coverage,
                ide::diagnostics_baseline::ResolvedPolicy::Skip,
            ) else {
                return diagnostics;
            };
            classified.new.into_iter().map(|item| item.diagnostic).collect()
        }
        DiagnosticsBaselineSnapshot::ReadySet { baseline, plan, project_path, .. } => {
            let Some(owner) = plan.owner_for_project_path(&relative) else { return diagnostics };
            let candidates = candidates
                .into_iter()
                .map(|candidate| {
                    ide::partitioned_diagnostics_baseline::PartitionedBaselineDiagnosticCandidate {
                        partition_id: owner.to_owned(),
                        candidate,
                    }
                })
                .collect();
            let Ok(coverage) =
                ide::partitioned_diagnostics_baseline::partitioned_coverage(plan, &coverage, None)
            else {
                return diagnostics;
            };
            // Only this file's active diagnostics are wanted; `resolved` would walk
            // every entry of every enabled partition on each publication and be
            // discarded, making editor latency grow with the accumulated debt.
            let Ok(classified) =
                ide::partitioned_diagnostics_baseline::classify_partitioned_diagnostics_with(
                    baseline,
                    plan,
                    project_path.clone(),
                    candidates,
                    &coverage,
                    ide::diagnostics_baseline::ResolvedPolicy::Skip,
                )
            else {
                return diagnostics;
            };
            classified
                .new
                .into_iter()
                .chain(classified.unsuppressed)
                .map(|item| item.diagnostic)
                .collect()
        }
        DiagnosticsBaselineSnapshot::Disabled | DiagnosticsBaselineSnapshot::Error { .. } => {
            return diagnostics;
        }
    };
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

    #[test]
    fn lsp_partitioned_baseline_publish_uses_the_owner_partition() {
        use ide::partitioned_diagnostics_baseline::{
            diagnostics_manifest, diagnostics_manifest_json, diagnostics_partition_json,
            partition_object_path, DiagnosticsBaselineManifestEntry,
        };
        use std::io::Write;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for source in ["src/cf", "src/cfe/Ext"] {
            std::fs::create_dir_all(root.join(source)).unwrap();
            std::fs::write(root.join(source).join("Configuration.xml"), "<Configuration/>")
                .unwrap();
        }
        std::fs::write(
            root.join("bsl-analyzer.toml"),
            r#"[source]
root = "src/cf"
extensions = [{ name = "Ext", path = "src/cfe/Ext" }]
[diagnostics.baseline]
directory = "baselines"
"#,
        )
        .unwrap();
        let text = "Процедура П()\nКонецПроцедуры\n";
        let relative = "src/cfe/Ext/Module.bsl";
        let known = DiagnosticCode::EmptyCodeBlock;
        let snippet = normalize_diagnostic_snippet(text.lines().next().unwrap());
        let entry = DiagnosticsBaselineEntry {
            fingerprint: diagnostic_fingerprint(relative, known.as_str(), &snippet, 0),
            path: relative.to_owned(),
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
        };
        let project = project_model::Project::new(root).unwrap();
        let plan = project.diagnostics_baseline_partition_plan().unwrap().unwrap();
        let directory =
            project_model::ManagedBaselineDirectory::open(root, "baselines", true).unwrap();
        let mut manifest_entries = Vec::new();
        for partition in &plan.partitions {
            let diagnostics =
                if partition.id == "extension:Ext" { vec![entry.clone()] } else { vec![] };
            let bytes =
                diagnostics_partition_json(partition.identity.clone(), diagnostics).unwrap();
            let hash = blake3::hash(&bytes).to_hex().to_string();
            let path = partition_object_path(&partition.id, &partition.key, &hash).unwrap();
            directory.create_file_new(&path).unwrap().write_all(&bytes).unwrap();
            manifest_entries.push(DiagnosticsBaselineManifestEntry {
                partition_id: partition.id.clone(),
                file: path,
                blake3: hash,
            });
        }
        let manifest =
            diagnostics_manifest(plan.project_scope_fingerprint.clone(), manifest_entries);
        directory
            .create_file_new("manifest.json")
            .unwrap()
            .write_all(&diagnostics_manifest_json(&manifest).unwrap())
            .unwrap();
        let snapshot = DiagnosticsBaselineSnapshot::load(&project);
        let active = active_for_file(
            &snapshot,
            root,
            &root.join(relative),
            text,
            vec![diagnostic(known), diagnostic(DiagnosticCode::UnknownSuppressionCode)],
        );
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].code, DiagnosticCode::UnknownSuppressionCode);

        let all = active_for_file(
            &DiagnosticsBaselineSnapshot::Error {
                path: None,
                observation_paths: vec![],
                selection: None,
                partitions_enabled: None,
                partitions_unsuppressed: None,
                code: "invalid_set".to_owned(),
                detail: "broken".to_owned(),
                epoch: "error".to_owned(),
                errors: vec![ide::diagnostics_baseline::DiagnosticsBaselineErrorSummary {
                    partition_id: None,
                    code: "invalid_set".to_owned(),
                    detail: "broken".to_owned(),
                    epoch: "error".to_owned(),
                }],
            },
            root,
            &root.join(relative),
            text,
            vec![diagnostic(known)],
        );
        assert_eq!(all.len(), 1, "LSP fails open for an invalid partition set");
    }
}
