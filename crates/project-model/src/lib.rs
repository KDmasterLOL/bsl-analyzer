//! Project model for bsl-analyzer.
//!
//! This crate handles project structure and configuration.
//!
//! The [`Project`] struct is the main entry point. It automatically discovers
//! the 1C configuration path using multiple strategies:
//! 1. `configurationRoot` from .bsl-analyzer.json or .bsl-language-server.json
//! 2. Search for Configuration.xml (max depth 2)
//! 3. Common patterns: src/cf, Configuration
//! 4. Fallback to project root

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// A BSL project.
#[derive(Debug, Clone)]
pub struct Project {
    /// Project root directory (workspace root).
    pub root: PathBuf,
    /// Loaded configuration from .bsl-analyzer.json or .bsl-language-server.json.
    pub config: ProjectConfig,
    /// Path to 1C configuration directory (containing Configuration.xml).
    /// This is computed automatically using multiple discovery strategies.
    source_path: Option<PathBuf>,
}

impl Project {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let config = ProjectConfig::load(&root).unwrap_or_default();
        let source_path = Self::discover_source_path(&root, &config);
        Self { root, config, source_path }
    }

    /// Returns the path to scan for BSL/OS source files.
    ///
    /// This is the directory containing 1C configuration (with Configuration.xml)
    /// or the project root if no configuration was found.
    pub fn source_path(&self) -> &Path {
        self.source_path.as_deref().unwrap_or(&self.root)
    }

    /// Returns the path to 1C configuration directory if found.
    ///
    /// Returns `None` if no Configuration.xml was discovered.
    pub fn configuration_path(&self) -> Option<&Path> {
        self.source_path.as_deref()
    }

    /// Discovers the source path using multiple strategies.
    fn discover_source_path(root: &Path, config: &ProjectConfig) -> Option<PathBuf> {
        // Strategy 1: Use configurationRoot from config file
        if let Some(ref config_root) = config.configuration_root {
            let path = root.join(config_root);
            if path.join("Configuration.xml").exists() {
                tracing::info!(?path, "found configuration from configurationRoot setting");
                return Some(path);
            } else {
                tracing::warn!(
                    config_root,
                    ?path,
                    "configurationRoot specified but Configuration.xml not found"
                );
            }
        }

        // Strategy 2: Search for Configuration.xml (max depth 2)
        if let Some(path) = search_configuration_xml(root, 2) {
            tracing::info!(?path, "found Configuration.xml by search");
            return Some(path);
        }

        // Strategy 3: Try common patterns
        for pattern in &["src/cf", "Configuration"] {
            let path = root.join(pattern);
            if path.join("Configuration.xml").exists() {
                tracing::info!(?path, pattern, "found configuration using common pattern");
                return Some(path);
            }
        }

        tracing::debug!(?root, "no 1C configuration found, will use project root");
        None
    }
}

/// Searches for Configuration.xml recursively up to max_depth.
fn search_configuration_xml(root: &Path, max_depth: usize) -> Option<PathBuf> {
    search_configuration_xml_recursive(root, max_depth, 0)
}

fn search_configuration_xml_recursive(
    dir: &Path,
    max_depth: usize,
    current_depth: usize,
) -> Option<PathBuf> {
    if current_depth > max_depth {
        return None;
    }

    // Check if Configuration.xml exists in current directory
    if dir.join("Configuration.xml").exists() {
        return Some(dir.to_path_buf());
    }

    // If we haven't reached max depth, search subdirectories
    if current_depth < max_depth {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if file_type.is_dir() {
                        let name = entry.file_name();
                        // Skip hidden and common non-source directories
                        if !name.to_string_lossy().starts_with('.') {
                            if let Some(path) = search_configuration_xml_recursive(
                                &entry.path(),
                                max_depth,
                                current_depth + 1,
                            ) {
                                return Some(path);
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

/// Project configuration (from .bsl-analyzer.json or .bsl-language-server.json).
///
/// ## JSON Format (bsl-language-server compatible)
///
/// ```json
/// {
///   "configurationRoot": "src/cf",
///   "diagnostics": {
///     "ordinaryAppSupport": false,
///     "dataflowMaxIterations": 10000,
///     "parameters": {
///       "EmptyCodeBlock": false,
///       "LineLength": { "maxLength": 120 }
///     }
///   }
/// }
/// ```
///
/// The `diagnostics` field is stored as raw JSON and parsed by `ide_diagnostics::DiagnosticsConfig`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
    /// Raw diagnostics configuration.
    ///
    /// Stored as raw JSON to avoid coupling project_model to ide_diagnostics.
    /// Convert to `ide_diagnostics::DiagnosticsConfig` via:
    /// ```ignore
    /// let config: DiagnosticsConfig = serde_json::from_value(proj_config.diagnostics.clone())
    ///     .unwrap_or_default();
    /// ```
    #[serde(default)]
    pub diagnostics: serde_json::Value,

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

    /// Load configuration from a specific file path.
    pub fn load_from_file(path: &Path) -> Option<Self> {
        if path.exists() {
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "failed to read config file");
                    return None;
                }
            };
            match serde_json::from_str(&content) {
                Ok(config) => Some(config),
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "failed to parse config file");
                    None
                }
            }
        } else {
            None
        }
    }

    fn try_load(root: &Path, filename: &str) -> Option<Self> {
        let config_path = root.join(filename);
        if config_path.exists() {
            match Self::load_from_file(&config_path) {
                Some(config) => {
                    tracing::info!(
                        path = %config_path.display(),
                        diagnostics_has_content = !config.diagnostics.is_null(),
                        "loaded project config"
                    );
                    Some(config)
                }
                None => {
                    tracing::warn!(
                        path = %config_path.display(),
                        "config file exists but failed to parse"
                    );
                    None
                }
            }
        } else {
            None
        }
    }

    pub fn configuration_path(&self, project_root: &Path) -> Option<PathBuf> {
        self.configuration_root.as_ref().map(|root| project_root.join(root))
    }

    /// Load 1C metadata (Configuration.xml, CommonModules, etc.) from configuration root.
    ///
    /// Returns `None` if:
    /// - No `configurationRoot` specified in config
    /// - Configuration root directory doesn't exist
    /// - Failed to parse metadata files
    pub fn load_metadata(&self, workspace_root: &Path) -> Option<bsl_metadata::Configuration> {
        let cfg_path = self.configuration_path(workspace_root)?;

        if !cfg_path.exists() {
            tracing::warn!(path = ?cfg_path, "Configuration root not found");
            return None;
        }

        tracing::info!(path = ?cfg_path, "Loading 1C metadata");
        let start = std::time::Instant::now();

        match bsl_metadata::load_from_directory(&cfg_path) {
            Ok(config) => {
                let elapsed = start.elapsed();
                tracing::info!(
                    elapsed_ms = elapsed.as_millis(),
                    common_modules = config.common_modules().len(),
                    "1C metadata loaded"
                );
                Some(config)
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to load 1C metadata");
                None
            }
        }
    }
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
