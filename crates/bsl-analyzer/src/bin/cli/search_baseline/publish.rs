use std::{error::Error, io};

use super::{
    documents, postgres, publish_policy, snapshot_id, SearchBaselineCorpusCli,
    SearchBaselinePublishArgs,
};

/// Whether a walk may stand behind a published snapshot.
///
/// A snapshot is not a best effort: `snapshot_files` says what the corpus holds and
/// `snapshot_deletions` says the rest is gone from the tree. So a walk that could not read the
/// tree whole publishes two lies at once — it drops files, and it tells every consumer those
/// files were deleted. The barrier next to this one only asks whether the corpus is EMPTY, and
/// an unreadable subtree leaves it comfortably non-empty.
///
/// Both counters are asked, because they answer different questions. `unreadable` says the file
/// list is short. `canonical_fallbacks` says a file's identity is a guess: its key may have
/// shifted, and a shifted key reads downstream as one file deleted and another created. Loops
/// and dead links are deliberately not consulted — neither hides anything.
fn ensure_the_walk_can_speak_for_the_tree(
    walk: &project_model::SourceSet,
) -> Result<(), io::Error> {
    if walk.clean() {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "refusing to publish a corpus built from a tree this walk could not read whole: \
             {} unreadable place(s), {} file(s) whose physical name could not be resolved. \
             A snapshot asserts both what exists and what was deleted, so an incomplete walk \
             would report live files as removed. Fix the tree and publish again.",
            walk.unreadable, walk.canonical_fallbacks
        ),
    ))
}

pub(super) fn run(args: SearchBaselinePublishArgs) -> Result<(), Box<dyn Error + Send + Sync>> {
    use bsl_search::{
        fingerprint_documents, fingerprint_indexed_documents, BaselinePublisher, CorpusId,
        Embedder, EmbeddingProgress, SemanticPublishPhase, SemanticPublishProgress, Snapshot,
        SnapshotPublishMetadata,
    };

    let project = project_model::Project::new(&args.source_dir)?;
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
        CorpusId::WorkspaceCode => {
            let corpus = documents::build_workspace_code(&source_path)?;
            // Before anything is sent anywhere: a snapshot does not merely omit what its walk
            // could not read — `snapshot_deletions` tells every consumer those files are GONE.
            ensure_the_walk_can_speak_for_the_tree(&corpus.walk)?;
            (corpus.indexed_files, corpus.documents)
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn write(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn module(name: &str) -> String {
        format!("Процедура {name}()\n    Сообщить(\"{name}\");\nКонецПроцедуры\n")
    }

    /// A tree with one readable module and one subtree the walk cannot enter.
    /// Returns `None` where permissions cannot hide anything — under an effective UID 0 a
    /// mode of 000 is not a barrier, and a stand that cannot build its own precondition must
    /// say so rather than pass.
    #[cfg(unix)]
    fn tree_with_a_closed_subtree(dir: &Path) -> Option<std::path::PathBuf> {
        use std::os::unix::fs::PermissionsExt;

        let root = dir.join("configuration");
        write(&root.join("CommonModules/Видимый/Ext/Module.bsl"), &module("Видимый"));
        let closed = root.join("CommonModules/Закрытый");
        write(&closed.join("Ext/Module.bsl"), &module("Закрытый"));
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_dir(&closed).is_ok() {
            std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o755)).unwrap();
            return None;
        }
        Some(root)
    }

    #[cfg(unix)]
    #[test]
    fn a_tree_that_could_not_be_read_whole_is_not_published() {
        let dir = tempfile::tempdir().unwrap();
        let Some(root) = tree_with_a_closed_subtree(dir.path()) else { return };

        let corpus = documents::build_workspace_code(&root).unwrap();

        assert!(
            ensure_the_walk_can_speak_for_the_tree(&corpus.walk).is_err(),
            "a snapshot claims both what exists and what was deleted, so a walk that saw less \
             than the tree holds may not stand behind one"
        );
    }

    /// The positive control for the refusal above: without it, an implementation that refuses
    /// everything would look just as correct.
    #[test]
    fn a_tree_read_whole_is_published() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("configuration");
        write(&root.join("CommonModules/Видимый/Ext/Module.bsl"), &module("Видимый"));

        let corpus = documents::build_workspace_code(&root).unwrap();

        assert!(!corpus.documents.is_empty(), "the stand must produce a corpus to judge");
        assert!(
            ensure_the_walk_can_speak_for_the_tree(&corpus.walk).is_ok(),
            "a complete walk publishes; the gate must be able to say yes"
        );
    }

    /// The pre-existing barrier next to this one asks only whether the corpus is EMPTY, and an
    /// unreadable subtree leaves it comfortably non-empty. This pins that the new refusal is a
    /// different question, not a louder version of the old one.
    #[cfg(unix)]
    #[test]
    fn an_incomplete_walk_is_refused_while_its_corpus_is_not_empty() {
        let dir = tempfile::tempdir().unwrap();
        let Some(root) = tree_with_a_closed_subtree(dir.path()) else { return };

        let corpus = documents::build_workspace_code(&root).unwrap();

        assert!(
            !corpus.documents.is_empty(),
            "the readable part must reach the corpus, or this test proves nothing about the \
             emptiness barrier"
        );
        assert!(ensure_the_walk_can_speak_for_the_tree(&corpus.walk).is_err());
    }

    /// The refusal must be WIRED, not merely defined. Proving the decision function in isolation
    /// says nothing about whether the publisher consults it, and that gap is the whole defect
    /// wearing a different hat: a correct policy nobody applies publishes exactly what a missing
    /// policy would.
    ///
    /// No live database is needed, and that is the point — the refusal is placed before the
    /// adapter is built, so a run that reaches PostgreSQL has already failed this test's premise.
    /// Credentials are resolved earlier still, by an external helper program, so the stand
    /// supplies one that answers without a network.
    #[cfg(unix)]
    #[test]
    fn the_publisher_refuses_before_it_reaches_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let Some(root) = tree_with_a_closed_subtree(dir.path()) else { return };
        std::fs::write(
            root.join("bsl-analyzer.toml"),
            "[search.baseline.postgres]\n\
             host = \"127.0.0.1\"\n\
             port = 1\n\
             dbname = \"unreachable\"\n\
             schema = \"bsl_search\"\n\
             vault_role_base = \"probe\"\n\
             [search.baseline.postgres.credential_helper]\n\
             program = \"/bin/sh\"\n\
             args = [\"-c\", \"cat > /dev/null;              printf '{\\\"protocol\\\":\\\"bsl-analyzer.postgres-helper.v1\\\",\\\"ok\\\":true,\\\"url\\\":\\\"postgres://u:p@127.0.0.1:1/unreachable\\\"}'\"]\n",
        )
        .unwrap();

        let error = run(SearchBaselinePublishArgs {
            corpus: SearchBaselineCorpusCli::WorkspaceCode,
            source_dir: root.clone(),
            snapshot_id: Some("probe:incomplete".to_owned()),
            branch: None,
            commit: None,
            parent_snapshot_id: None,
            allow_non_policy_branch: true,
        })
        .expect_err("an incomplete walk must stop the publish");

        let message = error.to_string();
        assert!(
            message.contains("could not read whole"),
            "the run must stop on the incomplete walk, not on something later; got: {message}"
        );
    }

    /// Completeness has two legs and they fail differently: a short list, and a file whose
    /// identity is a guess. No filesystem stand can produce the second one — a walk either
    /// resolves a path or has no path — so the decision is asked about each leg directly.
    /// Without this, an implementation consulting only `unreadable` passes every other test here.
    #[test]
    fn each_leg_of_completeness_refuses_on_its_own() {
        use project_model::SourceSet;

        let short = SourceSet { unreadable: 1, ..SourceSet::default() };
        assert!(
            ensure_the_walk_can_speak_for_the_tree(&short).is_err(),
            "a walk that was kept out of somewhere cannot say what the tree holds"
        );

        let degraded = SourceSet { canonical_fallbacks: 1, ..SourceSet::default() };
        assert!(
            ensure_the_walk_can_speak_for_the_tree(&degraded).is_err(),
            "a file whose physical name could not be resolved may carry a shifted key, and a \
             shifted key reads downstream as one file deleted and another created"
        );

        let benign = SourceSet { loops: 3, dangling: 2, ..SourceSet::default() };
        assert!(
            ensure_the_walk_can_speak_for_the_tree(&benign).is_ok(),
            "a loop and a dead link hide nothing; refusing on them would block publication for \
             a tree that is entirely readable"
        );
    }
}
