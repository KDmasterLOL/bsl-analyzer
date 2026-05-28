use std::{error::Error, io};

use super::{
    corpus_cli_to_domain, format_ratio, postgres, shorten_fingerprint,
    SearchBaselineListEmbeddingsArgs, SearchBaselineListFileObjectsArgs,
    SearchBaselineListSnapshotsArgs, SearchBaselineShowEmbeddingCoverageArgs,
    SearchBaselineShowFileObjectArgs, SearchBaselineShowSnapshotArgs,
};

pub(super) fn list_snapshots(
    args: SearchBaselineListSnapshotsArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let adapter = postgres::build_project_adapter(
        &args.source_dir,
        project_model::PostgresAccessMode::Reader,
    )?;
    let corpus = args.corpus.map(corpus_cli_to_domain);
    let snapshots = adapter.list_snapshots(
        corpus.as_ref().map(|corpus| corpus.as_str()),
        args.branch.as_deref(),
        args.commit.as_deref(),
        args.limit,
    )?;

    if snapshots.is_empty() {
        println!("No snapshots found.");
        return Ok(());
    }

    println!("Published search baselines:");
    for snapshot in snapshots {
        println!();
        println!("  Snapshot:  {}", snapshot.snapshot_id);
        println!("  Corpus:    {}", snapshot.corpus);
        println!("  Created:   {}", snapshot.created_at);
        println!("  Parent:    {}", snapshot.parent_snapshot_id.as_deref().unwrap_or("-"));
        println!("  Branch:    {}", snapshot.branch.as_deref().unwrap_or("-"));
        println!("  Commit:    {}", snapshot.commit.as_deref().unwrap_or("-"));
        println!("  Files:     {}", snapshot.files);
        println!("  Chunks:    {}", snapshot.documents);
        println!(
            "  Fingerprint: {}",
            snapshot.fingerprint.as_deref().map(shorten_fingerprint).unwrap_or("-")
        );
    }

    Ok(())
}

pub(super) fn show_snapshot(
    args: SearchBaselineShowSnapshotArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let adapter = postgres::build_project_adapter(
        &args.source_dir,
        project_model::PostgresAccessMode::Reader,
    )?;
    let Some(details) = adapter.snapshot_details(&args.snapshot_id)? else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("snapshot '{}' was not found", args.snapshot_id),
        )
        .into());
    };

    println!("Search baseline snapshot:");
    println!("  Snapshot:    {}", details.snapshot.snapshot_id);
    println!("  Corpus:      {}", details.snapshot.corpus);
    println!("  Created:     {}", details.snapshot.created_at);
    println!("  Parent:      {}", details.snapshot.parent_snapshot_id.as_deref().unwrap_or("-"));
    println!("  Branch:      {}", details.snapshot.branch.as_deref().unwrap_or("-"));
    println!("  Commit:      {}", details.snapshot.commit.as_deref().unwrap_or("-"));
    println!("  Files:       {}", details.snapshot.files);
    println!("  Chunks:      {}", details.snapshot.documents);
    println!(
        "  Fingerprint: {}",
        details.snapshot.fingerprint.as_deref().map(shorten_fingerprint).unwrap_or("-")
    );

    if details.collections.is_empty() {
        println!("  Collections: -");
    } else {
        println!("  Collections:");
        for collection in details.collections {
            println!(
                "    {}: files={}, chunks={}",
                collection.collection, collection.files, collection.documents
            );
        }
    }

    Ok(())
}

pub(super) fn list_file_objects(
    args: SearchBaselineListFileObjectsArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let adapter = postgres::build_project_adapter(
        &args.source_dir,
        project_model::PostgresAccessMode::Reader,
    )?;
    let file_objects = adapter.list_file_objects(args.collection.as_deref(), args.limit)?;

    if file_objects.is_empty() {
        println!("No file objects found.");
        return Ok(());
    }

    println!("Shared baseline file objects:");
    for file_object in file_objects {
        println!();
        println!("  File object:  {}", file_object.file_object_id);
        println!("  Collection:   {}", file_object.collection);
        println!("  Snapshots:    {}", file_object.snapshots);
        println!("  Chunks:       {}", file_object.documents);
        println!("  Fingerprint:  {}", shorten_fingerprint(&file_object.fingerprint));
    }

    Ok(())
}

pub(super) fn show_file_object(
    args: SearchBaselineShowFileObjectArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let adapter = postgres::build_project_adapter(
        &args.source_dir,
        project_model::PostgresAccessMode::Reader,
    )?;
    let Some(details) = adapter.file_object_details(&args.file_object_id)? else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("file object '{}' was not found", args.file_object_id),
        )
        .into());
    };

    println!("Shared baseline file object:");
    println!("  File object:  {}", details.file_object.file_object_id);
    println!("  Collection:   {}", details.file_object.collection);
    println!("  Snapshots:    {}", details.file_object.snapshots);
    println!("  Chunks:       {}", details.file_object.documents);
    println!("  Fingerprint:  {}", shorten_fingerprint(&details.file_object.fingerprint));
    if details.references.is_empty() {
        println!("  References:   -");
    } else {
        println!("  References:");
        for reference in details.references {
            println!("    {} -> {}", reference.snapshot_id, reference.path);
        }
    }

    Ok(())
}

pub(super) fn list_embeddings(
    args: SearchBaselineListEmbeddingsArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let adapter = postgres::build_project_adapter(
        &args.source_dir,
        project_model::PostgresAccessMode::Reader,
    )?;
    let models = adapter.list_embedding_models(args.model_id.as_deref(), args.dimension)?;

    if models.is_empty() {
        println!("No embeddings found.");
        return Ok(());
    }

    println!("Shared embedding inventories:");
    for model in models {
        println!();
        println!("  Model:       {}", model.model_id);
        println!("  Dimension:   {}", model.dimension);
        println!("  Embeddings:  {}", model.embeddings);
    }

    Ok(())
}

pub(super) fn show_embedding_coverage(
    args: SearchBaselineShowEmbeddingCoverageArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let adapter = postgres::build_project_adapter(
        &args.source_dir,
        project_model::PostgresAccessMode::Reader,
    )?;
    let coverage = adapter.embedding_coverage(args.model_id.as_deref(), args.dimension)?;

    if coverage.is_empty() {
        println!("No embedding inventories found.");
        return Ok(());
    }

    println!("Shared embedding coverage:");
    for record in coverage {
        println!();
        println!("  Model:            {}", record.model_id);
        println!("  Dimension:        {}", record.dimension);
        println!("  Active payloads:  {}", record.active_payloads);
        println!("  Covered payloads: {}", record.covered_payloads);
        println!(
            "  Coverage:         {}",
            format_ratio(record.covered_payloads, record.active_payloads)
        );
        println!("  Embeddings:       {}", record.embeddings);
    }

    Ok(())
}
