//! Console reporter for analysis results.

use std::path::Path;

use super::{AnalysisResults, Reporter};

/// Console reporter that prints results to stdout.
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
        println!("Time elapsed: {:.2}s", results.elapsed_secs);
        println!("Speed: {:.0} files/sec", results.files_analyzed as f64 / results.elapsed_secs);

        // Show files with most issues (top 10)
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
