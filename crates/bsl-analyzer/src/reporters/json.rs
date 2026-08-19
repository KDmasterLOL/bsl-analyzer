use std::path::Path;

use super::{AnalysisResults, Reporter};

pub struct JsonReporter;

impl Reporter for JsonReporter {
    fn key(&self) -> &'static str {
        "json"
    }

    fn report(&self, results: &AnalysisResults, output_dir: &Path) -> anyhow::Result<()> {
        let json_output = serde_json::json!({
            "summary": {
                "files_analyzed": results.files_analyzed,
                "files_with_issues": results.files_with_issues,
                "total_diagnostics": results.total_diagnostics,
                "elapsed_secs": results.elapsed_secs,
            },
            "baseline": results.baseline,
            "diagnostics": results.diagnostics.iter().map(|f| {
                serde_json::json!({
                    "file": f.relative_path.to_string_lossy(),
                    "issues": f.diagnostics.len(),
                })
            }).collect::<Vec<_>>(),
        });

        let output_file = output_dir.join("bsl-json.json");
        let json_str = serde_json::to_string_pretty(&json_output)?;
        std::fs::write(&output_file, json_str)?;

        tracing::info!("JSON report written to {:?}", output_file);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn json_baseline_summary_is_a_backward_compatible_root_field() {
        let temp = tempfile::tempdir().unwrap();
        let mut results = AnalysisResults {
            files_analyzed: 0,
            files_with_issues: 0,
            total_diagnostics: 0,
            elapsed_secs: 0.0,
            diagnostics: Vec::new(),
            source_dir: PathBuf::from("."),
            workspace_dir: PathBuf::from("."),
            baseline: ide::diagnostics_baseline::DiagnosticsBaselineSummary::disabled(),
        };
        JsonReporter.report(&results, temp.path()).unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(temp.path().join("bsl-json.json")).unwrap())
                .unwrap();
        assert_eq!(value["baseline"]["state"], "disabled");
        assert!(value["summary"].is_object());
        assert!(value["diagnostics"].is_array());

        for state in [
            ide::diagnostics_baseline::DiagnosticsBaselineState::Full,
            ide::diagnostics_baseline::DiagnosticsBaselineState::Partial,
        ] {
            results.baseline = ide::diagnostics_baseline::DiagnosticsBaselineSummary {
                state,
                new: Some(1),
                known: Some(2),
                resolved: Some(3),
                path: Some("baseline.json".to_owned()),
                schema_version: Some(1),
                manifest_schema_version: None,
                complete: state == ide::diagnostics_baseline::DiagnosticsBaselineState::Full,
                error_code: None,
                detail: None,
                partitions: vec![],
                errors: vec![],
            };
            JsonReporter.report(&results, temp.path()).unwrap();
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(temp.path().join("bsl-json.json")).unwrap())
                    .unwrap();
            assert_eq!(value["baseline"]["new"], 1);
            assert_eq!(
                value["baseline"]["complete"],
                state == ide::diagnostics_baseline::DiagnosticsBaselineState::Full
            );
        }
    }
}
