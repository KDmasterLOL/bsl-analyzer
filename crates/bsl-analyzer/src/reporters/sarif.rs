use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};

use super::{AnalysisResults, Reporter};

const SARIF_SCHEMA: &str = "https://json.schemastore.org/sarif-2.1.0.json";

pub struct SarifReporter;

impl Reporter for SarifReporter {
    fn key(&self) -> &'static str {
        "sarif"
    }

    fn report(&self, results: &AnalysisResults, output_dir: &Path) -> anyhow::Result<()> {
        let sarif_output = sarif_document(results);
        let output_file = output_dir.join("bsl-analyzer.sarif");
        let sarif_str = serde_json::to_string_pretty(&sarif_output)?;
        std::fs::write(&output_file, sarif_str)?;

        tracing::info!("SARIF report written to {:?}", output_file);
        Ok(())
    }
}

fn sarif_document(results: &AnalysisResults) -> Value {
    let mut rules = BTreeMap::new();
    let mut sarif_results = Vec::new();

    for file in &results.diagnostics {
        let uri = sarif_uri(&file.relative_path);

        for diagnostic in &file.diagnostics {
            rules.entry(diagnostic.code.clone()).or_insert_with(|| {
                json!({
                    "id": diagnostic.code.clone(),
                    "name": diagnostic.code.clone(),
                    "shortDescription": {
                        "text": diagnostic.code.clone(),
                    },
                })
            });

            sarif_results.push(json!({
                "ruleId": diagnostic.code.clone(),
                "level": sarif_level(&diagnostic.severity),
                "message": {
                    "text": diagnostic.message.clone(),
                },
                "locations": [
                    {
                        "physicalLocation": {
                            "artifactLocation": {
                                "uri": uri.clone(),
                            },
                            "region": {
                                "startLine": diagnostic.start_line + 1,
                                "startColumn": diagnostic.start_column + 1,
                                "endLine": diagnostic.end_line + 1,
                                "endColumn": diagnostic.end_column + 1,
                            },
                        },
                    },
                ],
            }));
        }
    }

    json!({
        "$schema": SARIF_SCHEMA,
        "version": "2.1.0",
        "runs": [
            {
                "tool": {
                    "driver": {
                        "name": "bsl-analyzer",
                        "semanticVersion": env!("CARGO_PKG_VERSION"),
                        "rules": rules.into_values().collect::<Vec<_>>(),
                    },
                },
                "results": sarif_results,
            },
        ],
    })
}

fn sarif_uri(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn sarif_level(severity: &str) -> &'static str {
    match severity {
        "Blocker" | "Critical" | "Major" | "Error" => "error",
        "Warning" => "warning",
        "Information" | "Hint" => "note",
        _ => "warning",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ide::DiagnosticOutput;
    use tempfile::TempDir;

    use super::*;
    use crate::reporters::{AnalysisResults, FileAnalysis, Reporter};

    #[test]
    fn writes_sarif_document_with_rules_results_and_locations() {
        let temp = TempDir::new().expect("tempdir");
        let results = sample_results();

        SarifReporter.report(&results, temp.path()).expect("write sarif");

        let sarif: Value = serde_json::from_str(
            &std::fs::read_to_string(temp.path().join("bsl-analyzer.sarif")).expect("sarif report"),
        )
        .expect("valid json");

        assert_eq!(sarif["$schema"], SARIF_SCHEMA);
        assert_eq!(sarif["version"], "2.1.0");

        let driver = &sarif["runs"][0]["tool"]["driver"];
        assert_eq!(driver["name"], "bsl-analyzer");
        assert_eq!(driver["semanticVersion"], env!("CARGO_PKG_VERSION"));
        assert_eq!(driver["rules"][0]["id"], "LineLength");

        let result = &sarif["runs"][0]["results"][0];
        assert_eq!(result["ruleId"], "LineLength");
        assert_eq!(result["message"]["text"], "Line too long");
        assert_eq!(result["level"], "warning");
        assert_eq!(
            result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "CommonModules/Module/Ext/Module.bsl"
        );
        assert_eq!(result["locations"][0]["physicalLocation"]["region"]["startLine"], 3);
        assert_eq!(result["locations"][0]["physicalLocation"]["region"]["startColumn"], 5);
        assert_eq!(result["locations"][0]["physicalLocation"]["region"]["endLine"], 3);
        assert_eq!(result["locations"][0]["physicalLocation"]["region"]["endColumn"], 16);
    }

    #[test]
    fn maps_diagnostic_severity_to_sarif_level() {
        assert_eq!(sarif_level("Blocker"), "error");
        assert_eq!(sarif_level("Critical"), "error");
        assert_eq!(sarif_level("Major"), "error");
        assert_eq!(sarif_level("Error"), "error");
        assert_eq!(sarif_level("Warning"), "warning");
        assert_eq!(sarif_level("Information"), "note");
        assert_eq!(sarif_level("Hint"), "note");
    }

    #[test]
    fn normalizes_windows_path_separators_in_uri() {
        assert_eq!(
            sarif_uri(Path::new(r"CommonModules\Module\Ext\Module.bsl")),
            "CommonModules/Module/Ext/Module.bsl"
        );
    }

    fn sample_results() -> AnalysisResults {
        AnalysisResults {
            files_analyzed: 1,
            files_with_issues: 1,
            total_diagnostics: 1,
            elapsed_secs: 0.1,
            diagnostics: vec![FileAnalysis {
                path: PathBuf::from("CommonModules/Module/Ext/Module.bsl"),
                relative_path: PathBuf::from("CommonModules/Module/Ext/Module.bsl"),
                diagnostics: vec![DiagnosticOutput {
                    code: "LineLength".to_string(),
                    message: "Line too long".to_string(),
                    severity: "Warning".to_string(),
                    start_line: 2,
                    start_column: 4,
                    end_line: 2,
                    end_column: 15,
                    tags: vec![],
                }],
            }],
            source_dir: PathBuf::from("."),
            workspace_dir: PathBuf::from("."),
        }
    }
}
