use std::path::{Path, PathBuf};

use ide::DiagnosticOutput;

pub mod codequality;
pub mod console;
pub mod json;
pub mod junit;
pub mod sarif;

#[derive(Debug, Clone)]
pub struct AnalysisResults {
    pub files_analyzed: usize,
    pub files_with_issues: usize,
    pub total_diagnostics: usize,
    pub elapsed_secs: f64,
    pub diagnostics: Vec<FileAnalysis>,
    pub source_dir: PathBuf,
    pub workspace_dir: PathBuf,
    pub baseline: ide::diagnostics_baseline::DiagnosticsBaselineSummary,
}

#[derive(Debug, Clone)]
pub struct FileAnalysis {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub diagnostics: Vec<DiagnosticOutput>,

    /// Normalized source line for each entry of `diagnostics`, index-aligned.
    ///
    /// Captured once at analysis time, where the file text is already loaded, so
    /// the Code Quality reporter can build a line-shift-stable fingerprint without
    /// re-reading the file (a third read of every module) or rescanning its text
    /// per finding. Empty when the producer did not supply snippets; the reporter
    /// then falls back to a path/code/occurrence fingerprint.
    pub line_snippets: Vec<String>,

    /// Ordinal of each diagnostic among identical ones in its file, index-aligned with
    /// `diagnostics` and captured BEFORE any suppression. The Code Quality fingerprint
    /// folds it in, and a reporter counting it over the already-suppressed list would
    /// hand an active finding the fingerprint of a suppressed one — the very value the
    /// baseline file holds for a different finding. Empty when the producer did not
    /// supply it; the reporter then counts positions itself.
    pub occurrences: Vec<u32>,
}

impl FileAnalysis {
    /// Drops findings by index, moving every parallel vector with them.
    ///
    /// `occurrences` holds the ordinal a finding had in the FULL set. A filter must
    /// prune it alongside the findings and never renumber: left behind, it shifts
    /// against `diagnostics` and hands a survivor the ordinal — and so the Code
    /// Quality fingerprint — of a finding that was dropped. `line_snippets` and
    /// `occurrences` are empty when the producer supplied neither, and stay empty.
    pub fn retain_findings(&mut self, mut keep: impl FnMut(usize) -> bool) {
        let kept: Vec<bool> = (0..self.diagnostics.len()).map(&mut keep).collect();
        let retain_aligned = |values: &mut Vec<String>| {
            let mut index = 0;
            values.retain(|_| {
                let keep = kept.get(index).copied().unwrap_or(true);
                index += 1;
                keep
            });
        };
        retain_aligned(&mut self.line_snippets);
        let mut index = 0;
        self.occurrences.retain(|_| {
            let keep = kept.get(index).copied().unwrap_or(true);
            index += 1;
            keep
        });
        let mut index = 0;
        self.diagnostics.retain(|_| {
            let keep = kept[index];
            index += 1;
            keep
        });
    }
}

pub trait Reporter: Send + Sync {
    fn key(&self) -> &'static str;

    fn report(&self, results: &AnalysisResults, output_dir: &Path) -> anyhow::Result<()>;
}

pub struct ReporterRegistry {
    reporters: Vec<Box<dyn Reporter>>,
}

impl ReporterRegistry {
    pub fn new() -> Self {
        Self {
            reporters: vec![
                Box::new(console::ConsoleReporter),
                Box::new(json::JsonReporter),
                Box::new(sarif::SarifReporter),
                Box::new(codequality::CodeQualityReporter),
                Box::new(junit::JunitReporter),
            ],
        }
    }

    pub fn get(&self, key: &str) -> Option<&dyn Reporter> {
        self.reporters.iter().find(|r| r.key() == key).map(|r| r.as_ref())
    }

    pub fn keys(&self) -> Vec<&'static str> {
        self.reporters.iter().map(|r| r.key()).collect()
    }
}

impl Default for ReporterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::ReporterRegistry;

    #[test]
    fn registry_contains_expected_reporters() {
        let registry = ReporterRegistry::new();
        let keys = registry.keys();
        for expected in ["console", "json", "sarif", "codequality", "junit"] {
            assert!(keys.contains(&expected), "missing reporter: {expected}");
        }
    }
}
