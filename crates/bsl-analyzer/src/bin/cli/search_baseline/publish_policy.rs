use std::{env, io};

pub(super) fn pick_first_non_empty<'a>(
    candidates: impl IntoIterator<Item = Option<&'a str>>,
) -> Option<String> {
    candidates
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(super) fn resolve_publish_branch(
    project_branch: Option<&str>,
    project_root: &std::path::Path,
) -> Option<String> {
    let git_branch = project_model::current_git_branch(project_root);
    pick_first_non_empty([
        project_branch,
        env::var("CI_COMMIT_BRANCH").ok().as_deref(),
        env::var("CI_COMMIT_REF_NAME").ok().as_deref(),
        git_branch.as_deref(),
    ])
}

pub(super) fn resolve_publish_commit(
    explicit_commit: Option<&str>,
    project_root: &std::path::Path,
) -> Option<String> {
    let git_commit = project_model::current_git_commit(project_root);
    pick_first_non_empty([
        explicit_commit,
        env::var("CI_COMMIT_SHA").ok().as_deref(),
        env::var("GITHUB_SHA").ok().as_deref(),
        git_commit.as_deref(),
    ])
}

pub(super) fn validate_workspace_publish_policy(
    policy: &project_model::SearchBaselinePolicyConfig,
    branch: Option<&str>,
    allow_non_policy_branch: bool,
) -> Result<(), io::Error> {
    if !policy.is_configured() {
        return Ok(());
    }

    if policy.publish_branches.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "workspace baseline policy is configured but 'workspaceCode.policy.publishBranches' is empty",
        ));
    }

    let Some(branch) = branch.map(str::trim).filter(|branch| !branch.is_empty()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "workspace baseline publish policy requires a branch; pass --branch, set CI_COMMIT_BRANCH, or run inside a git repository",
        ));
    };

    if project_model::is_publish_branch_allowed(policy, branch) || allow_non_policy_branch {
        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "branch '{branch}' is not allowed by workspace baseline publish policy; use --allow-non-policy-branch to override"
        ),
    ))
}

#[cfg(test)]
mod tests {
    use std::{env, fs};

    use tempfile::tempdir;

    use super::{
        pick_first_non_empty, resolve_publish_branch, resolve_publish_commit,
        validate_workspace_publish_policy,
    };

    #[test]
    fn pick_first_non_empty_skips_blank_values() {
        let value = pick_first_non_empty([Some(""), Some("  "), None, Some("main")]);

        assert_eq!(value.as_deref(), Some("main"));
    }

    #[test]
    fn resolve_publish_branch_uses_git_when_cli_and_ci_are_missing() {
        let saved_branch = env::var("CI_COMMIT_BRANCH").ok();
        let saved_ref = env::var("CI_COMMIT_REF_NAME").ok();
        env::remove_var("CI_COMMIT_BRANCH");
        env::remove_var("CI_COMMIT_REF_NAME");

        let dir = tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        fs::create_dir_all(&git_dir).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/feature/demo\n").unwrap();

        let branch = resolve_publish_branch(None, dir.path());

        if let Some(v) = saved_branch {
            env::set_var("CI_COMMIT_BRANCH", v);
        }
        if let Some(v) = saved_ref {
            env::set_var("CI_COMMIT_REF_NAME", v);
        }

        assert_eq!(branch.as_deref(), Some("feature/demo"));
    }

    #[test]
    fn resolve_publish_commit_uses_git_when_cli_and_ci_are_missing() {
        let saved_ci = env::var("CI_COMMIT_SHA").ok();
        let saved_gha = env::var("GITHUB_SHA").ok();
        env::remove_var("CI_COMMIT_SHA");
        env::remove_var("GITHUB_SHA");

        let dir = tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        let ref_file = git_dir.join("refs/heads/feature/demo");
        fs::create_dir_all(ref_file.parent().unwrap()).unwrap();
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/feature/demo\n").unwrap();
        fs::write(&ref_file, "0123456789abcdef\n").unwrap();

        let commit = resolve_publish_commit(None, dir.path());

        if let Some(v) = saved_ci {
            env::set_var("CI_COMMIT_SHA", v);
        }
        if let Some(v) = saved_gha {
            env::set_var("GITHUB_SHA", v);
        }

        assert_eq!(commit.as_deref(), Some("0123456789abcdef"));
    }

    #[test]
    fn workspace_publish_policy_blocks_branch_outside_allowlist() {
        let policy: project_model::SearchBaselinePolicyConfig =
            serde_json::from_value(serde_json::json!({
                "publishBranches": ["vendor", "develop"],
                "branches": [
                    { "match": "*", "selectBranch": "develop", "fallbackBranch": "vendor" }
                ]
            }))
            .unwrap();

        let error =
            validate_workspace_publish_policy(&policy, Some("feature/demo"), false).unwrap_err();

        assert!(error.to_string().contains("not allowed by workspace baseline publish policy"));
    }

    #[test]
    fn workspace_publish_policy_allows_override_flag() {
        let policy: project_model::SearchBaselinePolicyConfig =
            serde_json::from_value(serde_json::json!({
                "publishBranches": ["vendor", "develop"],
                "branches": [
                    { "match": "*", "selectBranch": "develop", "fallbackBranch": "vendor" }
                ]
            }))
            .unwrap();

        validate_workspace_publish_policy(&policy, Some("feature/demo"), true).unwrap();
    }
}
