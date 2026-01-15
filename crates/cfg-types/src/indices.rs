//! Opaque index types for breaking circular dependency cfg ↔ hir-def.
//!
//! This module defines newtype wrappers around u32 that can be converted
//! to/from `la_arena::Idx<T>` without depending on the concrete types (Expr, Stmt, Binding).
//!
//! ## Architecture
//!
//! ```text
//! cfg → cfg-types (opaque IDs: ExprId, StmtId, BindingId)
//! hir-def → cfg-types (opaque IDs)
//! hir-def → cfg (for CFG analysis)
//! ```
//!
//! No cycle! cfg-types has no dependency on hir-def.

use la_arena::{Idx, RawIdx};

// ────────────────────────────────────────────────────────────────────────────
// Opaque ID Types
// ────────────────────────────────────────────────────────────────────────────

/// Opaque expression identifier (no dependency on hir_def::Expr).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprId(u32);

/// Opaque statement identifier (no dependency on hir_def::Stmt).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StmtId(u32);

/// Opaque binding identifier (no dependency on hir_def::Binding).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindingId(u32);

// ────────────────────────────────────────────────────────────────────────────
// Conversion Trait
// ────────────────────────────────────────────────────────────────────────────

/// Zero-cost conversion between opaque IDs and typed `Idx<T>`.
///
/// Used at the boundary between cfg and hir-def:
/// - hir-def allocates `Idx<Expr>` in arena
/// - Converts to `ExprId` when passing to cfg
/// - Converts back to `Idx<Expr>` when reading from arena
pub trait IdConversion<T> {
    /// Convert from typed arena index.
    fn from_idx(idx: Idx<T>) -> Self;

    /// Convert to typed arena index.
    fn to_idx(self) -> Idx<T>;
}

// ────────────────────────────────────────────────────────────────────────────
// Implementations
// ────────────────────────────────────────────────────────────────────────────

impl<T> IdConversion<T> for ExprId {
    fn from_idx(idx: Idx<T>) -> Self {
        ExprId(idx.into_raw().into())
    }

    fn to_idx(self) -> Idx<T> {
        Idx::from_raw(RawIdx::from(self.0))
    }
}

impl<T> IdConversion<T> for StmtId {
    fn from_idx(idx: Idx<T>) -> Self {
        StmtId(idx.into_raw().into())
    }

    fn to_idx(self) -> Idx<T> {
        Idx::from_raw(RawIdx::from(self.0))
    }
}

impl<T> IdConversion<T> for BindingId {
    fn from_idx(idx: Idx<T>) -> Self {
        BindingId(idx.into_raw().into())
    }

    fn to_idx(self) -> Idx<T> {
        Idx::from_raw(RawIdx::from(self.0))
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Compatibility Methods (for BodySourceMap and other uses)
// ────────────────────────────────────────────────────────────────────────────

impl ExprId {
    /// Get the raw u32 value (for BodySourceMap indexing).
    pub fn into_raw(self) -> RawIdx {
        RawIdx::from(self.0)
    }

    /// Create from raw u32 value.
    pub fn from_raw(raw: RawIdx) -> Self {
        ExprId(raw.into())
    }
}

impl StmtId {
    /// Get the raw u32 value (for BodySourceMap indexing).
    pub fn into_raw(self) -> RawIdx {
        RawIdx::from(self.0)
    }

    /// Create from raw u32 value.
    pub fn from_raw(raw: RawIdx) -> Self {
        StmtId(raw.into())
    }
}

impl BindingId {
    /// Get the raw u32 value (for BodySourceMap indexing).
    pub fn into_raw(self) -> RawIdx {
        RawIdx::from(self.0)
    }

    /// Create from raw u32 value.
    pub fn from_raw(raw: RawIdx) -> Self {
        BindingId(raw.into())
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use la_arena::Arena;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum MockExpr {
        Literal(i32),
        Add(ExprId, ExprId),
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    #[allow(dead_code)]
    enum MockStmt {
        Expr(ExprId),
        Return(Option<ExprId>),
    }

    #[test]
    fn test_expr_id_roundtrip() {
        let mut arena: Arena<MockExpr> = Arena::new();
        let typed_id = arena.alloc(MockExpr::Literal(42));

        // Convert to opaque
        let opaque_id = ExprId::from_idx(typed_id);
        assert_eq!(opaque_id.0, typed_id.into_raw().into());

        // Convert back
        let converted_back: Idx<MockExpr> = opaque_id.to_idx();
        assert_eq!(converted_back, typed_id);

        // Verify arena access works
        assert_eq!(arena[converted_back], MockExpr::Literal(42));
    }

    #[test]
    fn test_stmt_id_roundtrip() {
        let mut arena: Arena<MockStmt> = Arena::new();
        let typed_id = arena.alloc(MockStmt::Return(None));

        let opaque_id = StmtId::from_idx(typed_id);
        let converted_back: Idx<MockStmt> = opaque_id.to_idx();

        assert_eq!(converted_back, typed_id);
        assert_eq!(arena[converted_back], MockStmt::Return(None));
    }

    #[test]
    fn test_zero_cost_size() {
        use std::mem::size_of;

        // All IDs are 4 bytes (u32)
        assert_eq!(size_of::<ExprId>(), 4);
        assert_eq!(size_of::<StmtId>(), 4);
        assert_eq!(size_of::<BindingId>(), 4);

        // Same size as Idx<T>
        assert_eq!(size_of::<ExprId>(), size_of::<Idx<MockExpr>>());
        assert_eq!(size_of::<StmtId>(), size_of::<Idx<MockStmt>>());
    }

    #[test]
    fn test_usage_in_cfg_like_structure() {
        let mut exprs: Arena<MockExpr> = Arena::new();

        // Simulate hir-def allocating expressions
        let lit1_typed = exprs.alloc(MockExpr::Literal(1));
        let lit2_typed = exprs.alloc(MockExpr::Literal(2));

        // Convert to opaque IDs for cfg storage
        let lit1_opaque = ExprId::from_idx(lit1_typed);
        let lit2_opaque = ExprId::from_idx(lit2_typed);

        // Store opaque IDs in MockExpr (simulates cfg storing ExprId)
        let add_typed = exprs.alloc(MockExpr::Add(lit1_opaque, lit2_opaque));

        // Verify we can read back
        let add_opaque = ExprId::from_idx(add_typed);
        let add_recovered: Idx<MockExpr> = add_opaque.to_idx();

        match &exprs[add_recovered] {
            MockExpr::Add(a, b) => {
                // Convert opaque back to typed for arena access
                let a_typed: Idx<MockExpr> = a.to_idx();
                let b_typed: Idx<MockExpr> = b.to_idx();

                assert_eq!(exprs[a_typed], MockExpr::Literal(1));
                assert_eq!(exprs[b_typed], MockExpr::Literal(2));
            }
            _ => panic!("Expected Add"),
        }
    }
}
