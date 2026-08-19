#![cfg(target_os = "linux")]

use std::io::{BufWriter, Write};

use ide::diagnostics_baseline::{
    diagnostic_fingerprint, BaselineDiagnosticCandidate, DiagnosticsBaselineCoverage,
    DiagnosticsBaselineEntry, DiagnosticsBaselineRange,
};
use ide::partitioned_diagnostics_baseline::{
    classify_partitioned_diagnostics, diagnostics_manifest, diagnostics_manifest_json,
    load_diagnostics_baseline_set_reusing, partition_object_path, DiagnosticsBaselineManifestEntry,
    PartitionedBaselineDiagnosticCandidate,
};
use project_model::{
    DiagnosticsBaselinePartition, DiagnosticsBaselinePartitionIdentity,
    DiagnosticsBaselinePartitionPlan, DiagnosticsBaselineProjectScope,
    DiagnosticsBaselineRootOwner, ManagedBaselineDirectory,
};

const RECORDS: usize = 1_600_000;

fn resident_bytes() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:")?.split_whitespace().next()?.parse::<u64>().ok()
        })
        .unwrap()
        * 1024
}

#[test]
#[ignore = "release-only 1.6M-entry memory gate"]
fn loads_and_classifies_1_6m_entries_with_bounded_rss() {
    let temp = tempfile::tempdir().unwrap();
    let baseline_root = temp.path().join("baselines");
    let object_dir = baseline_root.join("objects/main");
    std::fs::create_dir_all(&object_dir).unwrap();
    let temporary = object_dir.join("large.tmp");
    let file = std::fs::File::create(&temporary).unwrap();
    let mut writer = BufWriter::new(file);
    let mut hasher = blake3::Hasher::new();
    {
        let mut write = |bytes: &[u8]| {
            writer.write_all(bytes).unwrap();
            hasher.update(bytes);
        };
        write(br#"{"schema_version":2,"partition":{"kind":"main","path":""},"diagnostics":["#);
        for index in 0..RECORDS {
            if index > 0 {
                write(b",");
            }
            let path = format!("modules/{index}.bsl");
            let snippet = format!("Message({index});");
            let entry = DiagnosticsBaselineEntry {
                fingerprint: diagnostic_fingerprint(&path, "LineLength", &snippet, 0),
                path,
                code: "LineLength".to_owned(),
                snippet,
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
            write(&serde_json::to_vec(&entry).unwrap());
        }
        write(b"]}");
    }
    writer.flush().unwrap();
    writer.get_ref().sync_all().unwrap();
    let hash = hasher.finalize().to_hex().to_string();

    let identity = DiagnosticsBaselinePartitionIdentity::Main { path: String::new() };
    let key = blake3::hash(b"main").to_hex().to_string();
    let object = partition_object_path("main", &key, &hash).unwrap();
    std::fs::rename(&temporary, baseline_root.join(&object)).unwrap();
    let plan = DiagnosticsBaselinePartitionPlan {
        project_scope: DiagnosticsBaselineProjectScope {
            source_root: String::new(),
            extensions: vec![],
        },
        project_scope_fingerprint: "a".repeat(64),
        partitions: vec![DiagnosticsBaselinePartition { id: "main".to_owned(), key, identity }],
        roots: vec![DiagnosticsBaselineRootOwner {
            root: String::new(),
            partition_id: "main".to_owned(),
        }],
    };
    let manifest = diagnostics_manifest(
        plan.project_scope_fingerprint.clone(),
        vec![DiagnosticsBaselineManifestEntry {
            partition_id: "main".to_owned(),
            file: object.clone(),
            blake3: hash,
        }],
    );
    std::fs::write(
        baseline_root.join("manifest.json"),
        diagnostics_manifest_json(&manifest).unwrap(),
    )
    .unwrap();
    let input_bytes = std::fs::metadata(baseline_root.join(object)).unwrap().len();
    let before = resident_bytes();
    let directory = ManagedBaselineDirectory::open(temp.path(), "baselines", false).unwrap();
    let (snapshot, stats) = load_diagnostics_baseline_set_reusing(
        &directory,
        &plan,
        None,
        &std::collections::BTreeSet::new(),
    )
    .unwrap();
    let classified = classify_partitioned_diagnostics(
        &snapshot,
        "baselines".to_owned(),
        vec![PartitionedBaselineDiagnosticCandidate {
            partition_id: "main".to_owned(),
            candidate: BaselineDiagnosticCandidate {
                diagnostic: (),
                path: "modules/0.bsl".to_owned(),
                code: "LineLength".to_owned(),
                snippet: Some("Message(0);".to_owned()),
                message: "m".to_owned(),
                severity: "Warning".to_owned(),
                range: DiagnosticsBaselineRange {
                    start_line: 0,
                    start_column: 0,
                    end_line: 0,
                    end_column: 1,
                },
            },
        }],
        &std::collections::BTreeMap::from([("main".to_owned(), DiagnosticsBaselineCoverage::Full)]),
    )
    .unwrap();
    let added = resident_bytes().saturating_sub(before);
    assert_eq!(snapshot.partitions["main"].entries_len(), RECORDS);
    assert_eq!(classified.known.len(), 1);
    assert_eq!(classified.resolved.len(), RECORDS - 1);
    assert_eq!(classified.summary.resolved, Some(RECORDS - 1));
    assert_eq!(stats.partitions_parsed, 1);
    assert_eq!(stats.fingerprints_validated, RECORDS);
    assert!(
        added <= input_bytes * 3 / 2,
        "resident growth {added} exceeds 1.5x input {input_bytes}"
    );
    assert_eq!(classified.resolved.into_iter().count(), RECORDS - 1);
    let old = snapshot.partitions["main"].clone();
    let (reloaded, reload_stats) = load_diagnostics_baseline_set_reusing(
        &directory,
        &plan,
        Some(&snapshot),
        &std::collections::BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(reload_stats.partitions_parsed, 0);
    assert_eq!(reload_stats.fingerprints_validated, 0);
    assert!(std::sync::Arc::ptr_eq(&old, &reloaded.partitions["main"]));
}
