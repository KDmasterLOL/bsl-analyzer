//! Disk probes for constructed conventional paths.
//!
//! Policy: probe the canonical spelling first — a configurator-written tree
//! hits it and pays nothing extra — and only on a miss list the parent once,
//! matching case-insensitively. The returned path always carries the REAL
//! on-disk spelling, because it flows into `module_file` fields and URIs that
//! must agree with the scanned universe.
//!
//! On a case-insensitive filesystem the exact probe hits for any spelling, so
//! behaviour there is unchanged from the historical `exists()` probes: the
//! constructed spelling is returned. The real-spelling guarantee holds on
//! case-sensitive filesystems, where a wrong-case construction actually
//! misses.

use std::path::{Path, PathBuf};

use crate::tree::{DirTree, RealFs};

/// Find a child whose name is a WHOLLY conventional spelling (`Ext`,
/// `Module.bsl`, `Configuration.xml`): the entire name matches
/// case-insensitively. Never use this for a name built from an object name —
/// that is [`find_child_stem_exact`]'s contract.
pub fn find_child_ci(dir: &Path, conventional: &str) -> Option<PathBuf> {
    find_child_ci_in(&RealFs, dir, conventional)
}

/// [`find_child_ci`] against a given tree.
pub fn find_child_ci_in(tree: &dyn DirTree, dir: &Path, conventional: &str) -> Option<PathBuf> {
    let exact = dir.join(conventional);
    if tree.kind_of(&exact).is_some() {
        return Some(exact);
    }
    for entry in tree.entries(dir) {
        let Some(name) = entry.path.file_name().map(|n| n.to_owned()) else { continue };
        if name.to_str().is_some_and(|n| n.eq_ignore_ascii_case(conventional)) {
            return Some(dir.join(name));
        }
    }
    None
}

/// Find a child built as `{stem}.{ext}` where `stem` is an OBJECT name: the
/// stem must match EXACTLY (an object's case is significant), only the
/// extension is compared case-insensitively. `Alpha.xml` therefore never
/// matches a neighbour's `alpha.xml`.
pub fn find_child_stem_exact(dir: &Path, stem: &str, ext: &str) -> Option<PathBuf> {
    find_child_stem_exact_in(&RealFs, dir, stem, ext)
}

/// [`find_child_stem_exact`] against a given tree.
pub fn find_child_stem_exact_in(
    tree: &dyn DirTree,
    dir: &Path,
    stem: &str,
    ext: &str,
) -> Option<PathBuf> {
    let exact = dir.join(format!("{stem}.{ext}"));
    if tree.kind_of(&exact).is_some() {
        return Some(exact);
    }
    for entry in tree.entries(dir) {
        let Some(name) = entry.path.file_name().map(|n| n.to_owned()) else { continue };
        let path: &Path = name.as_ref();
        let stem_matches = path.file_stem().is_some_and(|s| s == std::ffi::OsStr::new(stem));
        let ext_matches =
            path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(ext));
        if stem_matches && ext_matches {
            return Some(dir.join(name));
        }
    }
    None
}

/// Resolve a chain of wholly conventional components (`["Ext", "Module.bsl"]`)
/// under `dir`, each level by [`find_child_ci`]. A single-level listing cannot
/// survive `EXT/MODULE.BSL` — when the joined probe misses, every component
/// may be misspelled, so each is resolved in turn.
pub fn resolve_chain_ci(dir: &Path, components: &[&str]) -> Option<PathBuf> {
    resolve_chain_ci_in(&RealFs, dir, components)
}

/// [`resolve_chain_ci`] against a given tree.
pub fn resolve_chain_ci_in(tree: &dyn DirTree, dir: &Path, components: &[&str]) -> Option<PathBuf> {
    let mut current = dir.to_path_buf();
    for component in components {
        current = find_child_ci_in(tree, &current, component)?;
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }

    #[test]
    fn an_exact_probe_hits_without_listing() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("Module.bsl"));
        let found = find_child_ci(dir.path(), "Module.bsl").unwrap();
        assert!(found.ends_with("Module.bsl"));
    }

    /// На регистронезависимой ФС точная проба попадает при любом написании и
    /// листинг не выполняется — там возвращается сконструированное написание,
    /// как и до этого хелпера (граница узла).
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    #[test]
    fn a_case_variant_is_found_by_listing_and_keeps_its_real_spelling() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("Module.BSL"));
        let found = find_child_ci(dir.path(), "Module.bsl").unwrap();
        assert_eq!(found.file_name().unwrap(), "Module.BSL", "real spelling, not the probe's");
    }

    #[test]
    fn an_absent_child_is_none() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("Other.bsl"));
        assert_eq!(find_child_ci(dir.path(), "Module.bsl"), None);
    }

    #[test]
    fn a_stem_probe_takes_a_case_variant_extension() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("Товар.XML"));
        let found = find_child_stem_exact(dir.path(), "Товар", "xml").unwrap();
        assert_eq!(found.file_name().unwrap(), "Товар.XML");
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    #[test]
    fn a_stem_probe_never_takes_a_case_variant_stem() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("alpha.xml"));
        assert_eq!(
            find_child_stem_exact(dir.path(), "Alpha", "xml"),
            None,
            "an object name's case is significant: alpha.xml is another object"
        );
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    #[test]
    fn a_chain_resolves_each_component_by_its_own_listing() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("EXT/MODULE.BSL"));
        let found = resolve_chain_ci(dir.path(), &["Ext", "Module.bsl"]).unwrap();
        assert!(found.ends_with("EXT/MODULE.BSL"), "{}", found.display());
    }

    #[test]
    fn a_chain_with_a_missing_link_is_none() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("EXT")).unwrap();
        assert_eq!(resolve_chain_ci(dir.path(), &["Ext", "Module.bsl"]), None);
    }
}
