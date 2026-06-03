//! Agent-facing diagnostics tool actions.
//!
//! Diagnostics are the second non-grep-able semantic primitive beside the call
//! `graph`: grep cannot tell unreachable code, a type mismatch, an unresolved call,
//! or an unused variable from ordinary text, but the analyzer can.
//!
//! This module ships the `catalog` action — the static, database-free list of every
//! registered diagnostic code with the metadata an agent needs for cold-start
//! discovery and the request→narrow→request entry point. Rule prose lives here,
//! keyed by `code`, so a later per-file action's findings never repeat it. The
//! `schema` action advertises the contract. Both are computed from compile-time
//! metadata, so no resident analysis database is required.

use std::str::FromStr;

use ide::{catalog_entry, diagnostic_catalog, DiagnosticCode, Locale};
use rmcp::model::CallToolResult;
use serde_json::{json, Value};

use crate::tools::response::structured;

/// The static catalog of diagnostic codes in `locale`, optionally narrowed to
/// `codes`. Unparseable / unknown requested codes are reported back in
/// `unknown_codes` rather than silently dropped, so the agent can correct itself.
pub fn catalog(locale: Locale, codes: &[String]) -> CallToolResult {
    let (entries, unknown): (Vec<_>, Vec<String>) = if codes.is_empty() {
        (diagnostic_catalog(locale), Vec::new())
    } else {
        let mut entries = Vec::with_capacity(codes.len());
        let mut unknown = Vec::new();
        for raw in codes {
            match DiagnosticCode::from_str(raw).ok().and_then(|c| catalog_entry(c, locale)) {
                Some(entry) => entries.push(entry),
                None => unknown.push(raw.clone()),
            }
        }
        (entries, unknown)
    };

    let mut body = json!({
        "action": "catalog",
        "locale": locale_str(locale),
        "count": entries.len(),
        "entries": entries,
    });
    if !unknown.is_empty() {
        body["unknown_codes"] = json!(unknown);
    }
    structured(body)
}

/// Static contract for cold-start discovery, mirroring `graph schema`. `schema_version`
/// is bumped in lockstep with any response-shape change.
pub fn schema() -> CallToolResult {
    structured(schema_json())
}

fn schema_json() -> Value {
    json!({
        "schema_version": "1",
        "actions": ["catalog", "schema"],
        "severities": ["error", "warning", "info", "hint"],
        "catalog_entry": {
            "code": "string — stable diagnostic code (e.g. CyclomaticComplexity)",
            "title": "string — localized name",
            "default_severity": "error | warning | info | hint",
            "type": "error | code_smell | vulnerability | security_hotspot",
            "activated_by_default": "bool — whether enabled under the default config",
            "clean_code_attribute": "consistent | intentional | adaptable | responsible",
            "tags": "string[] — omitted when empty"
        },
        "catalog_params": {
            "codes": "string[] — narrow the catalog to these codes (optional)",
            "locale": "ru | en (default ru) — title language"
        }
    })
}

fn locale_str(locale: Locale) -> &'static str {
    match locale {
        Locale::Ru => "ru",
        Locale::En => "en",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_of(result: &CallToolResult) -> &Value {
        result.structured_content.as_ref().expect("structuredContent must be populated")
    }

    /// The text content block must parse back to exactly the `structuredContent`
    /// field, so structured-aware and plain clients see byte-identical JSON.
    fn assert_structured_mirrors_text(result: &CallToolResult) {
        let structured = body_of(result);
        let text = result.content[0].raw.as_text().expect("text mirror").text.as_str();
        let parsed: Value = serde_json::from_str(text).expect("text mirror must be valid JSON");
        assert_eq!(&parsed, structured, "text mirror must match structuredContent");
    }

    #[test]
    fn schema_advertises_the_catalog_contract() {
        let result = schema();
        assert_structured_mirrors_text(&result);
        let body = body_of(&result);
        assert_eq!(body["schema_version"], "1");
        let actions = body["actions"].as_array().unwrap();
        assert!(actions.iter().any(|a| a == "catalog"));
        let sev = body["severities"].as_array().unwrap();
        assert_eq!(sev.len(), 4);
        assert!(sev.iter().any(|s| s == "error"));
        assert!(sev.iter().any(|s| s == "hint"));
    }

    #[test]
    fn catalog_lists_every_code_with_required_fields() {
        let result = catalog(Locale::Ru, &[]);
        assert_structured_mirrors_text(&result);
        let body = body_of(&result);
        assert_eq!(body["action"], "catalog");
        assert_eq!(body["locale"], "ru");
        let count = body["count"].as_u64().unwrap();
        assert!(count >= 170, "expected the full catalog, got {count}");
        let entries = body["entries"].as_array().unwrap();
        assert_eq!(entries.len() as u64, count);
        let first = &entries[0];
        for field in ["code", "title", "default_severity", "type", "activated_by_default"] {
            assert!(!first[field].is_null(), "entry missing `{field}`");
        }
        assert!(body.get("unknown_codes").is_none(), "no unknown codes for full catalog");
    }

    #[test]
    fn catalog_filters_to_requested_codes() {
        let result = catalog(Locale::En, &["CyclomaticComplexity".to_string()]);
        let body = body_of(&result);
        assert_eq!(body["count"], 1);
        assert_eq!(body["entries"][0]["code"], "CyclomaticComplexity");
        assert!(body.get("unknown_codes").is_none());
    }

    #[test]
    fn catalog_reports_unknown_codes() {
        let result =
            catalog(Locale::Ru, &["CyclomaticComplexity".to_string(), "NoSuchCode".to_string()]);
        let body = body_of(&result);
        assert_eq!(body["count"], 1, "only the valid code yields an entry");
        let unknown = body["unknown_codes"].as_array().unwrap();
        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0], "NoSuchCode");
    }
}
