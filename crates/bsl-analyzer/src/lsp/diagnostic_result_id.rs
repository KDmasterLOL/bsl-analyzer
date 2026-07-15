use std::fmt::Write as _;

use ide::Diagnostic;

/// A content hash of a file's computed diagnostics, used as the LSP pull-diagnostics
/// `resultId`.
///
/// Two pulls that produce the same diagnostics yield the same id, so the server can
/// answer an unchanged pull with a tiny `Unchanged` report instead of resending the
/// full set. The hash covers everything the client renders (code, range, severity,
/// message, tags) but deliberately not code-action fixes, which never change the
/// reported diagnostic itself.
///
/// This is correct under `inter_file_dependencies` because the caller recomputes the
/// diagnostics (Salsa-memoized, so an untouched dependency cone is cheap) and re-hashes
/// on every pull — the id reflects the actual result, not the file's own text, so a
/// diagnostic that shifts because a *base* module changed still produces a new id.
pub fn diagnostics_result_id(diagnostics: &[Diagnostic]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&(diagnostics.len() as u64).to_le_bytes());
    let mut scratch = String::new();
    for d in diagnostics {
        let start: u32 = d.range.start().into();
        let end: u32 = d.range.end().into();
        hasher.update(&start.to_le_bytes());
        hasher.update(&end.to_le_bytes());
        hasher.update(d.code.as_str().as_bytes());
        hasher.update(&[0]);
        hasher.update(d.message.as_bytes());
        hasher.update(&[0]);
        scratch.clear();
        // Severity and tag are small fieldless enums; their Debug form is a stable,
        // allocation-free discriminant projection sufficient to key the hash.
        let _ = write!(scratch, "{:?}|{:?}", d.severity, d.tags);
        hasher.update(scratch.as_bytes());
        hasher.update(&[0]);
    }
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide::{Diagnostic, DiagnosticCode, Severity, TextRange};

    fn diag(code: DiagnosticCode, message: &str, start: u32, end: u32) -> Diagnostic {
        Diagnostic {
            code,
            message: message.to_string(),
            severity: Severity::Warning,
            range: TextRange::new(start.into(), end.into()),
            tags: Vec::new(),
            fixes: Vec::new(),
        }
    }

    fn some_code() -> DiagnosticCode {
        DiagnosticCode::UnreachableCode
    }

    #[test]
    fn deterministic_for_identical_input() {
        let a = vec![diag(some_code(), "unused", 0, 4)];
        let b = vec![diag(some_code(), "unused", 0, 4)];
        assert_eq!(diagnostics_result_id(&a), diagnostics_result_id(&b));
    }

    #[test]
    fn empty_is_stable_and_nonempty() {
        let id = diagnostics_result_id(&[]);
        assert_eq!(id, diagnostics_result_id(&[]));
        assert!(!id.is_empty());
    }

    #[test]
    fn message_change_changes_id() {
        let a = vec![diag(some_code(), "one", 0, 4)];
        let b = vec![diag(some_code(), "two", 0, 4)];
        assert_ne!(diagnostics_result_id(&a), diagnostics_result_id(&b));
    }

    #[test]
    fn range_change_changes_id() {
        let a = vec![diag(some_code(), "x", 0, 4)];
        let b = vec![diag(some_code(), "x", 1, 4)];
        assert_ne!(diagnostics_result_id(&a), diagnostics_result_id(&b));
    }

    #[test]
    fn order_is_significant() {
        let d1 = diag(some_code(), "a", 0, 1);
        let d2 = diag(some_code(), "b", 2, 3);
        let ab = vec![d1.clone(), d2.clone()];
        let ba = vec![d2, d1];
        assert_ne!(diagnostics_result_id(&ab), diagnostics_result_id(&ba));
    }

    #[test]
    fn fixes_do_not_affect_id() {
        let mut with_fix = diag(some_code(), "x", 0, 4);
        let baseline = diagnostics_result_id(std::slice::from_ref(&with_fix));
        with_fix.fixes.push(ide::Fix::safe("quick fix", Vec::new()));
        assert_eq!(baseline, diagnostics_result_id(&[with_fix]));
    }
}
