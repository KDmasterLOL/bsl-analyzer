//! Static, database-free projection of the diagnostic catalog and the agent-facing
//! 4-bucket severity collapse.
//!
//! The catalog is the cold-start discovery surface for diagnostic tooling: every
//! registered [`DiagnosticCode`] with its title, default severity, type, and the
//! metadata an agent needs to decide whether a finding matters — all sourced from
//! the compile-time [`DiagnosticMetadata`] and embedded docs, so no Salsa database
//! is required.
//!
//! [`SeverityBucket`] lives here (not in `mcp-server`) so the 7→4 collapse is shared
//! with the LSP severity surface (`to_proto::severity`) instead of duplicated: both
//! map the same internal [`Severity`] onto the same four observable grades.

use ide_db::base_db::Locale;
use serde::Serialize;

use crate::{
    all_diagnostic_codes, docs, get_metadata, CleanCodeAttribute, DiagnosticType, MetadataTag,
    Severity,
};

/// Agent-facing severity, collapsing the internal 7-grade [`Severity`] into the four
/// buckets the editor already sees through `to_proto::severity`. Four semantic labels
/// are easier for an LLM to reason over than seven near-synonyms, and they map 1:1
/// onto the LSP `DiagnosticSeverity` shown in the UI.
///
/// Ordered ascending (`Hint < Info < Warning < Error`) so `min_severity` can be used
/// as an inclusive floor via the derived `Ord`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SeverityBucket {
    Hint,
    Info,
    Warning,
    Error,
}

impl SeverityBucket {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hint => "hint",
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    /// Parse a `min_severity` floor label; `None` for anything outside the vocabulary.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "hint" => Some(Self::Hint),
            "info" => Some(Self::Info),
            "warning" => Some(Self::Warning),
            "error" => Some(Self::Error),
            _ => None,
        }
    }
}

impl From<Severity> for SeverityBucket {
    fn from(s: Severity) -> Self {
        match s {
            // Mirrors `to_proto::severity`: the four error-grades collapse to `error`.
            Severity::Blocker | Severity::Critical | Severity::Major | Severity::Error => {
                Self::Error
            }
            Severity::Warning => Self::Warning,
            Severity::Information => Self::Info,
            Severity::Hint => Self::Hint,
        }
    }
}

/// One catalog row: a registered diagnostic with the metadata an agent needs to
/// decide relevance, sourced entirely from compile-time data (no database).
#[derive(Debug, Clone, Serialize)]
pub struct CatalogEntry {
    pub code: &'static str,
    pub title: String,
    pub default_severity: SeverityBucket,
    /// `error | code_smell | vulnerability | security_hotspot`.
    pub r#type: &'static str,
    pub activated_by_default: bool,
    pub clean_code_attribute: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<&'static str>,
}

/// Build the full diagnostic catalog in the requested locale. ~189 entries, all from
/// embedded metadata and docs — cheap enough to materialise per request, no caching.
pub fn diagnostic_catalog(locale: Locale) -> Vec<CatalogEntry> {
    all_diagnostic_codes().filter_map(|code| catalog_entry(code, locale)).collect()
}

/// One catalog row for a single code, or `None` when the code carries no registered
/// metadata (every shipping diagnostic has metadata; this guards generated codes).
pub fn catalog_entry(code: crate::DiagnosticCode, locale: Locale) -> Option<CatalogEntry> {
    let meta = get_metadata(code)?;
    let doc = docs::get_docs(code);
    let name = match locale {
        Locale::Ru => doc.name_ru,
        Locale::En => doc.name_en,
    };
    let title = if name.is_empty() { code.as_str().to_string() } else { name.to_string() };
    Some(CatalogEntry {
        code: code.as_str(),
        title,
        // Default config applies no override, so the emitted severity equals the
        // metadata's computed grade (`EffectiveMetadata::severity_value`).
        default_severity: meta.calculate_severity().into(),
        r#type: diagnostic_type_str(meta.diagnostic_type),
        activated_by_default: meta.activated_by_default,
        clean_code_attribute: clean_code_attribute_str(meta.clean_code_attribute),
        tags: meta.tags.iter().map(|t| metadata_tag_str(*t)).collect(),
    })
}

fn diagnostic_type_str(ty: DiagnosticType) -> &'static str {
    match ty {
        DiagnosticType::Error => "error",
        DiagnosticType::CodeSmell => "code_smell",
        DiagnosticType::Vulnerability => "vulnerability",
        DiagnosticType::SecurityHotspot => "security_hotspot",
    }
}

fn clean_code_attribute_str(attr: CleanCodeAttribute) -> &'static str {
    match attr {
        CleanCodeAttribute::Consistent => "consistent",
        CleanCodeAttribute::Intentional => "intentional",
        CleanCodeAttribute::Adaptable => "adaptable",
        CleanCodeAttribute::Responsible => "responsible",
    }
}

fn metadata_tag_str(tag: MetadataTag) -> &'static str {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_bucket_collapses_seven_into_four() {
        assert_eq!(SeverityBucket::from(Severity::Blocker), SeverityBucket::Error);
        assert_eq!(SeverityBucket::from(Severity::Critical), SeverityBucket::Error);
        assert_eq!(SeverityBucket::from(Severity::Major), SeverityBucket::Error);
        assert_eq!(SeverityBucket::from(Severity::Error), SeverityBucket::Error);
        assert_eq!(SeverityBucket::from(Severity::Warning), SeverityBucket::Warning);
        assert_eq!(SeverityBucket::from(Severity::Information), SeverityBucket::Info);
        assert_eq!(SeverityBucket::from(Severity::Hint), SeverityBucket::Hint);
    }

    #[test]
    fn severity_bucket_orders_as_an_inclusive_floor() {
        assert!(SeverityBucket::Hint < SeverityBucket::Info);
        assert!(SeverityBucket::Info < SeverityBucket::Warning);
        assert!(SeverityBucket::Warning < SeverityBucket::Error);
        // `min_severity = warning` keeps warning and error, drops info and hint.
        let floor = SeverityBucket::Warning;
        assert!(SeverityBucket::Error >= floor);
        assert!(SeverityBucket::Warning >= floor);
        assert!(SeverityBucket::Info < floor);
    }

    #[test]
    fn severity_bucket_parse_roundtrips_the_vocabulary() {
        for b in [
            SeverityBucket::Hint,
            SeverityBucket::Info,
            SeverityBucket::Warning,
            SeverityBucket::Error,
        ] {
            assert_eq!(SeverityBucket::parse(b.as_str()), Some(b));
        }
        assert_eq!(SeverityBucket::parse("blocker"), None);
        assert_eq!(SeverityBucket::parse(""), None);
    }

    #[test]
    fn catalog_covers_every_documented_code_with_required_fields() {
        let ru = diagnostic_catalog(Locale::Ru);
        // Every registered code with metadata appears.
        let with_meta = all_diagnostic_codes().filter(|c| get_metadata(*c).is_some()).count();
        assert_eq!(ru.len(), with_meta);
        assert!(ru.len() >= 170, "expected the full catalog, got {}", ru.len());
        for entry in &ru {
            assert!(!entry.code.is_empty());
            assert!(!entry.title.is_empty(), "{} has no title", entry.code);
            assert!(
                matches!(
                    entry.r#type,
                    "error" | "code_smell" | "vulnerability" | "security_hotspot"
                ),
                "{} has unexpected type {}",
                entry.code,
                entry.r#type
            );
        }
    }

    #[test]
    fn catalog_title_follows_locale() {
        let code = crate::DiagnosticCode::CyclomaticComplexity;
        let ru = catalog_entry(code, Locale::Ru).unwrap();
        let en = catalog_entry(code, Locale::En).unwrap();
        assert!(ru.title.contains("Цикломатическая"), "ru title was {:?}", ru.title);
        assert_ne!(ru.title, en.title, "ru and en titles should differ");
    }

    #[test]
    fn catalog_entry_serializes_to_the_expected_shape() {
        let entry = catalog_entry(crate::DiagnosticCode::CyclomaticComplexity, Locale::En).unwrap();
        let v = serde_json::to_value(&entry).unwrap();
        assert!(v["code"].is_string());
        assert!(v["title"].is_string());
        assert!(v["default_severity"].is_string());
        assert!(v["type"].is_string(), "raw `r#type` field must serialize as `type`");
        assert!(v["activated_by_default"].is_boolean());
        assert!(v["clean_code_attribute"].is_string());
    }
}
