//! Project model for bsl-analyzer.
//!
//! This crate handles project structure and configuration.
//!
//! The [`Project`] struct is the main entry point. It automatically discovers
//! the 1C configuration path using multiple strategies:
//! 1. `source.root` from bsl-analyzer.toml, or `configurationRoot` from .bsl-analyzer.json / .bsl-language-server.json
//! 2. Search for Configuration.xml (max depth 2)
//! 3. Common patterns: src/cf, Configuration
//! 4. Fallback to project root

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
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
    /// Resolved extension paths: (name, absolute_path).
    /// Each extension must contain Configuration.xml.
    extension_paths: Vec<(String, PathBuf)>,
}

impl Project {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let config = ProjectConfig::load(&root).unwrap_or_default();
        let source_path = Self::discover_source_path(&root, &config);
        let extension_paths = Self::resolve_extensions(&root, &config);
        Self { root, config, source_path, extension_paths }
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

    /// Returns resolved extension paths as (name, path) pairs.
    pub fn extension_paths(&self) -> &[(String, PathBuf)] {
        &self.extension_paths
    }

    /// Resolves extension paths from config, filtering to those that exist
    /// and contain Configuration.xml.
    fn resolve_extensions(root: &Path, config: &ProjectConfig) -> Vec<(String, PathBuf)> {
        config
            .extensions
            .iter()
            .filter_map(|ext_path_str| {
                let path = root.join(ext_path_str);
                if !path.exists() {
                    tracing::warn!(path = %path.display(), "extension path not found, skipping");
                    return None;
                }
                if !path.join("Configuration.xml").exists() {
                    tracing::warn!(
                        path = %path.display(),
                        "extension has no Configuration.xml, skipping"
                    );
                    return None;
                }
                // Derive name from last path component
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| ext_path_str.clone());
                tracing::info!(name = %name, path = %path.display(), "resolved extension");
                Some((name, path))
            })
            .collect()
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

    /// Extension paths relative to project root.
    ///
    /// Each path should point to an extension directory containing Configuration.xml.
    /// Extensions are loaded as separate configurations visible in the shared context.
    ///
    /// ```json
    /// { "extensions": ["src/cfe/BMS_RU_UT", "src/cfe/YAxUnit"] }
    /// ```
    #[serde(default)]
    pub extensions: Vec<String>,

    #[serde(default)]
    pub search: SearchConfig,

    #[serde(skip)]
    pub postgres_credentials: Option<PostgresCredentialConfig>,
}

impl ProjectConfig {
    /// Loads configuration with priority: `bsl-analyzer.toml` > `.bsl-analyzer.json` > `.bsl-language-server.json`.
    ///
    /// If `bsl-analyzer.toml` exists but fails to parse, JSON files are NOT consulted —
    /// the parse error is logged and `None` is returned so callers get defaults
    /// instead of silently falling back to stale JSON.
    pub fn load(root: &Path) -> Option<Self> {
        let toml_path = root.join("bsl-analyzer.toml");
        if toml_path.exists() {
            return Self::try_load_toml_file(&toml_path);
        }
        Self::try_load(root, ".bsl-analyzer.json")
            .or_else(|| Self::try_load(root, ".bsl-language-server.json"))
    }

    fn try_load_toml_file(config_path: &Path) -> Option<Self> {
        let content = match std::fs::read_to_string(config_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(path = %config_path.display(), error = %e, "failed to read TOML config");
                return None;
            }
        };
        match toml::from_str::<TomlConfig>(&content) {
            Ok(toml_config) => {
                let config = ProjectConfig::from(toml_config);
                tracing::info!(
                    path = %config_path.display(),
                    diagnostics_has_content = !config.diagnostics.is_null(),
                    "loaded TOML project config"
                );
                Some(config)
            }
            Err(e) => {
                tracing::error!(
                    path = %config_path.display(),
                    error = %e,
                    "bsl-analyzer.toml exists but failed to parse; JSON fallback is disabled"
                );
                None
            }
        }
    }

    /// Load configuration from a specific file path.
    pub fn load_from_file(path: &Path) -> Option<Self> {
        if !path.exists() {
            return None;
        }
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "failed to read config file");
                return None;
            }
        };
        if path.extension().is_some_and(|ext| ext == "toml") {
            match toml::from_str::<TomlConfig>(&content) {
                Ok(toml_config) => Some(ProjectConfig::from(toml_config)),
                Err(e) => {
                    tracing::error!(path = %path.display(), error = %e, "failed to parse TOML config");
                    None
                }
            }
        } else {
            match serde_json::from_str(&content) {
                Ok(config) => Some(config),
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "failed to parse config file");
                    None
                }
            }
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
    #[serde(default, alias = "show_cognitive_complexity")]
    pub show_cognitive_complexity: bool,

    #[serde(default, alias = "show_cyclomatic_complexity")]
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

/// Search subsystem configuration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchConfig {
    #[serde(default)]
    pub baseline: SearchBaselineConfig,
}

/// Shared baseline configuration for centralized search backends.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchBaselineConfig {
    #[serde(default)]
    pub backend: SearchBaselineBackend,

    #[serde(default)]
    pub postgres: SearchPostgresConfig,

    #[serde(default, alias = "workspace_code")]
    pub workspace_code: SearchBaselineTargetConfig,

    #[serde(default)]
    pub reference: SearchBaselineTargetConfig,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SearchBaselineBackend {
    #[default]
    Sqlite,
    Postgres,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPostgresConfig {
    #[serde(default)]
    pub url: Option<String>,

    #[serde(default)]
    pub schema: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchBaselineTargetConfig {
    #[serde(default, alias = "snapshot_id")]
    pub snapshot_id: Option<String>,

    #[serde(default)]
    pub branch: Option<String>,

    #[serde(default)]
    pub commit: Option<String>,

    #[serde(default)]
    pub policy: SearchBaselinePolicyConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchBaselinePolicyConfig {
    #[serde(default, alias = "publish_branches")]
    pub publish_branches: Vec<String>,

    #[serde(default)]
    pub branches: Vec<SearchBaselineBranchPolicyRuleConfig>,

    #[serde(default)]
    pub support: SearchBaselineSupportConfig,

    #[serde(default)]
    pub retention: SearchBaselineRetentionConfig,
}

impl SearchBaselinePolicyConfig {
    pub fn is_configured(&self) -> bool {
        !self.publish_branches.is_empty() || !self.branches.is_empty()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchBaselineSupportConfig {
    #[serde(default = "default_workspace_stale_after_days", alias = "stale_after_days")]
    pub stale_after_days: u32,

    #[serde(default = "default_workspace_expire_after_days", alias = "expire_after_days")]
    pub expire_after_days: u32,
}

impl Default for SearchBaselineSupportConfig {
    fn default() -> Self {
        Self {
            stale_after_days: default_workspace_stale_after_days(),
            expire_after_days: default_workspace_expire_after_days(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchBaselineRetentionConfig {
    #[serde(default = "default_develop_retention_days", alias = "develop_retention_days")]
    pub develop_retention_days: u32,

    #[serde(default = "default_vendor_keep_heads", alias = "vendor_keep_heads")]
    pub vendor_keep_heads: usize,

    #[serde(default = "default_min_snapshots_per_branch", alias = "min_snapshots_per_branch")]
    pub min_snapshots_per_branch: usize,
}

impl Default for SearchBaselineRetentionConfig {
    fn default() -> Self {
        Self {
            develop_retention_days: default_develop_retention_days(),
            vendor_keep_heads: default_vendor_keep_heads(),
            min_snapshots_per_branch: default_min_snapshots_per_branch(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchBaselineSupportState {
    Supported,
    Stale,
    Expired,
}

impl SearchBaselineSupportState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Stale => "stale",
            Self::Expired => "expired",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWorkspaceBaselineSupport {
    pub state: SearchBaselineSupportState,
    pub workspace_branch: Option<String>,
    pub selected_branch: Option<String>,
    pub snapshot_age_days: u32,
    pub stale_after_days: u32,
    pub expire_after_days: u32,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchBaselineBranchPolicyRuleConfig {
    #[serde(rename = "match")]
    pub pattern: String,

    #[serde(alias = "select_branch")]
    pub select_branch: String,

    #[serde(default, alias = "fallback_branch")]
    pub fallback_branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedWorkspaceBranchPolicy {
    pub workspace_branch: Option<String>,
    pub matched_pattern: String,
    pub select_branch: String,
    pub fallback_branch: Option<String>,
}

impl ResolvedWorkspaceBranchPolicy {
    pub fn candidate_branches(&self) -> Vec<String> {
        let mut branches = vec![self.select_branch.clone()];
        if let Some(fallback_branch) = &self.fallback_branch {
            if branches.iter().all(|branch| branch != fallback_branch) {
                branches.push(fallback_branch.clone());
            }
        }
        branches
    }

    pub fn selection_description(&self) -> String {
        let workspace_branch = self.workspace_branch.as_deref().unwrap_or("<unknown>");
        let chain = self
            .candidate_branches()
            .into_iter()
            .map(|branch| format!("branch {branch}"))
            .collect::<Vec<_>>()
            .join(" -> ");
        format!("workspace branch {workspace_branch} -> {chain}")
    }
}

pub fn resolve_workspace_branch_policy(
    policy: &SearchBaselinePolicyConfig,
    workspace_branch: Option<&str>,
) -> Option<ResolvedWorkspaceBranchPolicy> {
    let workspace_branch =
        workspace_branch.map(str::trim).filter(|branch| !branch.is_empty()).map(ToOwned::to_owned);
    let rule = policy
        .branches
        .iter()
        .find(|rule| branch_pattern_matches(&rule.pattern, workspace_branch.as_deref()))?;

    Some(ResolvedWorkspaceBranchPolicy {
        workspace_branch,
        matched_pattern: rule.pattern.clone(),
        select_branch: rule.select_branch.clone(),
        fallback_branch: rule.fallback_branch.clone(),
    })
}

pub fn is_publish_branch_allowed(policy: &SearchBaselinePolicyConfig, branch: &str) -> bool {
    let branch = branch.trim();
    !branch.is_empty()
        && policy
            .publish_branches
            .iter()
            .any(|pattern| branch_pattern_matches(pattern, Some(branch)))
}

pub fn branch_pattern_matches(pattern: &str, branch: Option<&str>) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    if pattern == "*" {
        return true;
    }

    let Some(branch) = branch.map(str::trim).filter(|branch| !branch.is_empty()) else {
        return false;
    };

    if let Some(prefix) = pattern.strip_suffix("/*") {
        return branch
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/') && suffix.len() > 1);
    }

    branch == pattern
}

pub fn current_git_branch(start_dir: &Path) -> Option<String> {
    let git_dir = discover_git_dir(start_dir)?;
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let ref_path = head.trim().strip_prefix("ref: ")?;
    ref_path.strip_prefix("refs/heads/").map(ToOwned::to_owned)
}

pub fn parse_timestamp_utc(value: &str) -> Option<DateTime<Utc>> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    DateTime::parse_from_rfc3339(value)
        .or_else(|_| DateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f%:z"))
        .or_else(|_| DateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%:z"))
        .map(|value| value.with_timezone(&Utc))
        .ok()
}

pub fn evaluate_workspace_baseline_support_now(
    policy: &SearchBaselinePolicyConfig,
    workspace_branch: Option<&str>,
    selected_branch: Option<&str>,
    snapshot_created_at: Option<DateTime<Utc>>,
) -> Option<ResolvedWorkspaceBaselineSupport> {
    evaluate_workspace_baseline_support(
        policy,
        workspace_branch,
        selected_branch,
        snapshot_created_at,
        Utc::now(),
    )
}

pub fn evaluate_workspace_baseline_support(
    policy: &SearchBaselinePolicyConfig,
    workspace_branch: Option<&str>,
    selected_branch: Option<&str>,
    snapshot_created_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<ResolvedWorkspaceBaselineSupport> {
    if !policy.is_configured() {
        return None;
    }

    let snapshot_created_at = snapshot_created_at?;
    let selected_branch =
        selected_branch.map(str::trim).filter(|branch| !branch.is_empty()).map(ToOwned::to_owned);
    let workspace_branch =
        workspace_branch.map(str::trim).filter(|branch| !branch.is_empty()).map(ToOwned::to_owned);

    let stale_after_days = policy.support.stale_after_days.min(policy.support.expire_after_days);
    let expire_after_days = policy.support.expire_after_days.max(stale_after_days);
    let age_days = now.signed_duration_since(snapshot_created_at).num_days().max(0) as u32;
    let state = if age_days >= expire_after_days {
        SearchBaselineSupportState::Expired
    } else if age_days >= stale_after_days {
        SearchBaselineSupportState::Stale
    } else {
        SearchBaselineSupportState::Supported
    };

    let reason = match (workspace_branch.as_deref(), selected_branch.as_deref()) {
        (Some(workspace_branch), Some(selected_branch)) if workspace_branch != selected_branch => {
            format!(
                "workspace branch '{workspace_branch}' uses shared baseline branch '{selected_branch}' published {age_days} days ago"
            )
        }
        (Some(workspace_branch), _) => {
            format!("workspace branch '{workspace_branch}' uses a shared baseline published {age_days} days ago")
        }
        (None, Some(selected_branch)) => {
            format!("shared baseline branch '{selected_branch}' was published {age_days} days ago")
        }
        (None, None) => format!("shared baseline was published {age_days} days ago"),
    };

    Some(ResolvedWorkspaceBaselineSupport {
        state,
        workspace_branch,
        selected_branch,
        snapshot_age_days: age_days,
        stale_after_days,
        expire_after_days,
        reason,
    })
}

fn default_workspace_stale_after_days() -> u32 {
    21
}

fn default_workspace_expire_after_days() -> u32 {
    30
}

fn default_develop_retention_days() -> u32 {
    30
}

fn default_vendor_keep_heads() -> usize {
    2
}

fn default_min_snapshots_per_branch() -> usize {
    1
}

fn discover_git_dir(start_dir: &Path) -> Option<PathBuf> {
    for candidate in start_dir.ancestors() {
        let dot_git = candidate.join(".git");
        if dot_git.is_dir() {
            return Some(dot_git);
        }
        if dot_git.is_file() {
            let content = std::fs::read_to_string(&dot_git).ok()?;
            let path = content.trim().strip_prefix("gitdir: ")?;
            let git_dir = candidate.join(path);
            if git_dir.exists() {
                return Some(git_dir);
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// TOML configuration (bsl-analyzer.toml)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct TomlConfig {
    #[serde(default)]
    source: TomlSourceConfig,
    #[serde(default = "default_toml_table")]
    diagnostics: toml::Value,
    #[serde(default)]
    code_lens: CodeLensConfig,
    #[serde(default)]
    formatting: FormattingConfig,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    search: TomlSearchConfig,
}

impl Default for TomlConfig {
    fn default() -> Self {
        Self {
            source: TomlSourceConfig::default(),
            diagnostics: default_toml_table(),
            code_lens: CodeLensConfig::default(),
            formatting: FormattingConfig::default(),
            extensions: Vec::new(),
            search: TomlSearchConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct TomlSourceConfig {
    #[serde(default)]
    root: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct TomlSearchConfig {
    #[serde(default)]
    baseline: TomlSearchBaselineConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct TomlSearchBaselineConfig {
    #[serde(default)]
    backend: SearchBaselineBackend,
    #[serde(default)]
    postgres: TomlSearchPostgresConfig,
    #[serde(default)]
    workspace_code: SearchBaselineTargetConfig,
    #[serde(default)]
    reference: SearchBaselineTargetConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct TomlSearchPostgresConfig {
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    dbname: Option<String>,
    #[serde(default)]
    schema: Option<String>,
    #[serde(default)]
    url_env: Option<String>,
    #[serde(default)]
    url_file: Option<String>,
    #[serde(default)]
    url_command: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    credential_helper: Option<String>,
}

fn default_toml_table() -> toml::Value {
    toml::Value::Table(Default::default())
}

/// Credential resolution configuration for PostgreSQL connections.
///
/// Resolution priority:
/// 1. `url_env` — read from named environment variable
/// 2. `url_file` — read URL from file (tilde-expanded)
/// 3. `url_command` — execute shell command, read stdout
/// 4. `url` — plaintext URL
/// 5. Build from `host`/`port`/`dbname` + `credential_helper`
#[derive(Debug, Clone, Default)]
pub struct PostgresCredentialConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub dbname: Option<String>,
    pub url_env: Option<String>,
    pub url_file: Option<String>,
    pub url_command: Option<String>,
    pub url: Option<String>,
    pub credential_helper: Option<String>,
}

impl From<TomlConfig> for ProjectConfig {
    fn from(toml: TomlConfig) -> Self {
        let diagnostics = toml_value_to_json(toml.diagnostics);
        let pg = &toml.search.baseline.postgres;
        let has_any_pg_field = pg.host.is_some()
            || pg.url.is_some()
            || pg.url_env.is_some()
            || pg.url_file.is_some()
            || pg.url_command.is_some()
            || pg.credential_helper.is_some();
        let postgres_credentials = if has_any_pg_field {
            Some(PostgresCredentialConfig {
                host: pg.host.clone(),
                port: pg.port,
                dbname: pg.dbname.clone(),
                url_env: pg.url_env.clone(),
                url_file: pg.url_file.clone(),
                url_command: pg.url_command.clone(),
                url: pg.url.clone(),
                credential_helper: pg.credential_helper.clone(),
            })
        } else {
            None
        };
        Self {
            diagnostics,
            code_lens: toml.code_lens,
            formatting: toml.formatting,
            configuration_root: toml.source.root,
            language: None,
            extensions: toml.extensions,
            search: SearchConfig {
                baseline: SearchBaselineConfig {
                    backend: toml.search.baseline.backend,
                    postgres: SearchPostgresConfig {
                        url: pg.url.clone(),
                        schema: pg.schema.clone(),
                    },
                    workspace_code: toml.search.baseline.workspace_code,
                    reference: toml.search.baseline.reference,
                },
            },
            postgres_credentials,
        }
    }
}

fn toml_value_to_json(value: toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s),
        toml::Value::Integer(i) => serde_json::json!(i),
        toml::Value::Float(f) => serde_json::json!(f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(b),
        toml::Value::Datetime(d) => serde_json::Value::String(d.to_string()),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(toml_value_to_json).collect())
        }
        toml::Value::Table(table) => {
            let map = table.into_iter().map(|(k, v)| (k, toml_value_to_json(v))).collect();
            serde_json::Value::Object(map)
        }
    }
}

/// Resolves a PostgreSQL connection URL using the credential resolver chain.
pub fn resolve_postgres_url(creds: &PostgresCredentialConfig) -> Option<String> {
    // 1. url_env
    if let Some(ref key) = creds.url_env {
        if let Ok(val) = std::env::var(key) {
            let val = val.trim().to_owned();
            if !val.is_empty() {
                tracing::debug!(env_var = key, "resolved PG URL from url_env");
                return Some(val);
            }
        }
    }
    // 2. url_file
    if let Some(ref path) = creds.url_file {
        let expanded = expand_tilde(path);
        if let Ok(content) = std::fs::read_to_string(&expanded) {
            let url = content.trim().to_owned();
            if !url.is_empty() {
                tracing::debug!(path = %expanded.display(), "resolved PG URL from url_file");
                return Some(url);
            }
        }
    }
    // 3. url_command (Unix only, executes shell — trust the config source)
    #[cfg(unix)]
    if let Some(ref cmd) = creds.url_command {
        tracing::info!(command = cmd, "executing url_command from config");
        match run_command_with_timeout(cmd, COMMAND_TIMEOUT) {
            Ok(url) if !url.is_empty() => {
                tracing::debug!("resolved PG URL from url_command");
                return Some(url);
            }
            Ok(_) => {
                tracing::warn!(command = cmd, "url_command returned empty output");
            }
            Err(e) => {
                tracing::warn!(command = cmd, error = %e, "url_command failed");
            }
        }
    }
    // 4. url (plaintext)
    if let Some(ref url) = creds.url {
        let url = url.trim();
        if !url.is_empty() {
            return Some(url.to_owned());
        }
    }
    // 5. host/port/dbname + credential_helper (custom protocol, not git-credential)
    if let (Some(ref host), Some(ref dbname)) = (&creds.host, &creds.dbname) {
        let port = creds.port.unwrap_or(5432);
        if let Some(ref helper) = creds.credential_helper {
            tracing::info!(command = helper, "executing credential_helper from config");
            match run_credential_helper(helper, host, port, dbname) {
                Ok((username, password)) => {
                    let url = format!(
                        "postgres://{}:{}@{}:{}/{}",
                        percent_encode(&username),
                        percent_encode(&password),
                        host,
                        port,
                        dbname
                    );
                    tracing::debug!("resolved PG URL from credential_helper");
                    return Some(url);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "credential_helper failed");
                }
            }
        } else {
            return Some(format!("postgres://{}:{}/{}", host, port, dbname));
        }
    }
    None
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

/// Timeout for external commands (`url_command`, `credential_helper`).
#[cfg(unix)]
const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Runs a shell command with a timeout, returns trimmed stdout.
#[cfg(unix)]
fn run_command_with_timeout(command: &str, timeout: std::time::Duration) -> Result<String, String> {
    use std::process::{Command, Stdio};

    let mut child = Command::new("sh")
        .args(["-c", command])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn: {e}"))?;

    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output =
                    child.wait_with_output().map_err(|e| format!("read output failed: {e}"))?;
                if !status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(format!("exited with {status}: {stderr}"));
                }
                return Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned());
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    return Err(format!("timed out after {}s", timeout.as_secs()));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => return Err(format!("wait failed: {e}")),
        }
    }
}

/// Runs a credential helper (custom bsl-analyzer protocol, not git-credential).
///
/// Sends `host=…\nport=…\ndbname=…\n\n` on stdin.
/// Expects `username=…\npassword=…\n` on stdout.
/// Times out after [`COMMAND_TIMEOUT`].
#[cfg(unix)]
fn run_credential_helper(
    command: &str,
    host: &str,
    port: u16,
    dbname: &str,
) -> Result<(String, String), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("sh")
        .args(["-c", command])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn credential helper: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = write!(stdin, "host={host}\nport={port}\ndbname={dbname}\n\n");
    }

    let deadline = std::time::Instant::now() + COMMAND_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|e| format!("credential helper read failed: {e}"))?;
                if !status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(format!(
                        "credential helper exited with {}: {stderr}",
                        output.status
                    ));
                }
                let stdout = String::from_utf8_lossy(&output.stdout);
                return parse_credential_output(&stdout);
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    return Err(format!(
                        "credential helper timed out after {}s",
                        COMMAND_TIMEOUT.as_secs()
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => return Err(format!("credential helper wait failed: {e}")),
        }
    }
}

fn parse_credential_output(stdout: &str) -> Result<(String, String), String> {
    let mut username = None;
    let mut password = None;
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "username" => username = Some(value.trim().to_owned()),
                "password" => password = Some(value.trim().to_owned()),
                _ => {}
            }
        }
    }

    match (username, password) {
        (Some(u), Some(p)) => Ok((u, p)),
        _ => Err("credential helper did not return username and password".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        branch_pattern_matches, current_git_branch, evaluate_workspace_baseline_support,
        is_publish_branch_allowed, parse_timestamp_utc, resolve_postgres_url,
        resolve_workspace_branch_policy, ProjectConfig, SearchBaselineBackend,
        SearchBaselinePolicyConfig, SearchBaselineSupportState,
    };
    use chrono::{Duration, TimeZone, Utc};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    #[test]
    fn project_config_defaults_search_baseline_to_sqlite() {
        let config: ProjectConfig = serde_json::from_str("{}").unwrap();

        assert_eq!(config.search.baseline.backend, SearchBaselineBackend::Sqlite);
        assert!(config.search.baseline.postgres.url.is_none());
        assert!(config.search.baseline.workspace_code.branch.is_none());
        assert!(config.search.baseline.reference.snapshot_id.is_none());
    }

    #[test]
    fn project_config_deserializes_search_baseline_settings() {
        let config: ProjectConfig = serde_json::from_str(
            r#"{
                "search": {
                    "baseline": {
                        "backend": "postgres",
                        "postgres": {
                            "url": "postgres://shared-search",
                            "schema": "corp_search"
                        },
                        "workspaceCode": {
                            "branch": "main",
                            "commit": "abc123"
                        },
                        "reference": {
                            "snapshotId": "reference:0.1.104"
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        assert_eq!(config.search.baseline.backend, SearchBaselineBackend::Postgres);
        assert_eq!(
            config.search.baseline.postgres.url.as_deref(),
            Some("postgres://shared-search")
        );
        assert_eq!(config.search.baseline.postgres.schema.as_deref(), Some("corp_search"));
        assert_eq!(config.search.baseline.workspace_code.branch.as_deref(), Some("main"));
        assert_eq!(config.search.baseline.workspace_code.commit.as_deref(), Some("abc123"));
        assert_eq!(
            config.search.baseline.reference.snapshot_id.as_deref(),
            Some("reference:0.1.104")
        );
    }

    #[test]
    fn project_config_deserializes_workspace_branch_policy() {
        let config: ProjectConfig = serde_json::from_str(
            r#"{
                "search": {
                    "baseline": {
                        "workspaceCode": {
                            "policy": {
                                "publishBranches": ["vendor", "develop"],
                                "branches": [
                                    { "match": "vendor", "selectBranch": "vendor" },
                                    {
                                        "match": "feature/*",
                                        "selectBranch": "develop",
                                        "fallbackBranch": "vendor"
                                    },
                                    {
                                        "match": "*",
                                        "selectBranch": "develop",
                                        "fallbackBranch": "vendor"
                                    }
                                ]
                            }
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        let policy = &config.search.baseline.workspace_code.policy;
        assert!(policy.is_configured());
        assert_eq!(policy.publish_branches, vec!["vendor", "develop"]);
        assert_eq!(policy.branches.len(), 3);
        assert_eq!(policy.branches[1].pattern, "feature/*");
        assert_eq!(policy.branches[1].select_branch, "develop");
        assert_eq!(policy.branches[1].fallback_branch.as_deref(), Some("vendor"));
        assert_eq!(policy.support.stale_after_days, 21);
        assert_eq!(policy.support.expire_after_days, 30);
        assert_eq!(policy.retention.develop_retention_days, 30);
        assert_eq!(policy.retention.vendor_keep_heads, 2);
    }

    #[test]
    fn parse_timestamp_supports_postgres_text_format() {
        let parsed = parse_timestamp_utc("2026-04-02 09:01:53.271613+00:00").unwrap();

        assert_eq!(
            parsed,
            Utc.with_ymd_and_hms(2026, 4, 2, 9, 1, 53).unwrap() + Duration::microseconds(271613)
        );
    }

    #[test]
    fn workspace_baseline_support_becomes_stale_and_expired_by_age() {
        let policy: SearchBaselinePolicyConfig = serde_json::from_value(serde_json::json!({
            "publishBranches": ["vendor", "develop"],
            "branches": [{ "match": "*", "selectBranch": "develop", "fallbackBranch": "vendor" }],
            "support": { "staleAfterDays": 10, "expireAfterDays": 20 }
        }))
        .unwrap();
        let now = Utc.with_ymd_and_hms(2026, 4, 2, 12, 0, 0).unwrap();

        let stale = evaluate_workspace_baseline_support(
            &policy,
            Some("feature/demo"),
            Some("develop"),
            Some(now - Duration::days(12)),
            now,
        )
        .unwrap();
        assert_eq!(stale.state, SearchBaselineSupportState::Stale);
        assert!(stale.reason.contains("feature/demo"));
        assert!(stale.reason.contains("develop"));

        let expired = evaluate_workspace_baseline_support(
            &policy,
            Some("feature/demo"),
            Some("develop"),
            Some(now - Duration::days(25)),
            now,
        )
        .unwrap();
        assert_eq!(expired.state, SearchBaselineSupportState::Expired);
        assert_eq!(expired.snapshot_age_days, 25);
    }

    #[test]
    fn workspace_baseline_support_is_none_when_policy_is_not_configured() {
        let now = Utc.with_ymd_and_hms(2026, 4, 2, 12, 0, 0).unwrap();

        assert!(evaluate_workspace_baseline_support(
            &SearchBaselinePolicyConfig::default(),
            Some("feature/demo"),
            Some("develop"),
            Some(now - Duration::days(5)),
            now,
        )
        .is_none());
    }

    #[test]
    fn branch_pattern_matches_exact_prefix_and_wildcard() {
        assert!(branch_pattern_matches("develop", Some("develop")));
        assert!(branch_pattern_matches("feature/*", Some("feature/test")));
        assert!(branch_pattern_matches("*", Some("custom/branch")));
        assert!(!branch_pattern_matches("feature/*", Some("feature")));
        assert!(!branch_pattern_matches("feature/*", Some("fix/test")));
    }

    #[test]
    fn workspace_branch_policy_resolves_branch_chain() {
        let policy = SearchBaselinePolicyConfig {
            publish_branches: vec!["vendor".to_owned(), "develop".to_owned()],
            branches: serde_json::from_value(serde_json::json!([
                { "match": "vendor", "selectBranch": "vendor" },
                { "match": "develop", "selectBranch": "develop", "fallbackBranch": "vendor" },
                { "match": "feature/*", "selectBranch": "develop", "fallbackBranch": "vendor" },
                { "match": "*", "selectBranch": "develop", "fallbackBranch": "vendor" }
            ]))
            .unwrap(),
            ..SearchBaselinePolicyConfig::default()
        };

        let resolved = resolve_workspace_branch_policy(&policy, Some("feature/test")).unwrap();
        assert_eq!(resolved.workspace_branch.as_deref(), Some("feature/test"));
        assert_eq!(resolved.matched_pattern, "feature/*");
        assert_eq!(resolved.candidate_branches(), vec!["develop", "vendor"]);
        assert_eq!(
            resolved.selection_description(),
            "workspace branch feature/test -> branch develop -> branch vendor"
        );
    }

    #[test]
    fn workspace_branch_policy_uses_wildcard_for_unknown_branch() {
        let policy = SearchBaselinePolicyConfig {
            publish_branches: vec![],
            branches: serde_json::from_value(serde_json::json!([
                { "match": "*", "selectBranch": "develop", "fallbackBranch": "vendor" }
            ]))
            .unwrap(),
            ..SearchBaselinePolicyConfig::default()
        };

        let resolved = resolve_workspace_branch_policy(&policy, Some("release/1.0")).unwrap();
        assert_eq!(resolved.matched_pattern, "*");
        assert_eq!(resolved.candidate_branches(), vec!["develop", "vendor"]);
    }

    #[test]
    fn publish_branch_policy_uses_pattern_matching() {
        let policy = SearchBaselinePolicyConfig {
            publish_branches: vec!["vendor".to_owned(), "develop".to_owned()],
            branches: vec![],
            ..SearchBaselinePolicyConfig::default()
        };

        assert!(is_publish_branch_allowed(&policy, "vendor"));
        assert!(is_publish_branch_allowed(&policy, "develop"));
        assert!(!is_publish_branch_allowed(&policy, "feature/test"));
    }

    #[test]
    fn current_git_branch_reads_direct_git_dir() {
        let dir = tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/feature/test\n").unwrap();

        assert_eq!(current_git_branch(dir.path()).as_deref(), Some("feature/test"));
    }

    #[test]
    fn current_git_branch_reads_gitdir_file() {
        let dir = tempdir().unwrap();
        let actual_git_dir = dir.path().join(".git-data");
        fs::create_dir_all(&actual_git_dir).unwrap();
        fs::write(actual_git_dir.join("HEAD"), "ref: refs/heads/develop\n").unwrap();
        fs::write(dir.path().join(".git"), "gitdir: .git-data\n").unwrap();

        assert_eq!(current_git_branch(dir.path()).as_deref(), Some("develop"));
    }

    #[test]
    fn current_git_branch_returns_none_for_detached_head() {
        let dir = tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "0123456789abcdef\n").unwrap();

        assert_eq!(current_git_branch(dir.path()), None);
    }

    #[test]
    fn project_config_defaults_workspace_policy_to_empty() {
        let config: ProjectConfig = serde_json::from_str("{}").unwrap();

        assert!(!config.search.baseline.workspace_code.policy.is_configured());
        assert!(config.search.baseline.reference.policy.branches.is_empty());
    }

    #[test]
    fn toml_config_deserializes_minimal() {
        let config: super::TomlConfig = toml::from_str("").unwrap();
        let project = ProjectConfig::from(config);
        assert_eq!(project.search.baseline.backend, SearchBaselineBackend::Sqlite);
        assert!(project.configuration_root.is_none());
        assert!(project.postgres_credentials.is_none()); // no PG fields in minimal config
    }

    #[test]
    fn toml_config_deserializes_full_baseline() {
        let toml_str = r#"
[source]
root = "src/cf"

[search.baseline]
backend = "postgres"

[search.baseline.postgres]
host = "pg-central.company.com"
port = 5432
dbname = "bsl_search"
schema = "bsl_search"
url_env = "BSL_SEARCH_BASELINE_PG_URL"
credential_helper = "rtools vault credential-helper --engine bsl-search"

[search.baseline.workspace_code]
branch = "develop"

[search.baseline.workspace_code.policy]
publish_branches = ["vendor", "develop"]

[[search.baseline.workspace_code.policy.branches]]
match = "develop"
select_branch = "develop"
fallback_branch = "vendor"

[[search.baseline.workspace_code.policy.branches]]
match = "feature/*"
select_branch = "develop"
fallback_branch = "vendor"
"#;
        let config: super::TomlConfig = toml::from_str(toml_str).unwrap();
        let project = ProjectConfig::from(config);
        assert_eq!(project.configuration_root.as_deref(), Some("src/cf"));
        assert_eq!(project.search.baseline.backend, SearchBaselineBackend::Postgres);
        assert_eq!(project.search.baseline.workspace_code.branch.as_deref(), Some("develop"));
        assert_eq!(
            project.search.baseline.workspace_code.policy.publish_branches,
            vec!["vendor", "develop"]
        );
        assert_eq!(project.search.baseline.workspace_code.policy.branches.len(), 2);
        assert_eq!(project.search.baseline.workspace_code.policy.branches[0].pattern, "develop");
        let creds = project.postgres_credentials.as_ref().unwrap();
        assert_eq!(creds.host.as_deref(), Some("pg-central.company.com"));
        assert_eq!(creds.port, Some(5432));
        assert_eq!(creds.dbname.as_deref(), Some("bsl_search"));
        assert_eq!(creds.url_env.as_deref(), Some("BSL_SEARCH_BASELINE_PG_URL"));
        assert_eq!(
            creds.credential_helper.as_deref(),
            Some("rtools vault credential-helper --engine bsl-search")
        );
    }

    #[test]
    fn toml_diagnostics_converts_to_json_value() {
        let toml_str = r#"
[diagnostics.parameters]
EmptyCodeBlock = false
LineLength = { maxLineLength = 120 }
"#;
        let config: super::TomlConfig = toml::from_str(toml_str).unwrap();
        let project = ProjectConfig::from(config);
        assert!(project.diagnostics.is_object());
        assert_eq!(project.diagnostics["parameters"]["EmptyCodeBlock"], false);
        assert_eq!(project.diagnostics["parameters"]["LineLength"]["maxLineLength"], 120);
    }

    #[test]
    fn load_prefers_toml_over_json() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("bsl-analyzer.toml"), "[source]\nroot = \"from-toml\"\n")
            .unwrap();
        fs::write(dir.path().join(".bsl-analyzer.json"), r#"{"configurationRoot": "from-json"}"#)
            .unwrap();
        let config = ProjectConfig::load(dir.path()).unwrap();
        assert_eq!(config.configuration_root.as_deref(), Some("from-toml"));
    }

    #[test]
    fn load_falls_back_to_json_when_no_toml() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".bsl-analyzer.json"), r#"{"configurationRoot": "from-json"}"#)
            .unwrap();
        let config = ProjectConfig::load(dir.path()).unwrap();
        assert_eq!(config.configuration_root.as_deref(), Some("from-json"));
    }

    #[test]
    fn credential_resolver_uses_url_env() {
        std::env::set_var("_BSL_TEST_PG_URL_CR1", "postgres://from-env");
        let creds = super::PostgresCredentialConfig {
            url_env: Some("_BSL_TEST_PG_URL_CR1".to_owned()),
            url: Some("postgres://fallback".to_owned()),
            ..Default::default()
        };
        let resolved = resolve_postgres_url(&creds);
        assert_eq!(resolved.as_deref(), Some("postgres://from-env"));
        std::env::remove_var("_BSL_TEST_PG_URL_CR1");
    }

    #[test]
    fn credential_resolver_uses_url_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("pg-url");
        fs::write(&file, "postgres://from-file\n").unwrap();
        let creds = super::PostgresCredentialConfig {
            url_file: Some(file.to_str().unwrap().to_owned()),
            url: Some("postgres://fallback".to_owned()),
            ..Default::default()
        };
        let resolved = resolve_postgres_url(&creds);
        assert_eq!(resolved.as_deref(), Some("postgres://from-file"));
    }

    #[test]
    fn credential_resolver_uses_url_command() {
        let creds = super::PostgresCredentialConfig {
            url_command: Some("echo postgres://from-command".to_owned()),
            ..Default::default()
        };
        let resolved = resolve_postgres_url(&creds);
        assert_eq!(resolved.as_deref(), Some("postgres://from-command"));
    }

    #[test]
    fn credential_resolver_uses_plaintext_url() {
        let creds = super::PostgresCredentialConfig {
            url: Some("postgres://plaintext".to_owned()),
            ..Default::default()
        };
        let resolved = resolve_postgres_url(&creds);
        assert_eq!(resolved.as_deref(), Some("postgres://plaintext"));
    }

    #[test]
    fn credential_resolver_builds_url_from_host_parts() {
        let creds = super::PostgresCredentialConfig {
            host: Some("db.example.com".to_owned()),
            port: Some(5433),
            dbname: Some("mydb".to_owned()),
            ..Default::default()
        };
        let resolved = resolve_postgres_url(&creds);
        assert_eq!(resolved.as_deref(), Some("postgres://db.example.com:5433/mydb"));
    }

    #[test]
    fn credential_resolver_returns_none_when_empty() {
        let creds = super::PostgresCredentialConfig::default();
        assert!(resolve_postgres_url(&creds).is_none());
    }

    #[test]
    fn credential_resolver_runs_credential_helper() {
        let creds = super::PostgresCredentialConfig {
            host: Some("localhost".to_owned()),
            dbname: Some("testdb".to_owned()),
            credential_helper: Some(
                "echo 'username=testuser'; echo 'password=testpass'".to_owned(),
            ),
            ..Default::default()
        };
        let resolved = resolve_postgres_url(&creds).unwrap();
        assert!(resolved.starts_with("postgres://testuser:testpass@localhost:5432/testdb"));
    }

    #[test]
    fn expand_tilde_expands_home() {
        let expanded = super::expand_tilde("~/foo/bar");
        assert!(!expanded.to_str().unwrap().starts_with('~'));
        assert!(expanded.to_str().unwrap().ends_with("foo/bar"));
    }

    #[test]
    fn expand_tilde_preserves_absolute_paths() {
        let expanded = super::expand_tilde("/absolute/path");
        assert_eq!(expanded, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn percent_encode_encodes_special_chars() {
        assert_eq!(super::percent_encode("user@host"), "user%40host");
        assert_eq!(super::percent_encode("p:ss/w rd"), "p%3Ass%2Fw%20rd");
        assert_eq!(super::percent_encode("simple"), "simple");
    }

    // --- Negative path tests ---

    #[test]
    fn toml_present_but_invalid_blocks_json_fallback() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("bsl-analyzer.toml"), "invalid {{{{ toml").unwrap();
        fs::write(dir.path().join(".bsl-analyzer.json"), r#"{"configurationRoot": "from-json"}"#)
            .unwrap();
        // Broken TOML must NOT fall back to JSON
        let config = ProjectConfig::load(dir.path());
        assert!(config.is_none());
    }

    #[test]
    fn credential_resolver_skips_empty_env_var() {
        std::env::set_var("_BSL_TEST_EMPTY_URL", "   ");
        let creds = super::PostgresCredentialConfig {
            url_env: Some("_BSL_TEST_EMPTY_URL".to_owned()),
            url: Some("postgres://fallback".to_owned()),
            ..Default::default()
        };
        let resolved = super::resolve_postgres_url(&creds);
        // Empty env var should be skipped, fallback to plaintext url
        assert_eq!(resolved.as_deref(), Some("postgres://fallback"));
        std::env::remove_var("_BSL_TEST_EMPTY_URL");
    }

    #[test]
    fn credential_resolver_url_command_failure_falls_through() {
        let creds = super::PostgresCredentialConfig {
            url_command: Some("exit 1".to_owned()),
            url: Some("postgres://fallback".to_owned()),
            ..Default::default()
        };
        let resolved = super::resolve_postgres_url(&creds);
        assert_eq!(resolved.as_deref(), Some("postgres://fallback"));
    }

    #[test]
    fn credential_helper_failure_falls_through() {
        let creds = super::PostgresCredentialConfig {
            host: Some("localhost".to_owned()),
            dbname: Some("testdb".to_owned()),
            credential_helper: Some("exit 1".to_owned()),
            ..Default::default()
        };
        // Failed helper → None (no further fallback)
        let resolved = super::resolve_postgres_url(&creds);
        assert!(resolved.is_none());
    }

    #[test]
    fn credential_helper_incomplete_output_fails() {
        let result = super::parse_credential_output("username=only_user\n");
        assert!(result.is_err());
    }

    #[test]
    fn postgres_credentials_none_when_no_pg_fields_in_toml() {
        let config: super::TomlConfig = toml::from_str("[source]\nroot = \"src/cf\"\n").unwrap();
        let project = ProjectConfig::from(config);
        assert!(project.postgres_credentials.is_none());
    }

    #[test]
    fn postgres_credentials_some_when_pg_fields_present() {
        let toml_str = r#"
[search.baseline.postgres]
url_env = "MY_PG_URL"
"#;
        let config: super::TomlConfig = toml::from_str(toml_str).unwrap();
        let project = ProjectConfig::from(config);
        assert!(project.postgres_credentials.is_some());
        assert_eq!(
            project.postgres_credentials.as_ref().unwrap().url_env.as_deref(),
            Some("MY_PG_URL")
        );
    }
}
