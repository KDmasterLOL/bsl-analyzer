use crate::mcp_install::model::{
    normalize_source_dir_for_scope, InstallPreset, InstallRequest, ServerSpec,
};

pub(super) fn build_server_spec(request: &InstallRequest, binary: &str) -> ServerSpec {
    match request.preset {
        InstallPreset::Workspace => build_workspace_spec(request, binary),
        InstallPreset::Reference => build_reference_spec(request, binary),
    }
}

/// The command an AI client should run to launch this MCP server. Prefers the absolute
/// path of the currently-running executable (`current_exe`), so the generated config
/// works even when the client starts the server in a reduced `PATH` that lacks the
/// install directory. Falls back to the bare name if the path cannot be resolved.
pub(super) fn resolve_self_binary() -> String {
    std::env::current_exe()
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "bsl-analyzer".to_owned())
}

fn build_workspace_spec(request: &InstallRequest, binary: &str) -> ServerSpec {
    let mut args = vec![
        "mcp".to_owned(),
        "serve".to_owned(),
        "--profile".to_owned(),
        "workspace".to_owned(),
        "--source-dir".to_owned(),
        normalize_source_dir_for_scope(request.scope, &request.project_dir, &request.source_dir),
    ];

    if let Some(url) = &request.onec_url {
        args.push("--onec-url".to_owned());
        args.push(url.clone());
    }

    if !request.onec_user.is_empty() {
        args.push("--onec-user".to_owned());
        args.push(request.onec_user.clone());
    }

    if !request.onec_password.is_empty() {
        args.push("--onec-password".to_owned());
        args.push(request.onec_password.clone());
    }

    ServerSpec {
        name: request.name.clone(),
        command: binary.to_owned(),
        args,
        env: request.env.clone(),
    }
}

fn build_reference_spec(request: &InstallRequest, binary: &str) -> ServerSpec {
    ServerSpec {
        name: request.name.clone(),
        command: binary.to_owned(),
        args: vec![
            "mcp".to_owned(),
            "serve".to_owned(),
            "--profile".to_owned(),
            "reference".to_owned(),
        ],
        env: request.env.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::PathBuf};

    use crate::mcp_install::model::{
        InstallPreset, InstallRequest, InstallScope, InstallTarget, InstallTargetSelector,
    };

    use super::build_server_spec;

    fn request() -> InstallRequest {
        InstallRequest {
            target: InstallTargetSelector::One(InstallTarget::Cursor),
            scope: InstallScope::Project,
            preset: InstallPreset::Workspace,
            name: "bsl-analyzer".to_owned(),
            project_dir: PathBuf::from("/tmp/workspace"),
            source_dir: PathBuf::from("/tmp/workspace/src"),
            onec_url: Some("http://localhost/base/hs/bsl-analyzer".to_owned()),
            onec_user: "admin".to_owned(),
            onec_password: "secret".to_owned(),
            env: BTreeMap::from([("NAPARNIK_TOKEN".to_owned(), "test".to_owned())]),
            force: false,
            dry_run: false,
        }
    }

    #[test]
    fn workspace_preset_builds_expected_stdio_command() {
        let spec = build_server_spec(&request(), "bsl-analyzer");

        assert_eq!(spec.name, "bsl-analyzer");
        assert_eq!(spec.command, "bsl-analyzer");
        assert_eq!(
            spec.args,
            vec![
                "mcp",
                "serve",
                "--profile",
                "workspace",
                "--source-dir",
                "src",
                "--onec-url",
                "http://localhost/base/hs/bsl-analyzer",
                "--onec-user",
                "admin",
                "--onec-password",
                "secret",
            ]
        );
        assert_eq!(spec.env.get("NAPARNIK_TOKEN"), Some(&"test".to_owned()));
    }

    #[test]
    fn reference_preset_builds_global_reference_command() {
        let mut req = request();
        req.preset = InstallPreset::Reference;

        let spec = build_server_spec(&req, "bsl-analyzer");

        assert_eq!(spec.name, "bsl-analyzer");
        assert_eq!(spec.command, "bsl-analyzer");
        assert_eq!(spec.args, vec!["mcp", "serve", "--profile", "reference"]);
        assert_eq!(spec.env.get("NAPARNIK_TOKEN"), Some(&"test".to_owned()));
    }

    /// The resolved binary path is written verbatim as the launch command, so a config
    /// generated with an absolute path keeps it (immune to the client's reduced PATH).
    #[test]
    fn server_spec_uses_provided_binary_path() {
        let spec = build_server_spec(&request(), "/usr/local/bin/bsl-analyzer");
        assert_eq!(spec.command, "/usr/local/bin/bsl-analyzer");

        let mut req = request();
        req.preset = InstallPreset::Reference;
        let spec = build_server_spec(&req, "/opt/bsl/bsl-analyzer");
        assert_eq!(spec.command, "/opt/bsl/bsl-analyzer");
    }

    /// `resolve_self_binary` yields an absolute path (the running executable), which is
    /// what makes the generated client config robust to a reduced PATH.
    #[test]
    fn resolve_self_binary_is_absolute() {
        let bin = super::resolve_self_binary();
        assert!(!bin.is_empty());
        assert!(std::path::Path::new(&bin).is_absolute(), "expected an absolute path, got {bin}");
    }
}
