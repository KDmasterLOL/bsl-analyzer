//! Discovery scanner for platform-help deprecation markers.
//!
//! Run: `cargo run -p bsl-platform --example deprecation_help_discovery`
//!
//! The 1C context help is neither a complete nor a clean source of platform
//! deprecation facts: most curated deprecations come from 1C development
//! standards (which the help does not mention), and the prose that *does*
//! mention "устаревш…" is noisy — it also talks about stale index state,
//! legacy property *values*, and message expiration dates. This tool is a
//! discovery aid for human review, not an automated ingestion path.
//!
//! It scans `documentation.description`/`notes` of every method, global
//! function, and property, keeps only entries whose prose marks the element
//! itself as deprecated (positive phrases), drops the known false-positive
//! shapes (negative phrases), and cross-references the curated
//! `bsl_platform::deprecation` registry so genuinely missing facts stand out.
//!
//! See `crates/bsl-platform/data/PROVENANCE.md` for the help's provenance.

use std::collections::HashSet;
use std::path::PathBuf;

use bsl_platform::deprecation;
use serde_json::Value;

/// Prose that marks *the documented element* as deprecated.
const POSITIVE: &[&str] = &[
    "признан устаревш",
    "признано устаревш",
    "признана устаревш",
    "является устаревш",
    "являются устаревш",
    "устаревает с верси",
    "устарел с верси",
    "устарела с верси",
    "метод устарел",
    "функция устарел",
    "свойство устарел",
];

/// Prose where "устаревш…" refers to something other than this element
/// (a legacy value, a stale state, an expiration date).
const NEGATIVE: &[&str] = &[
    "устаревшему значени",
    "устаревшим значени",
    "устаревшее значени",
    "устаревшего значени",
    "устаревших значени",
    "устаревшее состояни",
    "устаревшим состояни",
    "скорее всего, устаревш",
    "сообщение устарел",
];

const STEM: &str = "устар";

struct Row {
    kind: &'static str,
    owner: String,
    name: String,
    en: String,
    snippet: String,
}

fn main() {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "data", "platform_data.json"].iter().collect();
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let data: Value = serde_json::from_str(&raw).expect("platform_data.json is not valid JSON");

    let registry = registry_name_set();

    let mut strong_missing: Vec<Row> = Vec::new();
    let mut strong_known: Vec<Row> = Vec::new();
    let mut weak: Vec<Row> = Vec::new();
    let mut filtered = 0usize;
    let mut scanned = 0usize;

    for (kind, key) in
        [("global fn", "global_functions"), ("method", "methods"), ("property", "properties")]
    {
        let Some(arr) = data.get(key).and_then(Value::as_array) else { continue };
        for it in arr {
            scanned += 1;
            let (desc, notes) = documentation(it);
            let flat = collapse(&format!("{desc} {notes}"));
            let lower = flat.to_lowercase();
            if !lower.contains(STEM) {
                continue;
            }

            let positive = POSITIVE.iter().any(|p| lower.contains(p));
            let negative = NEGATIVE.iter().any(|p| lower.contains(p));
            if !positive && negative {
                filtered += 1;
                continue;
            }

            let name = str_field(it, "name");
            let en = str_field(it, "english_name");
            let in_registry = registry.contains(&name.to_lowercase())
                || (!en.is_empty() && registry.contains(&en.to_lowercase()));
            let row =
                Row { kind, owner: str_field(it, "type_name"), name, en, snippet: snippet(&flat) };

            match (positive, in_registry) {
                (true, false) => strong_missing.push(row),
                (true, true) => strong_known.push(row),
                (false, _) => weak.push(row),
            }
        }
    }

    print_section(
        "STRONG candidates MISSING from the deprecation registry (review → add)",
        &strong_missing,
    );
    print_section("STRONG candidates already covered by the registry", &strong_known);
    print_section("WEAK candidates (stem present, no clear marker — human review)", &weak);

    println!("\n──────────────────────────────────────────────");
    println!("scanned elements with prose : {scanned}");
    println!("filtered false positives    : {filtered}");
    println!("strong / missing            : {}", strong_missing.len());
    println!("strong / already in registry: {}", strong_known.len());
    println!("weak / needs review         : {}", weak.len());
    println!("registry entries (baseline) : {}", deprecation::registry().entries().len());
}

fn registry_name_set() -> HashSet<String> {
    let mut set = HashSet::new();
    for entry in deprecation::registry().entries() {
        if !entry.ru.is_empty() {
            set.insert(entry.ru.to_lowercase());
        }
        if !entry.en.is_empty() {
            set.insert(entry.en.to_lowercase());
        }
    }
    set
}

fn documentation(item: &Value) -> (String, String) {
    let Some(doc) = item.get("documentation").and_then(Value::as_object) else {
        return (String::new(), String::new());
    };
    let pick = |key: &str| doc.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    (pick("description"), pick("notes"))
}

fn str_field(item: &Value, field: &str) -> String {
    item.get(field).and_then(Value::as_str).unwrap_or("").to_string()
}

/// Collapse all runs of whitespace into single spaces.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A short, whitespace-collapsed window around the first deprecation stem.
fn snippet(flat: &str) -> String {
    let chars: Vec<char> = flat.chars().collect();
    let lower: Vec<char> = chars.iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect();
    let needle: Vec<char> = STEM.chars().collect();
    let start = lower.windows(needle.len()).position(|w| w == needle.as_slice()).unwrap_or(0);

    let from = start.saturating_sub(50);
    let to = (start + 120).min(chars.len());
    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    out.extend(&chars[from..to]);
    if to < chars.len() {
        out.push('…');
    }
    out
}

fn print_section(title: &str, rows: &[Row]) {
    println!("\n=== {title}: {} ===", rows.len());
    for row in rows {
        let qualified = if row.owner.is_empty() {
            row.name.clone()
        } else {
            format!("{}.{}", row.owner, row.name)
        };
        println!("  [{}] {qualified} [{}]", row.kind, row.en);
        println!("       {}", row.snippet);
    }
}
