use std::{
    error::Error,
    fmt,
    io::Write,
    path::{Path, PathBuf},
};

use clap::{Args, Subcommand, ValueEnum};
use ide::diagnostics_baseline::{
    classify_diagnostics, diagnostics_baseline_json, parse_diagnostics_baseline,
    BaselineDiagnosticCandidate, DiagnosticsBaseline, DiagnosticsBaselineCoverage,
    DiagnosticsBaselineEntry, DiagnosticsBaselineExtension, DiagnosticsBaselineRange,
    DiagnosticsBaselineScope, DiagnosticsBaselineSummary, DIAGNOSTICS_BASELINE_SCHEMA_VERSION,
};
use serde::Serialize;

use bsl_analyzer::reporters::FileAnalysis;

type Candidate<T> = BaselineDiagnosticCandidate<(T, usize)>;

#[derive(Debug, Subcommand)]
pub enum DiagnosticsCommand {
    Baseline {
        #[command(subcommand)]
        command: DiagnosticsBaselineCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum DiagnosticsBaselineCommand {
    Create(DiagnosticsBaselineArgs),
    Check(DiagnosticsBaselineArgs),
    Update(DiagnosticsBaselineArgs),
}

#[derive(Debug, Clone, Args)]
pub struct DiagnosticsBaselineArgs {
    #[arg(short = 's', long = "source-dir", default_value = ".")]
    pub source_dir: PathBuf,

    #[arg(short = 'c', long = "config")]
    pub config: Option<PathBuf>,

    #[arg(long = "format", value_enum, default_value_t)]
    pub format: DiagnosticsBaselineOutputFormat,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum DiagnosticsBaselineOutputFormat {
    #[default]
    Text,
    Json,
}

impl DiagnosticsBaselineCommand {
    pub fn output_format(&self) -> DiagnosticsBaselineOutputFormat {
        match self {
            Self::Create(args) | Self::Check(args) | Self::Update(args) => args.format,
        }
    }
}

pub fn run(command: DiagnosticsCommand) -> Result<(), Box<dyn Error + Send + Sync>> {
    let DiagnosticsCommand::Baseline { command } = command;
    super::analyze::diagnostics_baseline(command)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticsBaselineOperationResult {
    pub operation: &'static str,
    pub path: String,
    pub success: bool,
    pub added: usize,
    pub removed: usize,
    pub unchanged: usize,
    pub diagnostics: Vec<DiagnosticsBaselineEntry>,
}

impl fmt::Display for DiagnosticsBaselineOperationResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Diagnostics baseline {}: {} (added {}, removed {}, unchanged {})",
            self.operation, self.path, self.added, self.removed, self.unchanged
        )?;
        for diagnostic in &self.diagnostics {
            write!(
                f,
                "\n  {} {}:{} {}",
                diagnostic.code,
                diagnostic.path,
                diagnostic.range.start_line + 1,
                diagnostic.message,
            )?;
        }
        Ok(())
    }
}

pub fn apply(
    project: &project_model::Project,
    files: &[FileAnalysis],
    proof: &super::analyze::CoverageProof,
    command: DiagnosticsBaselineCommand,
) -> Result<DiagnosticsBaselineOperationResult, Box<dyn Error + Send + Sync>> {
    proof.require_full()?;
    if project.config.analysis.diff_base.is_some()
        || !project.config.analysis.ignored_authors.is_empty()
    {
        return Err(
            "full diagnostics coverage required: project analysis filters are active".into()
        );
    }

    let resolved =
        project.diagnostics_baseline()?.ok_or("[diagnostics.baseline].path is not configured")?;
    let scope = scope(&resolved.scope);
    let candidates = candidates(project, files.iter().enumerate())?;

    match command {
        DiagnosticsBaselineCommand::Create(_) => {
            if resolved.path.exists() {
                return Err(format!(
                    "diagnostics baseline already exists: {}",
                    resolved.path.display()
                )
                .into());
            }
            let empty = DiagnosticsBaseline {
                schema_version: DIAGNOSTICS_BASELINE_SCHEMA_VERSION,
                scope: scope.clone(),
                diagnostics: Vec::new(),
            };
            let classified = classify_diagnostics(
                &empty,
                resolved.project_path.clone(),
                candidates,
                &DiagnosticsBaselineCoverage::Full,
            )?;
            let diagnostics = classified.new.into_iter().map(|item| item.entry).collect();
            let baseline = DiagnosticsBaseline {
                schema_version: DIAGNOSTICS_BASELINE_SCHEMA_VERSION,
                scope,
                diagnostics,
            };
            let bytes = diagnostics_baseline_json(&baseline)?;
            publish_new(&resolved.path, &bytes)?;
            Ok(DiagnosticsBaselineOperationResult {
                operation: "created",
                path: resolved.project_path,
                success: true,
                added: baseline.diagnostics.len(),
                removed: 0,
                unchanged: 0,
                diagnostics: baseline.diagnostics,
            })
        }
        DiagnosticsBaselineCommand::Check(_) => {
            let baseline = read_baseline(&resolved.path, &scope)?;
            let classified = classify_diagnostics(
                &baseline,
                resolved.project_path.clone(),
                candidates,
                &DiagnosticsBaselineCoverage::Full,
            )?;
            let added = classified.new.len();
            let removed = classified.resolved.len();
            let mut diagnostics: Vec<_> = classified
                .new
                .into_iter()
                .map(|item| item.entry)
                .chain(classified.resolved)
                .collect();
            diagnostics.sort_by(|a, b| a.fingerprint.cmp(&b.fingerprint));
            Ok(DiagnosticsBaselineOperationResult {
                operation: "checked",
                path: resolved.project_path,
                success: added == 0 && removed == 0,
                added,
                removed,
                unchanged: classified.known.len(),
                diagnostics,
            })
        }
        DiagnosticsBaselineCommand::Update(_) => {
            let baseline = read_baseline(&resolved.path, &scope)?;
            let classified = classify_diagnostics(
                &baseline,
                resolved.project_path.clone(),
                candidates,
                &DiagnosticsBaselineCoverage::Full,
            )?;
            let added = classified.new.len();
            let removed = classified.resolved.len();
            let unchanged = classified.known.len();
            let mut diagnostics: Vec<_> =
                classified.new.into_iter().chain(classified.known).map(|item| item.entry).collect();
            let bytes = diagnostics_baseline_json(&DiagnosticsBaseline {
                schema_version: DIAGNOSTICS_BASELINE_SCHEMA_VERSION,
                scope,
                diagnostics: diagnostics.clone(),
            })?;
            replace(&resolved.path, &bytes)?;
            diagnostics.sort_by(|a, b| a.fingerprint.cmp(&b.fingerprint));
            Ok(DiagnosticsBaselineOperationResult {
                operation: "updated",
                path: resolved.project_path,
                success: true,
                added,
                removed,
                unchanged,
                diagnostics,
            })
        }
    }
}

fn scope(scope: &project_model::DiagnosticsBaselineProjectScope) -> DiagnosticsBaselineScope {
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

fn candidates<'a, T: Clone + 'a>(
    project: &project_model::Project,
    files: impl IntoIterator<Item = (T, &'a FileAnalysis)>,
) -> Result<Vec<Candidate<T>>, Box<dyn Error + Send + Sync>> {
    let root = project.root.canonicalize()?;
    let mut candidates = Vec::new();
    for (file_id, file) in files {
        let path = file
            .path
            .strip_prefix(&root)?
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        for (index, diagnostic) in file.diagnostics.iter().enumerate() {
            let snippet = file.line_snippets.get(index).cloned();
            candidates.push(BaselineDiagnosticCandidate {
                diagnostic: (file_id.clone(), index),
                path: path.clone(),
                code: diagnostic.code.clone(),
                snippet,
                message: diagnostic.message.clone(),
                severity: diagnostic.severity.clone(),
                range: DiagnosticsBaselineRange {
                    start_line: diagnostic.start_line.try_into()?,
                    start_column: diagnostic.start_column.try_into()?,
                    end_line: diagnostic.end_line.try_into()?,
                    end_column: diagnostic.end_column.try_into()?,
                },
            });
        }
    }
    Ok(candidates)
}

#[cfg(test)]
pub fn classify_files(
    project: &project_model::Project,
    files: &mut [Option<FileAnalysis>],
    coverage: DiagnosticsBaselineCoverage,
) -> Result<DiagnosticsBaselineSummary, Box<dyn Error + Send + Sync>> {
    let loaded = ide_host_core::diagnostics_baseline::DiagnosticsBaselineSnapshot::load(project);
    classify_files_with_loaded(project, files, coverage, &loaded)
}

pub fn classify_files_with_loaded(
    project: &project_model::Project,
    files: &mut [Option<FileAnalysis>],
    coverage: DiagnosticsBaselineCoverage,
    loaded: &ide_host_core::diagnostics_baseline::DiagnosticsBaselineSnapshot,
) -> Result<DiagnosticsBaselineSummary, Box<dyn Error + Send + Sync>> {
    use ide_host_core::diagnostics_baseline::DiagnosticsBaselineSnapshot;
    let (baseline, project_path) = match loaded {
        DiagnosticsBaselineSnapshot::Disabled => return Ok(DiagnosticsBaselineSummary::disabled()),
        DiagnosticsBaselineSnapshot::Ready { baseline, project_path, .. } => {
            (baseline, project_path)
        }
        DiagnosticsBaselineSnapshot::Error { detail, .. } => return Err(detail.clone().into()),
    };
    let current = candidates(
        project,
        files
            .iter()
            .enumerate()
            .filter_map(|(index, file)| file.as_ref().map(|file| (index, file))),
    )?;
    let classified = classify_diagnostics(baseline, project_path.clone(), current, &coverage)?;
    let known: std::collections::HashSet<_> =
        classified.known.iter().map(|item| item.diagnostic).collect();
    for (file_index, file) in files.iter_mut().enumerate() {
        let Some(analysis) = file else { continue };
        let mut diagnostic_index = 0;
        analysis.diagnostics.retain(|_| {
            let keep = !known.contains(&(file_index, diagnostic_index));
            diagnostic_index += 1;
            keep
        });
        let mut snippet_index = 0;
        analysis.line_snippets.retain(|_| {
            let keep = !known.contains(&(file_index, snippet_index));
            snippet_index += 1;
            keep
        });
        if analysis.diagnostics.is_empty() {
            *file = None;
        }
    }
    Ok(classified.summary)
}

fn read_baseline(
    path: &Path,
    scope: &DiagnosticsBaselineScope,
) -> Result<DiagnosticsBaseline, Box<dyn Error + Send + Sync>> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("cannot read diagnostics baseline {}: {error}", path.display()))?;
    Ok(parse_diagnostics_baseline(&bytes, scope)?)
}

fn synced_temp(path: &Path, bytes: &[u8]) -> Result<tempfile::NamedTempFile, std::io::Error> {
    let parent = path.parent().ok_or_else(|| std::io::Error::other("missing parent directory"))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    Ok(temp)
}

fn publish_new(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error + Send + Sync>> {
    publish_new_with(path, bytes, |source, target| std::fs::hard_link(source, target))
}

fn publish_new_with(
    path: &Path,
    bytes: &[u8],
    link: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let temp = synced_temp(path, bytes)?;
    link(temp.path(), path).map_err(|error| {
        format!("cannot atomically create diagnostics baseline {}: {error}", path.display())
    })?;
    Ok(())
}

fn replace(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error + Send + Sync>> {
    replace_with(path, bytes, |temp, target| {
        temp.persist(target)
            .map(|_| ())
            .map_err(|error| std::io::Error::new(error.error.kind(), error.error.to_string()))
    })
}

fn replace_with(
    path: &Path,
    bytes: &[u8],
    persist: impl FnOnce(tempfile::NamedTempFile, &Path) -> std::io::Result<()>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let temp = synced_temp(path, bytes)?;
    persist(temp, path).map_err(|error| {
        format!("cannot atomically replace diagnostics baseline {}: {error}", path.display())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide::DiagnosticOutput;
    use project_model::{DiagnosticsBaselineConfig, Project, ProjectConfig};

    fn args() -> DiagnosticsBaselineArgs {
        DiagnosticsBaselineArgs {
            source_dir: PathBuf::from("."),
            config: None,
            format: DiagnosticsBaselineOutputFormat::Text,
        }
    }

    fn project(root: &Path, configured: bool) -> Project {
        let mut config = ProjectConfig::default();
        if configured {
            config.diagnostics.baseline =
                Some(DiagnosticsBaselineConfig { path: "baseline.json".to_owned() });
        }
        Project::with_config(root, config).unwrap()
    }

    fn file(root: &Path) -> FileAnalysis {
        let path = root.join("module.bsl");
        std::fs::write(&path, "x = 1;\n").unwrap();
        FileAnalysis {
            path: path.clone(),
            relative_path: PathBuf::from("module.bsl"),
            diagnostics: vec![DiagnosticOutput {
                code: "UnusedLocalVariable".to_owned(),
                message: "unused".to_owned(),
                severity: "Warning".to_owned(),
                start_line: 0,
                start_column: 0,
                end_line: 0,
                end_column: 1,
                tags: Vec::new(),
            }],
            line_snippets: vec!["x = 1;".to_owned()],
        }
    }

    fn full() -> super::super::analyze::CoverageProof {
        super::super::analyze::CoverageProof { total: 1, analyzed: 1, ..Default::default() }
    }

    #[test]
    fn diagnostics_baseline_create_reports_text_and_machine_result() {
        let dir = tempfile::tempdir().unwrap();
        let project = project(dir.path(), true);
        let result = apply(
            &project,
            &[file(dir.path())],
            &full(),
            DiagnosticsBaselineCommand::Create(args()),
        )
        .unwrap();
        assert_eq!((result.added, result.removed, result.unchanged), (1, 0, 0));
        assert_eq!(result.diagnostics[0].path, "module.bsl");
        assert!(result.to_string().contains("added 1"));
        assert!(result.to_string().contains("UnusedLocalVariable module.bsl:1"));
        assert_eq!(serde_json::to_value(&result).unwrap()["added"], 1);
        assert!(dir.path().join("baseline.json").is_file());
    }

    #[test]
    fn diagnostics_baseline_create_requires_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let error = apply(
            &project(dir.path(), false),
            &[],
            &full(),
            DiagnosticsBaselineCommand::Create(args()),
        )
        .unwrap_err();
        assert!(error.to_string().contains("not configured"));
    }

    #[test]
    fn diagnostics_baseline_create_rejects_missing_snippet_before_write() {
        let dir = tempfile::tempdir().unwrap();
        let project = project(dir.path(), true);
        let mut file = file(dir.path());
        file.line_snippets.clear();

        let error = apply(&project, &[file], &full(), DiagnosticsBaselineCommand::Create(args()))
            .unwrap_err();

        assert!(error.to_string().contains("snippet"));
        assert!(!dir.path().join("baseline.json").exists());
    }

    #[test]
    fn diagnostics_baseline_create_never_overwrites_a_race_winner() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("baseline.json");
        publish_new(&target, b"winner").unwrap();
        assert!(publish_new(&target, b"loser").is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"winner");
    }

    #[test]
    fn diagnostics_baseline_create_cleans_temp_after_unsupported_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("baseline.json");
        let error = publish_new_with(&target, b"value", |_, _| {
            Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "hard links unsupported"))
        })
        .unwrap_err();
        assert!(error.to_string().contains("hard links unsupported"));
        assert!(!target.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    fn create_baseline(dir: &Path) -> (Project, Vec<FileAnalysis>, Vec<u8>) {
        let project = project(dir, true);
        let files = vec![file(dir)];
        apply(&project, &files, &full(), DiagnosticsBaselineCommand::Create(args())).unwrap();
        let bytes = std::fs::read(dir.join("baseline.json")).unwrap();
        (project, files, bytes)
    }

    fn assert_check_keeps_bytes(project: &Project, files: &[FileAnalysis], expected_ok: bool) {
        let path = project.root.join("baseline.json");
        let before = std::fs::read(&path).unwrap();
        let result = apply(project, files, &full(), DiagnosticsBaselineCommand::Check(args()));
        match result {
            Ok(result) => assert_eq!(result.success, expected_ok, "{result:?}"),
            Err(_) => assert!(!expected_ok),
        }
        assert_eq!(std::fs::read(path).unwrap(), before);
    }

    #[test]
    fn diagnostics_baseline_check_is_read_only_for_every_outcome() {
        let dir = tempfile::tempdir().unwrap();
        let (project, files, valid) = create_baseline(dir.path());
        assert_check_keeps_bytes(&project, &files, true);

        let mut new = files.clone();
        new[0].diagnostics[0].code = "MagicNumber".to_owned();
        assert_check_keeps_bytes(&project, &new, false);
        assert_check_keeps_bytes(&project, &[], false);

        let mut protected = files.clone();
        protected[0].diagnostics[0].code = "UnknownSuppressionCode".to_owned();
        assert_check_keeps_bytes(&project, &protected, false);

        std::fs::write(dir.path().join("baseline.json"), b"not json").unwrap();
        assert_check_keeps_bytes(&project, &files, false);
        assert_ne!(std::fs::read(dir.path().join("baseline.json")).unwrap(), valid);
    }

    #[test]
    fn diagnostics_baseline_update_refreshes_fields_and_reports_counts() {
        let dir = tempfile::tempdir().unwrap();
        let (project, mut files, _) = create_baseline(dir.path());
        files[0].diagnostics[0].message = "refreshed".to_owned();
        files[0].diagnostics[0].start_line = 7;
        files[0].diagnostics[0].end_line = 7;
        let result =
            apply(&project, &files, &full(), DiagnosticsBaselineCommand::Update(args())).unwrap();
        assert_eq!((result.added, result.removed, result.unchanged), (0, 0, 1));

        let resolved = project.diagnostics_baseline().unwrap().unwrap();
        let baseline = read_baseline(&resolved.path, &scope(&resolved.scope)).unwrap();
        assert_eq!(baseline.diagnostics[0].message, "refreshed");
        assert_eq!(baseline.diagnostics[0].range.start_line, 7);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);

        files[0].diagnostics[0].code = "MagicNumber".to_owned();
        let result =
            apply(&project, &files, &full(), DiagnosticsBaselineCommand::Update(args())).unwrap();
        assert_eq!((result.added, result.removed, result.unchanged), (1, 1, 0));
    }

    #[test]
    fn diagnostics_baseline_update_preserves_old_bytes_when_replace_fails() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("baseline.json");
        std::fs::write(&target, b"old").unwrap();
        let error = replace_with(&target, b"new", |_, _| {
            Err(std::io::Error::other("injected replacement failure"))
        })
        .unwrap_err();
        assert!(error.to_string().contains("injected replacement failure"));
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn analyze_diagnostics_baseline_filters_known_after_existing_suppressions() {
        let dir = tempfile::tempdir().unwrap();
        let (project, mut files, _) = create_baseline(dir.path());
        let mut active = files[0].diagnostics[0].clone();
        active.code = "MagicNumber".to_owned();
        active.message = "new".to_owned();
        let mut protected = active.clone();
        protected.code = "SuppressionWithoutCode".to_owned();
        files[0].diagnostics.extend([active, protected]);
        files[0].line_snippets.extend(["y = 2;".to_owned(), "z = 3;".to_owned()]);
        let mut results = vec![Some(files.remove(0))];

        let summary =
            classify_files(&project, &mut results, DiagnosticsBaselineCoverage::Full).unwrap();
        assert_eq!((summary.new, summary.known, summary.resolved), (Some(2), Some(1), Some(0)));
        let active = &results[0].as_ref().unwrap().diagnostics;
        assert_eq!(active.len(), 2);
        assert!(active.iter().any(|diagnostic| diagnostic.code == "MagicNumber"));
        assert!(active.iter().any(|diagnostic| diagnostic.code == "SuppressionWithoutCode"));

        // Diagnostics disabled by project rules or source directives never enter this
        // post-suppression list, so they cannot become baseline entries.
        assert!(!active.iter().any(|diagnostic| diagnostic.code == "DisabledByRule"));
        assert!(!active.iter().any(|diagnostic| diagnostic.code == "SuppressedInSource"));

        let disabled_project = self::project(dir.path(), false);
        let mut untouched = results.clone();
        let disabled =
            classify_files(&disabled_project, &mut untouched, DiagnosticsBaselineCoverage::Full)
                .unwrap();
        assert_eq!(disabled.state, ide::diagnostics_baseline::DiagnosticsBaselineState::Disabled);
        assert_eq!(
            untouched[0].as_ref().unwrap().diagnostics,
            results[0].as_ref().unwrap().diagnostics
        );
    }

    #[test]
    fn analyze_baseline_partial_scope_resolves_only_completed_files() {
        let dir = tempfile::tempdir().unwrap();
        let project = project(dir.path(), true);
        let first = file(dir.path());
        let mut second = first.clone();
        second.path = dir.path().join("second.bsl");
        second.relative_path = PathBuf::from("second.bsl");
        std::fs::write(&second.path, "x = 1;\n").unwrap();
        apply(
            &project,
            &[first, second],
            &super::super::analyze::CoverageProof { total: 2, analyzed: 2, ..Default::default() },
            DiagnosticsBaselineCommand::Create(args()),
        )
        .unwrap();

        let mut no_current_diagnostics = Vec::<Option<FileAnalysis>>::new();
        let summary = classify_files(
            &project,
            &mut no_current_diagnostics,
            DiagnosticsBaselineCoverage::Partial {
                completed_files: std::collections::BTreeSet::from(["module.bsl".to_owned()]),
            },
        )
        .unwrap();
        assert_eq!(summary.state, ide::diagnostics_baseline::DiagnosticsBaselineState::Partial);
        assert_eq!(summary.resolved, Some(1));
    }
}
