//! JOIN diagnostics for unprotected fields.

use super::context::LoweringContext;
use crate::diagnostics::SdblDiagnostic;
use crate::hir::{ExprHir, JoinHir, SdblHir, TableRef};

impl<'a> LoweringContext<'a> {
    pub(super) fn check_joins_for_unprotected_fields(&mut self, hir: &SdblHir) {
        // Build set of tables that are protected by WHERE IS NOT NULL checks
        // NOTE: Protection is local to this query, NOT inherited from parent or shared with siblings
        let protected_tables = self.find_tables_protected_by_where(hir);

        // Process only outer JOINs (LEFT/RIGHT/FULL)
        for join in hir.joins.iter().filter(|j| j.join_type.is_outer()) {
            match join.join_type {
                crate::hir::JoinType::Left => {
                    // Check joined table (right side)
                    if !protected_tables.contains(join.table.effective_name()) {
                        self.check_table_in_join(join, &join.table, hir);
                    }
                }
                crate::hir::JoinType::Right => {
                    // Check FROM table (left side)
                    for from_table in &hir.from {
                        if !protected_tables.contains(from_table.effective_name()) {
                            self.check_table_in_join(join, from_table, hir);
                        }
                    }
                }
                crate::hir::JoinType::Full => {
                    // Check both sides together for FULL JOIN
                    let mut all_unprotected = Vec::new();

                    // Check joined table if not protected
                    if !protected_tables.contains(join.table.effective_name()) {
                        self.collect_unprotected_refs(&join.table, hir, &mut all_unprotected);
                    }

                    // Check FROM tables if not protected
                    for from_table in &hir.from {
                        if !protected_tables.contains(from_table.effective_name()) {
                            self.collect_unprotected_refs(from_table, hir, &mut all_unprotected);
                        }
                    }

                    // Emit single diagnostic for FULL JOIN with all unprotected fields
                    if !all_unprotected.is_empty() {
                        self.diagnostics.push(SdblDiagnostic::FieldsFromJoinWithoutNullCheck {
                            join_type: join.join_type,
                            range: join.range,
                            unprotected_fields: all_unprotected,
                        });
                    }
                }
                _ => {} // Inner joins are safe
            }
        }

        // Recursively check UNION subqueries
        // Each UNION subquery has its own scope and protection rules
        for union in &hir.unions {
            self.check_joins_for_unprotected_fields(&union.query);
        }
    }

    /// Find tables that are protected by WHERE IS NOT NULL checks.
    ///
    /// Java BSL-LS semantics: if WHERE has IS NOT NULL check for ANY field from a table,
    /// the ENTIRE table is considered protected.
    fn find_tables_protected_by_where(&self, hir: &SdblHir) -> std::collections::HashSet<String> {
        let mut protected = std::collections::HashSet::new();

        if let Some(ref where_expr) = hir.where_clause {
            self.collect_protected_tables(where_expr, &mut protected);
        }

        protected
    }

    /// Recursively collect tables that have IS NOT NULL checks in expression.
    fn collect_protected_tables(
        &self,
        expr: &ExprHir,
        protected: &mut std::collections::HashSet<String>,
    ) {
        match expr {
            ExprHir::IsNull { expr: inner, negated, .. } => {
                // IS NOT NULL check protects the table
                if *negated {
                    // Extract table alias from inner expression recursively
                    if let Some(alias) = self.extract_table_alias(inner) {
                        protected.insert(alias);
                    }
                }
                // Always check nested expressions
                self.collect_protected_tables(inner, protected);
            }
            ExprHir::UnaryOp { op: crate::hir::UnaryOp::Not, expr: inner, .. } => {
                // NOT (field IS NULL) also protects - search recursively for IsNull
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

    /// Extract table alias from an expression (recursively handles wrapped expressions).
    #[allow(clippy::only_used_in_recursion)]
    fn extract_table_alias(&self, expr: &ExprHir) -> Option<String> {
        match expr {
            ExprHir::ColumnRef { table_alias: Some(alias), .. } => Some(alias.to_string()),
            ExprHir::BinaryOp { lhs, rhs, .. } => {
                // Try left side first, then right
                self.extract_table_alias(lhs).or_else(|| self.extract_table_alias(rhs))
            }
            ExprHir::UnaryOp { expr: inner, .. } => self.extract_table_alias(inner),
            ExprHir::FunctionCall { args, .. } => {
                // Try each argument
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

    /// Find IS NULL expression inside NOT operator and extract table alias.
    ///
    /// Searches recursively for pattern: NOT (... field IS NULL ...)
    fn find_is_null_in_not(&self, expr: &ExprHir) -> Option<String> {
        match expr {
            ExprHir::IsNull { expr: field_expr, negated: false, .. } => {
                // Found IS NULL (not negated) - extract table alias
                self.extract_table_alias(field_expr)
            }
            ExprHir::BinaryOp { lhs, rhs, .. } => {
                // Recurse into both sides
                self.find_is_null_in_not(lhs).or_else(|| self.find_is_null_in_not(rhs))
            }
            ExprHir::UnaryOp { expr: inner, .. } => {
                // Recurse into unary expression
                self.find_is_null_in_not(inner)
            }
            _ => None,
        }
    }

    /// Collect unprotected references for a table (used by FULL JOIN).
    fn collect_unprotected_refs(
        &mut self,
        table: &TableRef,
        hir: &SdblHir,
        all_unprotected: &mut Vec<crate::diagnostics::UnprotectedFieldRef>,
    ) {
        let table_alias = table.effective_name();
        let mut unprotected_refs = Vec::new();

        // Check SELECT clause
        for field in &hir.select.fields {
            self.find_unprotected_refs(&field.expr, table_alias, &mut unprotected_refs);
        }

        // Check WHERE clause
        if let Some(ref where_expr) = hir.where_clause {
            self.find_unprotected_refs(where_expr, table_alias, &mut unprotected_refs);
        }

        all_unprotected.extend(unprotected_refs);
    }

    /// Check if a table's fields are used without protection in a JOIN context.
    fn check_table_in_join(&mut self, join: &JoinHir, table: &TableRef, hir: &SdblHir) {
        let table_alias = table.effective_name();
        let mut unprotected_refs = Vec::new();

        // 1. Check SELECT clause
        for field in &hir.select.fields {
            self.find_unprotected_refs(&field.expr, table_alias, &mut unprotected_refs);
        }

        // 2. Check WHERE clause
        if let Some(ref where_expr) = hir.where_clause {
            self.find_unprotected_refs(where_expr, table_alias, &mut unprotected_refs);
        }

        // 3. Check other JOINs' ON conditions (but not this JOIN's own ON)
        for other_join in &hir.joins {
            // Skip the same JOIN (field in own ON is protected)
            if other_join.range != join.range {
                if let Some(ref on_expr) = other_join.condition {
                    self.find_unprotected_refs(on_expr, table_alias, &mut unprotected_refs);
                }
            }
        }

        // NOTE: Fields in this JOIN's own ON condition are NOT checked
        // because they are part of the join predicate itself, not vulnerable usage

        // Emit diagnostic if unprotected references found
        if !unprotected_refs.is_empty() {
            self.diagnostics.push(SdblDiagnostic::FieldsFromJoinWithoutNullCheck {
                join_type: join.join_type,
                range: join.range,
                unprotected_fields: unprotected_refs,
            });
        }
    }

    /// Find unprotected field references in an expression.
    ///
    /// Recursively traverses ExprHir, checking for ColumnRef matching the protected table.
    /// Stops traversal when encountering protection context (ISNULL, IS NULL, etc.).
    fn find_unprotected_refs(
        &self,
        expr: &ExprHir,
        protected_table: &str,
        unprotected_refs: &mut Vec<crate::diagnostics::UnprotectedFieldRef>,
    ) {
        // Check if this expression provides protection
        if self.is_protected_context(expr, protected_table) {
            return; // Protected - stop traversing this branch
        }

        match expr {
            ExprHir::ColumnRef { table_alias, column, range, .. } => {
                // Check if this references the protected table
                if let Some(alias) = table_alias {
                    if alias.eq_ignore_ascii_case(protected_table) {
                        // Found unprotected reference!
                        unprotected_refs.push(crate::diagnostics::UnprotectedFieldRef {
                            table_alias: alias.to_string(),
                            field_name: column.to_string(),
                            range: *range,
                        });
                    }
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
                // Note: ISNULL protection handled in is_protected_context
                // For other functions, check arguments
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
                for when in when_clauses {
                    self.find_unprotected_refs(&when.condition, protected_table, unprotected_refs);
                    self.find_unprotected_refs(&when.result, protected_table, unprotected_refs);
                }
                if let Some(else_e) = else_expr {
                    self.find_unprotected_refs(else_e, protected_table, unprotected_refs);
                }
            }

            ExprHir::IsNull { expr: inner, .. } => {
                // IS NULL itself provides protection (handled in is_protected_context)
                // But still traverse in case it's nested
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
                        // Check subquery fields
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
                // Check subquery SELECT fields
                for field in &query.select.fields {
                    self.find_unprotected_refs(&field.expr, protected_table, unprotected_refs);
                }
            }

            ExprHir::Literal { .. } | ExprHir::Parameter { .. } | ExprHir::Missing { .. } => {
                // No field references
            }
        }
    }

    /// Check if an expression provides NULL protection for a table's fields.
    ///
    /// Protection patterns:
    /// - ISNULL(table.field, default)
    /// - table.field IS NULL / IS NOT NULL
    /// - NOT (table.field IS NULL)
    fn is_protected_context(&self, expr: &ExprHir, protected_table: &str) -> bool {
        match expr {
            // ISNULL function protects its first argument
            ExprHir::FunctionCall { function, args, .. } => {
                if matches!(function, crate::hir::FunctionKind::Isnull) {
                    if let Some(first_arg) = args.first() {
                        return self.expr_references_table(first_arg, protected_table);
                    }
                }
                false
            }

            // IS NULL / IS NOT NULL protects the checked expression
            ExprHir::IsNull { expr: inner, .. } => {
                self.expr_references_table(inner, protected_table)
            }

            // NOT (field IS NULL) pattern
            ExprHir::UnaryOp { op: crate::hir::UnaryOp::Not, expr: inner, .. } => {
                if let ExprHir::IsNull { expr: field_expr, .. } = &**inner {
                    return self.expr_references_table(field_expr, protected_table);
                }
                false
            }

            _ => false,
        }
    }

    /// Check if an expression references a specific table.
    #[allow(clippy::only_used_in_recursion)]
    fn expr_references_table(&self, expr: &ExprHir, table_alias: &str) -> bool {
        match expr {
            ExprHir::ColumnRef { table_alias: Some(alias), .. } => {
                alias.eq_ignore_ascii_case(table_alias)
            }
            ExprHir::BinaryOp { lhs, rhs, .. } => {
                self.expr_references_table(lhs, table_alias)
                    || self.expr_references_table(rhs, table_alias)
            }
            ExprHir::UnaryOp { expr, .. } => self.expr_references_table(expr, table_alias),
            ExprHir::FunctionCall { args, .. } => {
                args.iter().any(|arg| self.expr_references_table(arg, table_alias))
            }
            _ => false,
        }
    }
}
