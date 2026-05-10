//! Post-lowering diagnostics.
//!
//! Diagnostics that are emitted after HIR is fully built,
//! using only HIR data (no AST access).

use super::context::LoweringContext;
use crate::diagnostics::SdblDiagnostic;
use crate::hir::{ExprHir, JoinHir, SdblHir, TableRef};

impl LoweringContext {
    pub(super) fn check_joins_for_unprotected_fields(&mut self, hir: &SdblHir) {
        // Build set of tables that are protected by WHERE IS NOT NULL checks
        // NOTE: Protection is local to this query, NOT inherited from parent or shared with siblings
        let protected_tables = self.find_tables_protected_by_where(hir);

        // Process only outer JOINs (LEFT/RIGHT/FULL)
        for join in hir.joins.iter().filter(|j| j.join_type.is_outer()) {
            match join.join_type {
                crate::hir::JoinType::Left
                    if !protected_tables.contains(join.table.effective_name()) =>
                {
                    // Check joined table (right side)
                    self.check_table_in_join(join, &join.table, hir);
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
    /// If WHERE has IS NOT NULL check for ANY field from a table,
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
            ExprHir::ColumnRef { parts, .. } if parts.len() >= 2 => Some(parts[0].to_string()),
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

        // NOTE: Fields in ANY JOIN's ON condition are NOT checked.
        // Using a joined table's field in a JOIN ON condition is standard practice.
        // NULL in ON simply means "no match", which is expected LEFT JOIN behavior.
        // This applies both to the JOIN's own ON and other JOINs' ON conditions.

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
            ExprHir::ColumnRef { parts, range, .. } => {
                // Check if this references the protected table (first part is alias for 2+ parts)
                if parts.len() >= 2 {
                    let alias = &parts[0];
                    if alias.eq_ignore_ascii_case(protected_table) {
                        // Found unprotected reference! Column is the second part
                        unprotected_refs.push(crate::diagnostics::UnprotectedFieldRef {
                            table_alias: alias.to_string(),
                            field_name: parts[1].to_string(),
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

            ExprHir::Tuple { elements, .. } => {
                // Check all elements of the tuple
                for elem in elements {
                    self.find_unprotected_refs(elem, protected_table, unprotected_refs);
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

    /// Check for nested field dereference by dot (N+1 query problem).
    ///
    /// Called after HIR is built, uses only HIR data (no AST access).
    ///
    /// Detects:
    /// - ColumnRef with 3+ parts (e.g., `T.Ссылка.Организация`)
    /// - ColumnRef with 2+ parts inside virtual table parameters
    /// - CAST function with 2+ member_access fields
    ///
    /// Excludes MDO type paths (e.g., `Справочник.Валюты.Код`).
    pub(super) fn check_nested_fields_by_dot(&mut self, hir: &SdblHir) {
        self.collect_nested_field_diagnostics(hir);

        // Recursively check UNION subqueries
        for union in &hir.unions {
            self.check_nested_fields_by_dot(&union.query);
        }
    }

    /// Collect nested field dereference diagnostics from HIR.
    fn collect_nested_field_diagnostics(&mut self, hir: &SdblHir) {
        // Check SELECT fields
        for field in &hir.select.fields {
            self.check_expr_for_nested_fields(&field.expr, false);
        }

        // Check FROM tables (including virtual table params)
        for table in &hir.from {
            self.check_table_ref_for_nested_fields(table);
        }

        // Check JOINs
        for join in &hir.joins {
            self.check_table_ref_for_nested_fields(&join.table);
            if let Some(ref cond) = join.condition {
                self.check_expr_for_nested_fields(cond, false);
            }
        }

        // Check WHERE
        if let Some(ref where_expr) = hir.where_clause {
            self.check_expr_for_nested_fields(where_expr, false);
        }

        // Check GROUP BY
        if let Some(ref group_by) = hir.group_by {
            for expr in &group_by.exprs {
                self.check_expr_for_nested_fields(expr, false);
            }
        }

        // Check HAVING
        if let Some(ref having) = hir.having {
            self.check_expr_for_nested_fields(having, false);
        }

        // Check ORDER BY
        if let Some(ref order_by) = hir.order_by {
            for item in &order_by.items {
                self.check_expr_for_nested_fields(&item.expr, false);
            }
        }
    }

    /// Check table reference for nested fields (including virtual table params).
    fn check_table_ref_for_nested_fields(&mut self, table: &TableRef) {
        // Check virtual table parameters - here even 2-part paths are dereferences
        if table.is_virtual_table {
            for param in &table.virtual_table_params {
                self.check_expr_for_nested_fields(param, true);
            }
        }

        // Check subqueries
        for subquery in &table.subquery {
            self.collect_nested_field_diagnostics(subquery);
        }
    }

    /// Check expression for nested field dereferences.
    ///
    /// `in_virtual_table_params`: if true, even 2-part column refs are considered dereferences.
    fn check_expr_for_nested_fields(&mut self, expr: &ExprHir, in_virtual_table_params: bool) {
        match expr {
            ExprHir::ColumnRef { parts, range, .. } => {
                // Hard floor: 2+ parts (one or more dereferences) and not an MDO type
                // path. Within virtual-table parameters, every 2+ part column ref is
                // already an N+1 vector; in normal contexts the handler enforces a
                // configurable `minPathDepth` floor (default 3) on top.
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
                // Check args recursively
                for arg in args {
                    self.check_expr_for_nested_fields(arg, in_virtual_table_params);
                }

                // CAST with 2+ member access fields is a dereference
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

            // Leaf nodes - no recursion needed
            ExprHir::Literal { .. } | ExprHir::Parameter { .. } | ExprHir::Missing { .. } => {}
        }
    }

    /// Check SELECT fields for missing AS keyword.
    ///
    /// Called after HIR is built, uses only HIR data (no AST access).
    ///
    /// Rules:
    /// - Skip asterisk fields (*, Table.*)
    /// - Skip fields with parse errors
    /// - Emit diagnostic if:
    ///   - No alias at all
    ///   - Has alias but without AS keyword
    ///
    /// # Arguments
    /// * `hir` - The lowered HIR (must have select.fields populated)
    ///
    /// # Note
    /// Applies uniformly to main and UNION queries — every SELECT clause must
    /// declare explicit aliases per BSL standards. This is a deliberate
    /// divergence from BSL-LS, which historically excluded UNION secondary
    /// queries from this check.
    pub(super) fn check_alias_without_as_keyword(&mut self, hir: &SdblHir) {
        for field in &hir.select.fields {
            // Skip asterisk fields
            if field.is_asterisk {
                continue;
            }

            // Skip fields with parse errors
            if field.has_parse_error {
                continue;
            }

            // Check if needs diagnostic
            let needs_diagnostic = match &field.alias {
                Some(_) => !field.has_as_keyword, // Has alias but missing AS
                None => true,                     // No alias at all
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

    /// Check for redundant .Ссылка (Reference) field access.
    ///
    /// Called after HIR is built, uses only HIR data (no AST access).
    ///
    /// Detects patterns where a field before `.Ссылка` is already of a reference type,
    /// making the `.Ссылка` redundant (it causes an unnecessary implicit LEFT JOIN).
    ///
    /// Requires metadata context for type resolution. Without metadata, emits no diagnostics
    /// to avoid false positives.
    pub(super) fn check_ref_overuse(&mut self, hir: &SdblHir) {
        // Check all expressions in query
        self.check_ref_overuse_in_select(hir);
        self.check_ref_overuse_in_where(hir);
        self.check_ref_overuse_in_group_by(hir);
        self.check_ref_overuse_in_having(hir);
        self.check_ref_overuse_in_order_by(hir);
        self.check_ref_overuse_in_joins(hir);

        // Recursively check UNION subqueries
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

    /// Check expression for redundant .Ссылка usage.
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

            // Leaf nodes - no check needed
            ExprHir::Literal { .. } | ExprHir::Parameter { .. } | ExprHir::Missing { .. } => {}
        }
    }

    /// Check column reference for redundant .Ссылка usage.
    ///
    /// Algorithm:
    /// Find all positions of "Ссылка"/"Reference" in parts. For each position `ref_idx`:
    /// - Skip if `ref_idx` == 0 (impossible: table alias can't be Ссылка in this context)
    /// - Skip if `ref_idx` == 1 (e.g. `Alias.Ссылка` — just accessing the reference field, not redundant)
    /// - If `ref_idx` >= 2 — there is a field before `.Ссылка`. Resolve its type:
    ///   - Skip MDO prefix if present (e.g. Справочник, Документ)
    ///   - If type `is_ref()` → emit RefOveruse diagnostic
    ///   - If type is Unknown → no metadata available, stay silent (no false positives)
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

            // ref_idx == 0: impossible case (table alias is Ссылка), skip
            if ref_idx == 0 {
                continue;
            }

            // ref_idx == 1: `Alias.Ссылка` — accessing the reference field of a table, NOT redundant
            if ref_idx == 1 {
                continue;
            }

            // ref_idx >= 2: there is at least one field between the alias and `.Ссылка`
            // Determine alias and field chain before `.Ссылка`
            let (alias, chain_start) = if crate::is_mdo_type(parts[0].as_str()) {
                // Skip MDO type prefix: parts[0] is type, parts[1] is object name
                // alias is parts[1], chain starts at parts[2]
                if parts.len() < 3 {
                    continue;
                }
                (parts[1].as_str(), 2usize)
            } else {
                (parts[0].as_str(), 1usize)
            };

            // chain_end is ref_idx (exclusive) — the fields between alias and `.Ссылка`
            if chain_start >= ref_idx {
                // After stripping MDO prefix, ref_idx == 1 relative to alias: `MDO.Obj.Ссылка` → skip
                continue;
            }

            let chain: Vec<String> =
                parts[chain_start..ref_idx].iter().map(|n| n.to_string()).collect();

            let field_type = self.scope.resolve_nested_field_type(alias, &chain);

            if field_type.is_ref() {
                self.diagnostics.push(SdblDiagnostic::RefOveruse { range });
                // Emit once per ColumnRef (the whole expression is wrong)
                return;
            }
            // Unknown type → no metadata, stay silent to avoid false positives
        }
    }
}
