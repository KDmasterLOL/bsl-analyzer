use super::context::LoweringContext;
use crate::diagnostics::SdblDiagnostic;
use crate::hir::{ExprHir, JoinHir, SdblHir, TableRef};

impl LoweringContext<'_> {
    pub(super) fn check_joins_for_unprotected_fields(&mut self, hir: &SdblHir) {
        let protected_tables = self.find_tables_protected_by_where(hir);

        for join in hir.joins.iter().filter(|j| j.join_type.is_outer()) {
            match join.join_type {
                crate::hir::JoinType::Left
                    if !protected_tables.contains(join.table.effective_name()) =>
                {
                    self.check_table_in_join(join, &join.table, hir);
                }
                crate::hir::JoinType::Right => {
                    for from_table in &hir.from {
                        if !protected_tables.contains(from_table.effective_name()) {
                            self.check_table_in_join(join, from_table, hir);
                        }
                    }
                }
                crate::hir::JoinType::Full => {
                    let mut all_unprotected = Vec::new();

                    if !protected_tables.contains(join.table.effective_name()) {
                        self.collect_unprotected_refs(&join.table, hir, &mut all_unprotected);
                    }

                    for from_table in &hir.from {
                        if !protected_tables.contains(from_table.effective_name()) {
                            self.collect_unprotected_refs(from_table, hir, &mut all_unprotected);
                        }
                    }

                    if !all_unprotected.is_empty() {
                        self.diagnostics.push(SdblDiagnostic::FieldsFromJoinWithoutNullCheck {
                            join_type: join.join_type,
                            range: join.range,
                            unprotected_fields: all_unprotected,
                        });
                    }
                }
                _ => {}
            }
        }

        for union in &hir.unions {
            self.check_joins_for_unprotected_fields(&union.query);
        }
    }

    fn find_tables_protected_by_where(&self, hir: &SdblHir) -> std::collections::HashSet<String> {
        let mut protected = std::collections::HashSet::new();

        if let Some(ref where_expr) = hir.where_clause {
            self.collect_protected_tables(where_expr, &mut protected);
        }

        protected
    }

    fn collect_protected_tables(
        &self,
        expr: &ExprHir,
        protected: &mut std::collections::HashSet<String>,
    ) {
        match expr {
            ExprHir::IsNull { expr: inner, negated, .. } => {
                if *negated {
                    if let Some(alias) = self.extract_table_alias(inner) {
                        protected.insert(alias);
                    }
                }
                self.collect_protected_tables(inner, protected);
            }
            ExprHir::UnaryOp { op: crate::hir::UnaryOp::Not, expr: inner, .. } => {
                if let Some(alias) = self.find_is_null_in_not(inner) {
                    protected.insert(alias);
                }
                self.collect_protected_tables(inner, protected);
            }
            ExprHir::BinaryOp { lhs, rhs, .. } => {
                self.collect_protected_tables(lhs, protected);
                self.collect_protected_tables(rhs, protected);
            }
            ExprHir::UnaryOp { expr, .. } => {
                self.collect_protected_tables(expr, protected);
            }
            ExprHir::FunctionCall { args, .. } => {
                for arg in args {
                    self.collect_protected_tables(arg, protected);
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::only_used_in_recursion)]
    fn extract_table_alias(&self, expr: &ExprHir) -> Option<String> {
        match expr {
            ExprHir::ColumnRef { parts, .. } if parts.len() >= 2 => Some(parts[0].to_string()),
            ExprHir::BinaryOp { lhs, rhs, .. } => {
                self.extract_table_alias(lhs).or_else(|| self.extract_table_alias(rhs))
            }
            ExprHir::UnaryOp { expr: inner, .. } => self.extract_table_alias(inner),
            ExprHir::FunctionCall { args, .. } => {
                for arg in args {
                    if let Some(alias) = self.extract_table_alias(arg) {
                        return Some(alias);
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn find_is_null_in_not(&self, expr: &ExprHir) -> Option<String> {
        match expr {
            ExprHir::IsNull { expr: field_expr, negated: false, .. } => {
                self.extract_table_alias(field_expr)
            }
            ExprHir::BinaryOp { lhs, rhs, .. } => {
                self.find_is_null_in_not(lhs).or_else(|| self.find_is_null_in_not(rhs))
            }
            ExprHir::UnaryOp { expr: inner, .. } => self.find_is_null_in_not(inner),
            _ => None,
        }
    }

    fn collect_unprotected_refs(
        &mut self,
        table: &TableRef,
        hir: &SdblHir,
        all_unprotected: &mut Vec<crate::diagnostics::UnprotectedFieldRef>,
    ) {
        let table_alias = table.effective_name();
        let mut unprotected_refs = Vec::new();

        for field in &hir.select.fields {
            self.find_unprotected_refs(&field.expr, table_alias, &mut unprotected_refs);
        }

        if let Some(ref where_expr) = hir.where_clause {
            self.find_unprotected_refs(where_expr, table_alias, &mut unprotected_refs);
        }

        all_unprotected.extend(unprotected_refs);
    }

    fn check_table_in_join(&mut self, join: &JoinHir, table: &TableRef, hir: &SdblHir) {
        let table_alias = table.effective_name();
        let mut unprotected_refs = Vec::new();

        for field in &hir.select.fields {
            self.find_unprotected_refs(&field.expr, table_alias, &mut unprotected_refs);
        }

        if let Some(ref where_expr) = hir.where_clause {
            self.find_unprotected_refs(where_expr, table_alias, &mut unprotected_refs);
        }

        if !unprotected_refs.is_empty() {
            self.diagnostics.push(SdblDiagnostic::FieldsFromJoinWithoutNullCheck {
                join_type: join.join_type,
                range: join.range,
                unprotected_fields: unprotected_refs,
            });
        }
    }

    fn find_unprotected_refs(
        &self,
        expr: &ExprHir,
        protected_table: &str,
        unprotected_refs: &mut Vec<crate::diagnostics::UnprotectedFieldRef>,
    ) {
        if self.is_protected_context(expr, protected_table) {
            return;
        }

        match expr {
            ExprHir::ColumnRef { parts, range, .. } => {
                if parts.len() >= 2 {
                    let alias = &parts[0];
                    if alias.eq_ignore_ascii_case(protected_table) {
                        unprotected_refs.push(crate::diagnostics::UnprotectedFieldRef {
                            table_alias: alias.to_string(),
                            field_name: parts[1].to_string(),
                            range: *range,
                        });
                    }
                }
            }

            ExprHir::BinaryOp { op: crate::hir::BinaryOp::Or, lhs, rhs, .. } => {
                // `Т.Поле ЕСТЬ NULL ИЛИ <выражение>`: the NULL case is handled
                // by the sibling disjunct, so the table's fields inside the
                // other operand are deliberate.
                if !self.condition_handles_table_null(rhs, protected_table) {
                    self.find_unprotected_refs(lhs, protected_table, unprotected_refs);
                }
                if !self.condition_handles_table_null(lhs, protected_table) {
                    self.find_unprotected_refs(rhs, protected_table, unprotected_refs);
                }
            }

            ExprHir::BinaryOp { lhs, rhs, .. } => {
                self.find_unprotected_refs(lhs, protected_table, unprotected_refs);
                self.find_unprotected_refs(rhs, protected_table, unprotected_refs);
            }

            ExprHir::UnaryOp { expr: inner, .. } => {
                self.find_unprotected_refs(inner, protected_table, unprotected_refs);
            }

            ExprHir::FunctionCall { function, args, .. } => {
                if !matches!(function, crate::hir::FunctionKind::Isnull) {
                    for arg in args {
                        self.find_unprotected_refs(arg, protected_table, unprotected_refs);
                    }
                }
            }

            ExprHir::Case { operand, when_clauses, else_expr, .. } => {
                if let Some(op) = operand {
                    self.find_unprotected_refs(op, protected_table, unprotected_refs);
                }
                // Branch conditions guard the table: ВЫБОР runs its КОГДА
                // clauses in order, so once a condition of the form
                // `Т ЕСТЬ NULL` has been passed, every later branch (and
                // ИНАЧЕ) executes only when the table's row is present.
                // The operand form compares values and carries no NULL
                // semantics, so it gets no guard tracking.
                let mut known_not_null = false;
                for when in when_clauses {
                    if !known_not_null {
                        self.find_unprotected_refs(
                            &when.condition,
                            protected_table,
                            unprotected_refs,
                        );
                    }
                    let then_guarded = known_not_null
                        || (operand.is_none()
                            && self
                                .condition_implies_table_present(&when.condition, protected_table));
                    if !then_guarded {
                        self.find_unprotected_refs(&when.result, protected_table, unprotected_refs);
                    }
                    known_not_null = known_not_null
                        || (operand.is_none()
                            && self.condition_handles_table_null(&when.condition, protected_table));
                }
                if !known_not_null {
                    if let Some(else_e) = else_expr {
                        self.find_unprotected_refs(else_e, protected_table, unprotected_refs);
                    }
                }
            }

            ExprHir::IsNull { expr: inner, .. } => {
                self.find_unprotected_refs(inner, protected_table, unprotected_refs);
            }

            ExprHir::In { expr, values, .. } => {
                self.find_unprotected_refs(expr, protected_table, unprotected_refs);
                match values {
                    crate::hir::InValues::List(exprs) => {
                        for e in exprs {
                            self.find_unprotected_refs(e, protected_table, unprotected_refs);
                        }
                    }
                    crate::hir::InValues::Subquery(subq) => {
                        for field in &subq.select.fields {
                            self.find_unprotected_refs(
                                &field.expr,
                                protected_table,
                                unprotected_refs,
                            );
                        }
                    }
                }
            }

            ExprHir::Between { expr, low, high, .. } => {
                self.find_unprotected_refs(expr, protected_table, unprotected_refs);
                self.find_unprotected_refs(low, protected_table, unprotected_refs);
                self.find_unprotected_refs(high, protected_table, unprotected_refs);
            }

            ExprHir::Like { expr, pattern, escape, .. } => {
                self.find_unprotected_refs(expr, protected_table, unprotected_refs);
                self.find_unprotected_refs(pattern, protected_table, unprotected_refs);
                if let Some(esc) = escape {
                    self.find_unprotected_refs(esc, protected_table, unprotected_refs);
                }
            }

            ExprHir::Subquery { query, .. } => {
                for field in &query.select.fields {
                    self.find_unprotected_refs(&field.expr, protected_table, unprotected_refs);
                }
            }

            ExprHir::Tuple { elements, .. } => {
                for elem in elements {
                    self.find_unprotected_refs(elem, protected_table, unprotected_refs);
                }
            }

            ExprHir::Literal { .. } | ExprHir::Parameter { .. } | ExprHir::Missing { .. } => {}
        }
    }

    fn is_protected_context(&self, expr: &ExprHir, protected_table: &str) -> bool {
        match expr {
            ExprHir::FunctionCall { function, args, .. } => {
                if matches!(function, crate::hir::FunctionKind::Isnull) {
                    if let Some(first_arg) = args.first() {
                        return self.expr_references_table(first_arg, protected_table);
                    }
                }
                false
            }

            ExprHir::IsNull { expr: inner, .. } => {
                self.expr_references_table(inner, protected_table)
            }

            ExprHir::UnaryOp { op: crate::hir::UnaryOp::Not, expr: inner, .. } => {
                if let ExprHir::IsNull { expr: field_expr, .. } = &**inner {
                    return self.expr_references_table(field_expr, protected_table);
                }
                false
            }

            _ => false,
        }
    }

    /// Does this condition handle the NULL case of `table` positively, i.e.
    /// does `Т.Поле ЕСТЬ NULL` dominate it through ИЛИ-combinations? Then the
    /// condition being FALSE guarantees the table's row is present, and a
    /// sibling disjunct sees the NULL case explicitly handled. И-combinations
    /// give no such guarantee: `Т ЕСТЬ NULL И X` can be false with a NULL row.
    /// Only a direct column reference observes NULL — wrappers like
    /// ЕСТЬNULL(…) never yield NULL, so an IS-NULL test over them is
    /// constant-false and proves nothing.
    fn condition_handles_table_null(&self, expr: &ExprHir, table: &str) -> bool {
        match expr {
            ExprHir::IsNull { expr: inner, negated: false, .. } => {
                self.is_direct_column_of_table(inner, table)
            }
            ExprHir::BinaryOp { op: crate::hir::BinaryOp::Or, lhs, rhs, .. } => {
                self.condition_handles_table_null(lhs, table)
                    || self.condition_handles_table_null(rhs, table)
            }
            _ => false,
        }
    }

    /// Does this condition being TRUE guarantee the table's row is present
    /// (not NULL)? Covers `Т.Поле ЕСТЬ НЕ NULL`, `НЕ (Т.Поле ЕСТЬ NULL)`, and
    /// the sentinel test `ЕСТЬNULL(Т.Поле, З) <> З`: an absent row turns the
    /// call into the fallback `З`, the inequality fails, and the branch is
    /// skipped. A conjunct guarantee suffices for И; for ИЛИ both operands
    /// must guarantee.
    fn condition_implies_table_present(&self, expr: &ExprHir, table: &str) -> bool {
        match expr {
            ExprHir::IsNull { expr: inner, negated: true, .. } => {
                self.is_direct_column_of_table(inner, table)
            }
            ExprHir::UnaryOp { op: crate::hir::UnaryOp::Not, expr: inner, .. } => {
                self.condition_handles_table_null(inner, table)
            }
            ExprHir::BinaryOp { op: crate::hir::BinaryOp::And, lhs, rhs, .. } => {
                self.condition_implies_table_present(lhs, table)
                    || self.condition_implies_table_present(rhs, table)
            }
            ExprHir::BinaryOp { op: crate::hir::BinaryOp::Or, lhs, rhs, .. } => {
                self.condition_implies_table_present(lhs, table)
                    && self.condition_implies_table_present(rhs, table)
            }
            ExprHir::BinaryOp { op: crate::hir::BinaryOp::Ne, lhs, rhs, .. } => {
                self.is_isnull_sentinel_test(lhs, rhs, table)
                    || self.is_isnull_sentinel_test(rhs, lhs, table)
            }
            _ => false,
        }
    }

    /// `ЕСТЬNULL(Т.Поле, <литерал>)` compared via `<>` against the SAME
    /// literal: with the row absent the call yields exactly that fallback, the
    /// inequality is guaranteed false, and the branch cannot execute. Any
    /// other comparison shape (equality, a different literal, a non-literal
    /// fallback) can still select the branch on an absent row and must not
    /// count as a guard.
    fn is_isnull_sentinel_test(
        &self,
        isnull_side: &ExprHir,
        sentinel_side: &ExprHir,
        table: &str,
    ) -> bool {
        let ExprHir::FunctionCall { function: crate::hir::FunctionKind::Isnull, args, .. } =
            isnull_side
        else {
            return false;
        };
        let (Some(column), Some(fallback)) = (args.first(), args.get(1)) else {
            return false;
        };
        if !self.is_direct_column_of_table(column, table) {
            return false;
        }
        match (fallback, sentinel_side) {
            (
                ExprHir::Literal { value: fallback_value, .. },
                ExprHir::Literal { value: sentinel_value, .. },
            ) => fallback_value == sentinel_value,
            _ => false,
        }
    }

    fn is_direct_column_of_table(&self, expr: &ExprHir, table: &str) -> bool {
        matches!(
            expr,
            ExprHir::ColumnRef { parts, .. }
                if parts.len() >= 2 && parts[0].eq_ignore_ascii_case(table)
        )
    }

    #[allow(clippy::only_used_in_recursion)]
    fn expr_references_table(&self, expr: &ExprHir, table_name: &str) -> bool {
        match expr {
            ExprHir::ColumnRef { parts, .. } if parts.len() >= 2 => {
                parts[0].eq_ignore_ascii_case(table_name)
            }
            ExprHir::BinaryOp { lhs, rhs, .. } => {
                self.expr_references_table(lhs, table_name)
                    || self.expr_references_table(rhs, table_name)
            }
            ExprHir::UnaryOp { expr, .. } => self.expr_references_table(expr, table_name),
            ExprHir::FunctionCall { args, .. } => {
                args.iter().any(|arg| self.expr_references_table(arg, table_name))
            }
            _ => false,
        }
    }

    pub(super) fn check_nested_fields_by_dot(&mut self, hir: &SdblHir) {
        self.collect_nested_field_diagnostics(hir);

        for union in &hir.unions {
            self.check_nested_fields_by_dot(&union.query);
        }
    }

    fn collect_nested_field_diagnostics(&mut self, hir: &SdblHir) {
        for field in &hir.select.fields {
            self.check_expr_for_nested_fields(&field.expr, false);
        }

        for table in &hir.from {
            self.check_table_ref_for_nested_fields(table);
        }

        for join in &hir.joins {
            self.check_table_ref_for_nested_fields(&join.table);
            if let Some(ref cond) = join.condition {
                self.check_expr_for_nested_fields(cond, false);
            }
        }

        if let Some(ref where_expr) = hir.where_clause {
            self.check_expr_for_nested_fields(where_expr, false);
        }

        if let Some(ref group_by) = hir.group_by {
            for expr in &group_by.exprs {
                self.check_expr_for_nested_fields(expr, false);
            }
        }

        if let Some(ref having) = hir.having {
            self.check_expr_for_nested_fields(having, false);
        }

        if let Some(ref order_by) = hir.order_by {
            for item in &order_by.items {
                self.check_expr_for_nested_fields(&item.expr, false);
            }
        }
    }

    fn check_table_ref_for_nested_fields(&mut self, table: &TableRef) {
        if table.is_virtual_table {
            for param in &table.virtual_table_params {
                self.check_expr_for_nested_fields(param, true);
            }
        }

        for subquery in &table.subquery {
            self.collect_nested_field_diagnostics(subquery);
        }
    }

    fn check_expr_for_nested_fields(&mut self, expr: &ExprHir, in_virtual_table_params: bool) {
        match expr {
            ExprHir::ColumnRef { parts, range, .. } => {
                if parts.len() >= 2 && !crate::is_mdo_type(parts[0].as_str()) {
                    let parts_count =
                        if in_virtual_table_params { None } else { Some(parts.len() as u32) };
                    self.diagnostics.push(SdblDiagnostic::QueryNestedFieldsByDot {
                        range: *range,
                        parts_count,
                    });
                }
            }

            ExprHir::FunctionCall { function, args, member_access, range, .. } => {
                for arg in args {
                    self.check_expr_for_nested_fields(arg, in_virtual_table_params);
                }

                if matches!(function, crate::hir::FunctionKind::Cast) && member_access.len() > 1 {
                    self.diagnostics.push(SdblDiagnostic::QueryNestedFieldsByDot {
                        range: *range,
                        parts_count: None,
                    });
                }
            }

            ExprHir::BinaryOp { lhs, rhs, .. } => {
                self.check_expr_for_nested_fields(lhs, in_virtual_table_params);
                self.check_expr_for_nested_fields(rhs, in_virtual_table_params);
            }

            ExprHir::UnaryOp { expr: inner, .. } => {
                self.check_expr_for_nested_fields(inner, in_virtual_table_params);
            }

            ExprHir::Case { operand, when_clauses, else_expr, .. } => {
                if let Some(op) = operand {
                    self.check_expr_for_nested_fields(op, in_virtual_table_params);
                }
                for clause in when_clauses {
                    self.check_expr_for_nested_fields(&clause.condition, in_virtual_table_params);
                    self.check_expr_for_nested_fields(&clause.result, in_virtual_table_params);
                }
                if let Some(else_e) = else_expr {
                    self.check_expr_for_nested_fields(else_e, in_virtual_table_params);
                }
            }

            ExprHir::Subquery { query, .. } => {
                self.collect_nested_field_diagnostics(query);
            }

            ExprHir::In { expr: inner, values, .. } => {
                self.check_expr_for_nested_fields(inner, in_virtual_table_params);
                match values {
                    crate::hir::InValues::List(items) => {
                        for item in items {
                            self.check_expr_for_nested_fields(item, in_virtual_table_params);
                        }
                    }
                    crate::hir::InValues::Subquery(sq) => {
                        self.collect_nested_field_diagnostics(sq);
                    }
                }
            }

            ExprHir::Between { expr: inner, low, high, .. } => {
                self.check_expr_for_nested_fields(inner, in_virtual_table_params);
                self.check_expr_for_nested_fields(low, in_virtual_table_params);
                self.check_expr_for_nested_fields(high, in_virtual_table_params);
            }

            ExprHir::Like { expr: inner, pattern, escape, .. } => {
                self.check_expr_for_nested_fields(inner, in_virtual_table_params);
                self.check_expr_for_nested_fields(pattern, in_virtual_table_params);
                if let Some(esc) = escape {
                    self.check_expr_for_nested_fields(esc, in_virtual_table_params);
                }
            }

            ExprHir::IsNull { expr: inner, .. } => {
                self.check_expr_for_nested_fields(inner, in_virtual_table_params);
            }

            ExprHir::Tuple { elements, .. } => {
                for elem in elements {
                    self.check_expr_for_nested_fields(elem, in_virtual_table_params);
                }
            }

            ExprHir::Literal { .. } | ExprHir::Parameter { .. } | ExprHir::Missing { .. } => {}
        }
    }

    pub(super) fn check_alias_without_as_keyword(&mut self, hir: &SdblHir) {
        for field in &hir.select.fields {
            if field.is_asterisk {
                continue;
            }

            if field.has_parse_error {
                continue;
            }

            let needs_diagnostic = match &field.alias {
                Some(_) => !field.has_as_keyword,
                None => true,
            };

            if needs_diagnostic {
                self.diagnostics.push(SdblDiagnostic::AliasWithoutAsKeyword {
                    field_name: field.alias.as_ref().map(|n| n.to_string()),
                    raw_name: field.raw_name.as_ref().map(|n| n.to_string()),
                    range: field.diagnostic_range,
                });
            }
        }
    }

    pub(super) fn check_ref_overuse(&mut self, hir: &SdblHir) {
        self.check_ref_overuse_in_select(hir);
        self.check_ref_overuse_in_where(hir);
        self.check_ref_overuse_in_group_by(hir);
        self.check_ref_overuse_in_having(hir);
        self.check_ref_overuse_in_order_by(hir);
        self.check_ref_overuse_in_joins(hir);

        for union in &hir.unions {
            self.check_ref_overuse(&union.query);
        }
    }

    fn check_ref_overuse_in_select(&mut self, hir: &SdblHir) {
        for field in &hir.select.fields {
            self.check_expr_for_ref_overuse(&field.expr);
        }
    }

    fn check_ref_overuse_in_where(&mut self, hir: &SdblHir) {
        if let Some(ref where_expr) = hir.where_clause {
            self.check_expr_for_ref_overuse(where_expr);
        }
    }

    fn check_ref_overuse_in_group_by(&mut self, hir: &SdblHir) {
        if let Some(ref group_by) = hir.group_by {
            for expr in &group_by.exprs {
                self.check_expr_for_ref_overuse(expr);
            }
        }
    }

    fn check_ref_overuse_in_having(&mut self, hir: &SdblHir) {
        if let Some(ref having) = hir.having {
            self.check_expr_for_ref_overuse(having);
        }
    }

    fn check_ref_overuse_in_order_by(&mut self, hir: &SdblHir) {
        if let Some(ref order_by) = hir.order_by {
            for item in &order_by.items {
                self.check_expr_for_ref_overuse(&item.expr);
            }
        }
    }

    fn check_ref_overuse_in_joins(&mut self, hir: &SdblHir) {
        for join in &hir.joins {
            if let Some(ref cond) = join.condition {
                self.check_expr_for_ref_overuse(cond);
            }
        }
    }

    fn check_expr_for_ref_overuse(&mut self, expr: &ExprHir) {
        match expr {
            ExprHir::ColumnRef { parts, range, .. } => {
                self.check_column_ref_for_ref_overuse(parts, *range);
            }

            ExprHir::BinaryOp { lhs, rhs, .. } => {
                self.check_expr_for_ref_overuse(lhs);
                self.check_expr_for_ref_overuse(rhs);
            }

            ExprHir::UnaryOp { expr: inner, .. } => {
                self.check_expr_for_ref_overuse(inner);
            }

            ExprHir::FunctionCall { args, .. } => {
                for arg in args {
                    self.check_expr_for_ref_overuse(arg);
                }
            }

            ExprHir::Case { operand, when_clauses, else_expr, .. } => {
                if let Some(op) = operand {
                    self.check_expr_for_ref_overuse(op);
                }
                for clause in when_clauses {
                    self.check_expr_for_ref_overuse(&clause.condition);
                    self.check_expr_for_ref_overuse(&clause.result);
                }
                if let Some(else_e) = else_expr {
                    self.check_expr_for_ref_overuse(else_e);
                }
            }

            ExprHir::Subquery { query, .. } => {
                self.check_ref_overuse(query);
            }

            ExprHir::In { expr: inner, values, .. } => {
                self.check_expr_for_ref_overuse(inner);
                match values {
                    crate::hir::InValues::List(items) => {
                        for item in items {
                            self.check_expr_for_ref_overuse(item);
                        }
                    }
                    crate::hir::InValues::Subquery(sq) => {
                        self.check_ref_overuse(sq);
                    }
                }
            }

            ExprHir::Between { expr: inner, low, high, .. } => {
                self.check_expr_for_ref_overuse(inner);
                self.check_expr_for_ref_overuse(low);
                self.check_expr_for_ref_overuse(high);
            }

            ExprHir::Like { expr: inner, pattern, escape, .. } => {
                self.check_expr_for_ref_overuse(inner);
                self.check_expr_for_ref_overuse(pattern);
                if let Some(esc) = escape {
                    self.check_expr_for_ref_overuse(esc);
                }
            }

            ExprHir::IsNull { expr: inner, .. } => {
                self.check_expr_for_ref_overuse(inner);
            }

            ExprHir::Tuple { elements, .. } => {
                for elem in elements {
                    self.check_expr_for_ref_overuse(elem);
                }
            }

            ExprHir::Literal { .. } | ExprHir::Parameter { .. } | ExprHir::Missing { .. } => {}
        }
    }

    fn check_column_ref_for_ref_overuse(
        &mut self,
        parts: &[crate::hir::Name],
        range: text_size::TextRange,
    ) {
        if parts.len() < 2 {
            return;
        }

        for ref_idx in 0..parts.len() {
            let p_lower = parts[ref_idx].to_lowercase();
            if p_lower != "ссылка" && p_lower != "reference" {
                continue;
            }

            if ref_idx == 0 {
                continue;
            }

            if ref_idx == 1 {
                continue;
            }

            let (alias, chain_start) = if crate::is_mdo_type(parts[0].as_str()) {
                if parts.len() < 3 {
                    continue;
                }
                (parts[1].as_str(), 2usize)
            } else {
                (parts[0].as_str(), 1usize)
            };

            if chain_start >= ref_idx {
                continue;
            }

            let chain: Vec<String> =
                parts[chain_start..ref_idx].iter().map(|n| n.to_string()).collect();

            let field_type = self.scope.resolve_nested_field_type(alias, &chain);

            if field_type.is_ref() {
                self.diagnostics.push(SdblDiagnostic::RefOveruse { range });
                return;
            }
        }
    }
}
