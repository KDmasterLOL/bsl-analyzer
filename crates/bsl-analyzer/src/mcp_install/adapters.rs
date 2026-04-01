use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::{Map, Value};
use toml_edit::{value, Array, DocumentMut, Item, Table};

use crate::mcp_install::{
    error::InstallError,
    model::{
        resolve_apply_decision, InstallAction, InstallEntryResult, InstallPlan, InstallResult,
        InstallScope, InstallStatus, InstallTarget, ServerSpec,
    },
    ports::{CommandOutput, CommandRunner, FileStore},
};

pub struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        cwd: &Path,
    ) -> Result<CommandOutput, InstallError> {
        let output = Command::new(program).args(args).current_dir(cwd).output().map_err(|err| {
            if err.kind() == io::ErrorKind::NotFound {
                InstallError::TargetBinaryNotFound { program: program.to_owned() }
            } else {
                InstallError::ExternalCommandFailed {
                    program: program.to_owned(),
                    status: -1,
                    message: err.to_string(),
                }
            }
        })?;

        Ok(CommandOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

pub struct RealFileStore;

impl FileStore for RealFileStore {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    fn write_string(&self, path: &Path, contents: &str) -> io::Result<()> {
        fs::write(path, contents)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

pub fn apply_install_plan(
    plan: &InstallPlan,
    runner: &dyn CommandRunner,
    files: &dyn FileStore,
) -> Result<InstallResult, InstallError> {
    let mut entries = Vec::with_capacity(plan.actions.len());

    for action in &plan.actions {
        entries.push(apply_action(action, runner, files)?);
    }

    Ok(InstallResult { entries })
}

fn apply_action(
    action: &InstallAction,
    runner: &dyn CommandRunner,
    files: &dyn FileStore,
) -> Result<InstallEntryResult, InstallError> {
    match (action.target, action.scope) {
        (InstallTarget::Codex, InstallScope::User) => install_codex_user(action, runner),
        (InstallTarget::Codex, InstallScope::Project) => install_codex_project(action, files),
        (InstallTarget::Gemini, InstallScope::User | InstallScope::Project) => {
            install_gemini(action, runner, files)
        }
        (
            InstallTarget::Claude,
            InstallScope::User | InstallScope::Project | InstallScope::Local,
        ) => install_claude(action, runner),
        (InstallTarget::Cursor, InstallScope::User | InstallScope::Project) => {
            install_cursor(action, files)
        }
        _ => Err(InstallError::UnsupportedScope { target: action.target, scope: action.scope }),
    }
}

fn install_codex_user(
    action: &InstallAction,
    runner: &dyn CommandRunner,
) -> Result<InstallEntryResult, InstallError> {
    let location = home_dir()?.join(".codex/config.toml");
    let exists = codex_server_exists(runner, &action.spec.name, &action.project_dir)?;
    let decision = resolve_apply_decision(
        exists,
        action.force,
        action.target,
        action.scope,
        &action.spec.name,
        &location.display().to_string(),
    )?;
    let cli_args = build_codex_add_args(&action.spec);

    if action.dry_run {
        return Ok(InstallEntryResult {
            target: action.target,
            scope: action.scope,
            status: InstallStatus::DryRun,
            location: location.display().to_string(),
            detail: render_dry_run_command(
                &location.display().to_string(),
                decision.action_label(),
                "codex",
                &cli_args,
                &action.spec,
            ),
        });
    }

    run_checked(runner, "codex", &cli_args, &action.project_dir)?;

    Ok(InstallEntryResult {
        target: action.target,
        scope: action.scope,
        status: decision.status(),
        location: location.display().to_string(),
        detail: if matches!(decision, crate::mcp_install::ApplyDecision::Update) {
            "updated via codex CLI".to_owned()
        } else {
            "installed via codex CLI".to_owned()
        },
    })
}

fn install_codex_project(
    action: &InstallAction,
    files: &dyn FileStore,
) -> Result<InstallEntryResult, InstallError> {
    let path = action.project_dir.join(".codex/config.toml");
    let doc = if files.exists(&path) {
        files
            .read_to_string(&path)
            .map_err(|err| InstallError::ConfigRead {
                path: path.display().to_string(),
                message: err.to_string(),
            })?
            .parse::<DocumentMut>()
            .map_err(|err| InstallError::ConfigParse {
                path: path.display().to_string(),
                format: "TOML",
                message: err.to_string(),
            })?
    } else {
        DocumentMut::new()
    };

    let exists = codex_project_contains_server(&doc, &action.spec.name);
    let decision = resolve_apply_decision(
        exists,
        action.force,
        action.target,
        action.scope,
        &action.spec.name,
        &path.display().to_string(),
    )?;

    let updated_doc = upsert_codex_project_server(doc, &action.spec);

    if action.dry_run {
        return Ok(InstallEntryResult {
            target: action.target,
            scope: action.scope,
            status: InstallStatus::DryRun,
            location: path.display().to_string(),
            detail: render_dry_run_config(
                &path.display().to_string(),
                decision.action_label(),
                updated_doc.to_string(),
                &action.spec,
            ),
        });
    }

    if let Some(parent) = path.parent() {
        files.create_dir_all(parent).map_err(|err| InstallError::ConfigWrite {
            path: parent.display().to_string(),
            message: err.to_string(),
        })?;
    }
    files.write_string(&path, &updated_doc.to_string()).map_err(|err| {
        InstallError::ConfigWrite { path: path.display().to_string(), message: err.to_string() }
    })?;

    Ok(InstallEntryResult {
        target: action.target,
        scope: action.scope,
        status: decision.status(),
        location: path.display().to_string(),
        detail: if matches!(decision, crate::mcp_install::ApplyDecision::Update) {
            "updated project-scoped codex MCP config".to_owned()
        } else {
            "created project-scoped codex MCP config".to_owned()
        },
    })
}

fn install_gemini(
    action: &InstallAction,
    runner: &dyn CommandRunner,
    files: &dyn FileStore,
) -> Result<InstallEntryResult, InstallError> {
    let location = gemini_config_path(action.scope, &action.project_dir)?;
    let exists = json_config_contains_server(files, &location, &action.spec.name, "mcpServers")?;
    let decision = resolve_apply_decision(
        exists,
        action.force,
        action.target,
        action.scope,
        &action.spec.name,
        &location.display().to_string(),
    )?;

    let cli_args = build_gemini_add_args(&action.spec, action.scope);
    if action.dry_run {
        return Ok(InstallEntryResult {
            target: action.target,
            scope: action.scope,
            status: InstallStatus::DryRun,
            location: location.display().to_string(),
            detail: render_dry_run_command(
                &location.display().to_string(),
                decision.action_label(),
                "gemini",
                &cli_args,
                &action.spec,
            ),
        });
    }

    run_checked(runner, "gemini", &cli_args, &action.project_dir)?;

    Ok(InstallEntryResult {
        target: action.target,
        scope: action.scope,
        status: decision.status(),
        location: location.display().to_string(),
        detail: if matches!(decision, crate::mcp_install::ApplyDecision::Update) {
            "updated via gemini CLI".to_owned()
        } else {
            "installed via gemini CLI".to_owned()
        },
    })
}

fn install_claude(
    action: &InstallAction,
    runner: &dyn CommandRunner,
) -> Result<InstallEntryResult, InstallError> {
    let location = claude_location_hint(action.scope, &action.project_dir)?;
    let exists = claude_server_exists(runner, &action.spec.name, &action.project_dir)?;
    let decision = resolve_apply_decision(
        exists,
        action.force,
        action.target,
        action.scope,
        &action.spec.name,
        &location,
    )?;

    let cli_args = build_claude_add_args(&action.spec, action.scope);
    if action.dry_run {
        let detail = render_dry_run_command(
            &location,
            decision.action_label(),
            "claude",
            &cli_args,
            &action.spec,
        );
        return Ok(InstallEntryResult {
            target: action.target,
            scope: action.scope,
            status: InstallStatus::DryRun,
            location,
            detail,
        });
    }

    if matches!(decision, crate::mcp_install::ApplyDecision::Update) {
        let remove_args = vec![
            "mcp".to_owned(),
            "remove".to_owned(),
            "-s".to_owned(),
            action.scope.to_string(),
            action.spec.name.clone(),
        ];
        run_checked(runner, "claude", &remove_args, &action.project_dir)?;
    }

    run_checked(runner, "claude", &cli_args, &action.project_dir)?;

    Ok(InstallEntryResult {
        target: action.target,
        scope: action.scope,
        status: decision.status(),
        location,
        detail: if matches!(decision, crate::mcp_install::ApplyDecision::Update) {
            "updated via claude CLI".to_owned()
        } else {
            "installed via claude CLI".to_owned()
        },
    })
}

fn install_cursor(
    action: &InstallAction,
    files: &dyn FileStore,
) -> Result<InstallEntryResult, InstallError> {
    let path = cursor_config_path(action.scope, &action.project_dir)?;
    let mut root = if files.exists(&path) {
        serde_json::from_str::<Value>(&files.read_to_string(&path).map_err(|err| {
            InstallError::ConfigRead { path: path.display().to_string(), message: err.to_string() }
        })?)
        .map_err(|err| InstallError::ConfigParse {
            path: path.display().to_string(),
            format: "JSON",
            message: err.to_string(),
        })?
    } else {
        Value::Object(Map::new())
    };

    let servers = ensure_json_object(root.as_object_mut(), "root object", &path)?;
    if !matches!(servers.get("mcpServers"), Some(Value::Object(_))) {
        servers.insert("mcpServers".to_owned(), Value::Object(Map::new()));
    }
    let mcp_servers = ensure_json_object(
        servers.get_mut("mcpServers").and_then(Value::as_object_mut),
        "mcpServers",
        &path,
    )?;

    let exists = mcp_servers.contains_key(&action.spec.name);
    let decision = resolve_apply_decision(
        exists,
        action.force,
        action.target,
        action.scope,
        &action.spec.name,
        &path.display().to_string(),
    )?;

    mcp_servers.insert(action.spec.name.clone(), cursor_server_value(&action.spec));
    let rendered =
        serde_json::to_string_pretty(&root).map_err(|err| InstallError::ConfigParse {
            path: path.display().to_string(),
            format: "JSON",
            message: err.to_string(),
        })?;

    if action.dry_run {
        return Ok(InstallEntryResult {
            target: action.target,
            scope: action.scope,
            status: InstallStatus::DryRun,
            location: path.display().to_string(),
            detail: render_dry_run_config(
                &path.display().to_string(),
                decision.action_label(),
                rendered,
                &action.spec,
            ),
        });
    }

    if let Some(parent) = path.parent() {
        files.create_dir_all(parent).map_err(|err| InstallError::ConfigWrite {
            path: parent.display().to_string(),
            message: err.to_string(),
        })?;
    }
    files.write_string(&path, &(rendered + "\n")).map_err(|err| InstallError::ConfigWrite {
        path: path.display().to_string(),
        message: err.to_string(),
    })?;

    Ok(InstallEntryResult {
        target: action.target,
        scope: action.scope,
        status: decision.status(),
        location: path.display().to_string(),
        detail: if matches!(decision, crate::mcp_install::ApplyDecision::Update) {
            "updated cursor MCP config".to_owned()
        } else {
            "created cursor MCP config".to_owned()
        },
    })
}

fn build_codex_add_args(spec: &ServerSpec) -> Vec<String> {
    let mut args = vec!["mcp".to_owned(), "add".to_owned(), spec.name.clone()];
    for (key, value) in &spec.env {
        args.push("--env".to_owned());
        args.push(format!("{key}={value}"));
    }
    args.push("--".to_owned());
    args.push(spec.command.clone());
    args.extend(spec.args.clone());
    args
}

fn build_gemini_add_args(spec: &ServerSpec, scope: InstallScope) -> Vec<String> {
    let mut args = vec!["mcp".to_owned(), "add".to_owned(), "-s".to_owned(), scope.to_string()];
    for (key, value) in &spec.env {
        args.push("-e".to_owned());
        args.push(format!("{key}={value}"));
    }
    args.push(spec.name.clone());
    args.push(spec.command.clone());
    args.extend(spec.args.clone());
    args
}

fn build_claude_add_args(spec: &ServerSpec, scope: InstallScope) -> Vec<String> {
    let mut args = vec![
        "mcp".to_owned(),
        "add".to_owned(),
        "-s".to_owned(),
        scope.to_string(),
        spec.name.clone(),
    ];
    for (key, value) in &spec.env {
        args.push("-e".to_owned());
        args.push(format!("{key}={value}"));
    }
    args.push("--".to_owned());
    args.push(spec.command.clone());
    args.extend(spec.args.clone());
    args
}

fn codex_server_exists(
    runner: &dyn CommandRunner,
    name: &str,
    cwd: &Path,
) -> Result<bool, InstallError> {
    let args = vec!["mcp".to_owned(), "get".to_owned(), name.to_owned(), "--json".to_owned()];
    let output = runner.run("codex", &args, cwd)?;
    if output.status == 0 {
        return Ok(true);
    }
    if output.stderr.contains("No MCP server named") {
        return Ok(false);
    }
    Err(InstallError::InspectionFailed {
        target: InstallTarget::Codex,
        name: name.to_owned(),
        message: output.stderr.trim().to_owned(),
    })
}

fn claude_server_exists(
    runner: &dyn CommandRunner,
    name: &str,
    cwd: &Path,
) -> Result<bool, InstallError> {
    let args = vec!["mcp".to_owned(), "get".to_owned(), name.to_owned()];
    let output = runner.run("claude", &args, cwd)?;
    if output.status == 0 {
        return Ok(true);
    }
    if output.stderr.contains("No MCP server found with name") {
        return Ok(false);
    }
    Err(InstallError::InspectionFailed {
        target: InstallTarget::Claude,
        name: name.to_owned(),
        message: output.stderr.trim().to_owned(),
    })
}

fn run_checked(
    runner: &dyn CommandRunner,
    program: &str,
    args: &[String],
    cwd: &Path,
) -> Result<CommandOutput, InstallError> {
    let output = runner.run(program, args, cwd)?;
    if output.status == 0 {
        Ok(output)
    } else {
        Err(InstallError::ExternalCommandFailed {
            program: program.to_owned(),
            status: output.status,
            message: if output.stderr.trim().is_empty() {
                output.stdout.trim()
            } else {
                output.stderr.trim()
            }
            .to_owned(),
        })
    }
}

fn codex_project_contains_server(doc: &DocumentMut, name: &str) -> bool {
    doc.get("mcp_servers").and_then(Item::as_table_like).and_then(|table| table.get(name)).is_some()
}

fn upsert_codex_project_server(mut doc: DocumentMut, spec: &ServerSpec) -> DocumentMut {
    let root = doc.as_table_mut();
    if !matches!(root.get("mcp_servers"), Some(Item::Table(_))) {
        root.insert("mcp_servers", Item::Table(Table::new()));
    }
    let servers = root["mcp_servers"].as_table_like_mut().expect("mcp_servers table must exist");
    servers.insert(&spec.name, Item::Table(codex_server_table(spec)));
    doc
}

fn codex_server_table(spec: &ServerSpec) -> Table {
    let mut table = Table::new();
    table["command"] = value(spec.command.clone());

    let mut args = Array::default();
    for arg in &spec.args {
        args.push(arg.as_str());
    }
    table["args"] = value(args);

    if !spec.env.is_empty() {
        let mut env_table = Table::new();
        for (key, env_value) in &spec.env {
            env_table[key] = value(env_value.clone());
        }
        table["env"] = Item::Table(env_table);
    }

    table
}

fn json_config_contains_server(
    files: &dyn FileStore,
    path: &Path,
    name: &str,
    servers_key: &str,
) -> Result<bool, InstallError> {
    if !files.exists(path) {
        return Ok(false);
    }

    let value = serde_json::from_str::<Value>(&files.read_to_string(path).map_err(|err| {
        InstallError::ConfigRead { path: path.display().to_string(), message: err.to_string() }
    })?)
    .map_err(|err| InstallError::ConfigParse {
        path: path.display().to_string(),
        format: "JSON",
        message: err.to_string(),
    })?;

    Ok(value
        .get(servers_key)
        .and_then(Value::as_object)
        .map(|servers| servers.contains_key(name))
        .unwrap_or(false))
}

fn cursor_server_value(spec: &ServerSpec) -> Value {
    let mut server = Map::new();
    server.insert("type".to_owned(), Value::String("stdio".to_owned()));
    server.insert("command".to_owned(), Value::String(spec.command.clone()));
    server.insert(
        "args".to_owned(),
        Value::Array(spec.args.iter().cloned().map(Value::String).collect()),
    );
    if !spec.env.is_empty() {
        server.insert(
            "env".to_owned(),
            Value::Object(
                spec.env
                    .iter()
                    .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                    .collect(),
            ),
        );
    }
    Value::Object(server)
}

fn ensure_json_object<'a>(
    value: Option<&'a mut Map<String, Value>>,
    what: &str,
    path: &Path,
) -> Result<&'a mut Map<String, Value>, InstallError> {
    value.ok_or_else(|| InstallError::InvalidJsonShape {
        path: path.display().to_string(),
        what: what.to_owned(),
    })
}

fn gemini_config_path(scope: InstallScope, project_dir: &Path) -> Result<PathBuf, InstallError> {
    match scope {
        InstallScope::User => Ok(home_dir()?.join(".gemini/settings.json")),
        InstallScope::Project => Ok(project_dir.join(".gemini/settings.json")),
        InstallScope::Local => {
            Err(InstallError::UnsupportedScope { target: InstallTarget::Gemini, scope })
        }
    }
}

fn cursor_config_path(scope: InstallScope, project_dir: &Path) -> Result<PathBuf, InstallError> {
    match scope {
        InstallScope::User => Ok(home_dir()?.join(".cursor/mcp.json")),
        InstallScope::Project => Ok(project_dir.join(".cursor/mcp.json")),
        InstallScope::Local => {
            Err(InstallError::UnsupportedScope { target: InstallTarget::Cursor, scope })
        }
    }
}

fn claude_location_hint(scope: InstallScope, project_dir: &Path) -> Result<String, InstallError> {
    Ok(match scope {
        InstallScope::User => home_dir()?.join(".claude.json").display().to_string(),
        InstallScope::Project => project_dir.join(".mcp.json").display().to_string(),
        InstallScope::Local => "~/.claude.json -> projects.<cwd>.mcpServers".to_owned(),
    })
}

fn home_dir() -> Result<PathBuf, InstallError> {
    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home));
    }
    if let Some(user_profile) = env::var_os("USERPROFILE") {
        return Ok(PathBuf::from(user_profile));
    }

    match (env::var_os("HOMEDRIVE"), env::var_os("HOMEPATH")) {
        (Some(drive), Some(path)) => {
            Ok(PathBuf::from(format!("{}{}", drive.to_string_lossy(), path.to_string_lossy())))
        }
        _ => Err(InstallError::HomeDirectoryUnavailable),
    }
}

fn shell_preview(program: &str, args: &[String]) -> String {
    let mut parts = Vec::with_capacity(args.len() + 1);
    parts.push(shell_escape(program));
    parts.extend(args.iter().map(|arg| shell_escape(arg)));
    parts.join(" ")
}

fn shell_escape(value: &str) -> String {
    if value.is_empty() {
        return "''".to_owned();
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '='))
    {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn render_dry_run_command(
    location: &str,
    action: &str,
    program: &str,
    args: &[String],
    spec: &ServerSpec,
) -> String {
    let mut lines = vec![
        format!("planned action: {action}"),
        format!("target config: {location}"),
        format!("command: {}", shell_preview(program, args)),
    ];
    if let Some(warning) = secret_warning(spec) {
        lines.push(warning);
    }
    lines.join("\n")
}

fn render_dry_run_config(
    location: &str,
    action: &str,
    config: String,
    spec: &ServerSpec,
) -> String {
    let mut lines =
        vec![format!("planned action: {action}"), format!("target config: {location}"), config];
    if let Some(warning) = secret_warning(spec) {
        lines.push(warning);
    }
    lines.join("\n")
}

fn secret_warning(spec: &ServerSpec) -> Option<String> {
    spec.args.windows(2).find(|pair| pair[0] == "--onec-password").map(|_| {
        "warning: --onec-password will be stored in the target MCP config as a process argument"
            .to_owned()
    })
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::{BTreeMap, HashMap},
        io,
        path::{Path, PathBuf},
    };

    use crate::mcp_install::{
        model::{InstallAction, InstallScope, InstallStatus, InstallTarget, ServerSpec},
        ports::{CommandOutput, CommandRunner, FileStore},
        InstallError,
    };

    use super::{apply_action, upsert_codex_project_server};

    struct FakeRunner {
        outputs: RefCell<Vec<CommandOutput>>,
        invocations: RefCell<Vec<(String, Vec<String>, PathBuf)>>,
    }

    impl FakeRunner {
        fn new(outputs: Vec<CommandOutput>) -> Self {
            Self { outputs: RefCell::new(outputs), invocations: RefCell::new(Vec::new()) }
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(
            &self,
            program: &str,
            args: &[String],
            cwd: &Path,
        ) -> Result<CommandOutput, InstallError> {
            self.invocations.borrow_mut().push((
                program.to_owned(),
                args.to_vec(),
                cwd.to_path_buf(),
            ));
            Ok(self.outputs.borrow_mut().remove(0))
        }
    }

    #[derive(Default)]
    struct MemoryFiles {
        files: RefCell<HashMap<PathBuf, String>>,
    }

    impl MemoryFiles {
        fn with(path: PathBuf, contents: &str) -> Self {
            let mut files = HashMap::new();
            files.insert(path, contents.to_owned());
            Self { files: RefCell::new(files) }
        }
    }

    impl FileStore for MemoryFiles {
        fn read_to_string(&self, path: &Path) -> io::Result<String> {
            self.files
                .borrow()
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "missing"))
        }

        fn write_string(&self, path: &Path, contents: &str) -> io::Result<()> {
            self.files.borrow_mut().insert(path.to_path_buf(), contents.to_owned());
            Ok(())
        }

        fn create_dir_all(&self, _path: &Path) -> io::Result<()> {
            Ok(())
        }

        fn exists(&self, path: &Path) -> bool {
            self.files.borrow().contains_key(path)
        }
    }

    fn spec() -> ServerSpec {
        ServerSpec {
            name: "bsl-analyzer".to_owned(),
            command: "bsl-analyzer".to_owned(),
            args: vec!["mcp".to_owned(), "--source-dir".to_owned(), ".".to_owned()],
            env: BTreeMap::new(),
        }
    }

    fn action(target: InstallTarget, scope: InstallScope) -> InstallAction {
        InstallAction {
            target,
            scope,
            spec: spec(),
            project_dir: PathBuf::from("/workspace"),
            force: true,
            dry_run: false,
        }
    }

    #[test]
    fn codex_project_merge_preserves_other_servers() {
        let initial = r#"[mcp_servers.other]
command = "other"
args = ["serve"]
"#
        .parse()
        .expect("valid toml");

        let updated = upsert_codex_project_server(initial, &spec());
        let rendered = updated.to_string();

        assert!(rendered.contains("[mcp_servers.other]"));
        assert!(rendered.contains("[mcp_servers.bsl-analyzer]"));
    }

    #[test]
    fn cursor_merge_preserves_existing_servers() {
        let path = PathBuf::from("/workspace/.cursor/mcp.json");
        let files = MemoryFiles::with(
            path.clone(),
            r#"{"mcpServers":{"other":{"type":"stdio","command":"other","args":["serve"]}}}"#,
        );
        let runner = FakeRunner::new(Vec::new());

        let result =
            apply_action(&action(InstallTarget::Cursor, InstallScope::Project), &runner, &files)
                .expect("cursor install succeeds");

        assert_eq!(result.status, InstallStatus::Installed);
        let written = files.read_to_string(&path).expect("file written");
        assert!(written.contains("\"other\""));
        assert!(written.contains("\"bsl-analyzer\""));
    }

    #[test]
    fn claude_force_reinstalls_existing_server() {
        let runner = FakeRunner::new(vec![
            CommandOutput { status: 0, stdout: String::new(), stderr: String::new() },
            CommandOutput { status: 0, stdout: String::new(), stderr: String::new() },
            CommandOutput { status: 0, stdout: String::new(), stderr: String::new() },
        ]);
        let files = MemoryFiles::default();

        let result =
            apply_action(&action(InstallTarget::Claude, InstallScope::Project), &runner, &files)
                .expect("claude install succeeds");

        assert_eq!(result.status, InstallStatus::Updated);
        let invocations = runner.invocations.borrow();
        assert_eq!(invocations.len(), 3);
        assert_eq!(invocations[1].1[1], "remove");
        assert_eq!(invocations[2].1[1], "add");
    }
}
