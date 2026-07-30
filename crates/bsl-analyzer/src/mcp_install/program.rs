//! Resolving an external CLI name to a file that Windows can actually start.
//!
//! `CreateProcess` does not apply `PATHEXT`, so a bare `claude` never matches the
//! `claude.cmd` shim npm installs. The candidate list is built platform-independently
//! so it stays under test on non-Windows CI, where the Windows binaries are only
//! cross-compiled.

use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
};

/// Used when `PATHEXT` is unset or empty; mirrors the Windows built-in default.
const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

/// The program to hand to `Command::new`. Everything but the platform check is compiled
/// on every target — a `cfg`-gated body would leave the lookup untested on the Linux CI
/// that builds the Windows artifacts. Falls back to `program` itself whenever no
/// candidate exists, so the OS keeps its own, wider search (process directory, current
/// directory, system directories) and the not-found error still comes from the spawn.
pub(super) fn resolve_program(program: &str) -> OsString {
    if !cfg!(windows) {
        return OsString::from(program);
    }

    let dirs: Vec<PathBuf> = env::split_paths(&env::var_os("PATH").unwrap_or_default()).collect();
    let pathext = env::var("PATHEXT").unwrap_or_default();

    candidates(program, &dirs, &pathext)
        .into_iter()
        .find(|candidate| candidate.is_file())
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from(program))
}

/// Paths to try for a bare `program`, in Windows lookup order: every extension is tried
/// within a directory before moving to the next one. Empty when the name needs no
/// completion — it already carries a known extension, or it is a path rather than a name.
fn candidates(program: &str, path_dirs: &[PathBuf], pathext: &str) -> Vec<PathBuf> {
    let extensions = split_pathext(pathext);

    if program.is_empty() || is_path(program) || has_known_extension(program, &extensions) {
        return Vec::new();
    }

    path_dirs
        .iter()
        .filter(|dir| !dir.as_os_str().is_empty())
        .flat_map(|dir| {
            extensions.iter().map(move |extension| dir.join(format!("{program}{extension}")))
        })
        .collect()
}

fn split_pathext(pathext: &str) -> Vec<String> {
    let extensions: Vec<String> = pathext
        .split(';')
        .map(str::trim)
        .filter(|extension| extension.starts_with('.') && extension.len() > 1)
        .map(str::to_owned)
        .collect();

    if extensions.is_empty() {
        return split_pathext(DEFAULT_PATHEXT);
    }

    extensions
}

fn is_path(program: &str) -> bool {
    program.contains(['/', '\\']) || Path::new(program).is_absolute()
}

/// Windows compares extensions case-insensitively, so `claude.CMD` must count as already
/// resolved even though `PATHEXT` spells the entry differently.
fn has_known_extension(program: &str, extensions: &[String]) -> bool {
    let Some(actual) = Path::new(program).extension().and_then(|extension| extension.to_str())
    else {
        return false;
    };

    extensions.iter().any(|candidate| candidate[1..].eq_ignore_ascii_case(actual))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::candidates;

    const PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

    fn dirs() -> Vec<PathBuf> {
        vec![PathBuf::from("/npm"), PathBuf::from("/tools")]
    }

    fn rendered(program: &str, pathext: &str) -> Vec<String> {
        candidates(program, &dirs(), pathext)
            .into_iter()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect()
    }

    #[test]
    fn tries_every_extension_within_a_directory_before_the_next_one() {
        assert_eq!(
            rendered("claude", PATHEXT),
            vec![
                "/npm/claude.COM",
                "/npm/claude.EXE",
                "/npm/claude.BAT",
                "/npm/claude.CMD",
                "/tools/claude.COM",
                "/tools/claude.EXE",
                "/tools/claude.BAT",
                "/tools/claude.CMD",
            ]
        );
    }

    #[test]
    fn keeps_the_pathext_order_of_the_environment() {
        assert_eq!(
            rendered("codex", ".CMD;.EXE"),
            vec!["/npm/codex.CMD", "/npm/codex.EXE", "/tools/codex.CMD", "/tools/codex.EXE"]
        );
    }

    #[test]
    fn falls_back_to_the_windows_default_when_pathext_is_unusable() {
        assert_eq!(rendered("claude", ""), rendered("claude", PATHEXT));
        assert_eq!(rendered("claude", ";exe; ;"), rendered("claude", PATHEXT));
    }

    #[test]
    fn skips_names_that_already_carry_a_known_extension() {
        assert!(rendered("claude.cmd", PATHEXT).is_empty());
        assert!(rendered("claude.CMD", PATHEXT).is_empty());
        assert!(rendered("claude.exe", PATHEXT).is_empty());
    }

    #[test]
    fn completes_names_whose_extension_is_not_executable() {
        assert_eq!(
            rendered("claude.next", ".EXE"),
            vec!["/npm/claude.next.EXE", "/tools/claude.next.EXE"]
        );
    }

    #[test]
    fn leaves_paths_to_the_operating_system() {
        assert!(rendered("C:\\npm\\claude", PATHEXT).is_empty());
        assert!(rendered("./claude", PATHEXT).is_empty());
        assert!(rendered("/usr/bin/claude", PATHEXT).is_empty());
    }

    #[test]
    fn ignores_empty_path_entries() {
        let dirs = vec![PathBuf::new(), PathBuf::from("/npm")];
        assert_eq!(
            candidates("claude", &dirs, ".EXE"),
            vec![PathBuf::from("/npm").join("claude.EXE")]
        );
    }

    #[test]
    fn resolves_nothing_without_a_program_name() {
        assert!(rendered("", PATHEXT).is_empty());
    }
}
