use la_arena::{Idx, RawIdx};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StmtId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BindingId(u32);

pub trait IdConversion<T> {
    fn from_idx(idx: Idx<T>) -> Self;

    fn to_idx(self) -> Idx<T>;
}

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

impl ExprId {
    pub fn into_raw(self) -> RawIdx {
        RawIdx::from(self.0)
    }

    pub fn from_raw(raw: RawIdx) -> Self {
        ExprId(raw.into())
    }
}

impl StmtId {
    pub fn into_raw(self) -> RawIdx {
        RawIdx::from(self.0)
    }

    pub fn from_raw(raw: RawIdx) -> Self {
        StmtId(raw.into())
    }
}

impl BindingId {
    pub fn into_raw(self) -> RawIdx {
        RawIdx::from(self.0)
    }

    pub fn from_raw(raw: RawIdx) -> Self {
        BindingId(raw.into())
    }
}

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

        let opaque_id = ExprId::from_idx(typed_id);
        assert_eq!(opaque_id.0, typed_id.into_raw().into());

        let converted_back: Idx<MockExpr> = opaque_id.to_idx();
        assert_eq!(converted_back, typed_id);

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

        assert_eq!(size_of::<ExprId>(), 4);
        assert_eq!(size_of::<StmtId>(), 4);
        assert_eq!(size_of::<BindingId>(), 4);

        assert_eq!(size_of::<ExprId>(), size_of::<Idx<MockExpr>>());
        assert_eq!(size_of::<StmtId>(), size_of::<Idx<MockStmt>>());
    }

    #[test]
    fn test_usage_in_cfg_like_structure() {
        let mut exprs: Arena<MockExpr> = Arena::new();

        let lit1_typed = exprs.alloc(MockExpr::Literal(1));
        let lit2_typed = exprs.alloc(MockExpr::Literal(2));

        let lit1_opaque = ExprId::from_idx(lit1_typed);
        let lit2_opaque = ExprId::from_idx(lit2_typed);

        let add_typed = exprs.alloc(MockExpr::Add(lit1_opaque, lit2_opaque));

        let add_opaque = ExprId::from_idx(add_typed);
        let add_recovered: Idx<MockExpr> = add_opaque.to_idx();

        match &exprs[add_recovered] {
            MockExpr::Add(a, b) => {
                let a_typed: Idx<MockExpr> = a.to_idx();
                let b_typed: Idx<MockExpr> = b.to_idx();

                assert_eq!(exprs[a_typed], MockExpr::Literal(1));
                assert_eq!(exprs[b_typed], MockExpr::Literal(2));
            }
            _ => panic!("Expected Add"),
        }
    }
}
