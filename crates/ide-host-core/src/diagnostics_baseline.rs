use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use ide::diagnostics_baseline::{
    parse_diagnostics_baseline, DiagnosticsBaseline, DiagnosticsBaselineError,
    DiagnosticsBaselineErrorSummary, DiagnosticsBaselineExtension, DiagnosticsBaselineScope,
    DiagnosticsBaselineState, DiagnosticsBaselineSummary,
};
use ide::partitioned_diagnostics_baseline::{
    load_diagnostics_baseline_set, load_diagnostics_baseline_set_reusing,
    DiagnosticsBaselineSetSnapshot, PartitionedDiagnosticsBaselineError,
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
    ReadySet {
        baseline: std::sync::Arc<DiagnosticsBaselineSetSnapshot>,
        plan: std::sync::Arc<project_model::DiagnosticsBaselinePartitionPlan>,
        project_path: String,
        path: PathBuf,
        epoch: String,
        observations: BTreeMap<String, String>,
    },
    Error {
        path: Option<PathBuf>,
        observation_paths: Vec<PathBuf>,
        code: String,
        detail: String,
        epoch: String,
        errors: Vec<DiagnosticsBaselineErrorSummary>,
    },
}

impl DiagnosticsBaselineSnapshot {
    pub fn load_reusing(project: &project_model::Project, previous: &Self) -> Self {
        let Self::ReadySet { baseline: previous_set, observations, path, .. } = previous else {
            return Self::load(project);
        };
        let changed: BTreeSet<_> = previous_set
            .manifest
            .partitions
            .iter()
            .filter(|entry| {
                observations.get(&entry.file) != Some(&observe(&path.join(&entry.file)))
            })
            .map(|entry| entry.file.clone())
            .collect();
        Self::load_partitioned(project, Some(previous_set), &changed)
            .unwrap_or_else(|| Self::load(project))
    }

    fn load_partitioned(
        project: &project_model::Project,
        previous: Option<&DiagnosticsBaselineSetSnapshot>,
        changed_objects: &BTreeSet<String>,
    ) -> Option<Self> {
        let resolved = project.diagnostics_baseline().ok()??;
        if !matches!(
            resolved.mode,
            project_model::DiagnosticsBaselineProjectMode::Partitioned { .. }
        ) {
            return None;
        }
        let plan = project.diagnostics_baseline_partition_plan().ok()??;
        let directory = project_model::ManagedBaselineDirectory::open(
            &project.root,
            &resolved.project_path,
            false,
        )
        .ok()?;
        let (baseline, _) =
            load_diagnostics_baseline_set_reusing(&directory, &plan, previous, changed_objects)
                .ok()?;
        let epoch = blake3::Hash::from_bytes(baseline.manifest_hash).to_hex().to_string();
        let observations = object_observations(&resolved.path, &baseline);
        Some(Self::ReadySet {
            baseline: std::sync::Arc::new(baseline),
            plan: std::sync::Arc::new(plan),
            project_path: resolved.project_path,
            path: resolved.path,
            epoch,
            observations,
        })
    }

    pub fn load(project: &project_model::Project) -> Self {
        let resolved = match project.diagnostics_baseline() {
            Ok(None) => return Self::Disabled,
            Ok(Some(resolved)) => resolved,
            Err(error) => {
                let detail = error.to_string();
                let path = match &error {
                    project_model::DiagnosticsBaselineProjectError::Symlink(path)
                    | project_model::DiagnosticsBaselineProjectError::NotAFile(path) => {
                        Some(path.clone())
                    }
                    _ => None,
                };
                return Self::error_observed(
                    path.clone(),
                    path,
                    "invalid_configuration",
                    detail.clone().as_bytes(),
                    detail,
                );
            }
        };
        if matches!(
            resolved.mode,
            project_model::DiagnosticsBaselineProjectMode::Partitioned { .. }
        ) {
            let plan = match project.diagnostics_baseline_partition_plan() {
                Ok(Some(plan)) => plan,
                Ok(None) => unreachable!("partitioned mode has a plan"),
                Err(error) => {
                    let detail = error.to_string();
                    return Self::error(
                        Some(resolved.path),
                        "invalid_configuration",
                        detail.clone().as_bytes(),
                        detail,
                    );
                }
            };
            let directory = match project_model::ManagedBaselineDirectory::open(
                &project.root,
                &resolved.project_path,
                false,
            ) {
                Ok(directory) => directory,
                Err(error) => {
                    let code = if error.kind() == std::io::ErrorKind::NotFound {
                        "missing"
                    } else {
                        "unreadable"
                    };
                    let detail = format!(
                        "cannot open diagnostics baseline directory {}: {error}",
                        resolved.path.display()
                    );
                    return Self::error_observed(
                        Some(resolved.path.clone()),
                        Some(resolved.path),
                        code,
                        detail.clone().as_bytes(),
                        detail,
                    );
                }
            };
            return match load_diagnostics_baseline_set(&directory, &plan) {
                Ok(baseline) => {
                    let epoch =
                        blake3::Hash::from_bytes(baseline.manifest_hash).to_hex().to_string();
                    let observations = object_observations(&resolved.path, &baseline);
                    Self::ReadySet {
                        baseline: std::sync::Arc::new(baseline),
                        plan: std::sync::Arc::new(plan),
                        project_path: resolved.project_path,
                        path: resolved.path,
                        epoch,
                        observations,
                    }
                }
                Err(error) => {
                    let detail = error.to_string();
                    let (observation_paths, observed_bytes) =
                        partitioned_error_observation(&project.root, &resolved.project_path);
                    let snapshot = Self::error_observed_many(
                        Some(resolved.path),
                        observation_paths,
                        error.info().code,
                        &observed_bytes,
                        detail,
                    );
                    Self::with_partition_errors(snapshot, &error)
                }
            };
        }
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
        let observation_paths = observation_path.iter().cloned().collect();
        Self::error_observed_many(path, observation_paths, code, bytes, detail)
    }

    fn error_observed_many(
        path: Option<PathBuf>,
        observation_paths: Vec<PathBuf>,
        code: &str,
        bytes: &[u8],
        detail: String,
    ) -> Self {
        let mut fingerprint = blake3::Hasher::new();
        fingerprint.update(code.as_bytes());
        fingerprint.update(&[0]);
        fingerprint.update(bytes);
        for observation_path in &observation_paths {
            fingerprint.update(&[0]);
            fingerprint.update(observation_path.to_string_lossy().as_bytes());
            fingerprint.update(&[0]);
            fingerprint.update(observe(observation_path).as_bytes());
        }
        let epoch = fingerprint.finalize().to_hex().to_string();
        Self::Error {
            path,
            observation_paths,
            code: code.to_owned(),
            detail: detail.clone(),
            epoch: epoch.clone(),
            errors: vec![DiagnosticsBaselineErrorSummary {
                partition_id: None,
                code: code.to_owned(),
                detail,
                epoch,
            }],
        }
    }

    fn with_partition_errors(
        mut snapshot: Self,
        error: &PartitionedDiagnosticsBaselineError,
    ) -> Self {
        let Self::Error { epoch, errors, .. } = &mut snapshot else { unreachable!() };
        errors.clear();
        let mut push = |partition_id: Option<String>, code: &str, detail: String| {
            let mut hasher = blake3::Hasher::new();
            hasher.update(epoch.as_bytes());
            hasher.update(&[0]);
            hasher.update(code.as_bytes());
            if let Some(partition_id) = &partition_id {
                hasher.update(&[0]);
                hasher.update(partition_id.as_bytes());
            }
            errors.push(DiagnosticsBaselineErrorSummary {
                partition_id,
                code: code.to_owned(),
                detail,
                epoch: hasher.finalize().to_hex().to_string(),
            });
        };
        match error {
            PartitionedDiagnosticsBaselineError::MissingPartitions { ids, orphan_ids } => {
                for id in ids {
                    push(Some(id.clone()), "missing_partition", format!("missing partition: {id}"));
                }
                for id in orphan_ids {
                    push(Some(id.clone()), "orphan_partition", format!("orphan partition: {id}"));
                }
            }
            PartitionedDiagnosticsBaselineError::OrphanPartitions(ids) => {
                for id in ids {
                    push(Some(id.clone()), "orphan_partition", format!("orphan partition: {id}"));
                }
            }
            _ => {
                let info = error.info();
                push(info.partition_id.map(str::to_owned), info.code, error.to_string());
            }
        }
        snapshot
    }

    pub fn epoch(&self) -> &str {
        match self {
            Self::Disabled => "disabled",
            Self::Ready { epoch, .. }
            | Self::ReadySet { epoch, .. }
            | Self::Error { epoch, .. } => epoch,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Ready { path, .. } | Self::ReadySet { path, .. } => Some(path),
            Self::Error { path, .. } => path.as_deref(),
            Self::Disabled => None,
        }
    }

    pub fn observation(&self) -> String {
        let paths = self.observation_paths();
        if paths.is_empty() {
            "disabled".to_owned()
        } else {
            observe_many(&paths)
        }
    }

    pub fn observation_paths(&self) -> Vec<PathBuf> {
        match self {
            Self::Ready { path, .. } => vec![path.clone()],
            Self::ReadySet { path, baseline, .. } => std::iter::once(path.join("manifest.json"))
                .chain(baseline.manifest.partitions.iter().map(|entry| path.join(&entry.file)))
                .collect(),
            Self::Error { path, observation_paths, .. } => {
                if observation_paths.is_empty() {
                    path.iter().cloned().collect()
                } else {
                    observation_paths.clone()
                }
            }
            Self::Disabled => vec![],
        }
    }

    pub fn ready(&self) -> Option<(&DiagnosticsBaseline, &str)> {
        match self {
            Self::Ready { baseline, project_path, .. } => Some((baseline, project_path)),
            _ => None,
        }
    }

    pub fn ready_set(
        &self,
    ) -> Option<(
        &DiagnosticsBaselineSetSnapshot,
        &project_model::DiagnosticsBaselinePartitionPlan,
        &str,
    )> {
        match self {
            Self::ReadySet { baseline, plan, project_path, .. } => {
                Some((baseline, plan, project_path))
            }
            _ => None,
        }
    }

    pub fn error_summary(&self) -> Option<DiagnosticsBaselineSummary> {
        let Self::Error { path, code, detail, errors, .. } = self else { return None };
        Some(DiagnosticsBaselineSummary {
            state: DiagnosticsBaselineState::Error,
            new: None,
            known: None,
            resolved: None,
            path: path.as_deref().map(normalize_path),
            schema_version: None,
            manifest_schema_version: None,
            complete: false,
            error_code: Some(code.clone()),
            detail: Some(detail.clone()),
            partitions: vec![],
            errors: errors.clone(),
        })
    }

    pub fn errors(&self) -> &[DiagnosticsBaselineErrorSummary] {
        match self {
            Self::Error { errors, .. } => errors,
            _ => &[],
        }
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

fn observe_many(paths: &[PathBuf]) -> String {
    let mut hasher = blake3::Hasher::new();
    for path in paths {
        hasher.update(path.to_string_lossy().as_bytes());
        hasher.update(&[0]);
        hasher.update(observe(path).as_bytes());
        hasher.update(&[0]);
    }
    hasher.finalize().to_hex().to_string()
}

fn object_observations(
    directory: &Path,
    baseline: &DiagnosticsBaselineSetSnapshot,
) -> BTreeMap<String, String> {
    baseline
        .manifest
        .partitions
        .iter()
        .map(|entry| (entry.file.clone(), observe(&directory.join(&entry.file))))
        .collect()
}

fn partitioned_error_observation(
    project_root: &Path,
    project_path: &str,
) -> (Vec<PathBuf>, [u8; 32]) {
    let directory = project_root.join(project_path);
    let manifest_path = directory.join("manifest.json");
    let mut hasher = blake3::Hasher::new();
    let Ok(managed) =
        project_model::ManagedBaselineDirectory::open(project_root, project_path, false)
    else {
        return (vec![], *hasher.finalize().as_bytes());
    };
    let mut paths = vec![manifest_path];
    let Ok(mut manifest_file) = managed.open_file("manifest.json") else {
        return (paths, *hasher.finalize().as_bytes());
    };
    let mut bytes = Vec::new();
    if manifest_file.read_to_end(&mut bytes).is_err() {
        return (paths, *hasher.finalize().as_bytes());
    }
    hasher.update(&bytes);
    if let Ok(manifest) = serde_json::from_slice::<
        ide::partitioned_diagnostics_baseline::DiagnosticsBaselineManifest,
    >(&bytes)
    {
        let mut buffer = [0u8; 64 * 1024];
        for entry in manifest.partitions {
            let Ok(relative) = managed.validated_relative_path(&entry.file) else { continue };
            paths.push(project_root.join(relative));
            let Ok(mut file) = managed.open_file(&entry.file) else { continue };
            hasher.update(entry.file.as_bytes());
            loop {
                match file.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => {
                        hasher.update(&buffer[..read]);
                    }
                }
            }
        }
    }
    (paths, *hasher.finalize().as_bytes())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn partitioned_error_observation_rejects_manifest_path_escape() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("baselines")).unwrap();
        std::fs::write(
            dir.path().join("baselines/manifest.json"),
            r#"{"schema_version":1,"generation":"x","project_scope_fingerprint":"x","partitions":[{"partition_id":"main","file":"../../outside","blake3":"x"}]}"#,
        )
        .unwrap();
        assert_eq!(
            partitioned_error_observation(dir.path(), "baselines").0,
            vec![dir.path().join("baselines/manifest.json")]
        );
    }

    #[test]
    fn partitioned_error_observation_never_enters_symlinked_directory() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("manifest.json"), b"secret").unwrap();
        symlink(outside.path(), dir.path().join("baselines")).unwrap();
        assert!(partitioned_error_observation(dir.path(), "baselines").0.is_empty());
    }

    #[test]
    fn partitioned_error_observation_hashes_manifest_bytes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("baselines")).unwrap();
        let manifest = dir.path().join("baselines/manifest.json");
        std::fs::write(&manifest, b"broken-a").unwrap();
        let first = partitioned_error_observation(dir.path(), "baselines").1;
        std::fs::write(&manifest, b"broken-b").unwrap();
        assert_ne!(partitioned_error_observation(dir.path(), "baselines").1, first);
    }

    #[test]
    fn partitioned_error_summary_preserves_deterministic_error_count() {
        let base = DiagnosticsBaselineSnapshot::error_observed_many(
            None,
            vec![],
            "missing_partition",
            b"missing",
            "missing partitions".to_owned(),
        );
        let snapshot = DiagnosticsBaselineSnapshot::with_partition_errors(
            base,
            &PartitionedDiagnosticsBaselineError::MissingPartitions {
                ids: vec!["extension:A".to_owned(), "extension:B".to_owned()],
                orphan_ids: vec!["extension:Old".to_owned()],
            },
        );
        let summary = snapshot.error_summary().unwrap();
        assert_eq!(summary.errors.len(), 3);
        assert_eq!(summary.errors[0].code, "missing_partition");
        assert_eq!(summary.errors[2].code, "orphan_partition");
    }

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
