use std::io::{BufWriter, Write};
use std::path::Path;

use super::{AnalysisResults, FileAnalysis, Reporter};

/// JUnit XML reporter.
///
/// CI systems (GitLab, Jenkins, TeamCity) render JUnit reports as a native
/// "tests" tab. Each analyzed file becomes a `<testsuite>`; each finding becomes
/// a failing `<testcase>` whose failure type is the rule code. Files without
/// findings contribute a single passing test case so the suite is visible as
/// green rather than absent.
pub struct JunitReporter;

impl Reporter for JunitReporter {
    fn key(&self) -> &'static str {
        "junit"
    }

    fn report(&self, results: &AnalysisResults, output_dir: &Path) -> anyhow::Result<()> {
        let output_file = output_dir.join("bsl-analyzer.junit.xml");
        let file = std::fs::File::create(&output_file)?;
        let mut w = BufWriter::new(file);

        // The CLI only produces a `FileAnalysis` for files that have findings, so
        // the root counters must be derived from what is actually emitted (each
        // clean file contributes one passing case, each finding one failing case)
        // — not from `files_analyzed`, which would inflate `tests` past the number
        // of `<testcase>` nodes and confuse CI parsers.
        let total_failures: usize = results.diagnostics.iter().map(|f| f.diagnostics.len()).sum();
        let total_tests: usize =
            results.diagnostics.iter().map(|f| f.diagnostics.len().max(1)).sum();

        writeln!(w, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
        writeln!(
            w,
            r#"<testsuites name="bsl-analyzer" tests="{total_tests}" failures="{total_failures}">"#
        )?;

        let mut files: Vec<&FileAnalysis> = results.diagnostics.iter().collect();
        files.sort_by_key(|f| junit_path(&f.relative_path));

        for file in files {
            write_suite(&mut w, file)?;
        }

        writeln!(w, "</testsuites>")?;
        w.flush()?;

        tracing::info!("JUnit report written to {:?}", output_file);
        Ok(())
    }
}

fn write_suite<W: Write>(w: &mut W, file: &FileAnalysis) -> std::io::Result<()> {
    let path = junit_path(&file.relative_path);

    if file.diagnostics.is_empty() {
        writeln!(w, r#"  <testsuite name="{}" tests="1" failures="0">"#, escape_attr(&path))?;
        writeln!(
            w,
            r#"    <testcase name="{}" classname="{}"/>"#,
            escape_attr(&path),
            escape_attr(&path)
        )?;
        writeln!(w, "  </testsuite>")?;
        return Ok(());
    }

    let mut diagnostics: Vec<_> = file.diagnostics.iter().collect();
    diagnostics.sort_by(|a, b| {
        (a.start_line, a.start_column, a.end_line, a.end_column, &a.code, &a.message).cmp(&(
            b.start_line,
            b.start_column,
            b.end_line,
            b.end_column,
            &b.code,
            &b.message,
        ))
    });

    writeln!(
        w,
        r#"  <testsuite name="{}" tests="{}" failures="{}">"#,
        escape_attr(&path),
        diagnostics.len(),
        diagnostics.len()
    )?;
    for d in diagnostics {
        let name = format!("{} [{}:{}]", d.code, d.start_line + 1, d.start_column + 1);
        writeln!(
            w,
            r#"    <testcase name="{}" classname="{}">"#,
            escape_attr(&name),
            escape_attr(&path)
        )?;
        writeln!(
            w,
            r#"      <failure message="{}" type="{}">{}:{}: {}</failure>"#,
            escape_attr(&d.message),
            escape_attr(&d.code),
            escape_text(&path),
            d.start_line + 1,
            escape_text(&d.message)
        )?;
        writeln!(w, "    </testcase>")?;
    }
    writeln!(w, "  </testsuite>")?;
    Ok(())
}

fn junit_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Drops characters outside XML 1.0's legal set (only tab, LF, CR and the
/// printable ranges are allowed). Source strings — paths and diagnostic messages
/// carrying identifiers or string-literal fragments — can in principle contain
/// control characters that would make the document non-well-formed; escaping
/// alone does not rescue them, so they are removed first.
fn xml_sanitize(s: &str) -> String {
    s.chars()
        .filter(|&c| {
            c == '\t'
                || c == '\n'
                || c == '\r'
                || ('\u{20}'..='\u{d7ff}').contains(&c)
                || ('\u{e000}'..='\u{fffd}').contains(&c)
                || c >= '\u{10000}'
        })
        .collect()
}

/// XML attribute-value escaping (`"` and `<`/`&` are the ones that break attrs).
fn escape_attr(s: &str) -> String {
    xml_sanitize(s)
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// XML text-content escaping.
fn escape_text(s: &str) -> String {
    xml_sanitize(s).replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ide::DiagnosticOutput;
    use tempfile::TempDir;

    use super::*;
    use crate::reporters::{AnalysisResults, FileAnalysis, Reporter};

    fn diag(code: &str, message: &str, line: usize) -> DiagnosticOutput {
        DiagnosticOutput {
            code: code.to_string(),
            message: message.to_string(),
            severity: "Warning".to_string(),
            start_line: line,
            start_column: 0,
            end_line: line,
            end_column: 1,
            tags: vec![],
        }
    }

    fn render(results: &AnalysisResults) -> String {
        let temp = TempDir::new().unwrap();
        JunitReporter.report(results, temp.path()).unwrap();
        std::fs::read_to_string(temp.path().join("bsl-analyzer.junit.xml")).unwrap()
    }

    #[test]
    fn renders_failures_and_is_well_formed() {
        let results = AnalysisResults {
            files_analyzed: 1,
            files_with_issues: 1,
            total_diagnostics: 1,
            elapsed_secs: 0.1,
            diagnostics: vec![FileAnalysis {
                path: PathBuf::from("a.bsl"),
                relative_path: PathBuf::from("a.bsl"),
                diagnostics: vec![diag("LineLength", "Line too long", 2)],
                line_snippets: vec![],
            }],
            source_dir: PathBuf::from("."),
            workspace_dir: PathBuf::from("."),
        };
        let xml = render(&results);

        assert!(xml.contains(r#"<testsuite name="a.bsl" tests="1" failures="1">"#));
        assert!(xml.contains(r#"type="LineLength""#));
        assert!(xml.contains("a.bsl:3: Line too long"));
        // Balanced tags → parseable by CI consumers.
        assert_eq!(xml.matches("<testcase").count(), xml.matches("</testcase>").count());
    }

    #[test]
    fn escapes_xml_metacharacters() {
        let results = AnalysisResults {
            files_analyzed: 1,
            files_with_issues: 1,
            total_diagnostics: 1,
            elapsed_secs: 0.1,
            diagnostics: vec![FileAnalysis {
                path: PathBuf::from("a.bsl"),
                relative_path: PathBuf::from("a.bsl"),
                diagnostics: vec![diag("Rule", r#"expected <Тип> & "value""#, 0)],
                line_snippets: vec![],
            }],
            source_dir: PathBuf::from("."),
            workspace_dir: PathBuf::from("."),
        };
        let xml = render(&results);

        assert!(!xml.contains("<Тип>"), "raw angle brackets must be escaped");
        assert!(xml.contains("&lt;"));
        assert!(xml.contains("&amp;"));
        assert!(xml.contains("&quot;"));
    }

    #[test]
    fn root_tests_count_matches_emitted_testcases() {
        // `files_analyzed` is larger than the number of files carried in
        // `diagnostics` (clean files are dropped upstream); the root `tests` must
        // still equal the emitted `<testcase>` count.
        let results = AnalysisResults {
            files_analyzed: 10,
            files_with_issues: 1,
            total_diagnostics: 2,
            elapsed_secs: 0.1,
            diagnostics: vec![FileAnalysis {
                path: PathBuf::from("a.bsl"),
                relative_path: PathBuf::from("a.bsl"),
                diagnostics: vec![diag("R", "m", 0), diag("R", "m", 1)],
                line_snippets: vec![],
            }],
            source_dir: PathBuf::from("."),
            workspace_dir: PathBuf::from("."),
        };
        let xml = render(&results);

        assert!(xml.contains(r#"<testsuites name="bsl-analyzer" tests="2" failures="2">"#));
        assert_eq!(xml.matches("<testcase").count(), 2);
    }

    #[test]
    fn strips_illegal_xml_control_characters() {
        let results = AnalysisResults {
            files_analyzed: 1,
            files_with_issues: 1,
            total_diagnostics: 1,
            elapsed_secs: 0.1,
            diagnostics: vec![FileAnalysis {
                path: PathBuf::from("a.bsl"),
                relative_path: PathBuf::from("a.bsl"),
                diagnostics: vec![diag("Rule", "bad\u{1}\u{8}value", 0)],
                line_snippets: vec![],
            }],
            source_dir: PathBuf::from("."),
            workspace_dir: PathBuf::from("."),
        };
        let xml = render(&results);

        assert!(!xml.contains('\u{1}'));
        assert!(!xml.contains('\u{8}'));
        assert!(xml.contains("badvalue"));
    }

    #[test]
    fn clean_file_emits_passing_case() {
        let results = AnalysisResults {
            files_analyzed: 1,
            files_with_issues: 0,
            total_diagnostics: 0,
            elapsed_secs: 0.1,
            diagnostics: vec![FileAnalysis {
                path: PathBuf::from("clean.bsl"),
                relative_path: PathBuf::from("clean.bsl"),
                diagnostics: vec![],
                line_snippets: vec![],
            }],
            source_dir: PathBuf::from("."),
            workspace_dir: PathBuf::from("."),
        };
        let xml = render(&results);

        assert!(xml.contains(r#"<testsuite name="clean.bsl" tests="1" failures="0">"#));
        assert!(xml.contains(r#"<testcase name="clean.bsl" classname="clean.bsl"/>"#));
    }
}
