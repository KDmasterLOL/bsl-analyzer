//! Lowering context for SDBL HIR.

use bsl_metadata::Configuration;

use crate::diagnostics::SdblDiagnostic;
use crate::scope::Scope;

/// Context for lowering SDBL AST to HIR.
///
/// Maintains:
/// - Metadata for table/field resolution
/// - Scope for name resolution
/// - Collected diagnostics
pub struct LoweringContext<'a> {
    /// 1C Configuration metadata (optional).
    pub(super) metadata: Option<&'a Configuration>,

    /// Current scope for name resolution.
    pub(super) scope: Scope,

    /// Collected semantic diagnostics.
    pub(super) diagnostics: Vec<SdblDiagnostic>,
}

impl<'a> LoweringContext<'a> {
    /// Create a new lowering context.
    pub fn new(metadata: Option<&'a Configuration>) -> Self {
        Self { metadata, scope: Scope::new(), diagnostics: Vec::new() }
    }

    /// Push a new scope frame (for subqueries).
    #[allow(dead_code)]
    pub fn push_scope(&mut self) {
        self.scope.push_frame();
    }

    /// Pop the current scope frame.
    #[allow(dead_code)]
    pub fn pop_scope(&mut self) {
        self.scope.pop_frame();
    }

    /// Add a diagnostic.
    #[allow(dead_code)]
    pub fn add_diagnostic(&mut self, diagnostic: SdblDiagnostic) {
        self.diagnostics.push(diagnostic);
    }
}
