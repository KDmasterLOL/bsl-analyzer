use std::{error::Error, fmt::Write as _, io};

use super::source_set::{
    configuration_root_provider, extensions_provider, SourceProvider, SourceSetArgs,
};

pub fn check_config(
    config: std::path::PathBuf,
    source_set: SourceSetArgs,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing::info!("Checking configuration: {:?}", config);

    let mut project_config = project_model::ProjectConfig::load_from_file(&config).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "failed to parse configuration file '{}': {}; expected a valid bsl-analyzer.toml, .bsl-analyzer.json, or .bsl-language-server.json",
                config.display(),
                e.message
            ),
        )
    })?;
    // Relative flag paths resolve against this command's project root — the
    // config file's directory — exactly as the paths inside that file do.
    let project_root = match config.parent() {
        Some(parent) if parent.as_os_str().is_empty() => std::path::Path::new("."),
        Some(parent) => parent,
        None => std::path::Path::new("."),
    };
    let providers = SourceProviders {
        configuration_root: configuration_root_provider(
            &source_set,
            project_config.configuration_root.as_deref(),
            project_root,
        ),
        extensions: extensions_provider(&source_set, project_config.extensions.as_ref()),
    };
    source_set.resolve(project_root)?.apply_to(&mut project_config);

    let diagnostics_config = diagnostics_config_from_project(&project_config)?;
    let diagnostics =
        mcp_server::resolve_project_baseline_diagnostics(config.parent(), &project_config);
    let project = project_model::Project::with_config(project_root, project_config.clone());
    let report = build_check_config_report(
        &config,
        &project_config,
        &project,
        &diagnostics_config,
        &diagnostics,
        &providers,
    );

    print!("{report}");

    if let Err(e) = &project {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("configuration is invalid: {e}"),
        )
        .into());
    }
    if baseline_diagnostics_have_issues(&diagnostics) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "configuration is invalid: search baseline diagnostics reported errors",
        )
        .into());
    }

    Ok(())
}

fn diagnostics_config_from_project(
    project_config: &project_model::ProjectConfig,
) -> Result<ide::DiagnosticsConfig, Box<dyn Error + Send + Sync>> {
    let locale = project_config.output.resolve_locale().unwrap_or_default();

    if project_config.diagnostics.is_null() {
        return Ok(ide::DiagnosticsConfig { locale, ..Default::default() });
    }

    let mut cfg: ide::DiagnosticsConfig = serde_json::from_value(
        project_config.diagnostics.clone(),
    )
    .map_err(|error| -> Box<dyn Error + Send + Sync> {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to parse diagnostics section: {error}"),
        )
        .into()
    })?;
    cfg.locale = locale;
    Ok(cfg)
}

fn baseline_diagnostics_have_issues(
    baseline_diagnostics: &mcp_server::BaselineConfigDiagnostics,
) -> bool {
    baseline_diagnostics.workspace.issue.is_some() || baseline_diagnostics.reference.issue.is_some()
}

/// Where each resolved source-set field came from. Reported per field: the root
/// and the extension list are decided independently, so one label for both
/// would misname the origin whenever only one of them is overridden.
struct SourceProviders {
    configuration_root: SourceProvider,
    extensions: SourceProvider,
}

fn build_check_config_report(
    config_path: &std::path::Path,
    project_config: &project_model::ProjectConfig,
    project: &Result<project_model::Project, project_model::ProjectError>,
    diagnostics_config: &ide::DiagnosticsConfig,
    baseline_diagnostics: &mcp_server::BaselineConfigDiagnostics,
    providers: &SourceProviders,
) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Configuration is {}.",
        if baseline_diagnostics_have_issues(baseline_diagnostics) || project.is_err() {
            "invalid"
        } else {
            "valid"
        }
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Config file: {}", config_path.display());
    let _ = writeln!(
        out,
        "Format: {}",
        match config_path.extension().and_then(|ext| ext.to_str()) {
            Some("toml") => "TOML",
            Some("json") => "JSON",
            _ => "auto",
        }
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Project:");
    let declared_root = project_config.configuration_root.as_deref();
    let used_root = match providers.configuration_root {
        SourceProvider::AutoDiscovery => None,
        _ => declared_root,
    };
    let _ = writeln!(
        out,
        "  Source root: {} [from: {}]",
        used_root.unwrap_or("auto-discovery"),
        providers.configuration_root.label(config_path)
    );
    // A declared root without a `Configuration.xml` does not decide anything —
    // the model warns and searches instead. Saying so beats printing the value
    // next to a provider that contradicts it.
    if used_root.is_none() {
        if let Some(ignored) = declared_root {
            let _ = writeln!(
                out,
                "               (declared \"{ignored}\" ignored: no Configuration.xml there)"
            );
        }
    }
    let _ = writeln!(
        out,
        "  Extensions:  {}",
        match &project_config.extensions {
            None => "auto-discovery (src/cfe/*)".to_owned(),
            Some(list) if list.is_empty() => "none".to_owned(),
            Some(list) => list
                .iter()
                .map(|decl| match decl {
                    project_model::ExtensionDecl::Path(path) => path.clone(),
                    project_model::ExtensionDecl::Structured(entry)
                        if entry.depends_on.is_empty() =>
                        format!("{} ({})", entry.name, entry.path),
                    project_model::ExtensionDecl::Structured(entry) => format!(
                        "{} ({}, dependsOn: {})",
                        entry.name,
                        entry.path,
                        entry.depends_on.join(", ")
                    ),
                })
                .collect::<Vec<_>>()
                .join(", "),
        }
    );
    let _ = writeln!(out, "  Extensions from: {}", providers.extensions.label(config_path));
    let _ =
        writeln!(out, "  Language:    {}", project_config.language.as_deref().unwrap_or("default"));
    let _ = writeln!(
        out,
        "  Diff base:   {}",
        project_config.analysis.diff_base.as_deref().unwrap_or("none (full analysis)")
    );
    let _ = writeln!(
        out,
        "  Ignored authors: {}",
        if project_config.analysis.ignored_authors.is_empty() {
            "none".to_owned()
        } else {
            project_config.analysis.ignored_authors.join(", ")
        }
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Extension topology:");
    match project {
        Ok(project) => {
            let topology = project.extension_topology();
            if topology.nodes().is_empty() {
                let _ = writeln!(out, "  no extensions resolved");
            } else {
                for node in topology.nodes() {
                    let deps = if node.depends_on().is_empty() {
                        "independent".to_owned()
                    } else {
                        format!(
                            "dependsOn: {}",
                            node.depends_on()
                                .iter()
                                .map(|id| topology.node(*id).name())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    };
                    let _ = writeln!(out, "  {} — {} ({deps})", node.name(), node.path().display());
                }
                let _ = writeln!(
                    out,
                    "  Order:       {}",
                    topology
                        .topological_order()
                        .iter()
                        .map(|id| topology.node(*id).name())
                        .collect::<Vec<_>>()
                        .join(" -> ")
                );
                let _ = writeln!(out, "  Fingerprint: {}", topology.fingerprint().to_hex());
            }
        }
        Err(e) => {
            let _ = writeln!(out, "  invalid: {e}");
        }
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "Diagnostics:");
    let _ = writeln!(out, "  ordinaryAppSupport: {}", diagnostics_config.ordinary_app_support);
    let _ =
        writeln!(out, "  dataflowMaxIterations: {}", diagnostics_config.dataflow_max_iterations);
    let _ = writeln!(out, "  Disabled:   {}", diagnostics_config.disabled.len());
    let _ = writeln!(out, "  Enabled:    {}", diagnostics_config.enabled.len());
    let _ = writeln!(out, "  Parameters: {}", diagnostics_config.parameters.len());
    let _ = writeln!(
        out,
        "  Disabled codes: {}",
        summarize_diagnostic_codes(diagnostics_config.disabled.iter().map(ToString::to_string))
    );
    let _ = writeln!(
        out,
        "  Explicitly enabled: {}",
        summarize_diagnostic_codes(diagnostics_config.enabled.iter().map(ToString::to_string))
    );
    let _ = writeln!(
        out,
        "  Parameterized: {}",
        summarize_diagnostic_codes(diagnostics_config.parameters.keys().map(ToString::to_string))
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Code lens:");
    let _ = writeln!(
        out,
        "  Cognitive complexity: {}",
        on_off(project_config.code_lens.show_cognitive_complexity)
    );
    let _ = writeln!(
        out,
        "  Cyclomatic complexity: {}",
        on_off(project_config.code_lens.show_cyclomatic_complexity)
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Formatting:");
    let _ = writeln!(out, "  Indent: {}", formatting_summary(&project_config.formatting));
    let _ = writeln!(out);
    let _ = writeln!(out, "Search baseline:");
    append_baseline_summary(&mut out, "Workspace", &baseline_diagnostics.workspace);
    append_baseline_summary(&mut out, "Reference", &baseline_diagnostics.reference);
    out
}

fn append_baseline_summary(
    out: &mut String,
    label: &str,
    summary: &mcp_server::BaselineResolutionSummary,
) {
    let _ = writeln!(out, "  {label}:");
    let _ = writeln!(out, "    Backend: {}", summary.backend);
    let _ = writeln!(out, "    Select:  {}", summary.selection);
    let _ = writeln!(out, "    Status:  {}", summary.issue.as_deref().unwrap_or("ready"));
}

fn on_off(value: bool) -> &'static str {
    if value {
        "on"
    } else {
        "off"
    }
}

fn formatting_summary(config: &project_model::FormattingConfig) -> String {
    if config.indent_size == 0 && !config.use_tabs {
        "not configured".to_owned()
    } else if config.use_tabs {
        format!("tabs x{}", config.indent_size)
    } else {
        format!("spaces x{}", config.indent_size)
    }
}

fn summarize_diagnostic_codes(codes: impl Iterator<Item = String>) -> String {
    let mut codes: Vec<String> = codes.collect();
    if codes.is_empty() {
        return "none".to_owned();
    }

    codes.sort();
    const LIMIT: usize = 8;
    if codes.len() <= LIMIT {
        return codes.join(", ");
    }

    let remaining = codes.len() - LIMIT;
    format!("{}, … (+{remaining} more)", codes[..LIMIT].join(", "))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        build_check_config_report, check_config, diagnostics_config_from_project, SourceProvider,
        SourceProviders, SourceSetArgs,
    };

    #[test]
    fn check_config_accepts_toml_project_config() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("bsl-analyzer.toml");
        fs::write(
            &config,
            r#"
[source]
root = "src/cf"

[search.baseline]
backend = "postgres"

[search.baseline.postgres]
host = "localhost"
port = 5432
dbname = "bsl_search"
schema = "bsl_search"
vault_role_base = "prod/search/bsl-analyzer"

[search.baseline.postgres.credential_helper]
program = "python3"
args = [
  "-c",
  "import sys; sys.stdin.readline(); sys.stdout.write(sys.argv[1])",
  '{"protocol":"bsl-analyzer.postgres-helper.v1","ok":true,"url":"postgres://user:pass@localhost:5432/bsl_search"}'
]

[search.baseline.workspace_code.policy]
publish_branches = ["develop"]

[[search.baseline.workspace_code.policy.branches]]
match = "*"
select_branch = "develop"
"#,
        )
        .unwrap();

        check_config(config, SourceSetArgs::default()).unwrap();
    }

    #[test]
    fn check_config_accepts_helper_based_json_project_config() {
        let dir = tempdir().unwrap();
        let config = dir.path().join(".bsl-analyzer.json");
        fs::write(
            &config,
            r#"{
                "search": {
                    "baseline": {
                        "backend": "postgres",
                        "postgres": {
                            "host": "localhost",
                            "port": 5432,
                            "dbname": "bsl_search",
                            "schema": "bsl_search",
                            "vaultRoleBase": "prod/search/bsl-analyzer",
                            "credentialHelper": {
                                "program": "python3",
                                "args": [
                                    "-c",
                                    "import sys; sys.stdin.readline(); sys.stdout.write(sys.argv[1])",
                                    "{\"protocol\":\"bsl-analyzer.postgres-helper.v1\",\"ok\":true,\"url\":\"postgres://user:pass@localhost:5432/bsl_search\"}"
                                ]
                            }
                        },
                        "workspaceCode": {
                            "policy": {
                                "publishBranches": ["develop"],
                                "branches": [
                                    { "match": "*", "selectBranch": "develop" }
                                ]
                            }
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        check_config(config, SourceSetArgs::default()).unwrap();
    }

    #[test]
    fn check_config_rejects_invalid_toml() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("bsl-analyzer.toml");
        fs::write(&config, "invalid {{{ toml").unwrap();

        let error = check_config(config, SourceSetArgs::default()).unwrap_err();

        assert!(error.to_string().contains("failed to parse configuration file"));
    }

    #[test]
    fn check_config_rejects_invalid_postgres_baseline_config() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("bsl-analyzer.toml");
        fs::write(
            &config,
            r#"
[search.baseline]
backend = "postgres"
"#,
        )
        .unwrap();

        let error = check_config(config, SourceSetArgs::default()).unwrap_err();

        assert!(error.to_string().contains("search baseline diagnostics reported errors"));
    }

    #[test]
    fn diagnostics_section_must_be_object_when_present() {
        let project_config: project_model::ProjectConfig = serde_json::from_str(
            r#"{
                "diagnostics": []
            }"#,
        )
        .unwrap();

        let error = diagnostics_config_from_project(&project_config).unwrap_err();

        assert!(error.to_string().contains("failed to parse diagnostics section"));
    }

    #[test]
    fn check_config_report_contains_project_and_diagnostics_summary() {
        let project_config: project_model::ProjectConfig = serde_json::from_str(
            r#"{
                "configurationRoot": "src/cf",
                "extensions": ["src/cfe/ExtA"],
                "analysis": { "diffBase": "vendor", "ignoredAuthors": ["Фирма 1С"] },
                "formatting": { "use_tabs": true, "indent_size": 1 },
                "codeLens": {
                    "showCognitiveComplexity": true,
                    "showCyclomaticComplexity": false
                },
                "diagnostics": {
                    "ordinaryAppSupport": true,
                    "dataflowMaxIterations": 20000,
                    "parameters": {
                        "CommentedCode": false,
                        "BadWords": true,
                        "CyclomaticComplexity": { "complexityThreshold": 15 }
                    }
                },
                "search": {
                    "baseline": {
                        "backend": "postgres",
                        "workspaceCode": {
                            "policy": {
                                "publishBranches": ["develop"],
                                "branches": [
                                    { "match": "*", "selectBranch": "develop" }
                                ]
                            }
                        }
                    }
                }
            }"#,
        )
        .unwrap();
        let diag_config = diagnostics_config_from_project(&project_config).unwrap();
        let baseline = mcp_server::resolve_project_baseline_diagnostics(
            Some(std::path::Path::new(".")),
            &project_config,
        );

        let project =
            project_model::Project::with_config(std::env::temp_dir(), project_config.clone());
        let report = build_check_config_report(
            std::path::Path::new("bsl-analyzer.toml"),
            &project_config,
            &project,
            &diag_config,
            &baseline,
            &SourceProviders {
                configuration_root: SourceProvider::ConfigFile,
                extensions: SourceProvider::ConfigFile,
            },
        );

        assert!(report.contains("Configuration is invalid."));
        assert!(report.contains("Project:"));
        assert!(report.contains("Source root: src/cf"));
        assert!(report.contains("Extensions:  src/cfe/ExtA"));
        assert!(report.contains("Diff base:   vendor"));
        assert!(report.contains("Ignored authors: Фирма 1С"));
        assert!(report.contains("ordinaryAppSupport: true"));
        assert!(report.contains("dataflowMaxIterations: 20000"));
        assert!(report.contains("Disabled codes: CommentedCode"));
        assert!(report.contains("Explicitly enabled: BadWords"));
        assert!(report.contains("Parameterized: CyclomaticComplexity"));
        assert!(report.contains("Code lens:"));
        assert!(report.contains("Cognitive complexity: on"));
        assert!(report.contains("Formatting:"));
        assert!(report.contains("Indent: tabs x1"));
        assert!(report.contains("Search baseline:"));
    }
}
