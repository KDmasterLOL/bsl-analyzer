use std::collections::BTreeSet;
use std::io::{BufWriter, Write};
use std::path::Path;

use serde::ser::{Serialize, SerializeSeq, Serializer};

use super::{AnalysisResults, Reporter};

const SARIF_SCHEMA: &str = "https://json.schemastore.org/sarif-2.1.0.json";

pub struct SarifReporter;

impl Reporter for SarifReporter {
    fn key(&self) -> &'static str {
        "sarif"
    }

    fn report(&self, results: &AnalysisResults, output_dir: &Path) -> anyhow::Result<()> {
        let output_file = output_dir.join("bsl-analyzer.sarif");
        let file = std::fs::File::create(&output_file)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(&mut writer, &SarifDocument { results })?;
        writer.flush()?;

        tracing::info!("SARIF report written to {:?}", output_file);
        Ok(())
    }
}

/// Streams the SARIF document straight to the output writer.
///
/// Findings are serialized element-by-element instead of being materialized as
/// a `serde_json::Value` tree and a pretty-printed `String`. On full-config runs
/// (millions of findings) those two intermediate copies dominated peak RSS; the
/// streaming form keeps only the already-resident `AnalysisResults` plus the I/O
/// buffer.
struct SarifDocument<'a> {
    results: &'a AnalysisResults,
}

impl Serialize for SarifDocument<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut doc = serializer.serialize_struct("sarif", 3)?;
        doc.serialize_field("$schema", SARIF_SCHEMA)?;
        doc.serialize_field("version", "2.1.0")?;
        doc.serialize_field("runs", &[Run { results: self.results }])?;
        doc.end()
    }
}

struct Run<'a> {
    results: &'a AnalysisResults,
}

impl Serialize for Run<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut run = serializer.serialize_struct("run", 2)?;
        run.serialize_field("tool", &Tool { results: self.results })?;
        run.serialize_field("results", &ResultsSeq { results: self.results })?;
        run.end()
    }
}

struct Tool<'a> {
    results: &'a AnalysisResults,
}

impl Serialize for Tool<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut tool = serializer.serialize_struct("tool", 1)?;
        tool.serialize_field("driver", &Driver { results: self.results })?;
        tool.end()
    }
}

struct Driver<'a> {
    results: &'a AnalysisResults,
}

impl Serialize for Driver<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut driver = serializer.serialize_struct("driver", 3)?;
        driver.serialize_field("name", "bsl-analyzer")?;
        driver.serialize_field("semanticVersion", env!("CARGO_PKG_VERSION"))?;
        driver.serialize_field("rules", &RulesSeq { results: self.results })?;
        driver.end()
    }
}

/// Unique rule descriptors. The rule set is bounded by the number of diagnostic
/// codes (~hundreds), so collecting it into a `BTreeSet` is cheap and keeps the
/// output deterministically ordered.
struct RulesSeq<'a> {
    results: &'a AnalysisResults,
}

impl Serialize for RulesSeq<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let codes: BTreeSet<&str> = self
            .results
            .diagnostics
            .iter()
            .flat_map(|file| file.diagnostics.iter())
            .map(|d| d.code.as_str())
            .collect();

        let mut seq = serializer.serialize_seq(Some(codes.len()))?;
        for code in codes {
            seq.serialize_element(&Rule { code })?;
        }
        seq.end()
    }
}

struct Rule<'a> {
    code: &'a str,
}

impl Serialize for Rule<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut rule = serializer.serialize_struct("rule", 3)?;
        rule.serialize_field("id", self.code)?;
        rule.serialize_field("name", self.code)?;
        rule.serialize_field("shortDescription", &Text { text: self.code })?;
        rule.end()
    }
}

/// Streams every finding without holding the full result array in memory.
struct ResultsSeq<'a> {
    results: &'a AnalysisResults,
}

impl Serialize for ResultsSeq<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(None)?;
        for file in &self.results.diagnostics {
            let uri = sarif_uri(&file.relative_path);
            for diagnostic in &file.diagnostics {
                seq.serialize_element(&ResultElem {
                    rule_id: &diagnostic.code,
                    level: sarif_level(&diagnostic.severity),
                    message: &diagnostic.message,
                    uri: &uri,
                    start_line: diagnostic.start_line + 1,
                    start_column: diagnostic.start_column + 1,
                    end_line: diagnostic.end_line + 1,
                    end_column: diagnostic.end_column + 1,
                })?;
            }
        }
        seq.end()
    }
}

struct ResultElem<'a> {
    rule_id: &'a str,
    level: &'static str,
    message: &'a str,
    uri: &'a str,
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
}

impl Serialize for ResultElem<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut result = serializer.serialize_struct("result", 4)?;
        result.serialize_field("ruleId", self.rule_id)?;
        result.serialize_field("level", self.level)?;
        result.serialize_field("message", &Text { text: self.message })?;
        result.serialize_field(
            "locations",
            &[Location {
                uri: self.uri,
                start_line: self.start_line,
                start_column: self.start_column,
                end_line: self.end_line,
                end_column: self.end_column,
            }],
        )?;
        result.end()
    }
}

struct Location<'a> {
    uri: &'a str,
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
}

impl Serialize for Location<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut location = serializer.serialize_struct("location", 1)?;
        location.serialize_field(
            "physicalLocation",
            &PhysicalLocation {
                uri: self.uri,
                start_line: self.start_line,
                start_column: self.start_column,
                end_line: self.end_line,
                end_column: self.end_column,
            },
        )?;
        location.end()
    }
}

struct PhysicalLocation<'a> {
    uri: &'a str,
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
}

impl Serialize for PhysicalLocation<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut physical = serializer.serialize_struct("physicalLocation", 2)?;
        physical.serialize_field("artifactLocation", &ArtifactLocation { uri: self.uri })?;
        physical.serialize_field(
            "region",
            &Region {
                start_line: self.start_line,
                start_column: self.start_column,
                end_line: self.end_line,
                end_column: self.end_column,
            },
        )?;
        physical.end()
    }
}

struct ArtifactLocation<'a> {
    uri: &'a str,
}

impl Serialize for ArtifactLocation<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut artifact = serializer.serialize_struct("artifactLocation", 1)?;
        artifact.serialize_field("uri", self.uri)?;
        artifact.end()
    }
}

struct Region {
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
}

impl Serialize for Region {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut region = serializer.serialize_struct("region", 4)?;
        region.serialize_field("startLine", &self.start_line)?;
        region.serialize_field("startColumn", &self.start_column)?;
        region.serialize_field("endLine", &self.end_line)?;
        region.serialize_field("endColumn", &self.end_column)?;
        region.end()
    }
}

struct Text<'a> {
    text: &'a str,
}

impl Serialize for Text<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut message = serializer.serialize_struct("text", 1)?;
        message.serialize_field("text", self.text)?;
        message.end()
    }
}

fn sarif_uri(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn sarif_level(severity: &str) -> &'static str {
    match severity {
        "Blocker" | "Critical" | "Major" | "Error" => "error",
        "Warning" => "warning",
        "Information" | "Hint" => "note",
        _ => "warning",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ide::DiagnosticOutput;
    use serde_json::Value;
    use tempfile::TempDir;

    use super::*;
    use crate::reporters::{AnalysisResults, FileAnalysis, Reporter};

    #[test]
    fn writes_sarif_document_with_rules_results_and_locations() {
        let temp = TempDir::new().expect("tempdir");
        let results = sample_results();

        SarifReporter.report(&results, temp.path()).expect("write sarif");

        let sarif: Value = serde_json::from_str(
            &std::fs::read_to_string(temp.path().join("bsl-analyzer.sarif")).expect("sarif report"),
        )
        .expect("valid json");

        assert_eq!(sarif["$schema"], SARIF_SCHEMA);
        assert_eq!(sarif["version"], "2.1.0");

        let driver = &sarif["runs"][0]["tool"]["driver"];
        assert_eq!(driver["name"], "bsl-analyzer");
        assert_eq!(driver["semanticVersion"], env!("CARGO_PKG_VERSION"));
        assert_eq!(driver["rules"][0]["id"], "LineLength");

        let result = &sarif["runs"][0]["results"][0];
        assert_eq!(result["ruleId"], "LineLength");
        assert_eq!(result["message"]["text"], "Line too long");
        assert_eq!(result["level"], "warning");
        assert_eq!(
            result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "CommonModules/Module/Ext/Module.bsl"
        );
        assert_eq!(result["locations"][0]["physicalLocation"]["region"]["startLine"], 3);
        assert_eq!(result["locations"][0]["physicalLocation"]["region"]["startColumn"], 5);
        assert_eq!(result["locations"][0]["physicalLocation"]["region"]["endLine"], 3);
        assert_eq!(result["locations"][0]["physicalLocation"]["region"]["endColumn"], 16);
    }

    #[test]
    fn deduplicates_rules_and_streams_every_result() {
        let temp = TempDir::new().expect("tempdir");
        let results = AnalysisResults {
            files_analyzed: 2,
            files_with_issues: 2,
            total_diagnostics: 3,
            elapsed_secs: 0.1,
            diagnostics: vec![
                FileAnalysis {
                    path: PathBuf::from("a.bsl"),
                    relative_path: PathBuf::from("a.bsl"),
                    diagnostics: vec![
                        diag("LineLength", "Warning"),
                        diag("UnusedLocalVariable", "Information"),
                    ],
                },
                FileAnalysis {
                    path: PathBuf::from("b.bsl"),
                    relative_path: PathBuf::from("b.bsl"),
                    diagnostics: vec![diag("LineLength", "Warning")],
                },
            ],
            source_dir: PathBuf::from("."),
            workspace_dir: PathBuf::from("."),
        };

        SarifReporter.report(&results, temp.path()).expect("write sarif");
        let sarif: Value = serde_json::from_str(
            &std::fs::read_to_string(temp.path().join("bsl-analyzer.sarif")).expect("sarif report"),
        )
        .expect("valid json");

        let rules = sarif["runs"][0]["tool"]["driver"]["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 2, "duplicate LineLength rule must collapse");

        let emitted = sarif["runs"][0]["results"].as_array().unwrap();
        assert_eq!(emitted.len(), 3, "every finding must be streamed");
    }

    #[test]
    fn maps_diagnostic_severity_to_sarif_level() {
        assert_eq!(sarif_level("Blocker"), "error");
        assert_eq!(sarif_level("Critical"), "error");
        assert_eq!(sarif_level("Major"), "error");
        assert_eq!(sarif_level("Error"), "error");
        assert_eq!(sarif_level("Warning"), "warning");
        assert_eq!(sarif_level("Information"), "note");
        assert_eq!(sarif_level("Hint"), "note");
    }

    #[test]
    fn normalizes_windows_path_separators_in_uri() {
        assert_eq!(
            sarif_uri(Path::new(r"CommonModules\Module\Ext\Module.bsl")),
            "CommonModules/Module/Ext/Module.bsl"
        );
    }

    fn diag(code: &str, severity: &str) -> DiagnosticOutput {
        DiagnosticOutput {
            code: code.to_string(),
            message: "msg".to_string(),
            severity: severity.to_string(),
            start_line: 0,
            start_column: 0,
            end_line: 0,
            end_column: 0,
            tags: vec![],
        }
    }

    fn sample_results() -> AnalysisResults {
        AnalysisResults {
            files_analyzed: 1,
            files_with_issues: 1,
            total_diagnostics: 1,
            elapsed_secs: 0.1,
            diagnostics: vec![FileAnalysis {
                path: PathBuf::from("CommonModules/Module/Ext/Module.bsl"),
                relative_path: PathBuf::from("CommonModules/Module/Ext/Module.bsl"),
                diagnostics: vec![DiagnosticOutput {
                    code: "LineLength".to_string(),
                    message: "Line too long".to_string(),
                    severity: "Warning".to_string(),
                    start_line: 2,
                    start_column: 4,
                    end_line: 2,
                    end_column: 15,
                    tags: vec![],
                }],
            }],
            source_dir: PathBuf::from("."),
            workspace_dir: PathBuf::from("."),
        }
    }
}
