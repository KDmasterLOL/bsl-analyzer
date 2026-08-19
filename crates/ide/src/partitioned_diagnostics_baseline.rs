use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Read;
use std::sync::Arc;

use project_model::{
    DiagnosticsBaselinePartitionIdentity, DiagnosticsBaselinePartitionPlan,
    ManagedBaselineDirectory,
};
use serde::de::{DeserializeSeed, Error as _, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::diagnostics_baseline::{
    diagnostic_fingerprint, normalize_diagnostic_snippet, BaselineDiagnosticCandidate,
    ClassifiedDiagnostic, DiagnosticsBaseline, DiagnosticsBaselineCoverage,
    DiagnosticsBaselineEntry, DiagnosticsBaselineError, DiagnosticsBaselinePartitionSummary,
    DiagnosticsBaselineRange, DiagnosticsBaselineState, DiagnosticsBaselineSummary,
    MissingDiagnosticSnippet,
};

pub const DIAGNOSTICS_BASELINE_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const DIAGNOSTICS_BASELINE_PARTITION_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsBaselineManifest {
    pub schema_version: u32,
    pub generation: String,
    pub project_scope_fingerprint: String,
    pub partitions: Vec<DiagnosticsBaselineManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsBaselineManifestEntry {
    pub partition_id: String,
    pub file: String,
    pub blake3: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsBaselinePartitionFile {
    pub schema_version: u32,
    pub partition: DiagnosticsBaselinePartitionIdentity,
    pub diagnostics: Vec<DiagnosticsBaselineEntry>,
}

#[derive(Debug, Clone)]
pub struct DiagnosticsBaselineSetSnapshot {
    pub manifest: DiagnosticsBaselineManifest,
    pub manifest_hash: [u8; 32],
    pub partitions: BTreeMap<String, Arc<PartitionSnapshot>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiagnosticsBaselineLoadStats {
    pub partitions_parsed: usize,
    pub fingerprints_validated: usize,
}

#[derive(Debug, Clone)]
pub struct PartitionSnapshot {
    pub identity: DiagnosticsBaselinePartitionIdentity,
    pub file: Arc<str>,
    pub hash: [u8; 32],
    entries: Vec<CompactBaselineEntry>,
}

#[derive(Debug, Clone)]
struct CompactBaselineEntry {
    fingerprint: [u8; 32],
    path: Arc<str>,
    code: Arc<str>,
    snippet: Box<str>,
    occurrence: u32,
    message: Box<str>,
    severity: Arc<str>,
    range: DiagnosticsBaselineRange,
}

impl CompactBaselineEntry {
    fn owned(&self) -> DiagnosticsBaselineEntry {
        DiagnosticsBaselineEntry {
            fingerprint: hex(&self.fingerprint),
            path: self.path.to_string(),
            code: self.code.to_string(),
            snippet: self.snippet.to_string(),
            occurrence: self.occurrence,
            message: self.message.to_string(),
            severity: self.severity.to_string(),
            range: self.range.clone(),
        }
    }
}

impl PartitionSnapshot {
    fn find(&self, fingerprint: &[u8; 32]) -> Option<&CompactBaselineEntry> {
        self.entries
            .binary_search_by_key(fingerprint, |entry| entry.fingerprint)
            .ok()
            .map(|index| &self.entries[index])
    }

    pub fn entries_len(&self) -> usize {
        self.entries.len()
    }

    pub fn owned_entries(&self) -> impl Iterator<Item = DiagnosticsBaselineEntry> + '_ {
        self.entries.iter().map(CompactBaselineEntry::owned)
    }
}

#[derive(Debug, Clone)]
pub struct PartitionedBaselineDiagnosticCandidate<T> {
    pub partition_id: String,
    pub candidate: BaselineDiagnosticCandidate<T>,
}

#[derive(Debug, Clone)]
pub struct ClassifiedPartitionedDiagnostics<T> {
    pub new: Vec<ClassifiedDiagnostic<T>>,
    pub known: Vec<ClassifiedDiagnostic<T>>,
    pub resolved: ResolvedPartitionedDiagnostics,
    pub summary: DiagnosticsBaselineSummary,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedPartitionedDiagnostics {
    partitions: Vec<ResolvedPartition>,
    len: usize,
}

#[derive(Debug, Clone)]
struct ResolvedPartition {
    id: String,
    partition: Arc<PartitionSnapshot>,
    matched: HashSet<[u8; 32]>,
    coverage: DiagnosticsBaselineCoverage,
}

impl ResolvedPartitionedDiagnostics {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn retain_partition(mut self, id: &str) -> Self {
        self.partitions.retain(|partition| partition.id == id);
        self.len = self
            .partitions
            .iter()
            .map(|partition| {
                partition
                    .partition
                    .entries
                    .iter()
                    .filter(|entry| {
                        let covered = match &partition.coverage {
                            DiagnosticsBaselineCoverage::Full => true,
                            DiagnosticsBaselineCoverage::Partial { completed_files } => {
                                completed_files.contains(entry.path.as_ref())
                            }
                        };
                        covered && !partition.matched.contains(&entry.fingerprint)
                    })
                    .count()
            })
            .sum();
        self
    }
}

pub struct ResolvedPartitionedDiagnosticsIter {
    partitions: std::vec::IntoIter<ResolvedPartition>,
    current: Option<(ResolvedPartition, usize)>,
    remaining: usize,
}

impl Iterator for ResolvedPartitionedDiagnosticsIter {
    type Item = DiagnosticsBaselineEntry;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let (partition, index) = match &mut self.current {
                Some(current) => current,
                None => {
                    self.current = Some((self.partitions.next()?, 0));
                    continue;
                }
            };
            let Some(entry) = partition.partition.entries.get(*index) else {
                self.current = None;
                continue;
            };
            *index += 1;
            let covered = match &partition.coverage {
                DiagnosticsBaselineCoverage::Full => true,
                DiagnosticsBaselineCoverage::Partial { completed_files } => {
                    completed_files.contains(entry.path.as_ref())
                }
            };
            if covered && !partition.matched.contains(&entry.fingerprint) {
                self.remaining -= 1;
                return Some(entry.owned());
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for ResolvedPartitionedDiagnosticsIter {}

impl IntoIterator for ResolvedPartitionedDiagnostics {
    type Item = DiagnosticsBaselineEntry;
    type IntoIter = ResolvedPartitionedDiagnosticsIter;

    fn into_iter(self) -> Self::IntoIter {
        ResolvedPartitionedDiagnosticsIter {
            partitions: self.partitions.into_iter(),
            current: None,
            remaining: self.len,
        }
    }
}

pub fn partitioned_coverage(
    plan: &project_model::DiagnosticsBaselinePartitionPlan,
    coverage: &DiagnosticsBaselineCoverage,
    all_project_files: Option<&BTreeSet<String>>,
) -> Result<BTreeMap<String, DiagnosticsBaselineCoverage>, String> {
    let mut result: BTreeMap<_, _> = plan
        .partitions
        .iter()
        .map(|partition| {
            (
                partition.id.clone(),
                match coverage {
                    DiagnosticsBaselineCoverage::Full => DiagnosticsBaselineCoverage::Full,
                    DiagnosticsBaselineCoverage::Partial { .. } => {
                        DiagnosticsBaselineCoverage::Partial { completed_files: BTreeSet::new() }
                    }
                },
            )
        })
        .collect();
    let DiagnosticsBaselineCoverage::Partial { completed_files } = coverage else {
        return Ok(result);
    };
    for path in completed_files {
        let owner = plan
            .owner_for_project_path(path)
            .ok_or_else(|| format!("completed diagnostics file has no partition owner: {path}"))?;
        let DiagnosticsBaselineCoverage::Partial { completed_files } =
            result.get_mut(owner).expect("plan owner exists")
        else {
            unreachable!()
        };
        completed_files.insert(path.clone());
    }
    let Some(all_project_files) = all_project_files else { return Ok(result) };
    let mut owner_files: BTreeMap<_, BTreeSet<_>> =
        plan.partitions.iter().map(|partition| (partition.id.clone(), BTreeSet::new())).collect();
    for path in all_project_files {
        let owner = plan
            .owner_for_project_path(path)
            .ok_or_else(|| format!("diagnostics file has no partition owner: {path}"))?;
        owner_files.get_mut(owner).expect("plan owner exists").insert(path.clone());
    }
    for (partition_id, partition_coverage) in &mut result {
        let DiagnosticsBaselineCoverage::Partial { completed_files } = partition_coverage else {
            continue;
        };
        if !owner_files[partition_id].is_empty()
            && owner_files[partition_id].is_subset(completed_files)
        {
            *partition_coverage = DiagnosticsBaselineCoverage::Full;
        }
    }
    Ok(result)
}

pub fn classify_partitioned_diagnostics<T>(
    snapshot: &DiagnosticsBaselineSetSnapshot,
    baseline_path: String,
    mut current: Vec<PartitionedBaselineDiagnosticCandidate<T>>,
    coverage: &BTreeMap<String, DiagnosticsBaselineCoverage>,
) -> Result<ClassifiedPartitionedDiagnostics<T>, MissingDiagnosticSnippet> {
    current.sort_by(|left, right| {
        let left = &left.candidate;
        let right = &right.candidate;
        (&left.path, &left.range, &left.code, &left.message).cmp(&(
            &right.path,
            &right.range,
            &right.code,
            &right.message,
        ))
    });
    let mut occurrences = HashMap::<(String, String, String), u32>::new();
    let mut matched = BTreeMap::<String, HashSet<[u8; 32]>>::new();
    let mut counts = BTreeMap::<String, (usize, usize)>::new();
    let mut new = Vec::new();
    let mut known = Vec::new();

    for item in current {
        let candidate = item.candidate;
        let snippet = normalize_diagnostic_snippet(
            candidate.snippet.as_deref().ok_or(MissingDiagnosticSnippet)?,
        );
        let occurrence = occurrences
            .entry((candidate.path.clone(), candidate.code.clone(), snippet.clone()))
            .or_default();
        let fingerprint =
            diagnostic_fingerprint(&candidate.path, &candidate.code, &snippet, *occurrence);
        let fingerprint_bytes = parse_hash(&fingerprint).expect("generated BLAKE3 is valid");
        let entry = DiagnosticsBaselineEntry {
            fingerprint,
            path: candidate.path,
            code: candidate.code,
            snippet,
            occurrence: *occurrence,
            message: candidate.message,
            severity: candidate.severity,
            range: candidate.range,
        };
        *occurrence += 1;
        let classified = ClassifiedDiagnostic { diagnostic: candidate.diagnostic, entry };
        let partition = snapshot.partitions.get(&item.partition_id);
        let is_known = !protected(&classified.entry.code)
            && partition.is_some_and(|partition| partition.find(&fingerprint_bytes).is_some());
        let partition_counts = counts.entry(item.partition_id.clone()).or_default();
        if is_known {
            partition_counts.1 += 1;
            matched.entry(item.partition_id).or_default().insert(fingerprint_bytes);
            known.push(classified);
        } else {
            partition_counts.0 += 1;
            new.push(classified);
        }
    }

    let mut resolved = ResolvedPartitionedDiagnostics::default();
    let mut partition_summaries = Vec::with_capacity(snapshot.partitions.len());
    for manifest_entry in &snapshot.manifest.partitions {
        let partition = snapshot.partitions[&manifest_entry.partition_id].clone();
        let partition_coverage = coverage
            .get(&manifest_entry.partition_id)
            .unwrap_or(&DiagnosticsBaselineCoverage::Full);
        let matched = matched.remove(&manifest_entry.partition_id).unwrap_or_default();
        let complete = matches!(partition_coverage, DiagnosticsBaselineCoverage::Full);
        let mut partition_resolved = 0;
        for entry in &partition.entries {
            let covered = match partition_coverage {
                DiagnosticsBaselineCoverage::Full => true,
                DiagnosticsBaselineCoverage::Partial { completed_files } => {
                    completed_files.contains(entry.path.as_ref())
                }
            };
            if covered && !matched.contains(&entry.fingerprint) {
                partition_resolved += 1;
            }
        }
        resolved.len += partition_resolved;
        resolved.partitions.push(ResolvedPartition {
            id: manifest_entry.partition_id.clone(),
            partition: partition.clone(),
            matched,
            coverage: partition_coverage.clone(),
        });
        let (partition_new, partition_known) =
            counts.get(&manifest_entry.partition_id).copied().unwrap_or_default();
        partition_summaries.push(DiagnosticsBaselinePartitionSummary {
            id: manifest_entry.partition_id.clone(),
            identity: partition.identity.clone(),
            path: manifest_entry.file.clone(),
            schema_version: DIAGNOSTICS_BASELINE_PARTITION_SCHEMA_VERSION,
            state: if complete {
                DiagnosticsBaselineState::Full
            } else {
                DiagnosticsBaselineState::Partial
            },
            new: partition_new,
            known: partition_known,
            resolved: partition_resolved,
            complete,
        });
    }
    let complete = partition_summaries.iter().all(|partition| partition.complete);
    let summary = DiagnosticsBaselineSummary {
        state: if complete {
            DiagnosticsBaselineState::Full
        } else {
            DiagnosticsBaselineState::Partial
        },
        new: Some(new.len()),
        known: Some(known.len()),
        resolved: Some(resolved.len()),
        path: Some(baseline_path),
        schema_version: Some(DIAGNOSTICS_BASELINE_PARTITION_SCHEMA_VERSION),
        manifest_schema_version: Some(DIAGNOSTICS_BASELINE_MANIFEST_SCHEMA_VERSION),
        complete,
        error_code: None,
        detail: None,
        partitions: partition_summaries,
        errors: vec![],
    };
    Ok(ClassifiedPartitionedDiagnostics { new, known, resolved, summary })
}

pub fn migrate_v1_to_partitioned(
    baseline: &DiagnosticsBaseline,
    plan: &DiagnosticsBaselinePartitionPlan,
) -> Result<BTreeMap<String, Vec<DiagnosticsBaselineEntry>>, PartitionedDiagnosticsBaselineError> {
    // Reuse the unchanged schema-v1 validator; migration must not define a second recipe.
    crate::diagnostics_baseline::diagnostics_baseline_json(baseline)?;
    let expected_scope = expected_v1_scope(plan);
    if baseline.scope != expected_scope {
        return Err(PartitionedDiagnosticsBaselineError::ScopeMismatch);
    }
    let mut result: BTreeMap<_, Vec<_>> =
        plan.partitions.iter().map(|partition| (partition.id.clone(), Vec::new())).collect();
    for entry in &baseline.diagnostics {
        let owner = plan.owner_for_project_path(&entry.path).ok_or_else(|| {
            PartitionedDiagnosticsBaselineError::UnownedDiagnostic(entry.path.clone())
        })?;
        result.get_mut(owner).expect("plan owner exists").push(entry.clone());
    }
    for entries in result.values_mut() {
        entries.sort_by(entry_sort);
    }
    Ok(result)
}

pub fn migrate_v1_reader<R, F>(
    reader: R,
    plan: &DiagnosticsBaselinePartitionPlan,
    mut write_entry: F,
) -> Result<usize, PartitionedDiagnosticsBaselineError>
where
    R: Read,
    F: FnMut(&str, &DiagnosticsBaselineEntry) -> Result<(), PartitionedDiagnosticsBaselineError>,
{
    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    let parsed = LegacyMigrationSeed { plan, write_entry: &mut write_entry }
        .deserialize(&mut deserializer)?;
    deserializer.end()?;
    if parsed.schema_version != crate::diagnostics_baseline::DIAGNOSTICS_BASELINE_SCHEMA_VERSION {
        return Err(DiagnosticsBaselineError::UnsupportedSchema {
            found: parsed.schema_version,
            expected: crate::diagnostics_baseline::DIAGNOSTICS_BASELINE_SCHEMA_VERSION,
        }
        .into());
    }
    if parsed.scope != expected_v1_scope(plan) {
        return Err(PartitionedDiagnosticsBaselineError::ScopeMismatch);
    }
    Ok(parsed.entries)
}

fn expected_v1_scope(
    plan: &DiagnosticsBaselinePartitionPlan,
) -> crate::diagnostics_baseline::DiagnosticsBaselineScope {
    crate::diagnostics_baseline::DiagnosticsBaselineScope {
        source_root: plan.project_scope.source_root.clone(),
        extensions: plan
            .project_scope
            .extensions
            .iter()
            .map(|extension| crate::diagnostics_baseline::DiagnosticsBaselineExtension {
                name: extension.name.clone(),
                path: extension.path.clone(),
                depends_on: extension.depends_on.clone(),
            })
            .collect(),
    }
}

struct LegacyMigration {
    schema_version: u32,
    scope: crate::diagnostics_baseline::DiagnosticsBaselineScope,
    entries: usize,
}

struct LegacyMigrationSeed<'a, F> {
    plan: &'a DiagnosticsBaselinePartitionPlan,
    write_entry: &'a mut F,
}

impl<'de, F> DeserializeSeed<'de> for LegacyMigrationSeed<'_, F>
where
    F: FnMut(&str, &DiagnosticsBaselineEntry) -> Result<(), PartitionedDiagnosticsBaselineError>,
{
    type Value = LegacyMigration;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_map(LegacyMigrationVisitor {
            plan: self.plan,
            write_entry: self.write_entry,
        })
    }
}

struct LegacyMigrationVisitor<'a, F> {
    plan: &'a DiagnosticsBaselinePartitionPlan,
    write_entry: &'a mut F,
}

impl<'de, F> Visitor<'de> for LegacyMigrationVisitor<'_, F>
where
    F: FnMut(&str, &DiagnosticsBaselineEntry) -> Result<(), PartitionedDiagnosticsBaselineError>,
{
    type Value = LegacyMigration;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a schema-v1 diagnostics baseline")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut schema_version = None;
        let mut scope = None;
        let mut entries = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "schema_version" if schema_version.is_none() => {
                    schema_version = Some(map.next_value()?);
                }
                "scope" if scope.is_none() => scope = Some(map.next_value()?),
                "diagnostics" if entries.is_none() => {
                    entries = Some(map.next_value_seed(LegacyEntriesSeed {
                        plan: self.plan,
                        write_entry: self.write_entry,
                    })?);
                }
                "schema_version" | "scope" | "diagnostics" => {
                    return Err(A::Error::custom(format!("duplicate field {key}")));
                }
                _ => {
                    let _: IgnoredAny = map.next_value()?;
                    return Err(A::Error::custom(format!("unknown field {key}")));
                }
            }
        }
        Ok(LegacyMigration {
            schema_version: schema_version
                .ok_or_else(|| A::Error::missing_field("schema_version"))?,
            scope: scope.ok_or_else(|| A::Error::missing_field("scope"))?,
            entries: entries.ok_or_else(|| A::Error::missing_field("diagnostics"))?,
        })
    }
}

struct LegacyEntriesSeed<'a, F> {
    plan: &'a DiagnosticsBaselinePartitionPlan,
    write_entry: &'a mut F,
}

impl<'de, F> DeserializeSeed<'de> for LegacyEntriesSeed<'_, F>
where
    F: FnMut(&str, &DiagnosticsBaselineEntry) -> Result<(), PartitionedDiagnosticsBaselineError>,
{
    type Value = usize;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(LegacyEntriesVisitor {
            plan: self.plan,
            write_entry: self.write_entry,
        })
    }
}

struct LegacyEntriesVisitor<'a, F> {
    plan: &'a DiagnosticsBaselinePartitionPlan,
    write_entry: &'a mut F,
}

impl<'de, F> Visitor<'de> for LegacyEntriesVisitor<'_, F>
where
    F: FnMut(&str, &DiagnosticsBaselineEntry) -> Result<(), PartitionedDiagnosticsBaselineError>,
{
    type Value = usize;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an array of schema-v1 diagnostics baseline entries")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        let mut seen = HashSet::new();
        let mut count = 0;
        while let Some(entry) = sequence.next_element::<DiagnosticsBaselineEntry>()? {
            validate_source_path(&entry.path, false).map_err(A::Error::custom)?;
            if protected(&entry.code) {
                return Err(A::Error::custom(
                    PartitionedDiagnosticsBaselineError::ProtectedDiagnostic(entry.code),
                ));
            }
            let expected =
                diagnostic_fingerprint(&entry.path, &entry.code, &entry.snippet, entry.occurrence);
            if entry.fingerprint != expected {
                return Err(A::Error::custom(
                    PartitionedDiagnosticsBaselineError::FingerprintMismatch(entry.fingerprint),
                ));
            }
            let fingerprint = parse_hash(&entry.fingerprint).map_err(A::Error::custom)?;
            if !seen.insert(fingerprint) {
                return Err(A::Error::custom(PartitionedDiagnosticsBaselineError::Duplicate(
                    entry.fingerprint,
                )));
            }
            let owner = self.plan.owner_for_project_path(&entry.path).ok_or_else(|| {
                A::Error::custom(PartitionedDiagnosticsBaselineError::UnownedDiagnostic(
                    entry.path.clone(),
                ))
            })?;
            (self.write_entry)(owner, &entry).map_err(A::Error::custom)?;
            count += 1;
        }
        Ok(count)
    }
}

#[derive(Default)]
struct StringPool(HashMap<String, Arc<str>>);

impl StringPool {
    fn intern(&mut self, value: String) -> Arc<str> {
        if let Some(existing) = self.0.get(&value) {
            return existing.clone();
        }
        let shared: Arc<str> = Arc::from(value.as_str());
        self.0.insert(value, shared.clone());
        shared
    }
}

pub fn load_diagnostics_baseline_set(
    directory: &ManagedBaselineDirectory,
    plan: &DiagnosticsBaselinePartitionPlan,
) -> Result<DiagnosticsBaselineSetSnapshot, PartitionedDiagnosticsBaselineError> {
    load_diagnostics_baseline_set_reusing(directory, plan, None, &BTreeSet::new())
        .map(|(snapshot, _)| snapshot)
}

pub fn load_diagnostics_baseline_set_reusing(
    directory: &ManagedBaselineDirectory,
    plan: &DiagnosticsBaselinePartitionPlan,
    previous: Option<&DiagnosticsBaselineSetSnapshot>,
    changed_objects: &BTreeSet<String>,
) -> Result<
    (DiagnosticsBaselineSetSnapshot, DiagnosticsBaselineLoadStats),
    PartitionedDiagnosticsBaselineError,
> {
    for attempt in 0..2 {
        match load_diagnostics_baseline_set_once(directory, plan, previous, changed_objects) {
            Err(PartitionedDiagnosticsBaselineError::ConcurrentUpdate) if attempt == 0 => {}
            result => return result,
        }
    }
    unreachable!("the bounded manifest retry always returns on its second attempt")
}

fn load_diagnostics_baseline_set_once(
    directory: &ManagedBaselineDirectory,
    plan: &DiagnosticsBaselinePartitionPlan,
    previous: Option<&DiagnosticsBaselineSetSnapshot>,
    changed_objects: &BTreeSet<String>,
) -> Result<
    (DiagnosticsBaselineSetSnapshot, DiagnosticsBaselineLoadStats),
    PartitionedDiagnosticsBaselineError,
> {
    let first_manifest = read_file(directory, "manifest.json")?;
    let loaded = (|| {
        let manifest: DiagnosticsBaselineManifest = serde_json::from_slice(&first_manifest)?;
        validate_manifest_shape(&manifest)?;
        let expected: BTreeMap<_, _> =
            plan.partitions.iter().map(|partition| (partition.id.as_str(), partition)).collect();
        let expected_ids: BTreeSet<_> = expected.keys().copied().collect();
        let actual_ids: BTreeSet<_> =
            manifest.partitions.iter().map(|partition| partition.partition_id.as_str()).collect();
        let missing: Vec<_> =
            expected_ids.difference(&actual_ids).map(|id| (*id).to_owned()).collect();
        let orphan: Vec<_> =
            actual_ids.difference(&expected_ids).map(|id| (*id).to_owned()).collect();
        if !missing.is_empty() {
            return Err(PartitionedDiagnosticsBaselineError::MissingPartitions {
                ids: missing,
                orphan_ids: orphan,
            });
        }
        if !orphan.is_empty() {
            return Err(PartitionedDiagnosticsBaselineError::OrphanPartitions(orphan));
        }

        let reusable: BTreeSet<_> = manifest
            .partitions
            .iter()
            .filter(|entry| {
                let expected_partition = expected[entry.partition_id.as_str()];
                previous
                    .and_then(|snapshot| snapshot.partitions.get(&entry.partition_id))
                    .is_some_and(|partition| {
                        !changed_objects.contains(&entry.file)
                            && partition.file.as_ref() == entry.file
                            && hex(&partition.hash) == entry.blake3
                            && partition.identity == expected_partition.identity
                    })
            })
            .map(|entry| entry.partition_id.as_str())
            .collect();
        let reused_partitions: Vec<_> = previous
            .into_iter()
            .flat_map(|snapshot| &snapshot.partitions)
            .filter(|(id, _)| reusable.contains(id.as_str()))
            .map(|(_, partition)| partition)
            .collect();

        let mut pool = StringPool::default();
        let mut seen_changed_fingerprints = HashSet::new();
        let mut partitions = BTreeMap::new();
        let mut stats = DiagnosticsBaselineLoadStats::default();
        for manifest_entry in &manifest.partitions {
            let expected_partition = expected[manifest_entry.partition_id.as_str()];
            if reusable.contains(manifest_entry.partition_id.as_str()) {
                let partition = previous.unwrap().partitions[&manifest_entry.partition_id].clone();
                partitions.insert(manifest_entry.partition_id.clone(), partition);
                continue;
            }
            let file = match directory.open_file(&manifest_entry.file) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Err(PartitionedDiagnosticsBaselineError::MissingPartitions {
                        ids: vec![manifest_entry.partition_id.clone()],
                        orphan_ids: vec![],
                    });
                }
                Err(error) => return Err(error.into()),
            };
            let mut reader = HashingReader::new(file);
            let mut deserializer = serde_json::Deserializer::from_reader(&mut reader);
            let parsed = PartitionSeed { pool: &mut pool }.deserialize(&mut deserializer)?;
            stats.partitions_parsed += 1;
            deserializer.end()?;
            drop(deserializer);
            let actual_hash = reader.finalize();
            if parsed.identity != expected_partition.identity {
                return Err(PartitionedDiagnosticsBaselineError::PartitionIdentityMismatch(
                    manifest_entry.partition_id.clone(),
                ));
            }
            let expected_path = partition_object_path(
                &expected_partition.id,
                &expected_partition.key,
                &manifest_entry.blake3,
            )?;
            if manifest_entry.file != expected_path {
                return Err(PartitionedDiagnosticsBaselineError::InvalidPath(
                    manifest_entry.file.clone(),
                ));
            }
            if hex(&actual_hash) != manifest_entry.blake3 {
                return Err(PartitionedDiagnosticsBaselineError::ObjectHashMismatch(
                    manifest_entry.partition_id.clone(),
                ));
            }
            if parsed.schema_version != DIAGNOSTICS_BASELINE_PARTITION_SCHEMA_VERSION {
                return Err(PartitionedDiagnosticsBaselineError::UnsupportedPartitionSchema(
                    parsed.schema_version,
                ));
            }
            for entry in &parsed.entries {
                stats.fingerprints_validated += 1;
                if plan.owner_for_project_path(&entry.path) != Some(&manifest_entry.partition_id) {
                    return Err(PartitionedDiagnosticsBaselineError::UnownedDiagnostic(
                        entry.path.to_string(),
                    ));
                }
                if !seen_changed_fingerprints.insert(entry.fingerprint)
                    || reused_partitions
                        .iter()
                        .any(|partition| partition.find(&entry.fingerprint).is_some())
                {
                    return Err(PartitionedDiagnosticsBaselineError::Duplicate(hex(
                        &entry.fingerprint
                    )));
                }
            }
            partitions.insert(
                manifest_entry.partition_id.clone(),
                Arc::new(PartitionSnapshot {
                    identity: parsed.identity,
                    file: Arc::from(manifest_entry.file.as_str()),
                    hash: actual_hash,
                    entries: parsed.entries,
                }),
            );
        }
        if manifest.project_scope_fingerprint != plan.project_scope_fingerprint {
            return Err(PartitionedDiagnosticsBaselineError::ScopeMismatch);
        }
        Ok((
            DiagnosticsBaselineSetSnapshot {
                manifest,
                manifest_hash: *blake3::hash(&first_manifest).as_bytes(),
                partitions,
            },
            stats,
        ))
    })();
    match read_file(directory, "manifest.json") {
        Ok(second_manifest) if second_manifest == first_manifest => loaded,
        Ok(_) | Err(_) => Err(PartitionedDiagnosticsBaselineError::ConcurrentUpdate),
    }
}

fn read_file(
    directory: &ManagedBaselineDirectory,
    path: &str,
) -> Result<Vec<u8>, PartitionedDiagnosticsBaselineError> {
    let mut bytes = Vec::new();
    directory.open_file(path)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

struct HashingReader<R> {
    inner: R,
    hasher: blake3::Hasher,
}

impl<R> HashingReader<R> {
    fn new(inner: R) -> Self {
        Self { inner, hasher: blake3::Hasher::new() }
    }

    fn finalize(&self) -> [u8; 32] {
        *self.hasher.clone().finalize().as_bytes()
    }
}

impl<R: Read> Read for HashingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.hasher.update(&buffer[..read]);
        Ok(read)
    }
}

struct ParsedPartition {
    schema_version: u32,
    identity: DiagnosticsBaselinePartitionIdentity,
    entries: Vec<CompactBaselineEntry>,
}

struct PartitionSeed<'a> {
    pool: &'a mut StringPool,
}

impl<'de> DeserializeSeed<'de> for PartitionSeed<'_> {
    type Value = ParsedPartition;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_map(PartitionVisitor { pool: self.pool })
    }
}

struct PartitionVisitor<'a> {
    pool: &'a mut StringPool,
}

impl<'de> Visitor<'de> for PartitionVisitor<'_> {
    type Value = ParsedPartition;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a diagnostics baseline partition")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut schema_version = None;
        let mut identity = None;
        let mut entries = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "schema_version" if schema_version.is_none() => {
                    schema_version = Some(map.next_value()?);
                }
                "partition" if identity.is_none() => identity = Some(map.next_value()?),
                "diagnostics" if entries.is_none() => {
                    entries = Some(map.next_value_seed(EntriesSeed { pool: self.pool })?);
                }
                "schema_version" | "partition" | "diagnostics" => {
                    return Err(A::Error::custom(format!("duplicate field {key}")));
                }
                _ => {
                    let _: IgnoredAny = map.next_value()?;
                    return Err(A::Error::custom(format!("unknown field {key}")));
                }
            }
        }
        Ok(ParsedPartition {
            schema_version: schema_version
                .ok_or_else(|| A::Error::missing_field("schema_version"))?,
            identity: identity.ok_or_else(|| A::Error::missing_field("partition"))?,
            entries: entries.ok_or_else(|| A::Error::missing_field("diagnostics"))?,
        })
    }
}

struct EntriesSeed<'a> {
    pool: &'a mut StringPool,
}

impl<'de> DeserializeSeed<'de> for EntriesSeed<'_> {
    type Value = Vec<CompactBaselineEntry>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(EntriesVisitor { pool: self.pool })
    }
}

struct EntriesVisitor<'a> {
    pool: &'a mut StringPool,
}

impl<'de> Visitor<'de> for EntriesVisitor<'_> {
    type Value = Vec<CompactBaselineEntry>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an array of diagnostics baseline entries")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        let mut entries = Vec::with_capacity(sequence.size_hint().unwrap_or(0));
        let mut seen = HashSet::new();
        while let Some(entry) = sequence.next_element::<DiagnosticsBaselineEntry>()? {
            validate_source_path(&entry.path, false).map_err(A::Error::custom)?;
            if protected(&entry.code) {
                return Err(A::Error::custom(
                    PartitionedDiagnosticsBaselineError::ProtectedDiagnostic(entry.code),
                ));
            }
            let expected =
                diagnostic_fingerprint(&entry.path, &entry.code, &entry.snippet, entry.occurrence);
            if entry.fingerprint != expected {
                return Err(A::Error::custom(
                    PartitionedDiagnosticsBaselineError::FingerprintMismatch(entry.fingerprint),
                ));
            }
            let fingerprint = parse_hash(&entry.fingerprint).map_err(A::Error::custom)?;
            if !seen.insert(fingerprint) {
                return Err(A::Error::custom(PartitionedDiagnosticsBaselineError::Duplicate(
                    entry.fingerprint,
                )));
            }
            entries.push(CompactBaselineEntry {
                fingerprint,
                path: self.pool.intern(entry.path),
                code: self.pool.intern(entry.code),
                snippet: entry.snippet.into_boxed_str(),
                occurrence: entry.occurrence,
                message: entry.message.into_boxed_str(),
                severity: self.pool.intern(entry.severity),
                range: entry.range,
            });
        }
        entries.sort_unstable_by_key(|entry| entry.fingerprint);
        Ok(entries)
    }
}

fn parse_hash(value: &str) -> Result<[u8; 32], PartitionedDiagnosticsBaselineError> {
    validate_hex(value)?;
    Ok(*blake3::Hash::from_hex(value)
        .map_err(|_| PartitionedDiagnosticsBaselineError::InvalidHash(value.to_owned()))?
        .as_bytes())
}

fn hex(value: &[u8; 32]) -> String {
    blake3::Hash::from_bytes(*value).to_hex().to_string()
}

pub fn diagnostics_partition_json(
    identity: DiagnosticsBaselinePartitionIdentity,
    mut diagnostics: Vec<DiagnosticsBaselineEntry>,
) -> Result<Vec<u8>, PartitionedDiagnosticsBaselineError> {
    diagnostics.sort_by(entry_sort);
    validate_entries(&diagnostics)?;
    let file = DiagnosticsBaselinePartitionFile {
        schema_version: DIAGNOSTICS_BASELINE_PARTITION_SCHEMA_VERSION,
        partition: identity,
        diagnostics,
    };
    pretty_json(&file)
}

pub fn diagnostics_manifest(
    project_scope_fingerprint: String,
    mut partitions: Vec<DiagnosticsBaselineManifestEntry>,
) -> DiagnosticsBaselineManifest {
    partitions.sort_by(|left, right| {
        partition_id_sort_key(&left.partition_id).cmp(&partition_id_sort_key(&right.partition_id))
    });
    let generation = baseline_generation(&project_scope_fingerprint, &partitions);
    DiagnosticsBaselineManifest {
        schema_version: DIAGNOSTICS_BASELINE_MANIFEST_SCHEMA_VERSION,
        generation,
        project_scope_fingerprint,
        partitions,
    }
}

fn partition_id_sort_key(id: &str) -> (u8, &str) {
    if id == "main" {
        (0, id)
    } else if id.starts_with("group:") {
        (1, id)
    } else {
        (2, id)
    }
}

pub fn diagnostics_manifest_json(
    manifest: &DiagnosticsBaselineManifest,
) -> Result<Vec<u8>, PartitionedDiagnosticsBaselineError> {
    validate_manifest_shape(manifest)?;
    pretty_json(manifest)
}

pub fn partition_object_path(
    id: &str,
    key: &str,
    hash: &str,
) -> Result<String, PartitionedDiagnosticsBaselineError> {
    validate_hex(key)?;
    validate_hex(hash)?;
    let category = if id == "main" {
        "main"
    } else if id.starts_with("extension:") {
        "extensions"
    } else if id.starts_with("group:") {
        "groups"
    } else {
        return Err(PartitionedDiagnosticsBaselineError::InvalidPartitionId(id.to_owned()));
    };
    Ok(if id == "main" {
        format!("objects/{category}/{hash}.json")
    } else {
        format!("objects/{category}/{key}/{hash}.json")
    })
}

pub fn baseline_generation(scope: &str, partitions: &[DiagnosticsBaselineManifestEntry]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"bsl-analyzer/diagnostics-baseline/generation/v1\0");
    hasher.update(scope.as_bytes());
    for partition in partitions {
        hasher.update(&[0]);
        hasher.update(partition.partition_id.as_bytes());
        hasher.update(&[0]);
        hasher.update(partition.blake3.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

fn pretty_json<T: Serialize>(value: &T) -> Result<Vec<u8>, PartitionedDiagnosticsBaselineError> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn entry_sort(
    left: &DiagnosticsBaselineEntry,
    right: &DiagnosticsBaselineEntry,
) -> std::cmp::Ordering {
    (&left.path, &left.code, &left.snippet, left.occurrence).cmp(&(
        &right.path,
        &right.code,
        &right.snippet,
        right.occurrence,
    ))
}

fn validate_entries(
    entries: &[DiagnosticsBaselineEntry],
) -> Result<(), PartitionedDiagnosticsBaselineError> {
    let mut fingerprints = HashSet::new();
    let mut identities = HashSet::new();
    for entry in entries {
        validate_source_path(&entry.path, false)?;
        if protected(&entry.code) {
            return Err(PartitionedDiagnosticsBaselineError::ProtectedDiagnostic(
                entry.code.clone(),
            ));
        }
        let expected =
            diagnostic_fingerprint(&entry.path, &entry.code, &entry.snippet, entry.occurrence);
        if entry.fingerprint != expected {
            return Err(PartitionedDiagnosticsBaselineError::FingerprintMismatch(
                entry.fingerprint.clone(),
            ));
        }
        if !fingerprints.insert(entry.fingerprint.as_str())
            || !identities.insert((
                entry.path.as_str(),
                entry.code.as_str(),
                entry.snippet.as_str(),
                entry.occurrence,
            ))
        {
            return Err(PartitionedDiagnosticsBaselineError::Duplicate(entry.fingerprint.clone()));
        }
    }
    Ok(())
}

fn validate_manifest_shape(
    manifest: &DiagnosticsBaselineManifest,
) -> Result<(), PartitionedDiagnosticsBaselineError> {
    if manifest.schema_version != DIAGNOSTICS_BASELINE_MANIFEST_SCHEMA_VERSION {
        return Err(PartitionedDiagnosticsBaselineError::UnsupportedManifestSchema(
            manifest.schema_version,
        ));
    }
    let mut ids = HashSet::new();
    for entry in &manifest.partitions {
        if !ids.insert(entry.partition_id.as_str()) {
            return Err(PartitionedDiagnosticsBaselineError::DuplicatePartition(
                entry.partition_id.clone(),
            ));
        }
        validate_managed_file(&entry.file)?;
        validate_hex(&entry.blake3)?;
    }
    if manifest.generation
        != baseline_generation(&manifest.project_scope_fingerprint, &manifest.partitions)
    {
        return Err(PartitionedDiagnosticsBaselineError::GenerationMismatch);
    }
    Ok(())
}

fn validate_hex(value: &str) -> Result<(), PartitionedDiagnosticsBaselineError> {
    if value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(PartitionedDiagnosticsBaselineError::InvalidHash(value.to_owned()))
    }
}

fn validate_managed_file(path: &str) -> Result<(), PartitionedDiagnosticsBaselineError> {
    if !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
    {
        Ok(())
    } else {
        Err(PartitionedDiagnosticsBaselineError::InvalidPath(path.to_owned()))
    }
}

fn validate_source_path(
    path: &str,
    allow_empty: bool,
) -> Result<(), PartitionedDiagnosticsBaselineError> {
    if (allow_empty && path.is_empty())
        || (!path.is_empty()
            && !path.starts_with('/')
            && !path.contains('\\')
            && path
                .split('/')
                .all(|component| !component.is_empty() && component != "." && component != ".."))
    {
        Ok(())
    } else {
        Err(PartitionedDiagnosticsBaselineError::InvalidPath(path.to_owned()))
    }
}

fn protected(code: &str) -> bool {
    matches!(code, "UnknownSuppressionCode" | "SuppressionWithoutCode")
}

#[derive(Debug, thiserror::Error)]
pub enum PartitionedDiagnosticsBaselineError {
    #[error("invalid partitioned diagnostics baseline JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Legacy(#[from] DiagnosticsBaselineError),
    #[error("diagnostics baseline I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported manifest schema {0}")]
    UnsupportedManifestSchema(u32),
    #[error("unsupported partition schema {0}")]
    UnsupportedPartitionSchema(u32),
    #[error("project scope does not match the manifest")]
    ScopeMismatch,
    #[error("partition identity does not match: {0}")]
    PartitionIdentityMismatch(String),
    #[error("missing diagnostics baseline partitions: {ids:?}")]
    MissingPartitions { ids: Vec<String>, orphan_ids: Vec<String> },
    #[error("orphan diagnostics baseline partitions: {0:?}")]
    OrphanPartitions(Vec<String>),
    #[error("invalid partition id: {0}")]
    InvalidPartitionId(String),
    #[error("invalid managed path: {0}")]
    InvalidPath(String),
    #[error("invalid BLAKE3 value: {0}")]
    InvalidHash(String),
    #[error("manifest generation does not match its partition hashes")]
    GenerationMismatch,
    #[error("partition object hash mismatch: {0}")]
    ObjectHashMismatch(String),
    #[error("duplicate partition: {0}")]
    DuplicatePartition(String),
    #[error("duplicate diagnostic: {0}")]
    Duplicate(String),
    #[error("diagnostic fingerprint does not match fields: {0}")]
    FingerprintMismatch(String),
    #[error("protected diagnostic cannot enter a baseline: {0}")]
    ProtectedDiagnostic(String),
    #[error("diagnostic path has no unique owner: {0}")]
    UnownedDiagnostic(String),
    #[error("diagnostics baseline changed while it was being loaded")]
    ConcurrentUpdate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartitionedDiagnosticsBaselineErrorInfo<'a> {
    pub partition_id: Option<&'a str>,
    pub code: &'static str,
}

impl PartitionedDiagnosticsBaselineError {
    pub fn info(&self) -> PartitionedDiagnosticsBaselineErrorInfo<'_> {
        match self {
            Self::MissingPartitions { ids, orphan_ids: _ } => {
                PartitionedDiagnosticsBaselineErrorInfo {
                    partition_id: ids.first().map(String::as_str),
                    code: "missing_partition",
                }
            }
            Self::OrphanPartitions(ids) => PartitionedDiagnosticsBaselineErrorInfo {
                partition_id: ids.first().map(String::as_str),
                code: "orphan_partition",
            },
            Self::PartitionIdentityMismatch(id) => PartitionedDiagnosticsBaselineErrorInfo {
                partition_id: Some(id),
                code: "partition_identity_mismatch",
            },
            Self::ScopeMismatch => PartitionedDiagnosticsBaselineErrorInfo {
                partition_id: None,
                code: "scope_mismatch",
            },
            _ => {
                PartitionedDiagnosticsBaselineErrorInfo { partition_id: None, code: "invalid_set" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use project_model::{
        DiagnosticsBaselinePartition, DiagnosticsBaselinePartitionIdentity,
        DiagnosticsBaselineRootOwner,
    };
    use std::io::Write;
    use tempfile::tempdir;

    fn entry(path: &str, code: &str) -> DiagnosticsBaselineEntry {
        let snippet = "Message(1);".to_owned();
        DiagnosticsBaselineEntry {
            fingerprint: diagnostic_fingerprint(path, code, &snippet, 0),
            path: path.to_owned(),
            code: code.to_owned(),
            snippet,
            occurrence: 0,
            message: "message".to_owned(),
            severity: "Warning".to_owned(),
            range: DiagnosticsBaselineRange {
                start_line: 1,
                start_column: 0,
                end_line: 1,
                end_column: 1,
            },
        }
    }

    fn candidate(
        partition_id: &str,
        path: &str,
        code: &str,
    ) -> PartitionedBaselineDiagnosticCandidate<()> {
        PartitionedBaselineDiagnosticCandidate {
            partition_id: partition_id.to_owned(),
            candidate: BaselineDiagnosticCandidate {
                diagnostic: (),
                path: path.to_owned(),
                code: code.to_owned(),
                snippet: Some("Message(1);".to_owned()),
                message: "message".to_owned(),
                severity: "Warning".to_owned(),
                range: DiagnosticsBaselineRange {
                    start_line: 1,
                    start_column: 0,
                    end_line: 1,
                    end_column: 1,
                },
            },
        }
    }

    fn plan() -> DiagnosticsBaselinePartitionPlan {
        let main = DiagnosticsBaselinePartitionIdentity::Main { path: "src/cf".to_owned() };
        let extension = DiagnosticsBaselinePartitionIdentity::Extension {
            name: "Ext".to_owned(),
            path: "src/cfe/Ext".to_owned(),
            depends_on: vec![],
        };
        DiagnosticsBaselinePartitionPlan {
            project_scope: project_model::DiagnosticsBaselineProjectScope {
                source_root: "src/cf".to_owned(),
                extensions: vec![project_model::DiagnosticsBaselineProjectExtension {
                    name: "Ext".to_owned(),
                    path: "src/cfe/Ext".to_owned(),
                    depends_on: vec![],
                }],
            },
            project_scope_fingerprint: "a".repeat(64),
            partitions: vec![
                DiagnosticsBaselinePartition {
                    id: "main".to_owned(),
                    key: blake3::hash(b"main").to_hex().to_string(),
                    identity: main,
                },
                DiagnosticsBaselinePartition {
                    id: "extension:Ext".to_owned(),
                    key: blake3::hash(b"extension:Ext").to_hex().to_string(),
                    identity: extension,
                },
            ],
            roots: vec![
                DiagnosticsBaselineRootOwner {
                    root: "src/cfe/Ext".to_owned(),
                    partition_id: "extension:Ext".to_owned(),
                },
                DiagnosticsBaselineRootOwner {
                    root: "src/cf".to_owned(),
                    partition_id: "main".to_owned(),
                },
            ],
        }
    }

    fn write_set(
        directory: &ManagedBaselineDirectory,
        plan: &DiagnosticsBaselinePartitionPlan,
    ) -> DiagnosticsBaselineManifest {
        let mut files = Vec::new();
        for partition in &plan.partitions {
            let path = if partition.id == "main" {
                "src/cf/CommonModules/A/Ext/Module.bsl"
            } else {
                "src/cfe/Ext/CommonModules/B/Ext/Module.bsl"
            };
            let bytes = diagnostics_partition_json(
                partition.identity.clone(),
                vec![entry(
                    path,
                    if partition.id == "main" { "LineLength" } else { "IfElseDuplicatedCodeBlock" },
                )],
            )
            .unwrap();
            let hash = blake3::hash(&bytes).to_hex().to_string();
            let file = partition_object_path(&partition.id, &partition.key, &hash).unwrap();
            directory.create_file_new(&file).unwrap().write_all(&bytes).unwrap();
            files.push(DiagnosticsBaselineManifestEntry {
                partition_id: partition.id.clone(),
                file,
                blake3: hash,
            });
        }
        let manifest = diagnostics_manifest(plan.project_scope_fingerprint.clone(), files);
        directory
            .create_file_new("manifest.json")
            .unwrap()
            .write_all(&diagnostics_manifest_json(&manifest).unwrap())
            .unwrap();
        manifest
    }

    #[test]
    fn partitioned_baseline_schema_v2_generation_is_byte_deterministic() {
        let plan = plan();
        let mut files = Vec::new();
        for partition in &plan.partitions {
            let path = if partition.id == "main" {
                "src/cf/CommonModules/A/Ext/Module.bsl"
            } else {
                "src/cfe/Ext/CommonModules/B/Ext/Module.bsl"
            };
            let bytes = diagnostics_partition_json(
                partition.identity.clone(),
                vec![entry(path, "LineLength")],
            )
            .unwrap();
            let hash = blake3::hash(&bytes).to_hex().to_string();
            files.push(DiagnosticsBaselineManifestEntry {
                partition_id: partition.id.clone(),
                file: partition_object_path(&partition.id, &partition.key, &hash).unwrap(),
                blake3: hash,
            });
        }
        let first = diagnostics_manifest(plan.project_scope_fingerprint.clone(), files.clone());
        let second = diagnostics_manifest(plan.project_scope_fingerprint, files);
        assert_eq!(
            diagnostics_manifest_json(&first).unwrap(),
            diagnostics_manifest_json(&second).unwrap()
        );
        assert_eq!(first.generation, second.generation);
    }

    #[test]
    fn partitioned_baseline_schema_v2_rejects_bad_schema_identity_and_paths() {
        let identity = DiagnosticsBaselinePartitionIdentity::Main { path: "src/cf".to_owned() };
        let bytes = diagnostics_partition_json(identity, vec![entry("src/cf/a.bsl", "LineLength")])
            .unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["schema_version"] = 3.into();
        let parsed: DiagnosticsBaselinePartitionFile = serde_json::from_value(value).unwrap();
        assert_ne!(parsed.schema_version, DIAGNOSTICS_BASELINE_PARTITION_SCHEMA_VERSION);
        assert!(validate_managed_file("../escape.json").is_err());
        assert!(validate_managed_file("objects\\escape.json").is_err());
    }

    #[test]
    fn partitioned_baseline_fingerprint_contract_matches_v1() {
        let item = entry("src/cf/a.bsl", "LineLength");
        let bytes = diagnostics_partition_json(
            DiagnosticsBaselinePartitionIdentity::Main { path: "src/cf".to_owned() },
            vec![item.clone()],
        )
        .unwrap();
        let parsed: DiagnosticsBaselinePartitionFile = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.diagnostics[0].fingerprint, item.fingerprint);
    }

    #[test]
    fn partitioned_baseline_set_loader_streams_and_validates_all_partitions() {
        let root = tempdir().unwrap();
        let directory = ManagedBaselineDirectory::open(root.path(), "baselines", true).unwrap();
        let plan = plan();
        let manifest = write_set(&directory, &plan);
        let snapshot = load_diagnostics_baseline_set(&directory, &plan).unwrap();
        assert_eq!(snapshot.manifest, manifest);
        assert_eq!(
            snapshot.manifest_hash,
            *blake3::hash(&std::fs::read(root.path().join("baselines/manifest.json")).unwrap())
                .as_bytes()
        );
        assert_eq!(snapshot.partitions.len(), 2);
        assert_eq!(snapshot.partitions["main"].entries_len(), 1);
        assert_eq!(snapshot.partitions["extension:Ext"].entries_len(), 1);
    }

    #[test]
    fn partitioned_baseline_reports_topology_before_identity_and_scope() {
        let root = tempdir().unwrap();
        let directory = ManagedBaselineDirectory::open(root.path(), "baselines", true).unwrap();
        let plan = plan();
        let mut manifest = write_set(&directory, &plan);
        let original_manifest = manifest.clone();
        manifest.partitions[1].partition_id = "extension:Orphan".to_owned();
        manifest =
            diagnostics_manifest(manifest.project_scope_fingerprint.clone(), manifest.partitions);
        std::fs::write(
            root.path().join("baselines/manifest.json"),
            diagnostics_manifest_json(&manifest).unwrap(),
        )
        .unwrap();
        let error = load_diagnostics_baseline_set(&directory, &plan).unwrap_err();
        let info = error.info();
        assert_eq!(info.code, "missing_partition");
        assert_eq!(info.partition_id, Some("extension:Ext"));

        let mut identity_plan = plan.clone();
        identity_plan.partitions[0].identity =
            DiagnosticsBaselinePartitionIdentity::Main { path: "src/other".to_owned() };
        identity_plan.project_scope_fingerprint = "b".repeat(64);
        std::fs::write(
            root.path().join("baselines/manifest.json"),
            diagnostics_manifest_json(&original_manifest).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            load_diagnostics_baseline_set(&directory, &identity_plan),
            Err(PartitionedDiagnosticsBaselineError::PartitionIdentityMismatch(id)) if id == "main"
        ));
    }

    #[test]
    fn partitioned_baseline_set_loader_reuses_unchanged_objects() {
        let root = tempdir().unwrap();
        let directory = ManagedBaselineDirectory::open(root.path(), "baselines", true).unwrap();
        let plan = plan();
        let first_manifest = write_set(&directory, &plan);
        let first = load_diagnostics_baseline_set(&directory, &plan).unwrap();
        let old_extension = first.partitions["extension:Ext"].clone();

        let main = &plan.partitions[0];
        let bytes = diagnostics_partition_json(
            main.identity.clone(),
            vec![entry("src/cf/Changed.bsl", "LineLength")],
        )
        .unwrap();
        let hash = blake3::hash(&bytes).to_hex().to_string();
        let file = partition_object_path(&main.id, &main.key, &hash).unwrap();
        directory.create_file_new(&file).unwrap().write_all(&bytes).unwrap();
        let mut entries = first_manifest.partitions;
        let current = entries.iter_mut().find(|entry| entry.partition_id == "main").unwrap();
        current.file = file;
        current.blake3 = hash;
        let manifest = diagnostics_manifest(plan.project_scope_fingerprint.clone(), entries);
        directory
            .create_file_new("manifest.next.json")
            .unwrap()
            .write_all(&diagnostics_manifest_json(&manifest).unwrap())
            .unwrap();
        directory.replace_file("manifest.next.json", "manifest.json").unwrap();

        let (second, stats) = load_diagnostics_baseline_set_reusing(
            &directory,
            &plan,
            Some(&first),
            &BTreeSet::new(),
        )
        .unwrap();
        assert_eq!(stats.partitions_parsed, 1);
        assert_eq!(stats.fingerprints_validated, 1);
        assert!(Arc::ptr_eq(&old_extension, &second.partitions["extension:Ext"]));
    }

    #[test]
    fn partitioned_baseline_set_loader_fails_closed_on_corruption() {
        let root = tempdir().unwrap();
        let directory = ManagedBaselineDirectory::open(root.path(), "baselines", true).unwrap();
        let plan = plan();
        let manifest = write_set(&directory, &plan);
        let main = manifest.partitions.iter().find(|entry| entry.partition_id == "main").unwrap();
        let path = root.path().join("baselines").join(&main.file);
        std::fs::write(path, b"{}\n").unwrap();
        assert!(matches!(
            load_diagnostics_baseline_set(&directory, &plan),
            Err(PartitionedDiagnosticsBaselineError::Json(_))
                | Err(PartitionedDiagnosticsBaselineError::ObjectHashMismatch(_))
        ));
    }

    #[test]
    fn partitioned_baseline_classification_and_summary_routes_once() {
        let root = tempdir().unwrap();
        let directory = ManagedBaselineDirectory::open(root.path(), "baselines", true).unwrap();
        let plan = plan();
        write_set(&directory, &plan);
        let snapshot = load_diagnostics_baseline_set(&directory, &plan).unwrap();
        let main_path = "src/cf/CommonModules/A/Ext/Module.bsl";
        let extension_path = "src/cfe/Ext/CommonModules/B/Ext/Module.bsl";
        let current = vec![
            candidate("main", main_path, "LineLength"),
            candidate("main", main_path, "UsingGoto"),
            candidate("main", main_path, "UnknownSuppressionCode"),
        ];
        let coverage = BTreeMap::from([
            ("main".to_owned(), DiagnosticsBaselineCoverage::Full),
            (
                "extension:Ext".to_owned(),
                DiagnosticsBaselineCoverage::Partial {
                    completed_files: BTreeSet::from([extension_path.to_owned()]),
                },
            ),
        ]);
        let classified =
            classify_partitioned_diagnostics(&snapshot, "baselines".to_owned(), current, &coverage)
                .unwrap();
        assert_eq!(
            (classified.new.len(), classified.known.len(), classified.resolved.len()),
            (2, 1, 1)
        );
        assert_eq!(classified.summary.state, DiagnosticsBaselineState::Partial);
        assert!(!classified.summary.complete);
        assert_eq!(classified.summary.partitions.len(), 2);
        assert_eq!(classified.summary.partitions[0].id, "main");
        assert_eq!(classified.summary.partitions[0].known, 1);
        assert_eq!(classified.summary.partitions[0].new, 2);
        assert_eq!(classified.summary.partitions[1].id, "extension:Ext");
        assert_eq!(classified.summary.partitions[1].resolved, 1);
        assert!(!classified.summary.partitions[1].complete);
        assert_eq!(classified.resolved.into_iter().count(), 1);
    }

    #[test]
    fn partitioned_baseline_coverage_promotes_only_complete_owner_file_sets() {
        let plan = plan();
        let main = "src/cf/CommonModules/A/Ext/Module.bsl".to_owned();
        let extension = "src/cfe/Ext/CommonModules/B/Ext/Module.bsl".to_owned();
        let coverage = partitioned_coverage(
            &plan,
            &DiagnosticsBaselineCoverage::Partial {
                completed_files: BTreeSet::from([main.clone()]),
            },
            Some(&BTreeSet::from([main, extension.clone()])),
        )
        .unwrap();
        assert_eq!(coverage["main"], DiagnosticsBaselineCoverage::Full);
        assert_eq!(
            coverage["extension:Ext"],
            DiagnosticsBaselineCoverage::Partial { completed_files: BTreeSet::new() }
        );

        let full = partitioned_coverage(
            &plan,
            &DiagnosticsBaselineCoverage::Partial {
                completed_files: BTreeSet::from([extension.clone()]),
            },
            Some(&BTreeSet::from([extension])),
        )
        .unwrap();
        assert_eq!(
            full["main"],
            DiagnosticsBaselineCoverage::Partial { completed_files: BTreeSet::new() }
        );
        assert_eq!(full["extension:Ext"], DiagnosticsBaselineCoverage::Full);
    }

    #[test]
    fn partitioned_baseline_v1_migration_preserves_entries_without_current_diagnostics() {
        let plan = plan();
        let entries = vec![
            entry("src/cf/CommonModules/A/Ext/Module.bsl", "LineLength"),
            entry("src/cfe/Ext/CommonModules/B/Ext/Module.bsl", "IfElseDuplicatedCodeBlock"),
        ];
        let baseline = DiagnosticsBaseline {
            schema_version: crate::diagnostics_baseline::DIAGNOSTICS_BASELINE_SCHEMA_VERSION,
            scope: crate::diagnostics_baseline::DiagnosticsBaselineScope {
                source_root: plan.project_scope.source_root.clone(),
                extensions: plan
                    .project_scope
                    .extensions
                    .iter()
                    .map(|extension| crate::diagnostics_baseline::DiagnosticsBaselineExtension {
                        name: extension.name.clone(),
                        path: extension.path.clone(),
                        depends_on: extension.depends_on.clone(),
                    })
                    .collect(),
            },
            diagnostics: entries.clone(),
        };
        let before = crate::diagnostics_baseline::diagnostics_baseline_json(&baseline).unwrap();
        let migrated = migrate_v1_to_partitioned(&baseline, &plan).unwrap();
        assert_eq!(migrated["main"], vec![entries[0].clone()]);
        assert_eq!(migrated["extension:Ext"], vec![entries[1].clone()]);
        assert_eq!(
            crate::diagnostics_baseline::diagnostics_baseline_json(&baseline).unwrap(),
            before
        );
    }

    #[test]
    fn partitioned_baseline_v1_migration_rejects_scope_and_unowned_paths() {
        let plan = plan();
        let mut baseline = DiagnosticsBaseline {
            schema_version: crate::diagnostics_baseline::DIAGNOSTICS_BASELINE_SCHEMA_VERSION,
            scope: crate::diagnostics_baseline::DiagnosticsBaselineScope {
                source_root: "wrong".to_owned(),
                extensions: vec![],
            },
            diagnostics: vec![],
        };
        assert!(matches!(
            migrate_v1_to_partitioned(&baseline, &plan),
            Err(PartitionedDiagnosticsBaselineError::ScopeMismatch)
        ));
        baseline.scope = crate::diagnostics_baseline::DiagnosticsBaselineScope {
            source_root: plan.project_scope.source_root.clone(),
            extensions: plan
                .project_scope
                .extensions
                .iter()
                .map(|extension| crate::diagnostics_baseline::DiagnosticsBaselineExtension {
                    name: extension.name.clone(),
                    path: extension.path.clone(),
                    depends_on: extension.depends_on.clone(),
                })
                .collect(),
        };
        baseline.diagnostics = vec![entry("outside/file.bsl", "LineLength")];
        assert!(matches!(
            migrate_v1_to_partitioned(&baseline, &plan),
            Err(PartitionedDiagnosticsBaselineError::UnownedDiagnostic(_))
        ));
    }
}
