//! The one place this crate turns a workspace path into a [`Project`].
//!
//! A source set given on the command line is not on disk, so a call that only
//! receives a root path cannot see it. That matters here more than elsewhere:
//! the crate re-derives the project from a bare path in a dozen places — graph
//! scans, freshness checks, drift identity, the resident diagnostics host, the
//! broker key — and none of them can reach `SharedState`. Threading a spec
//! through every one of them would mean widening a dozen signatures whose
//! callers do not know about source sets either.
//!
//! Instead the override is process state, set once from argv before anything
//! builds a project, and consulted here. A server process serves exactly one
//! workspace with exactly one source set for its whole life, so there is nothing
//! for a second value to belong to. The scope stays inside this crate:
//! `project-model` gains no global, and `Project::new` keeps meaning what it
//! says everywhere else.

use std::path::Path;
use std::sync::OnceLock;

use project_model::{Project, ProjectConfig, ProjectError, SourceSetOverride};

static OVERRIDE: OnceLock<SourceSetOverride> = OnceLock::new();

/// Installs the source set this process was launched with.
///
/// Must run before anything builds a project — in particular before the broker
/// computes its backend key, which happens before a server exists at all.
/// Returns `false` if a source set was already installed, which only happens if
/// a caller violates that ordering; the first value wins, because a project
/// already built under it cannot be un-built.
pub fn set_source_set_override(source_set: SourceSetOverride) -> bool {
    OVERRIDE.set(source_set).is_ok()
}

/// The source set installed for this process, if any.
pub fn source_set_override() -> Option<&'static SourceSetOverride> {
    OVERRIDE.get()
}

/// Builds the project at `root` under this process's source set.
pub fn at(root: &Path) -> Result<Project, ProjectError> {
    let mut config = ProjectConfig::load(root)?.unwrap_or_default();
    if let Some(source_set) = OVERRIDE.get() {
        source_set.apply_to(&mut config);
    }
    Project::with_config(root, config)
}
