use std::{error::Error, io};

use super::{
    documents, postgres, publish_policy, snapshot_id, SearchBaselineCorpusCli,
    SearchBaselinePublishArgs,
};

pub(super) fn run(args: SearchBaselinePublishArgs) -> Result<(), Box<dyn Error + Send + Sync>> {
    use bsl_search::{
        fingerprint_documents, fingerprint_indexed_documents, BaselinePublisher, CorpusId,
        Embedder, EmbeddingProgress, SemanticPublishPhase, SemanticPublishProgress, Snapshot,
        SnapshotPublishMetadata,
    };

    let project = project_model::Project::new(&args.source_dir);
    let source_path = project.source_path().to_path_buf();
    let branch = publish_policy::resolve_publish_branch(args.branch.as_deref(), &project.root);
    let commit = publish_policy::resolve_publish_commit(args.commit.as_deref(), &project.root);
    let corpus = match args.corpus {
        SearchBaselineCorpusCli::WorkspaceCode => CorpusId::WorkspaceCode,
        SearchBaselineCorpusCli::Reference => CorpusId::Reference,
    };
    if matches!(corpus, CorpusId::WorkspaceCode) {
        publish_policy::validate_workspace_publish_policy(
            &project.config.search.baseline.workspace_code.policy,
            branch.as_deref(),
            args.allow_non_policy_branch,
        )?;
    }
    let snapshot_id_value = snapshot_id::resolve(
        &corpus,
        args.snapshot_id.as_deref(),
        branch.as_deref(),
        commit.as_deref(),
    )?;
    let resolved_pg = postgres::resolve_project_url(
        &project.config.search.baseline.postgres,
        project_model::PostgresAccessMode::Writer,
    )
    .map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("failed to resolve PostgreSQL writer credentials: {error}"),
        )
    })?;

    eprintln!("[1/5] Indexing source files...");
    let (indexed_files, indexed_documents) = match corpus {
        CorpusId::WorkspaceCode => documents::build_workspace_code(&source_path)?,
        CorpusId::Reference => documents::build_reference()?,
        CorpusId::Custom(_) => unreachable!("CLI corpus variants are exhaustive"),
    };
    eprintln!("      {} files -> {} chunks", indexed_files, indexed_documents.len());

    if indexed_documents.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("no documents were indexed for corpus {}", corpus.as_str()),
        )
        .into());
    }

    let adapter = postgres::build_adapter(
        &resolved_pg.url,
        project.config.search.baseline.postgres.schema.as_deref(),
    )?;
    let schema_label = project
        .config
        .search
        .baseline
        .postgres
        .schema
        .clone()
        .unwrap_or_else(|| "bsl_search".to_owned());

    eprintln!("[2/5] Connecting to PostgreSQL (schema: {})...", schema_label);
    let fingerprint = match corpus {
        CorpusId::WorkspaceCode => fingerprint_indexed_documents(&indexed_documents),
        CorpusId::Reference => {
            let reference_documents = documents::build_reference_source_documents();
            fingerprint_documents(&reference_documents)
        }
        CorpusId::Custom(_) => unreachable!("CLI corpus variants are exhaustive"),
    };
    let mut snapshot =
        Snapshot::new(snapshot_id_value, corpus.clone()).with_fingerprint(fingerprint);
    if let Some(parent_snapshot_id) = snapshot_id::resolve_parent(
        &adapter,
        &corpus,
        branch.as_deref(),
        &snapshot.id.0,
        args.parent_snapshot_id.as_deref(),
        Some(&project.config.search.baseline.workspace_code.policy),
    )? {
        snapshot = snapshot.with_parent(parent_snapshot_id);
    }
    let publish_metadata =
        SnapshotPublishMetadata { branch: branch.clone(), commit: commit.clone() };

    eprintln!("[3/5] Publishing snapshot ({} chunks)...", indexed_documents.len());
    let embedder = postgres::embedder_config(&project).map(Embedder::new);
    let has_embedder = embedder.is_some();
    let embedding_progress = |event: EmbeddingProgress| match event {
        EmbeddingProgress::Plan { total_unique, cached, to_compute } => {
            eprintln!(
                "[4/5] Computing embeddings ({} unique, {} cached, {} to compute)...",
                total_unique, cached, to_compute
            );
        }
        EmbeddingProgress::Batch { processed, total, batches_done, total_batches } => {
            let pct = (processed * 100).checked_div(total).unwrap_or(100);
            eprintln!(
                "      {}/{} ({}%) — batch {}/{}",
                processed, total, pct, batches_done, total_batches
            );
        }
    };
    let publish_report = BaselinePublisher::new(postgres::embedding_execution_policy_from_env())
        .publish(
            &adapter,
            &snapshot,
            &publish_metadata,
            &indexed_documents,
            embedder.as_ref(),
            if has_embedder { Some(&embedding_progress) } else { None },
        )?;
    eprintln!(
        "      Reused {} files, wrote {}, deleted {}",
        publish_report.snapshot.reused_files,
        publish_report.snapshot.written_files,
        publish_report.snapshot.deleted_files,
    );

    if let Some(ref embedding_stats) = publish_report.embeddings {
        let format_duration = |duration: std::time::Duration| {
            if duration.as_secs() >= 1 {
                format!("{:.1}s", duration.as_secs_f64())
            } else {
                format!("{}ms", duration.as_millis())
            }
        };
        let semantic_phase_label = |phase: &SemanticPublishPhase| match phase {
            SemanticPublishPhase::PrepareRows => "Prepare semantic rows",
            SemanticPublishPhase::CopyParentRows => "Copy unchanged rows from parent",
            SemanticPublishPhase::WriteServingRows => "Write serving rows",
        };
        let semantic_progress = |event: SemanticPublishProgress| match event {
            SemanticPublishProgress::Plan {
                strategy,
                changed_files,
                deleted_paths,
                parent_snapshot_id,
                phase_count,
            } => {
                let parent_label = parent_snapshot_id.as_deref().unwrap_or("-");
                eprintln!(
                    "[5/5] Populating serving semantic index ({strategy}; {changed_files} changed files; {deleted_paths} deletions; {phase_count} phases; parent: {parent_label})..."
                );
            }
            SemanticPublishProgress::PhaseStarted { phase, phase_index, phase_count, detail } => {
                eprintln!(
                    "      [{}/{}] {} — {}",
                    phase_index,
                    phase_count,
                    semantic_phase_label(&phase),
                    detail
                );
            }
            SemanticPublishProgress::PhaseCompleted {
                phase,
                phase_index,
                phase_count,
                elapsed,
                output_rows,
            } => {
                eprintln!(
                    "      [{}/{}] {} done in {} ({} rows)",
                    phase_index,
                    phase_count,
                    semantic_phase_label(&phase),
                    format_duration(elapsed),
                    output_rows
                );
            }
            SemanticPublishProgress::Completed {
                total_rows,
                copied_rows,
                inserted_rows,
                missing_embeddings,
                total_elapsed,
            } => {
                eprintln!(
                    "      Done in {} — total {} rows (copied {}, inserted {}, missing embeddings {})",
                    format_duration(total_elapsed),
                    total_rows,
                    copied_rows,
                    inserted_rows,
                    missing_embeddings
                );
            }
        };
        let serving_count = adapter.populate_serving_semantic_with_progress(
            &snapshot.id.0,
            &embedding_stats.model_id,
            embedding_stats.dimension,
            Some(&semantic_progress),
        )?;
        eprintln!("      {} rows", serving_count);
    } else {
        eprintln!("[4/5] Skipped (no embedding config)");
        eprintln!("[5/5] Skipped (no embeddings)");
    }

    let published_files = indexed_documents
        .iter()
        .map(|document| document.path.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let branch_label = branch.as_deref().unwrap_or("-");
    let commit_label = commit.as_deref().unwrap_or("-");

    println!();
    println!("Published search baseline to PostgreSQL.");
    println!("  Corpus:        {}", corpus.as_str());
    println!("  Mode:          {}", if snapshot.parent_id.is_some() { "delta" } else { "root" });
    println!("  Snapshot:      {}", snapshot.id.0);
    println!("  Schema:        {}", schema_label);
    println!(
        "  Parent:        {}",
        snapshot.parent_id.as_ref().map(|value| value.0.as_str()).unwrap_or("-")
    );
    println!("  Branch:        {}", branch_label);
    println!("  Commit:        {}", commit_label);
    println!("  Indexed files: {}", indexed_files);
    println!("  Reused files:  {}", publish_report.snapshot.reused_files);
    println!("  Written files: {}", publish_report.snapshot.written_files);
    println!("  Deleted files: {}", publish_report.snapshot.deleted_files);
    println!("  Files:         {}", published_files);
    println!("  Reused chunks: {}", publish_report.snapshot.reused_documents);
    println!("  Written chunks: {}", publish_report.snapshot.written_documents);
    println!("  Chunks:        {}", indexed_documents.len());
    if let Some(ref embedding_stats) = publish_report.embeddings {
        println!("  Embedding model: {}", embedding_stats.model_id);
        println!("  Reused embeddings: {}", embedding_stats.reused);
        println!("  Stored embeddings: {}", embedding_stats.stored);
    }

    Ok(())
}
