use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub mod file_role;
pub use file_role::{
    file_role, is_bsl_source_path, is_common_module_body_path, is_metadata_path, FileRole,
    METADATA_WATCHED_EXTENSIONS, SOURCE_EXTENSIONS,
};

#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub config: ProjectConfig,
    source_path: Option<PathBuf>,
    extension_paths: Vec<(String, PathBuf)>,
}

impl Project {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let config = ProjectConfig::load(&root).unwrap_or_default();
        Self::with_config(root, config)
    }

    /// Like [`Project::new`] but with an already-resolved config, so a caller that
    /// loaded the config from an explicit path (e.g. the CLI `--config` flag) keeps
    /// it instead of having `bsl-analyzer.toml` re-discovered under `root`.
    pub fn with_config(root: impl Into<PathBuf>, config: ProjectConfig) -> Self {
        let root = root.into();
        let source_path = Self::discover_source_path(&root, &config);
        let extension_paths = Self::resolve_extensions(&root, &config);
        Self { root, config, source_path, extension_paths }
    }

    pub fn source_path(&self) -> &Path {
        self.source_path.as_deref().unwrap_or(&self.root)
    }

    /// Directories to scan for BSL sources: the configuration source root plus each
    /// resolved extension root. Scoping a file walk to these (instead of the raw
    /// project root) excludes vendored/build copies like `.build/vendor` that would
    /// otherwise be analyzed as a duplicate configuration.
    pub fn source_roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![self.source_path().to_path_buf()];
        roots.extend(self.extension_paths.iter().map(|(_, path)| path.clone()));
        roots
    }

    pub fn configuration_path(&self) -> Option<&Path> {
        self.source_path.as_deref()
    }

    fn discover_source_path(root: &Path, config: &ProjectConfig) -> Option<PathBuf> {
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

        if let Some(path) = search_configuration_xml(root, 2) {
            tracing::info!(?path, "found Configuration.xml by search");
            return Some(path);
        }

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

    pub fn extension_paths(&self) -> &[(String, PathBuf)] {
        &self.extension_paths
    }

    fn resolve_extensions(root: &Path, config: &ProjectConfig) -> Vec<(String, PathBuf)> {
        let mut resolved: Vec<(String, PathBuf)> = Vec::new();
        let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for ext_path_str in &config.extensions {
            let candidates = if ext_path_str.contains('*') {
                expand_extension_glob(root, ext_path_str)
            } else {
                vec![root.join(ext_path_str)]
            };
            for path in candidates {
                // Dedup on the real path so textual variants (`./src/cfe/Foo` and the
                // glob-produced `src/cfe/Foo`) collapse to one source root.
                let key = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
                if !seen.insert(key) {
                    continue;
                }
                if let Some(entry) = Self::validate_extension(ext_path_str, path) {
                    resolved.push(entry);
                }
            }
        }
        resolved
    }

    fn validate_extension(ext_path_str: &str, path: PathBuf) -> Option<(String, PathBuf)> {
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
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| ext_path_str.to_string());
        tracing::info!(name = %name, path = %path.display(), "resolved extension");
        Some((name, path))
    }
}

/// Expands an `extensions` entry whose final path segment contains `*`
/// (e.g. `src/cfe/*` or `src/cfe/БУС_*`) into every immediate child directory
/// of the parent that matches the wildcard. The wildcard is only honoured in
/// the last segment; results are sorted for deterministic source-root ordering.
fn expand_extension_glob(root: &Path, pattern: &str) -> Vec<PathBuf> {
    let normalized = pattern.replace('\\', "/");
    let (parent_rel, name_pattern) = match normalized.rsplit_once('/') {
        Some((parent, name)) => (parent, name),
        None => ("", normalized.as_str()),
    };
    // The wildcard must live solely in the final segment. This rejects a parent
    // wildcard (`src/*/Foo`) and a trailing slash (`src/cfe/*/` → empty final
    // segment), both of which would otherwise fall through to a literal `read_dir`
    // and silently resolve nothing.
    if parent_rel.contains('*') || !name_pattern.contains('*') {
        tracing::warn!(
            pattern = %pattern,
            "extension glob supports a wildcard only in the final path segment, skipping"
        );
        return Vec::new();
    }
    let parent_dir = if parent_rel.is_empty() { root.to_path_buf() } else { root.join(parent_rel) };
    let entries = match std::fs::read_dir(&parent_dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(
                path = %parent_dir.display(),
                error = %e,
                "extension glob parent directory not readable, skipping"
            );
            return Vec::new();
        }
    };
    let mut matched: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter(|entry| wildcard_matches(name_pattern, &entry.file_name().to_string_lossy()))
        .map(|entry| entry.path())
        .collect();
    matched.sort();
    matched
}

/// Case-insensitive single-segment wildcard match where `*` matches any run of
/// characters (including empty). Supports multiple `*` (e.g. `*_UT`, `БУС_*`).
fn wildcard_matches(pattern: &str, name: &str) -> bool {
    let pat: Vec<char> = pattern.chars().flat_map(char::to_lowercase).collect();
    let text: Vec<char> = name.chars().flat_map(char::to_lowercase).collect();
    let (mut p, mut t) = (0usize, 0usize);
    let (mut star, mut star_t) = (None, 0usize);
    while t < text.len() {
        if p < pat.len() && pat[p] == '*' {
            star = Some(p);
            star_t = t;
            p += 1;
        } else if p < pat.len() && pat[p] == text[t] {
            p += 1;
            t += 1;
        } else if let Some(sp) = star {
            p = sp + 1;
            star_t += 1;
            t = star_t;
        } else {
            return false;
        }
    }
    while p < pat.len() && pat[p] == '*' {
        p += 1;
    }
    p == pat.len()
}

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

    if dir.join("Configuration.xml").exists() {
        return Some(dir.to_path_buf());
    }

    if current_depth < max_depth {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if file_type.is_dir() {
                        let name = entry.file_name();
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

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConfig {
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

    #[serde(default)]
    pub extensions: Vec<String>,

    #[serde(default)]
    pub search: SearchConfig,

    #[serde(default)]
    pub features: FeaturesConfig,

    #[serde(default)]
    pub output: OutputConfig,
}

impl ProjectConfig {
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

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeLensConfig {
    #[serde(default, alias = "show_cognitive_complexity")]
    pub show_cognitive_complexity: bool,

    #[serde(default, alias = "show_cyclomatic_complexity")]
    pub show_cyclomatic_complexity: bool,
}

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

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FeaturesConfig {
    #[serde(default = "default_true", alias = "type_narrowing")]
    pub type_narrowing: bool,
}

impl Default for FeaturesConfig {
    fn default() -> Self {
        Self { type_narrowing: true }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputConfig {
    #[serde(default, alias = "display_language")]
    pub display_language: Option<String>,
}

impl OutputConfig {
    pub fn resolve_locale(&self) -> Option<base_db::Locale> {
        let raw = self.display_language.as_deref()?;
        match base_db::Locale::from_config_str(raw) {
            Ok(locale) => Some(locale),
            Err(e) => {
                tracing::warn!(
                    value = %e.0,
                    "[output] display_language has unknown value; ignoring (will use other locale signals)"
                );
                None
            }
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchConfig {
    #[serde(default)]
    pub baseline: SearchBaselineConfig,
}

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
    pub host: Option<String>,

    #[serde(default)]
    pub port: Option<u16>,

    #[serde(default)]
    pub dbname: Option<String>,

    #[serde(default)]
    pub schema: Option<String>,

    #[serde(default)]
    pub vault_role_base: Option<String>,

    #[serde(default)]
    pub credential_helper: SearchPostgresCredentialHelperConfig,
}

impl SearchPostgresConfig {
    pub fn is_configured(&self) -> bool {
        self.host.is_some()
            || self.port.is_some()
            || self.dbname.is_some()
            || self.schema.is_some()
            || self.vault_role_base.is_some()
            || self.credential_helper.program.is_some()
            || !self.credential_helper.args.is_empty()
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPostgresCredentialHelperConfig {
    #[serde(default)]
    pub program: Option<String>,

    #[serde(default)]
    pub args: Vec<String>,
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

pub fn current_git_commit(start_dir: &Path) -> Option<String> {
    let git_dir = discover_git_dir(start_dir)?;
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(ref_path) = head.strip_prefix("ref: ") {
        return std::fs::read_to_string(git_dir.join(ref_path))
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
    }

    (!head.is_empty()).then(|| head.to_owned())
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
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
    search: TomlSearchConfig,
    #[serde(default)]
    features: FeaturesConfig,
    #[serde(default)]
    output: OutputConfig,
}

impl Default for TomlConfig {
    fn default() -> Self {
        Self {
            source: TomlSourceConfig::default(),
            diagnostics: default_toml_table(),
            code_lens: CodeLensConfig::default(),
            formatting: FormattingConfig::default(),
            search: TomlSearchConfig::default(),
            features: FeaturesConfig::default(),
            output: OutputConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct TomlSourceConfig {
    #[serde(default)]
    root: Option<String>,
    #[serde(default)]
    extensions: Vec<String>,
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
    vault_role_base: Option<String>,
    #[serde(default)]
    credential_helper: TomlSearchPostgresCredentialHelperConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct TomlSearchPostgresCredentialHelperConfig {
    #[serde(default)]
    program: Option<String>,
    #[serde(default)]
    args: Vec<String>,
}

fn default_toml_table() -> toml::Value {
    toml::Value::Table(Default::default())
}

impl From<TomlConfig> for ProjectConfig {
    fn from(toml: TomlConfig) -> Self {
        let diagnostics = toml_value_to_json(toml.diagnostics);
        let pg = toml.search.baseline.postgres;
        Self {
            diagnostics,
            code_lens: toml.code_lens,
            formatting: toml.formatting,
            configuration_root: toml.source.root,
            language: None,
            extensions: toml.source.extensions,
            search: SearchConfig {
                baseline: SearchBaselineConfig {
                    backend: toml.search.baseline.backend,
                    postgres: SearchPostgresConfig {
                        host: pg.host,
                        port: pg.port,
                        dbname: pg.dbname,
                        schema: pg.schema,
                        vault_role_base: pg.vault_role_base,
                        credential_helper: SearchPostgresCredentialHelperConfig {
                            program: pg.credential_helper.program,
                            args: pg.credential_helper.args,
                        },
                    },
                    workspace_code: toml.search.baseline.workspace_code,
                    reference: toml.search.baseline.reference,
                },
            },
            features: toml.features,
            output: toml.output,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PostgresAccessMode {
    Reader,
    Writer,
    Migrator,
}

impl PostgresAccessMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reader => "reader",
            Self::Writer => "writer",
            Self::Migrator => "migrator",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPostgresUrl {
    pub url: String,
    pub lease_id: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub renewable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPostgresTarget {
    pub host: String,
    pub port: u16,
    pub dbname: String,
    pub schema: String,
}

impl SearchPostgresConfig {
    pub fn resolved_target(&self) -> Result<ResolvedPostgresTarget, ResolvePostgresUrlError> {
        let host = self
            .host
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or(ResolvePostgresUrlError::MissingField("search.baseline.postgres.host"))?
            .to_owned();
        let dbname = self
            .dbname
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or(ResolvePostgresUrlError::MissingField("search.baseline.postgres.dbname"))?
            .to_owned();
        let schema = self
            .schema
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or(ResolvePostgresUrlError::MissingField("search.baseline.postgres.schema"))?
            .to_owned();
        Ok(ResolvedPostgresTarget { host, port: self.port.unwrap_or(5432), dbname, schema })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvePostgresUrlError {
    MissingField(&'static str),
    MissingCredentialHelper,
    HelperSpawn {
        program: String,
        message: String,
    },
    HelperTimeout {
        program: String,
        timeout: std::time::Duration,
    },
    HelperProtocol {
        program: String,
        message: String,
        stderr: String,
        stdout: String,
    },
    HelperRejected {
        program: String,
        exit_code: Option<i32>,
        stderr: String,
        error: CredentialHelperErrorPayload,
    },
    UnsupportedUrlScheme(String),
    InvalidResolvedUrl(String),
    TargetMismatch {
        expected_host: String,
        expected_port: u16,
        expected_dbname: String,
        actual_host: Option<String>,
        actual_port: Option<u16>,
        actual_dbname: String,
    },
}

impl ResolvePostgresUrlError {
    pub fn retryable(&self) -> bool {
        match self {
            Self::HelperTimeout { .. } => true,
            Self::HelperRejected { error, .. } => error.retryable,
            _ => false,
        }
    }
}

impl std::fmt::Display for ResolvePostgresUrlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingField(field) => write!(f, "missing required PostgreSQL config field: {field}"),
            Self::MissingCredentialHelper => write!(
                f,
                "missing required PostgreSQL credential helper config: search.baseline.postgres.credential_helper.program"
            ),
            Self::HelperSpawn { program, message } => {
                write!(f, "failed to spawn credential helper '{program}': {message}")
            }
            Self::HelperTimeout { program, timeout } => write!(
                f,
                "credential helper '{program}' timed out after {}s",
                timeout.as_secs()
            ),
            Self::HelperProtocol { program, message, .. } => {
                write!(f, "credential helper '{program}' returned an invalid response: {message}")
            }
            Self::HelperRejected { program, exit_code, error, .. } => {
                write!(
                    f,
                    "credential helper '{program}' rejected the request{}: {}: {}",
                    exit_code
                        .map(|code| format!(" (exit {code})"))
                        .unwrap_or_default(),
                    error.code,
                    error.message
                )
            }
            Self::UnsupportedUrlScheme(scheme) => {
                write!(f, "credential helper returned unsupported URL scheme '{scheme}'")
            }
            Self::InvalidResolvedUrl(message) => write!(f, "credential helper returned invalid URL: {message}"),
            Self::TargetMismatch {
                expected_host,
                expected_port,
                expected_dbname,
                actual_host,
                actual_port,
                actual_dbname,
            } => write!(
                f,
                "credential helper returned URL for unexpected target: expected {}:{}/{} but got {}:{}/{}",
                expected_host,
                expected_port,
                expected_dbname,
                actual_host.as_deref().unwrap_or("<missing-host>"),
                actual_port.unwrap_or(5432),
                actual_dbname
            ),
        }
    }
}

impl std::error::Error for ResolvePostgresUrlError {}

#[derive(Debug, Clone, Serialize)]
struct CredentialHelperRequest {
    protocol: &'static str,
    action: &'static str,
    mode: PostgresAccessMode,
    vault_role_base: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CredentialHelperResponse {
    protocol: String,
    ok: bool,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    lease_id: Option<String>,
    #[serde(default)]
    expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    renewable: bool,
    #[serde(default)]
    error: Option<CredentialHelperErrorPayload>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CredentialHelperErrorPayload {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
}

const POSTGRES_HELPER_PROTOCOL: &str = "bsl-analyzer.postgres-helper.v1";
const COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub fn resolve_postgres_url(
    postgres: &SearchPostgresConfig,
    mode: PostgresAccessMode,
) -> Result<ResolvedPostgresUrl, ResolvePostgresUrlError> {
    let target = postgres.resolved_target()?;
    let vault_role_base = postgres
        .vault_role_base
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ResolvePostgresUrlError::MissingField("search.baseline.postgres.vault_role_base"))?
        .to_owned();
    let program = postgres
        .credential_helper
        .program
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ResolvePostgresUrlError::MissingCredentialHelper)?
        .to_owned();
    let request = CredentialHelperRequest {
        protocol: POSTGRES_HELPER_PROTOCOL,
        action: "resolve-url",
        mode,
        vault_role_base,
    };
    let response = run_credential_helper(&program, &postgres.credential_helper.args, &request)?;
    let url = response.url.ok_or_else(|| ResolvePostgresUrlError::HelperProtocol {
        program: program.clone(),
        message: "missing url in successful response".to_owned(),
        stderr: String::new(),
        stdout: String::new(),
    })?;
    validate_resolved_postgres_url(&url, &target)?;
    Ok(ResolvedPostgresUrl {
        url,
        lease_id: response.lease_id,
        expires_at: response.expires_at,
        renewable: response.renewable,
    })
}

fn run_credential_helper(
    program: &str,
    args: &[String],
    request: &CredentialHelperRequest,
) -> Result<CredentialHelperResponse, ResolvePostgresUrlError> {
    use std::io::Write;
    use std::process::Stdio;

    let mut child = std::process::Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| ResolvePostgresUrlError::HelperSpawn {
            program: program.to_owned(),
            message: error.to_string(),
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        let payload = serde_json::to_vec(request).map_err(|error| {
            ResolvePostgresUrlError::HelperProtocol {
                program: program.to_owned(),
                message: format!("failed to serialize helper request: {error}"),
                stderr: String::new(),
                stdout: String::new(),
            }
        })?;
        stdin.write_all(&payload).and_then(|_| stdin.write_all(b"\n")).map_err(|error| {
            ResolvePostgresUrlError::HelperProtocol {
                program: program.to_owned(),
                message: format!("failed to write helper request: {error}"),
                stderr: String::new(),
                stdout: String::new(),
            }
        })?;
    }

    let deadline = std::time::Instant::now() + COMMAND_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let output = child.wait_with_output().map_err(|error| {
                    ResolvePostgresUrlError::HelperProtocol {
                        program: program.to_owned(),
                        message: format!("failed to read helper output: {error}"),
                        stderr: String::new(),
                        stdout: String::new(),
                    }
                })?;
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                return parse_credential_helper_response(program, status.code(), &stdout, &stderr);
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ResolvePostgresUrlError::HelperTimeout {
                        program: program.to_owned(),
                        timeout: COMMAND_TIMEOUT,
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(error) => {
                return Err(ResolvePostgresUrlError::HelperProtocol {
                    program: program.to_owned(),
                    message: format!("failed to wait for helper: {error}"),
                    stderr: String::new(),
                    stdout: String::new(),
                });
            }
        }
    }
}

fn parse_credential_helper_response(
    program: &str,
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> Result<CredentialHelperResponse, ResolvePostgresUrlError> {
    let response = serde_json::from_str::<CredentialHelperResponse>(stdout).map_err(|error| {
        ResolvePostgresUrlError::HelperProtocol {
            program: program.to_owned(),
            message: format!("failed to parse helper JSON: {error}"),
            stderr: stderr.to_owned(),
            stdout: stdout.to_owned(),
        }
    })?;
    if response.protocol != POSTGRES_HELPER_PROTOCOL {
        return Err(ResolvePostgresUrlError::HelperProtocol {
            program: program.to_owned(),
            message: format!("unexpected helper protocol '{}'", response.protocol),
            stderr: stderr.to_owned(),
            stdout: stdout.to_owned(),
        });
    }
    if response.ok {
        if exit_code.unwrap_or(0) != 0 {
            return Err(ResolvePostgresUrlError::HelperProtocol {
                program: program.to_owned(),
                message: format!(
                    "successful helper response used non-zero exit code {:?}",
                    exit_code
                ),
                stderr: stderr.to_owned(),
                stdout: stdout.to_owned(),
            });
        }
        return Ok(response);
    }
    let error = response.error.ok_or_else(|| ResolvePostgresUrlError::HelperProtocol {
        program: program.to_owned(),
        message: "missing error payload in failed helper response".to_owned(),
        stderr: stderr.to_owned(),
        stdout: stdout.to_owned(),
    })?;
    Err(ResolvePostgresUrlError::HelperRejected {
        program: program.to_owned(),
        exit_code,
        stderr: stderr.to_owned(),
        error,
    })
}

fn validate_resolved_postgres_url(
    url: &str,
    target: &ResolvedPostgresTarget,
) -> Result<(), ResolvePostgresUrlError> {
    let parsed = url::Url::parse(url)
        .map_err(|error| ResolvePostgresUrlError::InvalidResolvedUrl(error.to_string()))?;
    if parsed.scheme() != "postgres" {
        return Err(ResolvePostgresUrlError::UnsupportedUrlScheme(parsed.scheme().to_owned()));
    }
    let actual_host = parsed.host_str().map(ToOwned::to_owned);
    let actual_port = parsed.port_or_known_default();
    let actual_dbname = parsed.path().trim_start_matches('/').to_owned();
    if actual_host.as_deref() != Some(target.host.as_str())
        || actual_port != Some(target.port)
        || actual_dbname != target.dbname
    {
        return Err(ResolvePostgresUrlError::TargetMismatch {
            expected_host: target.host.clone(),
            expected_port: target.port,
            expected_dbname: target.dbname.clone(),
            actual_host,
            actual_port,
            actual_dbname,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        branch_pattern_matches, current_git_branch, current_git_commit,
        evaluate_workspace_baseline_support, is_publish_branch_allowed, parse_timestamp_utc,
        resolve_postgres_url, resolve_workspace_branch_policy, wildcard_matches, FeaturesConfig,
        PostgresAccessMode, Project, ProjectConfig, ResolvePostgresUrlError, SearchBaselineBackend,
        SearchBaselinePolicyConfig, SearchBaselineSupportState, SearchPostgresConfig,
        SearchPostgresCredentialHelperConfig,
    };
    use chrono::{Duration, TimeZone, Utc};
    use std::fs;
    use tempfile::tempdir;

    fn helper_program(response: &str, exit_code: i32) -> SearchPostgresCredentialHelperConfig {
        SearchPostgresCredentialHelperConfig {
            program: Some("python3".to_owned()),
            args: vec![
                "-c".to_owned(),
                "import sys; sys.stdin.readline(); sys.stdout.write(sys.argv[1]); sys.exit(int(sys.argv[2]))"
                    .to_owned(),
                response.to_owned(),
                exit_code.to_string(),
            ],
        }
    }

    #[test]
    fn project_config_defaults_search_baseline_to_sqlite() {
        let config: ProjectConfig = serde_json::from_str("{}").unwrap();

        assert_eq!(config.search.baseline.backend, SearchBaselineBackend::Sqlite);
        assert!(!config.search.baseline.postgres.is_configured());
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
                            "host": "pg-central",
                            "port": 5432,
                            "dbname": "bsl_search",
                            "schema": "corp_search",
                            "vaultRoleBase": "search/bsl",
                            "credentialHelper": {
                                "program": "vault-helper",
                                "args": ["--mode"]
                            }
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
        assert_eq!(config.search.baseline.postgres.host.as_deref(), Some("pg-central"));
        assert_eq!(config.search.baseline.postgres.port, Some(5432));
        assert_eq!(config.search.baseline.postgres.dbname.as_deref(), Some("bsl_search"));
        assert_eq!(config.search.baseline.postgres.schema.as_deref(), Some("corp_search"));
        assert_eq!(config.search.baseline.postgres.vault_role_base.as_deref(), Some("search/bsl"));
        assert_eq!(
            config.search.baseline.postgres.credential_helper.program.as_deref(),
            Some("vault-helper")
        );
        assert_eq!(config.search.baseline.postgres.credential_helper.args, vec!["--mode"]);
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
    fn current_git_commit_reads_direct_git_dir_ref() {
        let dir = tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        let ref_file = git_dir.join("refs/heads/feature/test");
        fs::create_dir_all(ref_file.parent().unwrap()).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/feature/test\n").unwrap();
        fs::write(&ref_file, "0123456789abcdef\n").unwrap();

        assert_eq!(current_git_commit(dir.path()).as_deref(), Some("0123456789abcdef"));
    }

    #[test]
    fn current_git_commit_reads_detached_head() {
        let dir = tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "0123456789abcdef\n").unwrap();

        assert_eq!(current_git_commit(dir.path()).as_deref(), Some("0123456789abcdef"));
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
        assert!(!project.search.baseline.postgres.is_configured());
    }

    #[test]
    fn toml_config_reads_extensions_from_source_section() {
        let toml_str = r#"
[source]
root = "src/cf"
extensions = ["src/cfe/BMS_RU_UT", "src/cfe/YAxUnit"]
"#;
        let config: super::TomlConfig = toml::from_str(toml_str).unwrap();
        let project = ProjectConfig::from(config);
        assert_eq!(project.configuration_root.as_deref(), Some("src/cf"));
        assert_eq!(project.extensions, vec!["src/cfe/BMS_RU_UT", "src/cfe/YAxUnit"]);
    }

    #[test]
    fn toml_config_rejects_top_level_extensions() {
        let toml_str = r#"
extensions = ["src/cfe/BMS_RU_UT"]

[source]
root = "src/cf"
"#;
        let result: Result<super::TomlConfig, _> = toml::from_str(toml_str);
        assert!(
            result.is_err(),
            "top-level extensions must be rejected; use [source].extensions instead"
        );
    }

    fn touch_extension(root: &std::path::Path, rel: &str) {
        let dir = root.join(rel);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Configuration.xml"), "<xml/>").unwrap();
    }

    #[test]
    fn extension_glob_expands_to_all_subdirs_with_configuration_xml() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch_extension(root, "src/cfe/BMS_RU_UT");
        touch_extension(root, "src/cfe/YAxUnit");
        // A cfe subdir without Configuration.xml must be skipped.
        std::fs::create_dir_all(root.join("src/cfe/NotAnExtension")).unwrap();

        let config =
            ProjectConfig { extensions: vec!["src/cfe/*".to_string()], ..Default::default() };
        let resolved = Project::resolve_extensions(root, &config);

        let names: Vec<&str> = resolved.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["BMS_RU_UT", "YAxUnit"]);
    }

    #[test]
    fn extension_glob_honours_prefix_wildcard() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch_extension(root, "src/cfe/БУС_ОбменДанными");
        touch_extension(root, "src/cfe/YAxUnit");

        let config = ProjectConfig {
            extensions: vec!["src/cfe/БУС_*".to_string()],
            ..Default::default()
        };
        let resolved = Project::resolve_extensions(root, &config);

        let names: Vec<&str> = resolved.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["БУС_ОбменДанными"]);
    }

    #[test]
    fn extension_glob_and_explicit_paths_dedup() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch_extension(root, "src/cfe/BMS_RU_UT");

        let config = ProjectConfig {
            extensions: vec!["src/cfe/*".to_string(), "src/cfe/BMS_RU_UT".to_string()],
            ..Default::default()
        };
        let resolved = Project::resolve_extensions(root, &config);
        assert_eq!(resolved.len(), 1, "the same extension must not be added twice");
    }

    #[test]
    fn extension_glob_rejects_wildcard_outside_final_segment() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch_extension(root, "src/cfe/BMS_RU_UT");

        // Trailing slash (empty final segment) and parent-segment wildcards are
        // unsupported and must resolve nothing rather than silently misbehave.
        for pat in ["src/cfe/*/", "src/*/BMS_RU_UT"] {
            let config = ProjectConfig { extensions: vec![pat.to_string()], ..Default::default() };
            let resolved = Project::resolve_extensions(root, &config);
            assert!(resolved.is_empty(), "pattern {pat} must resolve no extensions");
        }
    }

    #[test]
    fn extension_glob_dedups_dot_slash_against_glob() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        touch_extension(root, "src/cfe/BMS_RU_UT");

        let config = ProjectConfig {
            extensions: vec!["src/cfe/*".to_string(), "./src/cfe/BMS_RU_UT".to_string()],
            ..Default::default()
        };
        let resolved = Project::resolve_extensions(root, &config);
        assert_eq!(resolved.len(), 1, "`./`-prefixed literal must dedup against the glob result");
    }

    #[test]
    fn wildcard_matches_basic_cases() {
        assert!(wildcard_matches("*", "anything"));
        assert!(wildcard_matches("БУС_*", "БУС_ОбщегоНазначения"));
        assert!(wildcard_matches("*_UT", "BMS_RU_UT"));
        assert!(wildcard_matches("a*b*c", "axxbyyc"));
        assert!(wildcard_matches("bms_ru_ut", "BMS_RU_UT")); // case-insensitive
        assert!(!wildcard_matches("БУС_*", "YAxUnit"));
        assert!(!wildcard_matches("a*b", "axxc"));
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
vault_role_base = "prod/search/bsl-analyzer"

[search.baseline.postgres.credential_helper]
program = "rtools"
args = ["vault", "credential-helper"]

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

        let pg = &project.search.baseline.postgres;
        assert_eq!(pg.host.as_deref(), Some("pg-central.company.com"));
        assert_eq!(pg.port, Some(5432));
        assert_eq!(pg.dbname.as_deref(), Some("bsl_search"));
        assert_eq!(pg.schema.as_deref(), Some("bsl_search"));
        assert_eq!(pg.vault_role_base.as_deref(), Some("prod/search/bsl-analyzer"));
        assert_eq!(pg.credential_helper.program.as_deref(), Some("rtools"));
        assert_eq!(pg.credential_helper.args, vec!["vault", "credential-helper"]);
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
    fn toml_present_but_invalid_blocks_json_fallback() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("bsl-analyzer.toml"), "invalid {{{{ toml").unwrap();
        fs::write(dir.path().join(".bsl-analyzer.json"), r#"{"configurationRoot": "from-json"}"#)
            .unwrap();
        let config = ProjectConfig::load(dir.path());
        assert!(config.is_none());
    }

    #[test]
    fn resolve_postgres_url_fails_when_helper_not_configured() {
        let config = SearchPostgresConfig {
            host: Some("localhost".to_owned()),
            dbname: Some("testdb".to_owned()),
            schema: Some("public".to_owned()),
            port: None,
            vault_role_base: Some("search/base".to_owned()),
            credential_helper: SearchPostgresCredentialHelperConfig { program: None, args: vec![] },
        };
        let result = resolve_postgres_url(&config, PostgresAccessMode::Reader);
        assert!(matches!(result, Err(ResolvePostgresUrlError::MissingCredentialHelper)));
    }

    #[test]
    fn resolve_postgres_url_fails_when_missing_vault_role_base() {
        let config = SearchPostgresConfig {
            host: Some("localhost".to_owned()),
            dbname: Some("testdb".to_owned()),
            port: None,
            schema: Some("public".to_owned()),
            vault_role_base: None,
            credential_helper: SearchPostgresCredentialHelperConfig {
                program: Some("echo".to_owned()),
                args: vec![],
            },
        };
        let result = resolve_postgres_url(&config, PostgresAccessMode::Reader);
        assert!(matches!(result, Err(ResolvePostgresUrlError::MissingField(_))));
    }

    #[test]
    fn resolve_postgres_url_fails_when_missing_required_target_field() {
        let config = SearchPostgresConfig {
            host: None,
            dbname: Some("testdb".to_owned()),
            schema: Some("public".to_owned()),
            port: None,
            vault_role_base: Some("search/base".to_owned()),
            credential_helper: SearchPostgresCredentialHelperConfig {
                program: Some("echo".to_owned()),
                args: vec![],
            },
        };
        let result = resolve_postgres_url(&config, PostgresAccessMode::Writer);
        assert!(matches!(result, Err(ResolvePostgresUrlError::MissingField(_))));
    }

    #[test]
    fn resolve_postgres_url_spawn_failure_is_terminal() {
        let config = SearchPostgresConfig {
            host: Some("localhost".to_owned()),
            dbname: Some("testdb".to_owned()),
            schema: Some("public".to_owned()),
            port: None,
            vault_role_base: Some("search/base".to_owned()),
            credential_helper: SearchPostgresCredentialHelperConfig {
                program: Some("nonexistent-program-xyz".to_owned()),
                args: vec![],
            },
        };
        let result = resolve_postgres_url(&config, PostgresAccessMode::Reader);
        assert!(matches!(result, Err(ResolvePostgresUrlError::HelperSpawn { .. })));
    }

    #[test]
    fn resolve_postgres_url_invalid_helper_response_is_protocol_error() {
        let config = SearchPostgresConfig {
            host: Some("localhost".to_owned()),
            dbname: Some("testdb".to_owned()),
            schema: Some("public".to_owned()),
            port: None,
            vault_role_base: Some("search/base".to_owned()),
            credential_helper: helper_program("just-a-plain-line", 0),
        };
        let result = resolve_postgres_url(&config, PostgresAccessMode::Reader);
        assert!(matches!(result, Err(ResolvePostgresUrlError::HelperProtocol { .. })));
    }

    #[test]
    fn resolve_postgres_url_helper_returns_wrong_protocol() {
        let config = SearchPostgresConfig {
            host: Some("localhost".to_owned()),
            dbname: Some("testdb".to_owned()),
            schema: Some("public".to_owned()),
            port: None,
            vault_role_base: Some("search/base".to_owned()),
            credential_helper: helper_program(
                r#"{"protocol":"wrong","ok":true,"url":"postgres://localhost:5432/testdb"}"#,
                0,
            ),
        };
        let result = resolve_postgres_url(&config, PostgresAccessMode::Reader);
        assert!(matches!(result, Err(ResolvePostgresUrlError::HelperProtocol { .. })));
    }

    #[test]
    fn resolve_postgres_url_helper_returns_unsupported_scheme() {
        let config = SearchPostgresConfig {
            host: Some("localhost".to_owned()),
            dbname: Some("testdb".to_owned()),
            schema: Some("public".to_owned()),
            port: None,
            vault_role_base: Some("search/base".to_owned()),
            credential_helper: helper_program(
                r#"{"protocol":"bsl-analyzer.postgres-helper.v1","ok":true,"url":"mysql://localhost:5432/testdb"}"#,
                0,
            ),
        };
        let result = resolve_postgres_url(&config, PostgresAccessMode::Reader);
        assert!(matches!(result, Err(ResolvePostgresUrlError::UnsupportedUrlScheme(_))));
    }

    #[test]
    fn resolve_postgres_url_target_mismatch_host() {
        let config = SearchPostgresConfig {
            host: Some("expected-host".to_owned()),
            dbname: Some("testdb".to_owned()),
            schema: Some("public".to_owned()),
            port: None,
            vault_role_base: Some("search/base".to_owned()),
            credential_helper: helper_program(
                r#"{"protocol":"bsl-analyzer.postgres-helper.v1","ok":true,"url":"postgres://different-host:5432/testdb","lease_id":"l1","renewable":false}"#,
                0,
            ),
        };
        let result = resolve_postgres_url(&config, PostgresAccessMode::Reader);
        assert!(matches!(result, Err(ResolvePostgresUrlError::TargetMismatch { .. })));
    }

    #[test]
    fn resolve_postgres_url_target_mismatch_dbname() {
        let config = SearchPostgresConfig {
            host: Some("localhost".to_owned()),
            port: Some(5433),
            dbname: Some("expected-db".to_owned()),
            schema: Some("public".to_owned()),
            vault_role_base: Some("search/base".to_owned()),
            credential_helper: helper_program(
                r#"{"protocol":"bsl-analyzer.postgres-helper.v1","ok":true,"url":"postgres://localhost:5433/other-db","lease_id":"l1","renewable":false}"#,
                0,
            ),
        };
        let result = resolve_postgres_url(&config, PostgresAccessMode::Reader);
        assert!(matches!(result, Err(ResolvePostgresUrlError::TargetMismatch { .. })));
    }

    #[test]
    fn resolve_postgres_url_resolves_via_tiny_helper_program() {
        let config = SearchPostgresConfig {
            host: Some("localhost".to_owned()),
            port: Some(5433),
            dbname: Some("mydb".to_owned()),
            schema: Some("bsl_search".to_owned()),
            vault_role_base: Some("search/base".to_owned()),
            credential_helper: helper_program(
                r#"{"protocol":"bsl-analyzer.postgres-helper.v1","ok":true,"url":"postgres://user:pass@localhost:5433/mydb","lease_id":"vault/lease/123","expires_at":"2026-06-01T00:00:00Z","renewable":false}"#,
                0,
            ),
        };
        let resolved = resolve_postgres_url(&config, PostgresAccessMode::Reader).unwrap();
        assert_eq!(resolved.url, "postgres://user:pass@localhost:5433/mydb");
        assert_eq!(resolved.lease_id.as_deref(), Some("vault/lease/123"));
        assert!(!resolved.renewable);
    }

    #[test]
    fn resolve_postgres_url_rejected_by_helper_is_terminal() {
        let config = SearchPostgresConfig {
            host: Some("localhost".to_owned()),
            dbname: Some("testdb".to_owned()),
            schema: Some("public".to_owned()),
            port: None,
            vault_role_base: Some("search/base".to_owned()),
            credential_helper: helper_program(
                r#"{"protocol":"bsl-analyzer.postgres-helper.v1","ok":false,"error":{"code":"vault_access_denied","message":"role not allowed","retryable":false}}"#,
                1,
            ),
        };
        let result = resolve_postgres_url(&config, PostgresAccessMode::Writer);
        assert!(matches!(result, Err(ResolvePostgresUrlError::HelperRejected { .. })));
        if let Err(ResolvePostgresUrlError::HelperRejected { error, .. }) = result {
            assert_eq!(error.code, "vault_access_denied");
            assert!(!error.retryable);
        }
    }

    #[test]
    fn resolved_target_defaults_port_to_5432() {
        let config = SearchPostgresConfig {
            host: Some("localhost".to_owned()),
            dbname: Some("mydb".to_owned()),
            schema: Some("public".to_owned()),
            port: None,
            vault_role_base: None,
            credential_helper: SearchPostgresCredentialHelperConfig::default(),
        };
        let target = config.resolved_target().unwrap();
        assert_eq!(target.host, "localhost");
        assert_eq!(target.port, 5432);
        assert_eq!(target.dbname, "mydb");
        assert_eq!(target.schema, "public");
    }

    #[test]
    fn is_configured_returns_false_for_empty_postgres_config() {
        let config = SearchPostgresConfig::default();
        assert!(!config.is_configured());
    }

    #[test]
    fn is_configured_returns_true_when_helper_present() {
        let config = SearchPostgresConfig {
            host: Some("localhost".to_owned()),
            dbname: Some("mydb".to_owned()),
            schema: Some("public".to_owned()),
            port: None,
            vault_role_base: Some("search/base".to_owned()),
            credential_helper: SearchPostgresCredentialHelperConfig {
                program: Some("helper".to_owned()),
                args: vec![],
            },
        };
        assert!(config.is_configured());
    }

    #[test]
    fn features_config_defaults_type_narrowing_to_true() {
        let features = FeaturesConfig::default();
        assert!(
            features.type_narrowing,
            "narrowing must default to enabled so fresh projects inherit the feature without opt-in"
        );
    }

    #[test]
    fn project_config_defaults_features_to_enabled() {
        let config: ProjectConfig = serde_json::from_str("{}").unwrap();
        assert!(
            config.features.type_narrowing,
            "omitted `features` section must yield `FeaturesConfig::default`"
        );
    }

    #[test]
    fn project_config_deserializes_features_disabled_json() {
        let config: ProjectConfig = serde_json::from_str(
            r#"{
                "features": { "typeNarrowing": false }
            }"#,
        )
        .unwrap();
        assert!(
            !config.features.type_narrowing,
            "explicit `typeNarrowing = false` must propagate through JSON deserialization"
        );
    }

    #[test]
    fn project_config_load_from_toml_disables_narrowing() {
        let dir = tempdir().unwrap();
        let toml_path = dir.path().join("bsl-analyzer.toml");
        fs::write(
            &toml_path,
            r#"
[features]
type_narrowing = false
"#,
        )
        .unwrap();

        let config = ProjectConfig::load(dir.path())
            .expect("ProjectConfig::load must succeed for a well-formed TOML");
        assert!(
            !config.features.type_narrowing,
            "`[features] type_narrowing = false` in TOML must round-trip to FeaturesConfig"
        );
    }
}
