use std::{error::Error, fs, path::PathBuf};

use clap::{Subcommand, ValueEnum};

#[derive(Subcommand)]
pub enum RulesCommands {
    Export {
        #[arg(long, value_enum, default_value_t = RulesFormat::Sonarqube)]
        format: RulesFormat,

        #[arg(long, default_value = "ru")]
        lang: String,

        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    List,
}

#[derive(Debug, Clone, Default, ValueEnum)]
pub enum RulesFormat {
    #[default]
    Sonarqube,
    Json,
}

pub fn run(command: RulesCommands) -> Result<(), Box<dyn Error + Send + Sync>> {
    use ide::{all_diagnostic_codes, docs, get_metadata};

    match command {
        RulesCommands::Export { format, lang, output } => {
            let rules = export_rules(&lang, &format);

            let json = serde_json::to_string_pretty(&rules)?;

            match output {
                Some(path) => {
                    fs::write(&path, &json)?;
                    eprintln!("Rules exported to: {:?}", path);
                }
                None => {
                    println!("{}", json);
                }
            }
        }
        RulesCommands::List => {
            println!("Available diagnostic codes:\n");
            for code in all_diagnostic_codes() {
                let docs = docs::get_docs(code);
                let name = if lang_is_russian() { docs.name_ru } else { docs.name_en };
                let status = if let Some(meta) = get_metadata(code) {
                    if meta.activated_by_default {
                        "enabled"
                    } else {
                        "disabled"
                    }
                } else {
                    "unknown"
                };
                println!("  {:40} [{}] {}", format!("{:?}", code), status, name);
            }
        }
    }

    Ok(())
}

fn lang_is_russian() -> bool {
    std::env::var("LANG").map(|l| l.starts_with("ru")).unwrap_or(true)
}

fn export_rules(lang: &str, format: &RulesFormat) -> serde_json::Value {
    use ide::{
        all_diagnostic_codes, docs, get_metadata, CleanCodeAttribute, DiagnosticSeverityLevel,
        DiagnosticType, ImpactSeverity, SoftwareQuality,
    };

    let is_ru = lang == "ru";

    let rules: Vec<serde_json::Value> = all_diagnostic_codes()
        .filter_map(|code| {
            let metadata = get_metadata(code)?;
            let docs = docs::get_docs(code);

            let name = if is_ru { docs.name_ru } else { docs.name_en };
            let description = if is_ru { docs.description_ru } else { docs.description_en };

            let sonar_type = match metadata.diagnostic_type {
                DiagnosticType::Error => "BUG",
                DiagnosticType::CodeSmell => "CODE_SMELL",
                DiagnosticType::Vulnerability => "VULNERABILITY",
                DiagnosticType::SecurityHotspot => "SECURITY_HOTSPOT",
            };

            let sonar_severity = match metadata.severity {
                DiagnosticSeverityLevel::Blocker => "BLOCKER",
                DiagnosticSeverityLevel::Critical => "CRITICAL",
                DiagnosticSeverityLevel::Major => "MAJOR",
                DiagnosticSeverityLevel::Minor => "MINOR",
                DiagnosticSeverityLevel::Info => "INFO",
            };

            let clean_code_attribute = match metadata.clean_code_attribute {
                CleanCodeAttribute::Consistent => "CONVENTIONAL",
                CleanCodeAttribute::Intentional => "CLEAR",
                CleanCodeAttribute::Adaptable => "FOCUSED",
                CleanCodeAttribute::Responsible => "TRUSTWORTHY",
            };

            let impacts: Vec<serde_json::Value> = metadata
                .impacts
                .iter()
                .map(|impact| {
                    let software_quality = match impact.software_quality {
                        SoftwareQuality::Maintainability => "MAINTAINABILITY",
                        SoftwareQuality::Reliability => "RELIABILITY",
                        SoftwareQuality::Security => "SECURITY",
                    };
                    let severity = match impact.severity {
                        ImpactSeverity::Low => "LOW",
                        ImpactSeverity::Medium => "MEDIUM",
                        ImpactSeverity::High => "HIGH",
                    };
                    serde_json::json!({
                        "softwareQuality": software_quality,
                        "severity": severity
                    })
                })
                .collect();

            let html_description = markdown_to_html(description);

            let tags: Vec<&str> = metadata.tags.iter().map(|t| tag_to_str(t)).collect();

            Some(serde_json::json!({
                "code": format!("{:?}", code),
                "name": if name.is_empty() { format!("{:?}", code) } else { name.to_string() },
                "description": html_description,
                "type": sonar_type,
                "severity": sonar_severity,
                "cleanCodeAttribute": clean_code_attribute,
                "impacts": impacts,
                "active": metadata.activated_by_default,
                "effortMinutes": metadata.minutes_to_fix,
                "tags": tags
            }))
        })
        .collect();

    match format {
        RulesFormat::Sonarqube => serde_json::json!({ "rules": rules }),
        RulesFormat::Json => serde_json::json!(rules),
    }
}

fn tag_to_str(tag: &ide::MetadataTag) -> &'static str {
    use ide::MetadataTag;
    match tag {
        MetadataTag::Standard => "standard",
        MetadataTag::Lockinos => "lockinos",
        MetadataTag::Sql => "sql",
        MetadataTag::Performance => "performance",
        MetadataTag::Brainoverload => "brainoverload",
        MetadataTag::Badpractice => "badpractice",
        MetadataTag::Clumsy => "clumsy",
        MetadataTag::Design => "design",
        MetadataTag::Suspicious => "suspicious",
        MetadataTag::Unpredictable => "unpredictable",
        MetadataTag::Deprecated => "deprecated",
        MetadataTag::Unused => "unused",
        MetadataTag::Error => "error",
        MetadataTag::Localize => "localize",
    }
}

fn markdown_to_html(md: &str) -> String {
    if md.is_empty() {
        return String::new();
    }

    let mut html = String::new();
    let mut in_code_block = false;
    let mut in_list = false;
    let mut current_paragraph = String::new();

    for line in md.lines() {
        if line.starts_with("<!--") || line.ends_with("-->") {
            continue;
        }

        if line.starts_with("```") {
            if in_code_block {
                html.push_str("</code></pre>\n");
                in_code_block = false;
            } else {
                flush_paragraph(&mut html, &mut current_paragraph);
                html.push_str("<pre><code>");
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            html.push_str(&line.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;"));
            html.push('\n');
            continue;
        }

        let trimmed = line.trim();

        if trimmed.is_empty() {
            flush_paragraph(&mut html, &mut current_paragraph);
            if in_list {
                html.push_str("</ul>\n");
                in_list = false;
            }
            continue;
        }

        if let Some(header) = trimmed.strip_prefix("# ") {
            flush_paragraph(&mut html, &mut current_paragraph);
            if !html.is_empty() {
                html.push_str(&format!("<h3>{}</h3>\n", escape_html(header)));
            }
            continue;
        }
        if let Some(header) = trimmed.strip_prefix("## ") {
            flush_paragraph(&mut html, &mut current_paragraph);
            html.push_str(&format!("<h4>{}</h4>\n", escape_html(header)));
            continue;
        }

        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            flush_paragraph(&mut html, &mut current_paragraph);
            if !in_list {
                html.push_str("<ul>\n");
                in_list = true;
            }
            let item = &trimmed[2..];
            html.push_str(&format!("<li>{}</li>\n", escape_html(item)));
            continue;
        }

        if !current_paragraph.is_empty() {
            current_paragraph.push(' ');
        }
        current_paragraph.push_str(trimmed);
    }

    flush_paragraph(&mut html, &mut current_paragraph);
    if in_list {
        html.push_str("</ul>\n");
    }
    if in_code_block {
        html.push_str("</code></pre>\n");
    }

    html.trim().to_string()
}

fn flush_paragraph(html: &mut String, paragraph: &mut String) {
    if !paragraph.is_empty() {
        html.push_str(&format!("<p>{}</p>\n", escape_html(paragraph)));
        paragraph.clear();
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
