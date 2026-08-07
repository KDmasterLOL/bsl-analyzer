use std::{error::Error, io};

use super::{
    documents, postgres, publish_policy, snapshot_id, SearchBaselineCorpusCli,
    SearchBaselinePublishArgs,
};

/// Whether a corpus may stand behind a published snapshot.
///
/// A snapshot is not a best effort: `snapshot_files` says what the corpus holds and
/// `snapshot_deletions` says the rest is gone from the tree. So anything missing from the corpus
/// is published as a DELETION, and the barrier next to this one — is the corpus empty? — is
/// comfortably satisfied by a corpus missing most of a configuration.
///
/// A file can go missing in three ways, and no one of them implies the others:
///
/// - `unreadable`: the walk was kept out of somewhere, so the file list is short;
/// - `canonical_fallbacks`: a file's physical name could not be resolved, so its key is a guess,
///   and a shifted key reads downstream as one file deleted and another created;
/// - `unread`: the walk reached the file and the ingest could not read its bytes. The walk's own
///   counters are blind here, because `stat` needs no read permission — such a file is
///   enumerated, classified and counted as perfectly healthy.
///
/// Loops and dead links are deliberately not consulted: neither hides anything.
fn ensure_the_corpus_covers_the_walk(
    walk: &project_model::SourceSet,
    unreadable_files: usize,
) -> Result<(), io::Error> {
    if walk.clean() && unreadable_files == 0 {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "refusing to publish a corpus that does not cover its own source tree: \
             {} unreadable place(s), {} file(s) whose physical name could not be resolved, \
             {} file(s) found but not readable. A snapshot asserts both what exists and what \
             was deleted, so every one of these would be published as a removal of a live file. \
             Fix the tree and publish again.",
            walk.unreadable, walk.canonical_fallbacks, unreadable_files
        ),
    ))
}

/// How many FILES a corpus of chunks stands for.
///
/// Counted by the whole identity `(root_id, path)`, not by the path alone: an extension
/// repeats the configuration's directory layout, so the same relative path under two roots is
/// the ordinary case, and counting by path would report one file where two were published.
fn distinct_files(documents: &[bsl_search::IndexedDocument]) -> usize {
    documents
        .iter()
        .map(|document| (document.root_id.as_str(), document.path.as_str()))
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

pub(super) fn run(args: SearchBaselinePublishArgs) -> Result<(), Box<dyn Error + Send + Sync>> {
    use bsl_search::{
        fingerprint_documents, fingerprint_indexed_documents, BaselinePublisher, CorpusId,
        Embedder, EmbeddingProgress, SemanticPublishPhase, SemanticPublishProgress, Snapshot,
        SnapshotPublishMetadata,
    };

    let project = project_model::Project::new(&args.source_dir)?;
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
            let corpus = documents::build_workspace_code(&project)?;
            // Before anything is sent anywhere: a snapshot does not merely omit what its walk
            // could not read — `snapshot_deletions` tells every consumer those files are GONE.
            ensure_the_corpus_covers_the_walk(&corpus.walk, corpus.unreadable_files)?;
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

    let published_files = distinct_files(&indexed_documents);
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

    fn project_at(root: &Path) -> project_model::Project {
        project_model::Project::new(root).unwrap()
    }

    /// The report tells the operator how many files went out. Two files at one relative path
    /// under two roots is what the composite key exists for, so a count that folds them into
    /// one understates the publish by exactly the rows an operator would come looking for.
    #[test]
    fn the_report_counts_one_path_under_two_roots_as_two_files() {
        let chunk = bsl_search::IndexedDocument {
            collection: "code".to_owned(),
            root_id: bsl_search::CONFIGURATION_ROOT_ID.to_owned(),
            path: "CommonModules/Общий/Ext/Module.bsl".to_owned(),
            symbol_name: "Общий".to_owned(),
            kind: "procedure".to_owned(),
            line_start: 1,
            line_end: 3,
            text: "Процедура Общий()".to_owned(),
            content_hash: "hash-1".to_owned(),
            graph_context: None,
        };
        let in_extension =
            bsl_search::IndexedDocument { root_id: "src/cfe/Расш".to_owned(), ..chunk.clone() };
        let second_chunk_of_the_same_file = bsl_search::IndexedDocument {
            symbol_name: "Второй".to_owned(),
            line_start: 5,
            ..chunk.clone()
        };

        assert_eq!(
            distinct_files(&[chunk, in_extension, second_chunk_of_the_same_file]),
            2,
            "two files, one of them chunked twice"
        );
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

    /// The same tree, with the closed subtree inside an EXTENSION instead of the
    /// configuration. Returns `None` under the same precondition as its sibling.
    #[cfg(unix)]
    fn tree_with_a_closed_subtree_in_an_extension(dir: &Path) -> Option<std::path::PathBuf> {
        use std::os::unix::fs::PermissionsExt;

        let root = dir.join("project");
        for owner in ["src/cf", "src/cfe/Расширение"] {
            let owner_root = root.join(owner);
            write(&owner_root.join("Configuration.xml"), "<MetaDataObject/>");
            write(&owner_root.join("CommonModules/Видимый/Ext/Module.bsl"), &module("Видимый"));
        }
        let closed = root.join("src/cfe/Расширение/CommonModules/Закрытый");
        write(&closed.join("Ext/Module.bsl"), &module("Закрытый"));
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_dir(&closed).is_ok() {
            std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o755)).unwrap();
            return None;
        }
        Some(root)
    }

    /// Completeness has to answer for the WHOLE published universe, not for the configuration
    /// half of it. A walk that never enters an extension reports itself perfectly clean, so
    /// the extension's files would be published as deletions with the verdict saying nothing.
    #[cfg(unix)]
    #[test]
    fn a_place_the_walk_could_not_read_inside_an_extension_is_not_published_either() {
        let dir = tempfile::tempdir().unwrap();
        let Some(root) = tree_with_a_closed_subtree_in_an_extension(dir.path()) else { return };

        let corpus = documents::build_workspace_code(&project_at(&root)).unwrap();

        assert!(
            ensure_the_corpus_covers_the_walk(&corpus.walk, corpus.unreadable_files).is_err(),
            "an unreadable place inside an extension hides files from the corpus exactly as one \
             inside the configuration does"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_tree_that_could_not_be_read_whole_is_not_published() {
        let dir = tempfile::tempdir().unwrap();
        let Some(root) = tree_with_a_closed_subtree(dir.path()) else { return };

        let corpus = documents::build_workspace_code(&project_at(&root)).unwrap();

        assert!(
            ensure_the_corpus_covers_the_walk(&corpus.walk, corpus.unreadable_files).is_err(),
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

        let corpus = documents::build_workspace_code(&project_at(&root)).unwrap();

        assert!(!corpus.documents.is_empty(), "the stand must produce a corpus to judge");
        assert!(
            ensure_the_corpus_covers_the_walk(&corpus.walk, corpus.unreadable_files).is_ok(),
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

        let corpus = documents::build_workspace_code(&project_at(&root)).unwrap();

        assert!(
            !corpus.documents.is_empty(),
            "the readable part must reach the corpus, or this test proves nothing about the \
             emptiness barrier"
        );
        assert!(ensure_the_corpus_covers_the_walk(&corpus.walk, corpus.unreadable_files).is_err());
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
            message.contains("does not cover its own source tree"),
            "the run must stop on the incomplete walk, not on something later; got: {message}"
        );
    }

    /// A rooted corpus that fails for an unrelated reason gets no advice about extensions.
    ///
    /// This is the completeness check for removing the schema's blanket refusal, and it replaces
    /// an enumeration that was declared complete four times and missed a member each time.
    /// Clippy cannot stand in for it: clippy catches ORPHANED code, while the hazard here is a
    /// stale function whose caller survived — reachable, warning-free, with its own tests green.
    ///
    /// Two details are load-bearing. The stand runs `run`, not the storage adapter, because the
    /// advice is attached at exactly one place and the adapter never emits it. And the parent
    /// snapshot is given explicitly, because otherwise `run` asks the unreachable database for
    /// the parent and fails before reaching the place under test.
    #[cfg(unix)]
    #[test]
    fn a_rooted_corpus_failing_for_another_reason_is_not_told_to_drop_its_extensions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        for owner in ["src/cf", "src/cfe/Расширение"] {
            let owner_root = root.join(owner);
            write(&owner_root.join("Configuration.xml"), "<MetaDataObject/>");
            write(&owner_root.join("CommonModules/Видимый/Ext/Module.bsl"), &module("Видимый"));
        }
        write(
            &root.join("bsl-analyzer.toml"),
            "[search.baseline.postgres]\n\
             host = \"127.0.0.1\"\n\
             port = 1\n\
             dbname = \"unreachable\"\n\
             schema = \"bsl_search\"\n\
             vault_role_base = \"probe\"\n\
             [search.baseline.postgres.credential_helper]\n\
             program = \"/bin/sh\"\n\
             args = [\"-c\", \"cat > /dev/null;              printf '{\\\"protocol\\\":\\\"bsl-analyzer.postgres-helper.v1\\\",\\\"ok\\\":true,\\\"url\\\":\\\"postgres://u:p@127.0.0.1:1/unreachable\\\"}'\"]\n",
        );

        let error = run(SearchBaselinePublishArgs {
            corpus: SearchBaselineCorpusCli::WorkspaceCode,
            source_dir: root.clone(),
            snapshot_id: Some("probe:rooted".to_owned()),
            branch: None,
            commit: None,
            parent_snapshot_id: Some("probe:parent".to_owned()),
            allow_non_policy_branch: true,
        })
        .expect_err("an unreachable database must stop the publish");

        let message = error.to_string();
        assert!(
            !message.contains("extension"),
            "a connection failure must not be dressed up as an extension problem; got: {message}"
        );
    }

    /// A file the walk REACHED but whose bytes could not be read is the same lie as a directory
    /// the walk could not enter: the corpus is short, and `snapshot_deletions` reports the file
    /// as removed from the tree. The walk's own counters cannot see it — `stat` needs no read
    /// permission, so such a file is enumerated, classified and counted as healthy.
    #[cfg(unix)]
    #[test]
    fn a_file_the_walk_found_but_could_not_read_stops_the_publish() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("configuration");
        write(&root.join("CommonModules/Читаемый/Ext/Module.bsl"), &module("Читаемый"));
        let closed = root.join("CommonModules/Закрытый/Ext/Module.bsl");
        write(&closed, &module("Закрытый"));
        std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_to_string(&closed).is_ok() {
            return;
        }

        let corpus = documents::build_workspace_code(&project_at(&root)).unwrap();

        assert_eq!(corpus.walk.unreadable, 0, "the walk reaches such a file and reports health");
        assert!(
            ensure_the_corpus_covers_the_walk(&corpus.walk, corpus.unreadable_files).is_err(),
            "a file that was found but not read is missing from the corpus just the same"
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
            ensure_the_corpus_covers_the_walk(&short, 0).is_err(),
            "a walk that was kept out of somewhere cannot say what the tree holds"
        );

        let degraded = SourceSet { canonical_fallbacks: 1, ..SourceSet::default() };
        assert!(
            ensure_the_corpus_covers_the_walk(&degraded, 0).is_err(),
            "a file whose physical name could not be resolved may carry a shifted key, and a \
             shifted key reads downstream as one file deleted and another created"
        );

        let unread = SourceSet::default();
        assert!(
            ensure_the_corpus_covers_the_walk(&unread, 1).is_err(),
            "a file the walk reached but the ingest could not read is missing from the corpus \
             just as surely, and the walk's own counters cannot see it"
        );

        let benign = SourceSet { loops: 3, dangling: 2, ..SourceSet::default() };
        assert!(
            ensure_the_corpus_covers_the_walk(&benign, 0).is_ok(),
            "a loop and a dead link hide nothing; refusing on them would block publication for \
             a tree that is entirely readable"
        );
    }
}
