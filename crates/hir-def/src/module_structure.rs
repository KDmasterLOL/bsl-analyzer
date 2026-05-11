//! BSL-domain facts about module structure: canonical region naming,
//! standard region sets per [`bsl_metadata::ModuleType`], and the shared
//! "real, observable code" predicate consumed by both `CodeOutOfRegion`
//! and `RegionTree::is_region_empty`.
//!
//! Track 2 Phase C §3 Slice 1: this module owns BSL semantic policy that
//! previously lived as ad-hoc tables in `ide-diagnostics` handlers
//! (layer fix per CLAUDE.md «слойная архитектура»).

pub mod canonical;
pub mod significant;
pub mod standard;
