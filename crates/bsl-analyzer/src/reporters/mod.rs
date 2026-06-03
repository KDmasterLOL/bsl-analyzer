use std::path::{Path, PathBuf};

use ide::DiagnosticOutput;

pub mod console;
pub mod json;
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
}

#[derive(Debug, Clone)]
pub struct FileAnalysis {
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub diagnostics: Vec<DiagnosticOutput>,
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
    fn registry_contains_sarif_reporter() {
        let registry = ReporterRegistry::new();
        assert!(registry.keys().contains(&"sarif"));
    }
}
