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
