use std::{error::Error, io};

pub(super) fn resolve(
    corpus: &bsl_search::CorpusId,
    snapshot_id: Option<&str>,
    branch: Option<&str>,
    commit: Option<&str>,
) -> Result<String, io::Error> {
    if let Some(snapshot_id) = snapshot_id.filter(|value| !value.trim().is_empty()) {
        return Ok(snapshot_id.to_owned());
    }

    let prefix = corpus.as_str();
    match (
        branch.filter(|value| !value.trim().is_empty()),
        commit.filter(|value| !value.trim().is_empty()),
    ) {
        (Some(branch), Some(commit)) => Ok(format!("{prefix}:{branch}@{commit}")),
        (Some(branch), None) => Ok(format!("{prefix}:{branch}")),
        (None, Some(commit)) => Ok(format!("{prefix}:{commit}")),
        _ if matches!(corpus, bsl_search::CorpusId::Reference) => {
            Ok(format!("reference:{}", env!("CARGO_PKG_VERSION")))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--snapshot-id is required unless --branch or --commit is provided",
        )),
    }
}

pub(super) fn resolve_parent(
    adapter: &bsl_search::ExternalBaselineAdapter,
    corpus: &bsl_search::CorpusId,
    branch: Option<&str>,
    current_snapshot_id: &str,
    explicit_parent_snapshot_id: Option<&str>,
    workspace_policy: Option<&project_model::SearchBaselinePolicyConfig>,
) -> Result<Option<String>, Box<dyn Error + Send + Sync>> {
    if let Some(parent_snapshot_id) =
        explicit_parent_snapshot_id.map(str::trim).filter(|value| !value.is_empty())
    {
        return Ok(Some(parent_snapshot_id.to_owned()));
    }

    let mut snapshot_groups = Vec::new();
    if let Some(branch) = branch.map(str::trim).filter(|value| !value.is_empty()) {
        let mut branch_candidates = vec![branch.to_owned()];
        if matches!(corpus, bsl_search::CorpusId::WorkspaceCode)
            && workspace_policy
                .is_some_and(project_model::SearchBaselinePolicyConfig::is_configured)
        {
            if let Some(policy) = workspace_policy {
                if let Some(selection) =
                    project_model::resolve_workspace_branch_policy(policy, Some(branch))
                {
                    for candidate in selection.candidate_branches() {
                        if branch_candidates.iter().all(|existing| existing != &candidate) {
                            branch_candidates.push(candidate);
                        }
                    }
                }
            }
        }

        for branch_candidate in branch_candidates {
            snapshot_groups.push(adapter.list_snapshots(
                Some(corpus.as_str()),
                Some(branch_candidate.as_str()),
                None,
                2,
            )?);
        }

        return Ok(select_parent_from_groups(current_snapshot_id, &snapshot_groups));
    }

    let snapshots = adapter.list_snapshots(Some(corpus.as_str()), branch, None, 2)?;
    Ok(select_parent(current_snapshot_id, &snapshots))
}

pub(super) fn select_parent_from_groups(
    current_snapshot_id: &str,
    snapshot_groups: &[Vec<bsl_search::BaselineSnapshotRecord>],
) -> Option<String> {
    snapshot_groups.iter().find_map(|snapshots| select_parent(current_snapshot_id, snapshots))
}

pub(super) fn select_parent(
    current_snapshot_id: &str,
    snapshots: &[bsl_search::BaselineSnapshotRecord],
) -> Option<String> {
    snapshots
        .iter()
        .find(|snapshot| snapshot.snapshot_id != current_snapshot_id)
        .map(|snapshot| snapshot.snapshot_id.clone())
}

#[cfg(test)]
mod tests {
    use bsl_search::{BaselineSnapshotRecord, CorpusId};

    use super::{resolve, select_parent, select_parent_from_groups};

    #[test]
    fn explicit_snapshot_id_has_priority() {
        let snapshot_id = resolve(
            &CorpusId::WorkspaceCode,
            Some("manual-snapshot"),
            Some("main"),
            Some("abc123"),
        )
        .unwrap();

        assert_eq!(snapshot_id, "manual-snapshot");
    }

    #[test]
    fn snapshot_id_is_derived_from_branch_and_commit() {
        let snapshot_id =
            resolve(&CorpusId::WorkspaceCode, None, Some("main"), Some("abc123")).unwrap();

        assert_eq!(snapshot_id, "workspace-code:main@abc123");
    }

    #[test]
    fn snapshot_id_is_derived_from_commit_when_branch_is_missing() {
        let snapshot_id = resolve(&CorpusId::WorkspaceCode, None, None, Some("abc123")).unwrap();

        assert_eq!(snapshot_id, "workspace-code:abc123");
    }

    #[test]
    fn snapshot_id_from_branch_alone() {
        let snapshot_id = resolve(&CorpusId::WorkspaceCode, None, Some("main"), None).unwrap();

        assert_eq!(snapshot_id, "workspace-code:main");
    }

    #[test]
    fn snapshot_id_requires_branch_or_commit() {
        let error = resolve(&CorpusId::WorkspaceCode, None, None, None).unwrap_err();

        assert!(error
            .to_string()
            .contains("--snapshot-id is required unless --branch or --commit is provided"));
    }

    #[test]
    fn reference_snapshot_id_defaults_to_package_version() {
        let snapshot_id = resolve(&CorpusId::Reference, None, None, None).unwrap();

        assert_eq!(snapshot_id, format!("reference:{}", env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn select_parent_picks_latest_different_snapshot() {
        let snapshots = vec![
            baseline_snapshot_record("workspace-code:main@new"),
            baseline_snapshot_record("workspace-code:main@old"),
        ];

        let parent = select_parent("workspace-code:main@new", &snapshots);

        assert_eq!(parent.as_deref(), Some("workspace-code:main@old"));
    }

    #[test]
    fn select_parent_uses_latest_when_current_is_not_published_yet() {
        let snapshots = vec![baseline_snapshot_record("workspace-code:main@old")];

        let parent = select_parent("workspace-code:main@new", &snapshots);

        assert_eq!(parent.as_deref(), Some("workspace-code:main@old"));
    }

    #[test]
    fn select_parent_returns_none_when_only_self_exists() {
        let snapshots = vec![baseline_snapshot_record("workspace-code:main@same")];

        let parent = select_parent("workspace-code:main@same", &snapshots);

        assert_eq!(parent, None);
    }

    #[test]
    fn select_parent_uses_fallback_branch_group() {
        let feature_group = vec![baseline_snapshot_record("workspace-code:feature@same")];
        let develop_group = vec![baseline_snapshot_record("workspace-code:develop@old")];
        let vendor_group = vec![baseline_snapshot_record("workspace-code:vendor@old")];

        let parent = select_parent_from_groups(
            "workspace-code:feature@same",
            &[feature_group, develop_group, vendor_group],
        );

        assert_eq!(parent.as_deref(), Some("workspace-code:develop@old"));
    }

    fn baseline_snapshot_record(snapshot_id: &str) -> BaselineSnapshotRecord {
        BaselineSnapshotRecord {
            snapshot_id: snapshot_id.to_owned(),
            corpus: "workspace-code".to_owned(),
            fingerprint: None,
            parent_snapshot_id: None,
            branch: Some("main".to_owned()),
            commit: None,
            created_at: "2026-04-02T00:00:00Z".to_owned(),
            files: 0,
            documents: 0,
        }
    }
}
