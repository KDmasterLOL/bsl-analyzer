use std::path::Path;

use super::{AnalysisResults, Reporter};

fn baseline_lines(baseline: &ide::diagnostics_baseline::DiagnosticsBaselineSummary) -> Vec<String> {
    use ide::diagnostics_baseline::DiagnosticsBaselineState;
    let state = match baseline.state {
        DiagnosticsBaselineState::Disabled => "disabled",
        DiagnosticsBaselineState::Full => "full",
        DiagnosticsBaselineState::Partial => "partial",
        DiagnosticsBaselineState::Error => "error",
    };
    let mut lines = vec![format!("Diagnostics baseline: {state}")];
    if let (Some(new), Some(known), Some(resolved)) =
        (baseline.new, baseline.known, baseline.resolved)
    {
        lines.push(format!("  New: {new}, known: {known}, resolved: {resolved}"));
    }
    for partition in &baseline.partitions {
        let state = match partition.state {
            DiagnosticsBaselineState::Disabled => "disabled",
            DiagnosticsBaselineState::Full => "full",
            DiagnosticsBaselineState::Partial => "partial",
            DiagnosticsBaselineState::Error => "error",
        };
        lines.push(format!(
            "  {}: {state} (new {}, known {}, resolved {})",
            partition.id, partition.new, partition.known, partition.resolved
        ));
    }
    lines
}

pub struct ConsoleReporter;

impl Reporter for ConsoleReporter {
    fn key(&self) -> &'static str {
        "console"
    }

    fn report(&self, results: &AnalysisResults, _output_dir: &Path) -> anyhow::Result<()> {
        println!("\n=== BSL Analyzer Results ===");
        println!("Files analyzed: {}", results.files_analyzed);
        println!("Files with issues: {}", results.files_with_issues);
        println!("Total diagnostics: {}", results.total_diagnostics);
        for line in baseline_lines(&results.baseline) {
            println!("{line}");
        }
        println!("Time elapsed: {:.2}s", results.elapsed_secs);
        println!("Speed: {:.0} files/sec", results.files_analyzed as f64 / results.elapsed_secs);

        if !results.diagnostics.is_empty() {
            println!("\nFiles with most issues:");
            let mut sorted = results.diagnostics.clone();
            sorted.sort_by_key(|f| std::cmp::Reverse(f.diagnostics.len()));

            for file_analysis in sorted.iter().take(10) {
                println!(
                    "  {} - {} issue{}",
                    file_analysis.relative_path.display(),
                    file_analysis.diagnostics.len(),
                    if file_analysis.diagnostics.len() == 1 { "" } else { "s" }
                );
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide::diagnostics_baseline::{DiagnosticsBaselineState, DiagnosticsBaselineSummary};

    #[test]
    fn console_baseline_summary_covers_disabled_full_and_partial() {
        assert_eq!(
            baseline_lines(&DiagnosticsBaselineSummary::disabled()),
            ["Diagnostics baseline: disabled"]
        );
        for (state, label) in [
            (DiagnosticsBaselineState::Full, "full"),
            (DiagnosticsBaselineState::Partial, "partial"),
        ] {
            let summary = DiagnosticsBaselineSummary {
                state,
                new: Some(1),
                known: Some(2),
                resolved: Some(3),
                path: Some("baseline.json".to_owned()),
                schema_version: Some(1),
                manifest_schema_version: None,
                complete: state == DiagnosticsBaselineState::Full,
                error_code: None,
                detail: None,
                partitions: vec![],
                errors: vec![],
            };
            let lines = baseline_lines(&summary);
            assert_eq!(lines[0], format!("Diagnostics baseline: {label}"));
            assert_eq!(lines[1], "  New: 1, known: 2, resolved: 3");
        }
    }
}
