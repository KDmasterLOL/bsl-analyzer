//! UNION clause lowering.

use crate::hir::SdblHir;
use syntax::ast::AstNode;

use super::context::LoweringContext;

impl<'a> LoweringContext<'a> {
    pub(super) fn lower_union_clauses(
        &mut self,
        subquery: &syntax::ast::SdblSubquery,
    ) -> Vec<crate::hir::UnionHir> {
        let mut unions = Vec::new();

        for union_clause in subquery.union_clauses() {
            // Record UNION keyword
            self.record_keyword_by_text(
                union_clause.syntax(),
                "UNION",
                "ОБЪЕДИНИТЬ",
                crate::source_map::TokenCategory::Modifier,
            );

            // Record ALL keyword if present
            if union_clause.has_all() {
                self.record_keyword_by_text(
                    union_clause.syntax(),
                    "ALL",
                    "ВСЕ",
                    crate::source_map::TokenCategory::Modifier,
                );
            }

            let Some(union_query) = union_clause.query() else {
                continue;
            };

            // Push a new scope frame for UNION query (preserves parent scope with temp tables)
            self.scope.push_frame();

            // Lower the UNION query in nested scope
            let union_hir = self.lower_query(&union_query);

            // Pop scope frame
            self.scope.pop_frame();

            unions.push(crate::hir::UnionHir {
                all: union_clause.has_all(),
                query: Box::new(union_hir),
                range: union_clause.syntax().text_range(),
            });
        }

        unions
    }

    /// Lower a SDBL query (called recursively for UNION subqueries).
    fn lower_query(&mut self, query: &syntax::ast::SdblQuery) -> SdblHir {
        // 1. Lower FROM clause first (establishes scope)
        let from = self.lower_from_clause(query.from_clause());

        // 2. Register tables in scope
        for table in &from {
            self.scope.add_table(table.clone());
        }

        // 3. Lower JOINs
        let joins = self.lower_joins(query);
        for join in &joins {
            self.scope.add_table(join.table.clone());
        }

        // 4. Extract DISTINCT and TOP
        let (distinct, top) = self.extract_limitations(query.syntax());

        // 5. Lower SELECT clause
        let select = self.lower_field_list(query.field_list(), distinct, top);

        // 6. Lower WHERE clause
        let where_clause = query.where_clause().map(|w| self.lower_where_clause(&w));

        let range = query.syntax().text_range();

        SdblHir {
            select,
            into_table: None,
            from,
            joins,
            where_clause,
            group_by: None,
            having: None,
            order_by: None,
            unions: Vec::new(),
            diagnostics: Vec::new(),
            range,
        }
    }
}
