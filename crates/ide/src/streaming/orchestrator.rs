use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::unbounded;
use rustc_hash::FxHashMap;
use tracing::{debug, error, info, warn};
use vfs::{file_set::FileSet, FileId};

use hir::{ItemTree, ModuleId, SymbolTree, WorkspaceSymbols};

use ide_db::streaming::{FileReader, GlobalContext, SharedState, StreamingProvider};

use ide_diagnostics::DiagnosticsConfig;

use super::file_processor::FileResult;
use super::worker_thread::worker_main;

#[derive(Debug, Clone)]
pub struct AnalysisResults {
    pub total_files: usize,

    pub total_diagnostics: usize,

    pub failed_files: usize,

    pub file_results: Vec<FileResult>,
}

#[derive(Debug, thiserror::Error)]
pub enum OrchestratorError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("Worker pool error: {0}")]
    WorkerPool(String),

    #[error("No files to analyze")]
    NoFiles,
}

pub struct AnalysisOrchestrator {
    num_workers: usize,

    workspace_root: PathBuf,

    configuration_path: Option<PathBuf>,

    diagnostics_config: Option<DiagnosticsConfig>,
}

impl AnalysisOrchestrator {
    pub fn builder() -> OrchestratorBuilder {
        OrchestratorBuilder::default()
    }

    pub fn analyze(
        &self,
        files: Vec<FileId>,
        file_set: FileSet,
    ) -> Result<AnalysisResults, OrchestratorError> {
        let _span = tracing::info_span!("orchestrator_analyze", num_files = files.len()).entered();

        if files.is_empty() {
            return Err(OrchestratorError::NoFiles);
        }

        info!(num_files = files.len(), num_workers = self.num_workers, "Starting batch analysis");

        let global_context = self.initialize_global_context(&files, file_set)?;

        let sorted_files = self.sort_files_by_priority(files, &global_context);
        let shared_state = SharedState::new(global_context.as_ref().clone(), sorted_files.clone());
        let provider = Arc::new(StreamingProvider::with_shared_state(
            global_context,
            Arc::clone(&shared_state),
        ));

        let config = Arc::new(self.load_diagnostics_config());

        let (results_tx, results_rx) = unbounded();

        let workers = self.spawn_workers(provider, shared_state, config, results_tx)?;

        let results = self.collect_results(results_rx, workers, sorted_files.len());

        info!(
            total_files = results.total_files,
            total_diagnostics = results.total_diagnostics,
            failed_files = results.failed_files,
            "Batch analysis completed"
        );

        Ok(results)
    }

    pub fn analyze_with_progress<F>(
        &self,
        files: Vec<FileId>,
        file_set: FileSet,
        mut on_progress: F,
    ) -> Result<AnalysisResults, OrchestratorError>
    where
        F: FnMut(usize, usize),
    {
        let _span = tracing::info_span!("orchestrator_analyze", num_files = files.len()).entered();

        if files.is_empty() {
            return Err(OrchestratorError::NoFiles);
        }

        let total_files = files.len();
        info!(num_files = total_files, num_workers = self.num_workers, "Starting batch analysis");

        let global_context = self.initialize_global_context(&files, file_set)?;

        let sorted_files = self.sort_files_by_priority(files, &global_context);
        let shared_state = SharedState::new(global_context.as_ref().clone(), sorted_files.clone());
        let provider = Arc::new(StreamingProvider::with_shared_state(
            global_context,
            Arc::clone(&shared_state),
        ));

        let config = Arc::new(self.load_diagnostics_config());

        let (results_tx, results_rx) = unbounded();

        let workers = self.spawn_workers(provider, shared_state, config, results_tx)?;

        let results =
            self.collect_results_with_progress(results_rx, workers, total_files, &mut on_progress);

        info!(
            total_files = results.total_files,
            total_diagnostics = results.total_diagnostics,
            failed_files = results.failed_files,
            "Batch analysis completed"
        );

        Ok(results)
    }

    fn initialize_global_context(
        &self,
        files: &[FileId],
        file_set: FileSet,
    ) -> Result<Arc<GlobalContext>, OrchestratorError> {
        let _span = tracing::info_span!("initialize_global_context").entered();

        info!(num_files = files.len(), "Building GlobalContext");

        let (configuration, config_root) = self.load_metadata();

        let file_set_arc = Arc::new(file_set.clone());
        let file_reader = FileReader::from_disk(self.workspace_root.clone(), file_set_arc.clone());

        use rayon::prelude::*;
        let all_file_ids: Vec<FileId> =
            file_set.iter().filter(|&file_id| hir::is_bsl_source(&file_set, file_id)).collect();
        info!(num_files = all_file_ids.len(), "Building SymbolTrees");

        let symbol_trees: FxHashMap<_, _> = all_file_ids
            .par_iter()
            .map(|&file_id| {
                let text = file_reader.read(file_id).unwrap_or_default();
                let parse = parser::parse(&text);
                let item_tree = ItemTree::from_parse(&parse);
                let module_id = ModuleId::new(file_id);
                let symbol_tree =
                    Arc::new(SymbolTree::from_item_tree(&item_tree, module_id, &parse, &text));
                (file_id, symbol_tree)
            })
            .collect();

        info!(num_symbol_trees = symbol_trees.len(), "SymbolTrees built successfully");

        let workspace_symbols = Arc::new(WorkspaceSymbols::default());
        info!("WorkspaceSymbols built (empty for now)");

        let module_index = {
            let paths = all_file_ids.iter().filter_map(|&file_id| {
                let vfs_path = file_set.path_for_file(&file_id)?;
                let path_str = vfs_path.as_path().to_str()?;
                Some((file_id, path_str))
            });
            Arc::new(hir::ModuleIndex::build_from_paths(paths))
        };
        info!(
            common_modules = module_index.common_module_count(),
            managers = module_index.manager_count(),
            "ModuleIndex built"
        );

        Ok(Arc::new(GlobalContext {
            configuration,
            symbol_trees,
            workspace_symbols,
            module_index,
            file_set: file_set_arc,
            file_reader,
            config_root,
        }))
    }

    fn sort_files_by_priority(
        &self,
        files: Vec<FileId>,
        global_context: &Arc<GlobalContext>,
    ) -> Vec<FileId> {
        let _span =
            tracing::info_span!("sort_files_by_priority", num_files = files.len()).entered();

        let configuration = global_context.configuration.as_ref();
        let file_set = &global_context.file_set;
        let workspace_root = &self.workspace_root;

        let mut files_with_priority: Vec<(FileId, u8, u64)> = files
            .into_iter()
            .map(|file_id| {
                let vfs_path = file_set.path_for_file(&file_id);
                let priority = vfs_path
                    .map(|p| super::file_priority::compute_priority(p.as_path(), configuration))
                    .unwrap_or(super::file_priority::priority::OTHER);

                let file_size = vfs_path
                    .and_then(|p| {
                        let path = if p.as_path().is_absolute() {
                            p.as_path().to_path_buf()
                        } else {
                            workspace_root.join(p.as_path())
                        };
                        std::fs::metadata(&path).ok().map(|m| m.len())
                    })
                    .unwrap_or(0);

                (file_id, priority, file_size)
            })
            .collect();

        files_with_priority.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| b.2.cmp(&a.2)));

        if tracing::enabled!(tracing::Level::DEBUG) {
            let mut counts = [0usize; 8];
            for &(_, priority, _) in &files_with_priority {
                if (priority as usize) < counts.len() {
                    counts[priority as usize] += 1;
                }
            }
            debug!(
                server = counts[0],
                server_call = counts[1],
                client_server = counts[2],
                client = counts[3],
                manager = counts[4],
                object = counts[5],
                form = counts[6],
                other = counts[7],
                "file priority distribution"
            );
        }

        files_with_priority.into_iter().map(|(file_id, _, _)| file_id).collect()
    }

    fn load_metadata(&self) -> (Option<Arc<bsl_metadata::Configuration>>, Option<PathBuf>) {
        let proj_config = if let Some(ref path) = self.configuration_path {
            project_model::ProjectConfig::load_from_file(path)
        } else {
            project_model::ProjectConfig::load(&self.workspace_root)
        };

        let Some(proj_config) = proj_config else {
            return (None, None);
        };

        let config_root = proj_config.configuration_path(&self.workspace_root);
        let metadata = proj_config.load_metadata(&self.workspace_root).map(Arc::new);
        (metadata, config_root)
    }

    fn load_diagnostics_config(&self) -> DiagnosticsConfig {
        if let Some(ref config) = self.diagnostics_config {
            info!("Using pre-configured DiagnosticsConfig");
            return config.clone();
        }

        let proj_config = if let Some(ref path) = self.configuration_path {
            project_model::ProjectConfig::load_from_file(path)
        } else {
            project_model::ProjectConfig::load(&self.workspace_root)
        };

        let Some(proj_config) = proj_config else {
            info!("No config file found, using default DiagnosticsConfig");
            return DiagnosticsConfig::default();
        };

        let output_locale = proj_config.output.resolve_locale().unwrap_or_default();

        match serde_json::from_value::<DiagnosticsConfig>(proj_config.diagnostics) {
            Ok(mut config) => {
                info!("Loaded diagnostics config from project file");
                config.locale = output_locale;
                config
            }
            Err(e) => {
                warn!(error = %e, "Failed to parse diagnostics config, using default");
                DiagnosticsConfig { locale: output_locale, ..Default::default() }
            }
        }
    }

    fn spawn_workers(
        &self,
        provider: Arc<StreamingProvider>,
        shared_state: Arc<SharedState>,
        config: Arc<DiagnosticsConfig>,
        results_tx: crossbeam_channel::Sender<FileResult>,
    ) -> Result<Vec<thread::JoinHandle<()>>, OrchestratorError> {
        let _span = tracing::info_span!("spawn_workers").entered();

        info!(num_workers = self.num_workers, "Spawning worker pool");

        let mut handles = Vec::with_capacity(self.num_workers);

        for worker_id in 0..self.num_workers {
            let provider = Arc::clone(&provider);
            let shared_state = Arc::clone(&shared_state);
            let config = Arc::clone(&config);
            let results_tx = results_tx.clone();

            let handle = thread::Builder::new()
                .name(format!("worker-{}", worker_id))
                .spawn(move || {
                    worker_main(worker_id, shared_state, provider, config, results_tx);
                })
                .map_err(|e| {
                    OrchestratorError::WorkerPool(format!(
                        "Failed to spawn worker {}: {}",
                        worker_id, e
                    ))
                })?;

            handles.push(handle);
        }

        Ok(handles)
    }

    fn collect_results(
        &self,
        results_rx: crossbeam_channel::Receiver<FileResult>,
        workers: Vec<thread::JoinHandle<()>>,
        expected_files: usize,
    ) -> AnalysisResults {
        let _span = tracing::info_span!("collect_results").entered();

        info!(expected_files, "Collecting results");

        let mut file_results = Vec::with_capacity(expected_files);
        let mut total_diagnostics = 0;
        let mut failed_files = 0;

        for result in results_rx {
            if result.error.is_some() {
                failed_files += 1;
            }

            total_diagnostics += result.diagnostics.len();
            file_results.push(result);
        }

        info!(collected_files = file_results.len(), expected_files, "All results collected");

        for (idx, handle) in workers.into_iter().enumerate() {
            if let Err(e) = handle.join() {
                error!(worker_id = idx, error = ?e, "Worker thread panicked");
            }
        }

        info!("All workers exited");

        AnalysisResults {
            total_files: file_results.len(),
            total_diagnostics,
            failed_files,
            file_results,
        }
    }

    fn collect_results_with_progress<F>(
        &self,
        results_rx: crossbeam_channel::Receiver<FileResult>,
        workers: Vec<thread::JoinHandle<()>>,
        expected_files: usize,
        on_progress: &mut F,
    ) -> AnalysisResults
    where
        F: FnMut(usize, usize),
    {
        let _span = tracing::info_span!("collect_results_with_progress").entered();

        info!(expected_files, "Collecting results with progress");

        let mut file_results = Vec::with_capacity(expected_files);
        let mut total_diagnostics = 0;
        let mut failed_files = 0;

        for result in results_rx {
            if result.error.is_some() {
                failed_files += 1;
            }

            total_diagnostics += result.diagnostics.len();
            file_results.push(result);

            on_progress(file_results.len(), expected_files);
        }

        info!(collected_files = file_results.len(), expected_files, "All results collected");

        for (idx, handle) in workers.into_iter().enumerate() {
            if let Err(e) = handle.join() {
                error!(worker_id = idx, error = ?e, "Worker thread panicked");
            }
        }

        info!("All workers exited");

        AnalysisResults {
            total_files: file_results.len(),
            total_diagnostics,
            failed_files,
            file_results,
        }
    }

    pub fn analyze_jsonl(
        &self,
        files: Vec<FileId>,
        file_set: FileSet,
    ) -> Result<super::jsonl::JsonlSummary, OrchestratorError> {
        use std::time::Instant;

        use super::jsonl::{DoneEvent, StartEvent};

        let _span =
            tracing::info_span!("orchestrator_analyze_jsonl", num_files = files.len()).entered();

        if files.is_empty() {
            return Err(OrchestratorError::NoFiles);
        }

        let start = Instant::now();
        info!(
            num_files = files.len(),
            num_workers = self.num_workers,
            "Starting JSONL streaming analysis"
        );

        let start_event = StartEvent::new(files.len());
        println!(
            "{}",
            serde_json::to_string(&start_event).expect("StartEvent serialization failed")
        );

        let global_context = self.initialize_global_context(&files, file_set.clone())?;

        let sorted_files = self.sort_files_by_priority(files, &global_context);
        let shared_state = SharedState::new(global_context.as_ref().clone(), sorted_files);
        let provider = Arc::new(StreamingProvider::with_shared_state(
            global_context,
            Arc::clone(&shared_state),
        ));

        let config = Arc::new(self.load_diagnostics_config());

        let (results_tx, results_rx) = unbounded();

        let workers = self.spawn_workers(provider, shared_state.clone(), config, results_tx)?;

        let summary = self.stream_jsonl_results(results_rx, workers, &file_set);

        let done_event = DoneEvent::new(
            start.elapsed().as_secs_f64(),
            summary.total_files,
            summary.total_diagnostics,
            summary.failed_files,
        );
        println!("{}", serde_json::to_string(&done_event).expect("DoneEvent serialization failed"));

        info!(
            total_files = summary.total_files,
            total_diagnostics = summary.total_diagnostics,
            failed_files = summary.failed_files,
            elapsed_secs = start.elapsed().as_secs_f64(),
            "JSONL streaming analysis completed"
        );

        Ok(summary)
    }

    fn stream_jsonl_results(
        &self,
        results_rx: crossbeam_channel::Receiver<FileResult>,
        workers: Vec<thread::JoinHandle<()>>,
        file_set: &FileSet,
    ) -> super::jsonl::JsonlSummary {
        use super::jsonl::{FileEvent, JsonlSummary};

        let _span = tracing::info_span!("stream_jsonl_results").entered();

        let mut total_diagnostics = 0;
        let mut total_files = 0;
        let mut failed_files = 0;

        for result in results_rx {
            let path = file_set
                .path_for_file(&result.file_id)
                .map(|p| p.as_path().display().to_string())
                .unwrap_or_else(|| format!("file:{}", result.file_id.index()));

            let file_event = FileEvent::new(
                path,
                result.diagnostics.clone(),
                result.metrics.clone(),
                result.error.as_ref().map(|e| e.to_string()),
            );

            println!(
                "{}",
                serde_json::to_string(&file_event).expect("FileEvent serialization failed")
            );

            total_diagnostics += result.diagnostics.len();
            total_files += 1;
            if result.error.is_some() {
                failed_files += 1;
            }
        }

        info!(total_files, "All JSONL results streamed");

        for (idx, handle) in workers.into_iter().enumerate() {
            if let Err(e) = handle.join() {
                error!(worker_id = idx, error = ?e, "Worker thread panicked");
            }
        }

        info!("All workers exited");

        JsonlSummary { total_files, total_diagnostics, failed_files }
    }
}

#[derive(Default)]
pub struct OrchestratorBuilder {
    num_workers: Option<usize>,
    workspace_root: Option<PathBuf>,
    configuration_path: Option<PathBuf>,
    diagnostics_config: Option<DiagnosticsConfig>,
}

impl OrchestratorBuilder {
    pub fn num_workers(mut self, n: usize) -> Self {
        self.num_workers = Some(n);
        self
    }

    pub fn workspace_root<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.workspace_root = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn configuration_path<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.configuration_path = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn diagnostics_config(mut self, config: DiagnosticsConfig) -> Self {
        self.diagnostics_config = Some(config);
        self
    }

    pub fn build(self) -> Result<AnalysisOrchestrator, OrchestratorError> {
        let workspace_root = self
            .workspace_root
            .ok_or_else(|| OrchestratorError::Configuration("workspace_root is required".into()))?;

        let num_workers = self.num_workers.unwrap_or_else(|| {
            let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
            info!(num_workers = cpus, "Using default worker count (CPU cores)");
            cpus
        });

        Ok(AnalysisOrchestrator {
            num_workers,
            workspace_root,
            configuration_path: self.configuration_path,
            diagnostics_config: self.diagnostics_config,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    use vfs::VfsPath;

    fn create_test_workspace(files: Vec<(&str, &str)>) -> (TempDir, Vec<FileId>, FileSet) {
        let temp_dir = TempDir::new().unwrap();

        let mut file_set = FileSet::default();
        let mut file_ids = Vec::new();

        for (idx, (name, content)) in files.iter().enumerate() {
            let file_path = temp_dir.path().join(name);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&file_path, content).unwrap();

            let file_id = FileId(idx as u32);
            let vfs_path = VfsPath::new(file_path.to_str().unwrap());
            file_set.insert(file_id, vfs_path);
            file_ids.push(file_id);
        }

        (temp_dir, file_ids, file_set)
    }

    #[test]
    fn test_orchestrator_builder() {
        let result =
            AnalysisOrchestrator::builder().workspace_root("/tmp/test").num_workers(2).build();

        assert!(result.is_ok());
        let orchestrator = result.unwrap();
        assert_eq!(orchestrator.num_workers, 2);
        assert_eq!(orchestrator.workspace_root, PathBuf::from("/tmp/test"));
    }

    #[test]
    fn test_orchestrator_builder_default_workers() {
        let result = AnalysisOrchestrator::builder().workspace_root("/tmp/test").build();

        assert!(result.is_ok());
        let orchestrator = result.unwrap();
        assert!(orchestrator.num_workers > 0);
    }

    #[test]
    fn test_orchestrator_builder_missing_workspace() {
        let result = AnalysisOrchestrator::builder().num_workers(2).build();

        assert!(result.is_err());
        assert!(matches!(result, Err(OrchestratorError::Configuration(_))));
    }

    #[test]
    fn test_analyze_empty_files() {
        let temp_dir = TempDir::new().unwrap();
        let orchestrator = AnalysisOrchestrator::builder()
            .workspace_root(temp_dir.path())
            .num_workers(1)
            .build()
            .unwrap();

        let file_set = FileSet::default();
        let result = orchestrator.analyze(vec![], file_set);

        assert!(result.is_err());
        assert!(matches!(result, Err(OrchestratorError::NoFiles)));
    }

    #[test]
    fn test_analyze_single_file() {
        let (_temp_dir, file_ids, file_set) =
            create_test_workspace(vec![("test.bsl", "Процедура Тест() КонецПроцедуры")]);

        let orchestrator = AnalysisOrchestrator::builder()
            .workspace_root(_temp_dir.path())
            .num_workers(1)
            .build()
            .unwrap();

        let result = orchestrator.analyze(file_ids, file_set);

        assert!(result.is_ok());
        let results = result.unwrap();
        assert_eq!(results.total_files, 1);
        assert_eq!(results.failed_files, 0);
    }

    #[test]
    fn test_analyze_multiple_files() {
        let (_temp_dir, file_ids, file_set) = create_test_workspace(vec![
            ("module1.bsl", "Процедура А() КонецПроцедуры"),
            ("module2.bsl", "Функция Б() КонецФункции"),
            ("module3.bsl", "Процедура В() КонецПроцедуры"),
        ]);

        let orchestrator = AnalysisOrchestrator::builder()
            .workspace_root(_temp_dir.path())
            .num_workers(2)
            .build()
            .unwrap();

        let result = orchestrator.analyze(file_ids, file_set);

        assert!(result.is_ok());
        let results = result.unwrap();
        assert_eq!(results.total_files, 3);
        assert_eq!(results.failed_files, 0);
    }

    #[test]
    fn test_analyze_with_parse_errors() {
        let (_temp_dir, file_ids, file_set) = create_test_workspace(vec![
            ("valid.bsl", "Процедура Тест() КонецПроцедуры"),
            ("invalid.bsl", "Процедура Ошибка( КонецПроцедуры"),
        ]);

        let orchestrator = AnalysisOrchestrator::builder()
            .workspace_root(_temp_dir.path())
            .num_workers(1)
            .build()
            .unwrap();

        let result = orchestrator.analyze(file_ids, file_set);

        assert!(result.is_ok());
        let results = result.unwrap();
        assert_eq!(results.total_files, 2);
    }

    #[test]
    fn test_analyze_concurrent_workers() {
        let files = (0..10)
            .map(|i| (format!("module{}.bsl", i), format!("Процедура Метод{}() КонецПроцедуры", i)))
            .collect::<Vec<_>>();

        let files_ref: Vec<_> = files.iter().map(|(n, c)| (n.as_str(), c.as_str())).collect();

        let (_temp_dir, file_ids, file_set) = create_test_workspace(files_ref);

        let orchestrator = AnalysisOrchestrator::builder()
            .workspace_root(_temp_dir.path())
            .num_workers(4)
            .build()
            .unwrap();

        let result = orchestrator.analyze(file_ids, file_set);

        assert!(result.is_ok());
        let results = result.unwrap();
        assert_eq!(results.total_files, 10);
        assert_eq!(results.failed_files, 0);
    }

    #[test]
    fn test_analyze_subset_with_full_file_set() {
        let (_temp_dir, all_file_ids, file_set) = create_test_workspace(vec![
            ("analyzed.bsl", "Процедура Тест() КонецПроцедуры"),
            ("dep1.bsl", "Процедура Зависимость1() Экспорт КонецПроцедуры"),
            ("dep2.bsl", "Функция Зависимость2() Экспорт Возврат 1; КонецФункции"),
        ]);

        let orchestrator = AnalysisOrchestrator::builder()
            .workspace_root(_temp_dir.path())
            .num_workers(1)
            .build()
            .unwrap();

        let files_to_analyze = vec![all_file_ids[0]];
        let result = orchestrator.analyze(files_to_analyze, file_set);

        assert!(result.is_ok());
        let results = result.unwrap();
        assert_eq!(results.total_files, 1);
        assert_eq!(results.failed_files, 0);
    }

    #[test]
    fn test_global_context_builds_symbol_trees_for_all_files() {
        let (_temp_dir, all_file_ids, file_set) = create_test_workspace(vec![
            ("analyzed.bsl", "Процедура Анализируемый() КонецПроцедуры"),
            ("dependency.bsl", "Процедура Зависимость(Парам1) Экспорт КонецПроцедуры"),
        ]);

        let orchestrator = AnalysisOrchestrator::builder()
            .workspace_root(_temp_dir.path())
            .num_workers(1)
            .build()
            .unwrap();

        let files_to_analyze = vec![all_file_ids[0]];

        let global_context =
            orchestrator.initialize_global_context(&files_to_analyze, file_set).unwrap();

        assert!(
            global_context.symbol_trees.contains_key(&all_file_ids[0]),
            "SymbolTree for analyzed file must exist"
        );
        assert!(
            global_context.symbol_trees.contains_key(&all_file_ids[1]),
            "SymbolTree for dependency file must exist (cross-module resolution)"
        );
        assert_eq!(
            global_context.symbol_trees.len(),
            2,
            "SymbolTrees must be built for ALL files in file_set"
        );
    }

    #[test]
    fn test_module_index_includes_all_files() {
        let (_temp_dir, all_file_ids, file_set) = create_test_workspace(vec![
            ("CommonModules/МодульА/Ext/Module.bsl", "Процедура МетодА() Экспорт КонецПроцедуры"),
            ("CommonModules/МодульБ/Ext/Module.bsl", "Процедура МетодБ() Экспорт КонецПроцедуры"),
        ]);

        let orchestrator = AnalysisOrchestrator::builder()
            .workspace_root(_temp_dir.path())
            .num_workers(1)
            .build()
            .unwrap();

        let files_to_analyze = vec![all_file_ids[0]];
        let global_context =
            orchestrator.initialize_global_context(&files_to_analyze, file_set).unwrap();

        assert_eq!(
            global_context.module_index.common_module_count(),
            2,
            "ModuleIndex must include all CommonModules from file_set, not just analyzed files"
        );
    }
}
