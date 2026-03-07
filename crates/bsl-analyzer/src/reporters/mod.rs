//! Reporter system for analysis results.
//!
//! This module provides a pluggable reporter system for outputting analysis results
//! in various formats (console, JSON, SARIF, etc.).

use std::path::{Path, PathBuf};

use ide::DiagnosticOutput;

pub mod console;
pub mod json;

/// Analysis results passed to reporters.
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

/// Analysis results for a single file.
#[derive(Debug, Clone)]
pub struct FileAnalysis {
    pub path: PathBuf,
    pub relative_path: PathBuf, // Relative to workspace
    pub diagnostics: Vec<DiagnosticOutput>,
}

/// Reporter trait for generating analysis reports.
pub trait Reporter: Send + Sync {
    /// Unique identifier for this reporter (e.g., "json", "sarif").
    fn key(&self) -> &'static str;

    /// Generate report from analysis results.
    fn report(&self, results: &AnalysisResults, output_dir: &Path) -> anyhow::Result<()>;
}

/// Factory for creating and managing reporters.
pub struct ReporterRegistry {
    reporters: Vec<Box<dyn Reporter>>,
}

impl ReporterRegistry {
    /// Create a new reporter registry with all available reporters.
    pub fn new() -> Self {
        Self {
            reporters: vec![
                Box::new(console::ConsoleReporter),
                Box::new(json::JsonReporter),
                // Phase 2: SARIF, Generic
                // Phase 3: TSLint, JUnit, CodeQuality
            ],
        }
    }

    /// Get a reporter by key.
    pub fn get(&self, key: &str) -> Option<&dyn Reporter> {
        self.reporters.iter().find(|r| r.key() == key).map(|r| r.as_ref())
    }

    /// Get all available reporter keys.
    pub fn keys(&self) -> Vec<&'static str> {
        self.reporters.iter().map(|r| r.key()).collect()
    }
}

impl Default for ReporterRegistry {
    fn default() -> Self {
        Self::new()
    }
}
