//! Project model for bsl-analyzer.
//!
//! This crate handles project structure and configuration.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// A BSL project.
#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub config: ProjectConfig,
}

impl Project {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let config = ProjectConfig::load(&root).unwrap_or_default();
        Self { root, config }
    }
}

/// Project configuration (from .bsl-analyzer.json or .bsl-language-server.json).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    #[serde(default)]
    pub diagnostics: DiagnosticsConfig,

    #[serde(default)]
    pub code_lens: CodeLensConfig,

    #[serde(default)]
    pub formatting: FormattingConfig,

    #[serde(default)]
    pub configuration_root: Option<String>,

    #[serde(default)]
    pub language: Option<String>,
}

impl ProjectConfig {
    /// Loads configuration from .bsl-analyzer.json (priority) or .bsl-language-server.json (fallback for compatibility).
    pub fn load(root: &Path) -> Option<Self> {
        Self::try_load(root, ".bsl-analyzer.json")
            .or_else(|| Self::try_load(root, ".bsl-language-server.json"))
    }

    fn try_load(root: &Path, filename: &str) -> Option<Self> {
        let config_path = root.join(filename);
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path).ok()?;
            serde_json::from_str(&content).ok()
        } else {
            None
        }
    }

    pub fn configuration_path(&self, project_root: &Path) -> Option<PathBuf> {
        self.configuration_root.as_ref().map(|root| project_root.join(root))
    }
}

/// Diagnostics configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DiagnosticsConfig {
    #[serde(default)]
    pub skip: Vec<String>,

    #[serde(default)]
    pub parameters: std::collections::HashMap<String, serde_json::Value>,
}

/// Code Lens configuration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeLensConfig {
    #[serde(default)]
    pub show_cognitive_complexity: bool,

    #[serde(default)]
    pub show_cyclomatic_complexity: bool,
}

/// Formatting configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct FormattingConfig {
    #[serde(default = "default_indent_size")]
    pub indent_size: u32,

    #[serde(default)]
    pub use_tabs: bool,
}

fn default_indent_size() -> u32 {
    4
}
