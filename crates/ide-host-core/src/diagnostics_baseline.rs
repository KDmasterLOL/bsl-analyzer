use std::path::{Path, PathBuf};

use ide::diagnostics_baseline::{
    parse_diagnostics_baseline, DiagnosticsBaseline, DiagnosticsBaselineError,
    DiagnosticsBaselineExtension, DiagnosticsBaselineScope, DiagnosticsBaselineState,
    DiagnosticsBaselineSummary,
};

#[derive(Debug, Clone)]
pub enum DiagnosticsBaselineSnapshot {
    Disabled,
    Ready {
        baseline: DiagnosticsBaseline,
        project_path: String,
        path: PathBuf,
        epoch: String,
    },
    Error {
        path: Option<PathBuf>,
        observation_path: Option<PathBuf>,
        code: String,
        detail: String,
        epoch: String,
    },
}

impl DiagnosticsBaselineSnapshot {
    pub fn load(project: &project_model::Project) -> Self {
        let resolved = match project.diagnostics_baseline() {
            Ok(None) => return Self::Disabled,
            Ok(Some(resolved)) => resolved,
            Err(error) => {
                let detail = error.to_string();
                let observation_path = match &error {
                    project_model::DiagnosticsBaselineProjectError::Symlink(path)
                    | project_model::DiagnosticsBaselineProjectError::NotAFile(path) => {
                        Some(path.clone())
                    }
                    _ => None,
                };
                return Self::error_observed(
                    None,
                    observation_path,
                    "invalid_configuration",
                    detail.clone().as_bytes(),
                    detail,
                );
            }
        };
        let bytes = match std::fs::read(&resolved.path) {
            Ok(bytes) => bytes,
            Err(error) => {
                let code = if error.kind() == std::io::ErrorKind::NotFound {
                    "missing"
                } else {
                    "unreadable"
                };
                let detail = format!(
                    "cannot read diagnostics baseline {}: {error}",
                    resolved.path.display()
                );
                return Self::error(Some(resolved.path), code, detail.clone().as_bytes(), detail);
            }
        };
        let scope = project_scope(&resolved.scope);
        match parse_diagnostics_baseline(&bytes, &scope) {
            Ok(baseline) => Self::Ready {
                baseline,
                project_path: resolved.project_path,
                path: resolved.path,
                epoch: blake3::hash(&bytes).to_hex().to_string(),
            },
            Err(error) => {
                let code = match error {
                    DiagnosticsBaselineError::UnsupportedSchema { .. } => "unsupported_schema",
                    DiagnosticsBaselineError::ScopeMismatch => "scope_mismatch",
                    _ => "invalid_file",
                };
                let detail = error.to_string();
                Self::error(Some(resolved.path), code, &bytes, detail)
            }
        }
    }

    fn error(path: Option<PathBuf>, code: &str, bytes: &[u8], detail: String) -> Self {
        Self::error_observed(path.clone(), path, code, bytes, detail)
    }

    fn error_observed(
        path: Option<PathBuf>,
        observation_path: Option<PathBuf>,
        code: &str,
        bytes: &[u8],
        detail: String,
    ) -> Self {
        let mut fingerprint = blake3::Hasher::new();
        fingerprint.update(code.as_bytes());
        fingerprint.update(&[0]);
        fingerprint.update(bytes);
        Self::Error {
            path,
            observation_path,
            code: code.to_owned(),
            detail,
            epoch: fingerprint.finalize().to_hex().to_string(),
        }
    }

    pub fn epoch(&self) -> &str {
        match self {
            Self::Disabled => "disabled",
            Self::Ready { epoch, .. } | Self::Error { epoch, .. } => epoch,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Ready { path, .. } => Some(path),
            Self::Error { path, .. } => path.as_deref(),
            Self::Disabled => None,
        }
    }

    pub fn observation(&self) -> String {
        let path = match self {
            Self::Ready { path, .. } => Some(path.as_path()),
            Self::Error { path, observation_path, .. } => {
                observation_path.as_deref().or(path.as_deref())
            }
            Self::Disabled => None,
        };
        let Some(path) = path else { return "disabled".to_owned() };
        observe(path)
    }

    pub fn ready(&self) -> Option<(&DiagnosticsBaseline, &str)> {
        match self {
            Self::Ready { baseline, project_path, .. } => Some((baseline, project_path)),
            _ => None,
        }
    }

    pub fn error_summary(&self) -> Option<DiagnosticsBaselineSummary> {
        let Self::Error { path, code, detail, .. } = self else { return None };
        Some(DiagnosticsBaselineSummary {
            state: DiagnosticsBaselineState::Error,
            new: None,
            known: None,
            resolved: None,
            path: path.as_deref().map(normalize_path),
            schema_version: None,
            complete: false,
            error_code: Some(code.clone()),
            detail: Some(detail.clone()),
        })
    }
}

fn project_scope(
    scope: &project_model::DiagnosticsBaselineProjectScope,
) -> DiagnosticsBaselineScope {
    DiagnosticsBaselineScope {
        source_root: scope.source_root.clone(),
        extensions: scope
            .extensions
            .iter()
            .map(|extension| DiagnosticsBaselineExtension {
                name: extension.name.clone(),
                path: extension.path.clone(),
                depends_on: extension.depends_on.clone(),
            })
            .collect(),
    }
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")
}

fn observe(path: &Path) -> String {
    let Ok(metadata) = std::fs::symlink_metadata(path) else { return "missing".to_owned() };
    let mut hasher = blake3::Hasher::new();
    hasher.update(&metadata.len().to_le_bytes());
    if let Ok(modified) = metadata.modified() {
        if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
            hasher.update(&duration.as_nanos().to_le_bytes());
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        hasher.update(&metadata.dev().to_le_bytes());
        hasher.update(&metadata.ino().to_le_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn invalid_symlink_observation_changes_after_replacement() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("real.json"), "{}").unwrap();
        symlink("real.json", dir.path().join("baseline.json")).unwrap();
        let config_path = dir.path().join("bsl-analyzer.json");
        std::fs::write(
            &config_path,
            r#"{"diagnostics":{"baseline":{"path":"baseline.json"}},"extensions":[]}"#,
        )
        .unwrap();
        let config = project_model::ProjectConfig::load_from_file(&config_path).unwrap();
        let project = project_model::Project::with_config(dir.path(), config).unwrap();
        let broken = DiagnosticsBaselineSnapshot::load(&project);
        let observation = broken.observation();
        assert!(matches!(broken, DiagnosticsBaselineSnapshot::Error { .. }));

        std::fs::remove_file(dir.path().join("baseline.json")).unwrap();
        let baseline = DiagnosticsBaseline {
            schema_version: ide::diagnostics_baseline::DIAGNOSTICS_BASELINE_SCHEMA_VERSION,
            scope: DiagnosticsBaselineScope { source_root: String::new(), extensions: vec![] },
            diagnostics: vec![],
        };
        std::fs::write(
            dir.path().join("baseline.json"),
            ide::diagnostics_baseline::diagnostics_baseline_json(&baseline).unwrap(),
        )
        .unwrap();

        assert_ne!(DiagnosticsBaselineSnapshot::load(&project).observation(), observation);
        assert!(matches!(
            DiagnosticsBaselineSnapshot::load(&project),
            DiagnosticsBaselineSnapshot::Ready { .. }
        ));
    }
}
