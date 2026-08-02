//! The completeness gate: no inline conventional-name literal outside the
//! dictionary crate.
//!
//! The scan is STRUCTURAL — string literals shaped like conventional names
//! (`*.bsl`/`*.xml`/`*.bin` suffixes, bare extension literals, bare
//! `…Module` stems, service segment names) — rather than dictionary-driven,
//! so a name missing from the dictionary still lands in the findings instead
//! of hiding behind its own omission. Every finding must be covered by an
//! explicit allowlist row (`gate_allowlist.tsv`: path, literal, count,
//! cause); an uncovered finding, a count drift or a stale row each fail the
//! test. Rows exist for exactly three causes: constructing a canonical
//! spelling, a site outside the class (XML element tags, analyzer config
//! names), or a comparison not yet routed through the dictionary — the last
//! kind is the migration's shrinking frontier.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Normal,
    LineComment,
    BlockComment(u32),
}

/// A string literal's inner text plus its byte span in the file.
struct Literal {
    text: String,
    start: usize,
}

/// Lex one source file: collect string literals and a masked copy where
/// comments and literal contents are blanked, so brace/attribute scanning
/// cannot be confused by code-shaped text inside them.
fn lex(source: &str) -> (Vec<Literal>, String) {
    let bytes = source.as_bytes();
    let mut masked = bytes.to_vec();
    let mut literals = Vec::new();
    let mut state = State::Normal;
    let mut i = 0;

    while i < bytes.len() {
        match state {
            State::LineComment => {
                if bytes[i] == b'\n' {
                    state = State::Normal;
                } else {
                    masked[i] = b' ';
                }
                i += 1;
            }
            State::BlockComment(depth) => {
                if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    masked[i] = b' ';
                    masked[i + 1] = b' ';
                    i += 2;
                    state = if depth == 1 { State::Normal } else { State::BlockComment(depth - 1) };
                } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    masked[i] = b' ';
                    masked[i + 1] = b' ';
                    i += 2;
                    state = State::BlockComment(depth + 1);
                } else {
                    if bytes[i] != b'\n' {
                        masked[i] = b' ';
                    }
                    i += 1;
                }
            }
            State::Normal => {
                let prev_is_ident =
                    i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
                if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
                    state = State::LineComment;
                    masked[i] = b' ';
                    i += 1;
                } else if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    state = State::BlockComment(1);
                    masked[i] = b' ';
                    i += 1;
                } else if !prev_is_ident && raw_string_open(bytes, i).is_some() {
                    let (content_start, hashes) = raw_string_open(bytes, i).unwrap();
                    let end = raw_string_end(bytes, content_start, hashes);
                    for m in &mut masked[content_start..end] {
                        if *m != b'\n' {
                            *m = b' ';
                        }
                    }
                    literals.push(Literal {
                        text: source[content_start..end].to_string(),
                        start: content_start,
                    });
                    i = (end + 1 + hashes).min(bytes.len());
                } else if bytes[i] == b'"' {
                    let content_start = i + 1;
                    let end = plain_string_end(bytes, content_start);
                    for m in &mut masked[content_start..end] {
                        if *m != b'\n' {
                            *m = b' ';
                        }
                    }
                    literals.push(Literal {
                        text: source[content_start..end].to_string(),
                        start: content_start,
                    });
                    i = (end + 1).min(bytes.len());
                } else if bytes[i] == b'\'' {
                    i = skip_char_or_lifetime(bytes, i, &mut masked);
                } else {
                    i += 1;
                }
            }
        }
    }

    let masked = String::from_utf8_lossy(&masked).into_owned();
    (literals, masked)
}

fn raw_string_open(bytes: &[u8], i: usize) -> Option<(usize, usize)> {
    let mut j = i;
    if matches!(bytes.get(j), Some(b'b') | Some(b'c')) {
        j += 1;
    }
    if bytes.get(j) != Some(&b'r') {
        return None;
    }
    j += 1;
    let mut hashes = 0;
    while bytes.get(j) == Some(&b'#') {
        hashes += 1;
        j += 1;
    }
    (bytes.get(j) == Some(&b'"')).then_some((j + 1, hashes))
}

fn raw_string_end(bytes: &[u8], content_start: usize, hashes: usize) -> usize {
    let mut i = content_start;
    while i < bytes.len() {
        if bytes[i] == b'"'
            && bytes[i + 1..].len() >= hashes
            && bytes[i + 1..i + 1 + hashes].iter().all(|&b| b == b'#')
        {
            return i;
        }
        i += 1;
    }
    bytes.len()
}

fn plain_string_end(bytes: &[u8], content_start: usize) -> usize {
    let mut i = content_start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return i,
            _ => i += 1,
        }
    }
    bytes.len()
}

/// Advance past a char literal, or past a bare lifetime tick. Char contents
/// are masked so a `'{'` cannot unbalance the brace scan.
fn skip_char_or_lifetime(bytes: &[u8], i: usize, masked: &mut [u8]) -> usize {
    match bytes.get(i + 1) {
        Some(b'\\') => {
            let mut j = i + 3;
            while j < bytes.len() && bytes[j] != b'\'' {
                j += 1;
            }
            for m in masked.iter_mut().take(j).skip(i + 1) {
                *m = b' ';
            }
            j + 1
        }
        Some(&c) if bytes.get(i + 2) == Some(&b'\'') => {
            let _ = c;
            masked[i + 1] = b' ';
            i + 3
        }
        _ => i + 1,
    }
}

/// Byte ranges of items behind a test-only `#[cfg(...)]`, found on the masked
/// text: attribute, any stacked attributes after it, then either a braced
/// body to its matching brace or a semicolon item.
fn test_regions(masked: &str) -> Vec<(usize, usize)> {
    let bytes = masked.as_bytes();
    let mut regions = Vec::new();
    let mut search_from = 0;
    while let Some(found) = masked[search_from..].find("#[") {
        let attr_start = search_from + found;
        let Some((attr_text, attr_end)) = balanced(bytes, attr_start + 1, b'[', b']') else {
            search_from = attr_start + 2;
            continue;
        };
        let attr_text = &masked[attr_text.0..attr_text.1];
        if !is_test_only_cfg(attr_text) {
            search_from = attr_end;
            continue;
        }
        let mut cursor = attr_end;
        loop {
            while bytes.get(cursor).is_some_and(|b| b.is_ascii_whitespace()) {
                cursor += 1;
            }
            if bytes.get(cursor) == Some(&b'#') && bytes.get(cursor + 1) == Some(&b'[') {
                match balanced(bytes, cursor + 1, b'[', b']') {
                    Some((_, end)) => cursor = end,
                    None => break,
                }
            } else {
                break;
            }
        }
        let mut depth = 0i32;
        let mut end = cursor;
        while end < bytes.len() {
            match bytes[end] {
                b'{' if depth == 0 => {
                    end = balanced(bytes, end, b'{', b'}').map(|(_, e)| e).unwrap_or(bytes.len());
                    break;
                }
                b';' if depth == 0 => {
                    end += 1;
                    break;
                }
                b'(' | b'[' => depth += 1,
                b')' | b']' => depth -= 1,
                _ => {}
            }
            end += 1;
        }
        regions.push((attr_start, end));
        search_from = end.max(attr_end);
    }
    regions
}

/// From an opener at `open_at`, the inner span and the index just past the
/// matching closer.
fn balanced(bytes: &[u8], open_at: usize, open: u8, close: u8) -> Option<((usize, usize), usize)> {
    debug_assert_eq!(bytes[open_at], open);
    let mut depth = 0;
    for (offset, &b) in bytes[open_at..].iter().enumerate() {
        if b == open {
            depth += 1;
        } else if b == close {
            depth -= 1;
            if depth == 0 {
                let end = open_at + offset;
                return Some(((open_at + 1, end), end + 1));
            }
        }
    }
    None
}

/// Whether this attribute makes its item TEST-ONLY: `cfg(test)` or
/// `cfg(all(test, …))`. `cfg(not(test))`, `cfg(any(test, …))` and
/// `cfg_attr(test, …)` all leave the item in production builds, so their
/// regions must stay in the scan.
fn is_test_only_cfg(attr_text: &str) -> bool {
    let normalized: String = attr_text.chars().filter(|c| !c.is_whitespace()).collect();
    normalized == "cfg(test)"
        || (normalized.starts_with("cfg(all(") && has_word(&normalized, "test"))
}

fn has_word(text: &str, word: &str) -> bool {
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(found) = text[from..].find(word) {
        let start = from + found;
        let end = start + word.len();
        let left_ok =
            start == 0 || !(bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
        let right_ok =
            end == bytes.len() || !(bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_');
        if left_ok && right_ok {
            return true;
        }
        from = end;
    }
    false
}

/// Is this literal shaped like a conventional name?
///
/// Every check is case-insensitive: the ci-comparison family spells its
/// literals in whatever case it likes (`eq_ignore_ascii_case("objectmodule")`),
/// so an exact-case scan would be blind to precisely the comparisons this gate
/// polices. Unrelated same-spelling words (graph node kinds `form`, `module`)
/// land in the allowlist as out-of-class rows.
fn is_conventional_shape(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.ends_with(".bsl") || lower.ends_with(".xml") || lower.ends_with(".bin") {
        return true;
    }
    if matches!(lower.as_str(), "bsl" | "xml" | "bin") {
        return true;
    }
    if matches!(lower.as_str(), "ext" | "form" | "forms" | "commands") {
        return true;
    }
    // Bare module stems the way the debugger compares them after
    // `file_stem()`: alphabetic, `…Module` in any ASCII case — a ci comparison
    // spells the literal however it likes. The bare word `module` itself is a
    // graph node kind, not a stem, hence the length cut plus the one canonical
    // spelling the debugger's table actually uses.
    if text == "Module"
        || (lower.ends_with("module")
            && lower.len() > "module".len()
            && text.bytes().all(|b| b.is_ascii_alphabetic()))
    {
        return true;
    }
    false
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

fn scanned_files(root: &Path) -> Vec<PathBuf> {
    let crates_dir = root.join("crates");
    let mut files = Vec::new();
    for crate_entry in fs::read_dir(&crates_dir).unwrap().flatten() {
        let crate_dir = crate_entry.path();
        // The dictionary and this gate legitimately spell every conventional
        // name, and the two fixture crates exist only for tests — none of the
        // three is production surface.
        let skip = crate_dir
            .file_name()
            .is_some_and(|n| n == "bsl-conventions" || n == "test-fixture" || n == "test-utils");
        if skip {
            continue;
        }
        for sub in ["src", "examples"] {
            let dir = crate_dir.join(sub);
            if !dir.is_dir() {
                continue;
            }
            for entry in walkdir::WalkDir::new(&dir).into_iter().flatten() {
                if entry.file_type().is_file()
                    && entry.path().extension().is_some_and(|e| e == "rs")
                {
                    files.push(entry.path().to_path_buf());
                }
            }
        }
        // Build scripts are production targets of their crate.
        let build_rs = crate_dir.join("build.rs");
        if build_rs.is_file() {
            files.push(build_rs);
        }
    }
    // So is the workspace's own task runner.
    let xtask_src = root.join("xtask/src");
    if xtask_src.is_dir() {
        for entry in walkdir::WalkDir::new(&xtask_src).into_iter().flatten() {
            if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "rs") {
                files.push(entry.path().to_path_buf());
            }
        }
    }
    files.sort();
    files
}

/// Test code that lives in its own file: the workspace convention names such
/// files `tests.rs` / `*_tests.rs` / `test_support.rs` / `test_utils.rs` or
/// puts them under a `tests/` directory inside `src`. Their `#[cfg(test)]`
/// lives on the DECLARATION (often behind `#[path]`), not in the file, so
/// the region scan cannot see it.
fn is_test_named(file: &Path) -> bool {
    let name_is_test = file.file_stem().and_then(|s| s.to_str()).is_some_and(|s| {
        s == "tests" || s == "test_support" || s == "test_utils" || s.ends_with("_tests")
    });
    name_is_test || file.components().any(|c| c.as_os_str() == "tests")
}

/// Module names declared as `#[cfg(test)] mod name;` inside the given test
/// regions: their bodies live in sibling files that are compiled only under
/// test, so those files must leave the scan entirely.
fn test_module_names(source: &str, masked: &str, regions: &[(usize, usize)]) -> Vec<String> {
    let mut names = Vec::new();
    for &(start, end) in regions {
        let region = &masked[start..end.min(masked.len())];
        if region.contains('{') {
            continue;
        }
        let Some(found) = region.find("mod ") else { continue };
        let rest = &region[found + 4..];
        let name: String =
            rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if name.is_empty() || !rest[name.len()..].trim_start().starts_with(';') {
            continue;
        }
        // A `#[path = "…"]` on the declaration points the module at an
        // arbitrary file; the masked text blanks string contents, so the
        // spelling comes from the ORIGINAL source at the same offsets.
        let region_src = &source[start..end.min(source.len())];
        if let Some(p) = region_src.find("path") {
            let after = &region_src[p..];
            if let (Some(q1), Some(_)) = (after.find('"'), after.find('=')) {
                if let Some(q2) = after[q1 + 1..].find('"') {
                    let target = &after[q1 + 1..q1 + 1 + q2];
                    if let Some(stem) = target.strip_suffix(".rs") {
                        names.push(stem.to_string());
                    }
                }
            }
        }
        names.push(name);
    }
    names
}

type Findings = BTreeMap<(String, String), usize>;

fn collect_findings(root: &Path) -> Findings {
    let mut lexed = Vec::new();
    let mut excluded_files: Vec<PathBuf> = Vec::new();
    for file in scanned_files(root) {
        if is_test_named(&file) {
            continue;
        }
        let Ok(source) = fs::read_to_string(&file) else { continue };
        let (literals, masked) = lex(&source);
        let regions = test_regions(&masked);
        // `mod x;` in `foo.rs` resolves into `foo/x.rs`, in `mod.rs`/`lib.rs`/
        // `main.rs` into a sibling; probe both spellings — a miss just adds a
        // path that never matches.
        let dir = file.parent().unwrap();
        let stem_dir = file.file_stem().map(|s| dir.join(s));
        for name in test_module_names(&source, &masked, &regions) {
            excluded_files.push(dir.join(format!("{name}.rs")));
            excluded_files.push(dir.join(&name));
            if let Some(stem_dir) = &stem_dir {
                excluded_files.push(stem_dir.join(format!("{name}.rs")));
                excluded_files.push(stem_dir.join(&name));
            }
        }
        lexed.push((file, literals, regions));
    }

    let mut findings: Findings = BTreeMap::new();
    for (file, literals, regions) in lexed {
        if excluded_files.iter().any(|ex| file == *ex || file.starts_with(ex)) {
            continue;
        }
        let rel = file.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        for literal in literals {
            if regions.iter().any(|&(s, e)| literal.start >= s && literal.start < e) {
                continue;
            }
            if is_conventional_shape(&literal.text) {
                *findings.entry((rel.clone(), literal.text)).or_default() += 1;
            }
        }
    }
    findings
}

struct AllowRow {
    count: usize,
    #[allow(dead_code, reason = "прочитано ради самодокументации строки; проверяется только счёт")]
    cause: String,
}

fn load_allowlist(path: &Path) -> BTreeMap<(String, String), AllowRow> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut rows = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split('\t');
        let (Some(file), Some(literal), Some(count), Some(cause)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            panic!("строка allowlist не из четырёх колонок: {line:?}");
        };
        let count: usize = count.parse().unwrap_or_else(|_| panic!("не число: {line:?}"));
        rows.insert(
            (file.to_string(), literal.to_string()),
            AllowRow { count, cause: cause.to_string() },
        );
    }
    rows
}

#[test]
fn no_conventional_name_literal_lives_outside_the_dictionary_unaccounted() {
    let root = workspace_root();
    let findings = collect_findings(&root);
    let allowlist_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/gate_allowlist.tsv");
    let allowlist = load_allowlist(&allowlist_path);

    let mut missing = Vec::new();
    let mut drifted = Vec::new();
    for ((file, literal), &count) in &findings {
        match allowlist.get(&(file.clone(), literal.clone())) {
            None => missing.push(format!("{file}\t{literal}\t{count}\tсравнение")),
            Some(row) if row.count != count => {
                drifted
                    .push(format!("{file}\t{literal}: в allowlist {}, найдено {count}", row.count));
            }
            Some(_) => {}
        }
    }
    let stale: Vec<String> = allowlist
        .keys()
        .filter(|key| !findings.contains_key(*key))
        .map(|(file, literal)| format!("{file}\t{literal}"))
        .collect();

    assert!(
        missing.is_empty() && drifted.is_empty() && stale.is_empty(),
        "гейт словаря разошёлся с кодом.\n\
         \nНе учтено в allowlist (готовые строки, причину назначить осознанно):\n{}\
         \nСчёт разошёлся:\n{}\
         \nПротухшие строки allowlist:\n{}\n",
        missing.join("\n"),
        drifted.join("\n"),
        stale.join("\n"),
    );
}

/// The gate must be able to fail: a synthetic source with one inline
/// comparison of each structural shape yields findings.
#[test]
fn the_scanner_sees_every_structural_shape() {
    let source = r#"
fn f(p: &std::path::Path) -> bool {
    let a = p.extension().is_some_and(|e| e == "bsl");
    let b = p.ends_with("Ext/Module.bsl");
    let c = p.file_stem().is_some_and(|s| s == "ObjectModule");
    let d = p.file_name().is_some_and(|n| n == "Configuration.xml");
    let e = p.file_name().is_some_and(|n| n == "Ext");
    a || b || c || d || e
}
#[cfg(test)]
mod tests {
    fn hidden() -> &'static str { "SessionModule.bsl" }
}
"#;
    let (literals, masked) = lex(source);
    let excluded = test_regions(&masked);
    let visible: Vec<&str> = literals
        .iter()
        .filter(|l| !excluded.iter().any(|&(s, e)| l.start >= s && l.start < e))
        .filter(|l| is_conventional_shape(&l.text))
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(
        visible,
        ["bsl", "Ext/Module.bsl", "ObjectModule", "Configuration.xml", "Ext"],
        "все пять форм видимы, а литерал под #[cfg(test)] исключён"
    );
}
