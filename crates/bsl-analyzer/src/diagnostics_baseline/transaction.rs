use std::collections::HashSet;
use std::fs::TryLockError;
use std::io::{self, BufWriter, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};

use ide::diagnostics_baseline::DiagnosticsBaselineEntry;
use ide::partitioned_diagnostics_baseline::{
    diagnostics_manifest, diagnostics_manifest_json, partition_object_path,
    DiagnosticsBaselineManifest, DiagnosticsBaselineManifestEntry,
    PartitionedDiagnosticsBaselineError, DIAGNOSTICS_BASELINE_PARTITION_SCHEMA_VERSION,
};
use project_model::{DiagnosticsBaselinePartition, ManagedBaselineDirectory};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub enum PreparedPartition {
    Write { id: String, key: String, bytes: Vec<u8> },
    Staged { id: String, key: String, path: String, hash: String },
    Reuse { id: String, key: String, entry: DiagnosticsBaselineManifestEntry },
    Carry { id: String, key: String, entry: DiagnosticsBaselineManifestEntry },
}

pub struct PartitionFileWriter<'a> {
    partition: &'a DiagnosticsBaselinePartition,
    file: HashingWriter<BufWriter<std::fs::File>>,
    temp: TemporaryFile<'a>,
    entries: usize,
}

impl<'a> PartitionFileWriter<'a> {
    pub fn new(
        directory: &'a ManagedBaselineDirectory,
        partition: &'a DiagnosticsBaselinePartition,
    ) -> Result<Self, PartitionedDiagnosticsBaselineError> {
        let temp = TemporaryFile::new(directory, temp_name("migration.json"));
        let file = directory.create_file_new(temp.path())?;
        let mut file = HashingWriter::new(BufWriter::new(file));
        // Spacing matters: these bytes must match `diagnostics_partition_json` exactly,
        // or an object streamed here cannot be regenerated for repair.
        file.write_all(br#"{"schema_version":"#)?;
        serde_json::to_writer(&mut file, &DIAGNOSTICS_BASELINE_PARTITION_SCHEMA_VERSION)?;
        file.write_all(br#","partition":"#)?;
        serde_json::to_writer(&mut file, &partition.identity)?;
        file.write_all(br#","diagnostics":["#)?;
        Ok(Self { partition, file, temp, entries: 0 })
    }

    pub fn write_entry(
        &mut self,
        entry: &DiagnosticsBaselineEntry,
    ) -> Result<(), PartitionedDiagnosticsBaselineError> {
        if self.entries > 0 {
            self.file.write_all(b",")?;
        }
        serde_json::to_writer(&mut self.file, entry)?;
        self.entries += 1;
        Ok(())
    }

    pub fn finish(
        mut self,
    ) -> Result<(PreparedPartition, usize), PartitionedDiagnosticsBaselineError> {
        self.file.write_all(b"]}\n")?;
        self.file.flush()?;
        self.file.inner.get_ref().sync_all()?;
        let hash = self.file.hash();
        let path = self.temp.path().to_owned();
        self.temp.disarm();
        Ok((
            PreparedPartition::Staged {
                id: self.partition.id.clone(),
                key: self.partition.key.clone(),
                path,
                hash,
            },
            self.entries,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishStats {
    pub serialized_partitions: usize,
    pub reused_partitions: usize,
}

#[derive(Debug)]
pub struct PublishResult {
    pub manifest: DiagnosticsBaselineManifest,
    pub stats: PublishStats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransactionStage {
    ObjectPublished,
    BeforeManifestReplace,
    AfterCommit,
}

pub fn publish_set(
    directory: &ManagedBaselineDirectory,
    project_scope_fingerprint: String,
    partitions: Vec<PreparedPartition>,
    expected_generation: Option<&str>,
) -> Result<PublishResult, Box<dyn std::error::Error + Send + Sync>> {
    publish_set_with_hook(
        directory,
        project_scope_fingerprint,
        partitions,
        expected_generation,
        |_| Ok(()),
    )
}

pub fn repair_object(
    directory: &ManagedBaselineDirectory,
    expected: &DiagnosticsBaselineManifestEntry,
    bytes: &[u8],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if blake3::hash(bytes).to_hex().as_str() != expected.blake3 {
        return Err("regenerated partition is not byte-identical to the manifest hash".into());
    }
    let lock = directory.open_or_create_file(".baseline.lock")?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "diagnostics baseline writer is busy",
            )
            .into())
        }
        Err(TryLockError::Error(error)) => return Err(error.into()),
    }
    let manifest_before = read_optional(directory, "manifest.json")?
        .ok_or("diagnostics baseline manifest is missing")?;
    let manifest: DiagnosticsBaselineManifest = serde_json::from_slice(&manifest_before)?;
    if !manifest.partitions.iter().any(|entry| entry == expected) {
        return Err("diagnostics baseline manifest changed".into());
    }
    atomic_write(directory, &expected.file, bytes)?;
    if read_optional(directory, "manifest.json")? != Some(manifest_before) {
        return Err("diagnostics baseline manifest changed".into());
    }
    Ok(())
}

fn publish_set_with_hook(
    directory: &ManagedBaselineDirectory,
    project_scope_fingerprint: String,
    partitions: Vec<PreparedPartition>,
    expected_generation: Option<&str>,
    mut hook: impl FnMut(TransactionStage) -> io::Result<()>,
) -> Result<PublishResult, Box<dyn std::error::Error + Send + Sync>> {
    let staged = StagedFiles {
        directory,
        paths: partitions
            .iter()
            .filter_map(|partition| match partition {
                PreparedPartition::Staged { path, .. } => Some(path.clone()),
                _ => None,
            })
            .collect(),
    };
    let lock = directory.open_or_create_file(".baseline.lock")?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "diagnostics baseline writer is busy",
            )
            .into())
        }
        Err(TryLockError::Error(error)) => return Err(error.into()),
    }
    let original = read_optional(directory, "manifest.json")?;
    let original_manifest = original
        .as_deref()
        .map(serde_json::from_slice::<DiagnosticsBaselineManifest>)
        .transpose()?;
    if original_manifest.as_ref().map(|manifest| manifest.generation.as_str())
        != expected_generation
    {
        return Err("diagnostics baseline generation changed".into());
    }

    let mut entries = Vec::with_capacity(partitions.len());
    let mut serialized = 0;
    let mut reused = 0;
    for partition in partitions {
        match partition {
            PreparedPartition::Write { id, key, bytes } => {
                let hash = blake3::hash(&bytes).to_hex().to_string();
                let path = partition_object_path(&id, &key, &hash)?;
                match hash_optional(directory, &path)? {
                    Some(existing) if existing == hash => reused += 1,
                    Some(_) => {
                        return Err(format!("content-addressed object is corrupt: {path}").into())
                    }
                    None => {
                        publish_object(directory, &path, &bytes)?;
                        serialized += 1;
                        hook(TransactionStage::ObjectPublished)?;
                    }
                }
                entries.push(DiagnosticsBaselineManifestEntry {
                    partition_id: id,
                    file: path,
                    blake3: hash,
                });
            }
            PreparedPartition::Staged { id, key, path: staged_path, hash } => {
                if hash_optional(directory, &staged_path)?.as_deref() != Some(hash.as_str()) {
                    return Err(
                        format!("staged diagnostics baseline object is corrupt: {id}").into()
                    );
                }
                let path = partition_object_path(&id, &key, &hash)?;
                match hash_optional(directory, &path)? {
                    Some(existing) if existing == hash => reused += 1,
                    Some(_) => {
                        return Err(format!("content-addressed object is corrupt: {path}").into())
                    }
                    None => {
                        directory.persist_file_new(&staged_path, &path)?;
                        serialized += 1;
                        hook(TransactionStage::ObjectPublished)?;
                    }
                }
                entries.push(DiagnosticsBaselineManifestEntry {
                    partition_id: id,
                    file: path,
                    blake3: hash,
                });
            }
            PreparedPartition::Reuse { id, key, entry } => {
                if entry.partition_id != id
                    || entry.file != partition_object_path(&id, &key, &entry.blake3)?
                {
                    return Err("invalid reused diagnostics baseline partition".into());
                }
                let actual = hash_optional(directory, &entry.file)?
                    .ok_or_else(|| format!("missing reused object: {}", entry.file))?;
                if actual != entry.blake3 {
                    return Err(
                        format!("content-addressed object is corrupt: {}", entry.file).into()
                    );
                }
                entries.push(entry);
                reused += 1;
            }
            PreparedPartition::Carry { id, key, entry } => {
                if entry.partition_id != id
                    || entry.file != partition_object_path(&id, &key, &entry.blake3)?
                {
                    return Err("invalid carried diagnostics baseline partition".into());
                }
                entries.push(entry);
                reused += 1;
            }
        }
    }
    let manifest = diagnostics_manifest(project_scope_fingerprint, entries);
    let bytes = diagnostics_manifest_json(&manifest)?;
    if read_optional(directory, "manifest.json")? != original {
        return Err("diagnostics baseline generation changed".into());
    }
    let mut temp = TemporaryFile::new(directory, temp_name("manifest.json"));
    write_synced(directory, temp.path(), &bytes)?;
    hook(TransactionStage::BeforeManifestReplace)?;
    directory.replace_file(temp.path(), "manifest.json")?;
    temp.disarm();
    if let Err(error) = directory.sync_all() {
        tracing::warn!(%error, "diagnostics baseline committed but directory sync failed");
    }
    if let Err(error) = hook(TransactionStage::AfterCommit) {
        tracing::warn!(%error, "diagnostics baseline committed with a cleanup warning");
    }

    if let Some(old) = original_manifest {
        if old.project_scope_fingerprint == manifest.project_scope_fingerprint {
            let retained: HashSet<_> =
                manifest.partitions.iter().map(|entry| entry.file.as_str()).collect();
            // Only a superseded VERSION of a partition that still exists is deleted. A
            // partition absent from the new manifest keeps its object: it may be dormant
            // under `include`, or regrouped (`extension:A` becoming part of `group:T`),
            // and the scope fingerprint does not cover grouping — it would report "same
            // plan" while the ids changed, and the deletion would be irreversible.
            let living: HashSet<_> =
                manifest.partitions.iter().map(|entry| entry.partition_id.as_str()).collect();
            for entry in old.partitions {
                if !living.contains(entry.partition_id.as_str()) {
                    continue;
                }
                // Only content-addressed object paths are ever deleted. The old manifest
                // is parsed as plain JSON, so a corrupted or hand-edited one could name
                // `manifest.json` — or anything else in the managed directory — and the
                // cleanup would obediently remove it.
                let object_path = ide::partitioned_diagnostics_baseline::partition_object_path(
                    &entry.partition_id,
                    &blake3::hash(entry.partition_id.as_bytes()).to_hex(),
                    &entry.blake3,
                );
                let addressed = entry.file.starts_with("objects/")
                    && object_path.is_ok_and(|path| {
                        std::path::Path::new(&path).file_name()
                            == std::path::Path::new(&entry.file).file_name()
                    });
                if !addressed {
                    tracing::warn!(
                        file = %entry.file,
                        "diagnostics baseline cleanup skipped a path that is not an object"
                    );
                    continue;
                }
                if !retained.contains(entry.file.as_str()) {
                    if let Err(error) = directory.remove_file(&entry.file) {
                        tracing::warn!(%error, path = entry.file, "diagnostics baseline cleanup failed");
                    }
                }
            }
        }
    }
    drop(staged);
    Ok(PublishResult {
        manifest,
        stats: PublishStats { serialized_partitions: serialized, reused_partitions: reused },
    })
}

struct StagedFiles<'a> {
    directory: &'a ManagedBaselineDirectory,
    paths: Vec<String>,
}

impl Drop for StagedFiles<'_> {
    fn drop(&mut self) {
        for path in &self.paths {
            match self.directory.remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => tracing::warn!(%error, %path, "staged baseline cleanup failed"),
            }
        }
    }
}

struct HashingWriter<W> {
    inner: W,
    hasher: blake3::Hasher,
}

impl<W> HashingWriter<W> {
    fn new(inner: W) -> Self {
        Self { inner, hasher: blake3::Hasher::new() }
    }

    fn hash(&self) -> String {
        self.hasher.clone().finalize().to_hex().to_string()
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buffer)?;
        self.hasher.update(&buffer[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn atomic_write(directory: &ManagedBaselineDirectory, path: &str, bytes: &[u8]) -> io::Result<()> {
    let mut temp = TemporaryFile::new(directory, temp_name(path));
    write_synced(directory, temp.path(), bytes)?;
    directory.replace_file(temp.path(), path)?;
    temp.disarm();
    directory.sync_all()
}

fn publish_object(
    directory: &ManagedBaselineDirectory,
    path: &str,
    bytes: &[u8],
) -> io::Result<()> {
    let mut temp = TemporaryFile::new(directory, temp_name(path));
    write_synced(directory, temp.path(), bytes)?;
    directory.persist_file_new(temp.path(), path)?;
    temp.disarm();
    directory.sync_all()
}

fn write_synced(directory: &ManagedBaselineDirectory, path: &str, bytes: &[u8]) -> io::Result<()> {
    let mut file = directory.create_file_new(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn read_optional(directory: &ManagedBaselineDirectory, path: &str) -> io::Result<Option<Vec<u8>>> {
    match directory.open_file(path) {
        Ok(mut file) => {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok(Some(bytes))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn hash_optional(directory: &ManagedBaselineDirectory, path: &str) -> io::Result<Option<String>> {
    match directory.open_file(path) {
        Ok(mut file) => {
            let mut hasher = blake3::Hasher::new();
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            Ok(Some(hasher.finalize().to_hex().to_string()))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

struct TemporaryFile<'a> {
    directory: &'a ManagedBaselineDirectory,
    path: String,
    armed: bool,
}

impl<'a> TemporaryFile<'a> {
    fn new(directory: &'a ManagedBaselineDirectory, path: String) -> Self {
        Self { directory, path, armed: true }
    }

    fn path(&self) -> &str {
        &self.path
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TemporaryFile<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        match self.directory.remove_file(&self.path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(%error, path = self.path, "temporary baseline cleanup failed")
            }
        }
    }
}

fn temp_name(path: &str) -> String {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
    let temp = format!(".{name}.tmp-{}-{sequence}", std::process::id());
    if parent.is_empty() {
        temp
    } else {
        format!("{parent}/{temp}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide::diagnostics_baseline::{diagnostic_fingerprint, DiagnosticsBaselineRange};
    use project_model::DiagnosticsBaselinePartitionIdentity;
    use tempfile::tempdir;

    fn partition(id: &str, bytes: &[u8]) -> PreparedPartition {
        PreparedPartition::Write {
            id: id.to_owned(),
            key: blake3::hash(id.as_bytes()).to_hex().to_string(),
            bytes: bytes.to_vec(),
        }
    }

    #[test]
    fn partitioned_baseline_transaction_publishes_streamed_partition() {
        let root = tempdir().unwrap();
        let directory = ManagedBaselineDirectory::open(root.path(), "baselines", true).unwrap();
        let partition = DiagnosticsBaselinePartition {
            id: "main".to_owned(),
            key: blake3::hash(b"main").to_hex().to_string(),
            identity: DiagnosticsBaselinePartitionIdentity::Main { path: "src/cf".to_owned() },
        };
        let path = "src/cf/Main.bsl";
        let snippet = "Message(1);";
        let entry = DiagnosticsBaselineEntry {
            fingerprint: diagnostic_fingerprint(path, "LineLength", snippet, 0),
            path: path.to_owned(),
            code: "LineLength".to_owned(),
            snippet: snippet.to_owned(),
            occurrence: 0,
            message: "m".to_owned(),
            severity: "Warning".to_owned(),
            range: DiagnosticsBaselineRange {
                start_line: 0,
                start_column: 0,
                end_line: 0,
                end_column: 1,
            },
        };
        let mut writer = PartitionFileWriter::new(&directory, &partition).unwrap();
        writer.write_entry(&entry).unwrap();
        let (prepared, count) = writer.finish().unwrap();
        assert_eq!(count, 1);
        let published = publish_set(&directory, "a".repeat(64), vec![prepared], None).unwrap();
        let object = &published.manifest.partitions[0];
        assert_eq!(hash_optional(&directory, &object.file).unwrap(), Some(object.blake3.clone()));
        assert!(std::fs::read_dir(root.path().join("baselines")).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("migration")));
    }

    /// The old manifest is parsed as plain JSON, so a corrupted one may name any path in
    /// the managed directory. Cleanup must touch only content-addressed objects.
    #[test]
    fn cleanup_refuses_to_delete_a_path_that_is_not_an_object() {
        let root = tempdir().unwrap();
        let directory = ManagedBaselineDirectory::open(root.path(), "baselines", true).unwrap();
        let scope = "a".repeat(64);
        let first =
            publish_set(&directory, scope.clone(), vec![partition("main", b"main-v1")], None)
                .unwrap();

        // A hand-edited manifest pointing at a file that is not an object.
        directory.create_file_new("innocent.json").unwrap().write_all(b"keep me").unwrap();
        let mut tampered = first.manifest.clone();
        tampered.partitions[0].file = "innocent.json".to_owned();
        atomic_write(&directory, "manifest.json", &diagnostics_manifest_json(&tampered).unwrap())
            .unwrap();

        publish_set(
            &directory,
            scope,
            vec![partition("main", b"main-v2")],
            Some(&tampered.generation),
        )
        .unwrap();

        assert!(
            read_optional(&directory, "innocent.json").unwrap().is_some(),
            "cleanup deleted a path the manifest named but that is not an object"
        );
    }

    /// Publication deletes superseded objects, but a partition that vanished from the
    /// manifest — dormant under `include`, or regrouped — must keep its file: the scope
    /// fingerprint does not cover grouping, so it would claim "same plan" while the ids
    /// changed, and the deletion cannot be undone.
    #[test]
    fn publish_keeps_objects_of_partitions_missing_from_the_new_manifest() {
        let root = tempdir().unwrap();
        let directory = ManagedBaselineDirectory::open(root.path(), "baselines", true).unwrap();
        let scope = "a".repeat(64);
        let first = publish_set(
            &directory,
            scope.clone(),
            vec![partition("main", b"main-v1"), partition("extension:A", b"ext-a")],
            None,
        )
        .unwrap();
        let dormant = first
            .manifest
            .partitions
            .iter()
            .find(|entry| entry.partition_id == "extension:A")
            .unwrap()
            .file
            .clone();

        // A new generation without `extension:A`, and with `main` rewritten.
        let second = publish_set(
            &directory,
            scope,
            vec![partition("main", b"main-v2")],
            Some(&first.manifest.generation),
        )
        .unwrap();
        let superseded = first
            .manifest
            .partitions
            .iter()
            .find(|entry| entry.partition_id == "main")
            .unwrap()
            .file
            .clone();
        assert_ne!(
            superseded, second.manifest.partitions[0].file,
            "positive control: main must actually have a new object"
        );

        assert!(
            read_optional(&directory, &superseded).unwrap().is_none(),
            "a superseded version of a living partition is still cleaned up"
        );
        assert!(
            read_optional(&directory, &dormant).unwrap().is_some(),
            "the object of a partition absent from the new manifest must survive"
        );
    }

    /// The streaming writer and the in-memory serializer produce the same object
    /// file. `repair_object` demands a byte-identical regeneration, so a set built
    /// by one of them must stay repairable by the other.
    #[test]
    fn streamed_and_serialized_partitions_are_byte_identical() {
        let root = tempdir().unwrap();
        let directory = ManagedBaselineDirectory::open(root.path(), "baselines", true).unwrap();
        let identity = DiagnosticsBaselinePartitionIdentity::Main { path: "src/cf".to_owned() };
        let partition = DiagnosticsBaselinePartition {
            id: "main".to_owned(),
            key: blake3::hash(b"main").to_hex().to_string(),
            identity: identity.clone(),
        };
        let path = "src/cf/Main.bsl";
        let snippet = "Message(1);";
        let entry = DiagnosticsBaselineEntry {
            fingerprint: diagnostic_fingerprint(path, "LineLength", snippet, 0),
            path: path.to_owned(),
            code: "LineLength".to_owned(),
            snippet: snippet.to_owned(),
            occurrence: 0,
            message: "m".to_owned(),
            severity: "Warning".to_owned(),
            range: DiagnosticsBaselineRange {
                start_line: 0,
                start_column: 0,
                end_line: 0,
                end_column: 1,
            },
        };

        let mut writer = PartitionFileWriter::new(&directory, &partition).unwrap();
        writer.write_entry(&entry).unwrap();
        let (prepared, _) = writer.finish().unwrap();
        let PreparedPartition::Staged { hash, .. } = &prepared else {
            panic!("the streaming writer stages its object")
        };

        let serialized = ide::partitioned_diagnostics_baseline::diagnostics_partition_json(
            identity,
            vec![entry],
        )
        .unwrap();
        assert_eq!(
            *hash,
            blake3::hash(&serialized).to_hex().to_string(),
            "a set created by migration would not be repairable by `create --partition`"
        );
    }

    #[test]
    fn partitioned_baseline_transaction_failure_before_manifest_preserves_old_generation() {
        let root = tempdir().unwrap();
        let directory = ManagedBaselineDirectory::open(root.path(), "baselines", true).unwrap();
        let first =
            publish_set(&directory, "a".repeat(64), vec![partition("main", b"one")], None).unwrap();
        let before = read_optional(&directory, "manifest.json").unwrap().unwrap();
        let error = publish_set_with_hook(
            &directory,
            "a".repeat(64),
            vec![partition("main", b"two")],
            Some(&first.manifest.generation),
            |stage| {
                if stage == TransactionStage::BeforeManifestReplace {
                    Err(io::Error::other("injected"))
                } else {
                    Ok(())
                }
            },
        );
        assert!(error.is_err());
        assert_eq!(read_optional(&directory, "manifest.json").unwrap().unwrap(), before);
        assert!(std::fs::read_dir(root.path().join("baselines")).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".manifest.json.tmp-")));
    }

    #[test]
    fn partitioned_baseline_transaction_reuses_unchanged_objects() {
        let root = tempdir().unwrap();
        let directory = ManagedBaselineDirectory::open(root.path(), "baselines", true).unwrap();
        let first = publish_set(
            &directory,
            "a".repeat(64),
            vec![partition("main", b"one"), partition("extension:Ext", b"two")],
            None,
        )
        .unwrap();
        let reused = first.manifest.partitions[1].clone();
        let second = publish_set(
            &directory,
            "a".repeat(64),
            vec![
                partition("main", b"changed"),
                PreparedPartition::Reuse {
                    id: "extension:Ext".to_owned(),
                    key: blake3::hash(b"extension:Ext").to_hex().to_string(),
                    entry: reused.clone(),
                },
            ],
            Some(&first.manifest.generation),
        )
        .unwrap();
        assert_eq!(second.stats, PublishStats { serialized_partitions: 1, reused_partitions: 1 });
        assert_eq!(second.manifest.partitions[1], reused);
    }

    #[test]
    fn cleanup_failure_does_not_change_committed_generation() {
        let root = tempdir().unwrap();
        let directory = ManagedBaselineDirectory::open(root.path(), "baselines", true).unwrap();
        let published = publish_set_with_hook(
            &directory,
            "a".repeat(64),
            vec![partition("main", b"one")],
            None,
            |stage| {
                if stage == TransactionStage::AfterCommit {
                    Err(io::Error::other("injected cleanup failure"))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap();
        let stored: DiagnosticsBaselineManifest =
            serde_json::from_slice(&read_optional(&directory, "manifest.json").unwrap().unwrap())
                .unwrap();
        assert_eq!(stored.generation, published.manifest.generation);
    }

    #[test]
    fn concurrent_selected_updates_never_lose_a_commit() {
        let root = tempdir().unwrap();
        let directory = ManagedBaselineDirectory::open(root.path(), "baselines", true).unwrap();
        let lock = directory.open_or_create_file(".baseline.lock").unwrap();
        lock.try_lock().unwrap();
        let error = publish_set(&directory, "a".repeat(64), vec![partition("main", b"one")], None)
            .unwrap_err();
        assert!(error.to_string().contains("busy"));
    }

    #[test]
    fn existing_content_addressed_object_is_verified_before_reuse() {
        let root = tempdir().unwrap();
        let directory = ManagedBaselineDirectory::open(root.path(), "baselines", true).unwrap();
        let first =
            publish_set(&directory, "a".repeat(64), vec![partition("main", b"one")], None).unwrap();
        let entry = first.manifest.partitions[0].clone();
        directory.remove_file(&entry.file).unwrap();
        directory.create_file_new(&entry.file).unwrap().write_all(b"corrupt").unwrap();
        let error = publish_set(
            &directory,
            "a".repeat(64),
            vec![PreparedPartition::Reuse {
                id: "main".to_owned(),
                key: blake3::hash(b"main").to_hex().to_string(),
                entry,
            }],
            Some(&first.manifest.generation),
        )
        .unwrap_err();
        assert!(error.to_string().contains("corrupt"));
    }

    #[test]
    fn selective_baseline_transaction_is_atomic_under_fault_and_concurrency() {
        let root = tempdir().unwrap();
        let directory = ManagedBaselineDirectory::open(root.path(), "baselines", true).unwrap();
        let first = publish_set(
            &directory,
            "a".repeat(64),
            vec![partition("main", b"one"), partition("extension:Ext", b"dormant")],
            None,
        )
        .unwrap();
        let dormant = first.manifest.partitions[1].clone();
        directory.remove_file(&dormant.file).unwrap();
        directory.create_file_new(&dormant.file).unwrap().write_all(b"corrupt").unwrap();

        let carried = PreparedPartition::Carry {
            id: "extension:Ext".to_owned(),
            key: blake3::hash(b"extension:Ext").to_hex().to_string(),
            entry: dormant.clone(),
        };
        let second = publish_set(
            &directory,
            "a".repeat(64),
            vec![partition("main", b"two"), carried],
            Some(&first.manifest.generation),
        )
        .unwrap();
        assert_eq!(second.manifest.partitions[1], dormant);

        let before = read_optional(&directory, "manifest.json").unwrap().unwrap();
        let error = publish_set_with_hook(
            &directory,
            "a".repeat(64),
            vec![partition("main", b"three")],
            Some(&second.manifest.generation),
            |stage| {
                if stage == TransactionStage::BeforeManifestReplace {
                    Err(io::Error::other("injected"))
                } else {
                    Ok(())
                }
            },
        );
        assert!(error.is_err());
        assert_eq!(read_optional(&directory, "manifest.json").unwrap().unwrap(), before);
        assert!(std::fs::read_dir(root.path().join("baselines")).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".manifest.json.tmp-")));

        let lock = directory.open_or_create_file(".baseline.lock").unwrap();
        lock.try_lock().unwrap();
        assert!(publish_set(
            &directory,
            "a".repeat(64),
            vec![partition("main", b"three")],
            Some(&second.manifest.generation),
        )
        .unwrap_err()
        .to_string()
        .contains("busy"));
    }
}
