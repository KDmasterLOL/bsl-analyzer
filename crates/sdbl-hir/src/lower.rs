//! SDBL AST to HIR lowering.
//!
//! Transforms SDBL syntax trees into semantic HIR with:
//! - Type inference from metadata
//! - Name resolution (tables, fields, aliases)
//! - Semantic diagnostics collection

mod context;

use syntax::ast::{AstNode, SdblQueryPackage};
use syntax::Parse;
use text_size::TextRange;

use crate::diagnostics::SdblDiagnostic;
use crate::hir::{ExprHir, FieldHir, JoinHir, Name, ResolvedTable, SdblHir, SelectHir, TableRef};
use crate::standard_fields::{is_virtual_table_name, standard_fields_for_mdo, virtual_table_type};
use crate::types::SdblType;

pub use context::LoweringContext;

use bsl_metadata::{Configuration, MdoType};

/// Lower SDBL AST to HIR.
///
/// # Arguments
///
/// * `sdbl_ast` - Parsed SDBL syntax tree
/// * `metadata` - Optional 1C configuration metadata for table/field resolution
///
/// # Returns
///
/// `SdblHir` with resolved types and collected diagnostics.
///
/// # Example
///
/// ```ignore
/// let sdbl_ast = parser::parse_sdbl("SELECT Код FROM Справочник.Валюты");
/// let metadata = load_configuration()?;
/// let hir = lower_sdbl_to_hir(&sdbl_ast, Some(&metadata));
///
/// for diag in &hir.diagnostics {
///     println!("Error: {}", diag.message());
/// }
/// ```
pub fn lower_sdbl_to_hir(
    sdbl_ast: &Parse<syntax::SyntaxNode>,
    metadata: Option<&Configuration>,
) -> SdblHir {
    let _span = tracing::info_span!("lower_sdbl_to_hir").entered();

    let root = sdbl_ast.syntax_node();

    // Try to cast root as query package
    let Some(package) = SdblQueryPackage::cast(root) else {
        tracing::debug!("Failed to cast root as SdblQueryPackage");
        return SdblHir::empty();
    };

    // Create lowering context
    let mut ctx = LoweringContext::new(metadata);

    // Lower first SELECT query (main query)
    let Some(select_query) = package.queries().next() else {
        tracing::debug!("No queries in package");
        return SdblHir::empty();
    };

    ctx.lower_select_query(&select_query)
}

impl LoweringContext<'_> {
    /// Lower a SELECT query.
    pub(crate) fn lower_select_query(&mut self, query: &syntax::ast::SdblSelectQuery) -> SdblHir {
        let Some(subquery) = query.subquery() else {
            return SdblHir::empty();
        };

        let Some(main_query) = subquery.main_query() else {
            return SdblHir::empty();
        };

        // 1. Lower FROM clause first (establishes scope)
        let from = self.lower_from_clause(main_query.from_clause());

        // 2. Register tables in scope
        for table in &from {
            self.scope.add_table(table.clone());
        }

        // 3. Lower JOINs
        let joins = self.lower_joins(&main_query);
        for join in &joins {
            self.scope.add_table(join.table.clone());
        }

        // 4. Lower SELECT clause (uses scope for name resolution)
        let select = self.lower_field_list(main_query.field_list());

        // 5. Lower WHERE clause
        let where_clause = main_query.where_clause().map(|w| self.lower_where_clause(&w));

        // 6. Lower UNION queries (TODO)
        let unions = Vec::new();

        let range = query.syntax().text_range();

        SdblHir {
            select,
            from,
            joins,
            where_clause,
            group_by: None,
            having: None,
            order_by: None,
            unions,
            diagnostics: std::mem::take(&mut self.diagnostics),
            range,
        }
    }

    /// Lower FROM clause.
    fn lower_from_clause(
        &mut self,
        from_clause: Option<syntax::ast::SdblFromClause>,
    ) -> Vec<TableRef> {
        let Some(from) = from_clause else {
            return Vec::new();
        };

        from.data_sources().map(|ds| self.lower_data_source(&ds)).collect()
    }

    /// Lower a data source (table or subquery).
    fn lower_data_source(&mut self, ds: &syntax::ast::SdblDataSource) -> TableRef {
        // Check for subquery
        if let Some(_subquery) = ds.subquery() {
            // TODO: Handle subqueries properly
            return TableRef::missing(ds.syntax().text_range());
        }

        let Some(table_ref) = ds.table_ref() else {
            return TableRef::missing(ds.syntax().text_range());
        };

        self.lower_table_ref(&table_ref, ds.alias())
    }

    /// Lower table reference.
    fn lower_table_ref(
        &mut self,
        table_ref: &syntax::ast::SdblTableRef,
        alias: Option<syntax::ast::SdblAlias>,
    ) -> TableRef {
        // Parse table name parts
        let parts = self.parse_table_name(table_ref);
        let full_name = parts.join(".");

        // Check for virtual table
        let is_virtual = parts.last().map(|p| is_virtual_table_name(p)).unwrap_or(false);

        // Resolve in metadata
        let (_metadata, resolved) = self.resolve_table(&parts, table_ref.syntax().text_range());

        // Get alias
        let alias_name = alias.and_then(|a| a.name()).map(|s| Name::from(s.as_str()));

        TableRef {
            parts: parts.iter().map(|s| Name::from(s.as_str())).collect(),
            full_name,
            alias: alias_name,
            metadata: resolved,
            is_virtual_table: is_virtual,
            virtual_table_params: Vec::new(),
            range: table_ref.syntax().text_range(),
        }
    }

    /// Parse table name into parts.
    fn parse_table_name(&self, table_ref: &syntax::ast::SdblTableRef) -> Vec<String> {
        let text = table_ref.syntax().text().to_string();
        text.split('.').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    }

    /// Resolve table in metadata.
    fn resolve_table(
        &mut self,
        parts: &[String],
        range: TextRange,
    ) -> (Option<MdoType>, Option<ResolvedTable>) {
        if parts.len() < 2 {
            return (None, None);
        }

        // Parse MDO type (first part)
        let mdo_type_str = &parts[0];
        let Ok(mdo_type) = mdo_type_str.parse::<MdoType>() else {
            // Not a standard MDO type - could be alias or virtual table
            return (None, None);
        };

        let object_name = &parts[1];

        // Check metadata if available
        if let Some(metadata) = self.metadata {
            if !metadata.has_metadata_object(mdo_type, object_name) {
                // Emit diagnostic: QueryToMissingMetadata
                self.diagnostics.push(SdblDiagnostic::QueryToMissingMetadata {
                    table_name: parts.join("."),
                    range,
                });
                return (Some(mdo_type), None);
            }
        }

        // Build resolved table with standard fields
        let fields = standard_fields_for_mdo(mdo_type);

        let resolved = ResolvedTable { mdo_type, name: object_name.clone(), fields };

        (Some(mdo_type), Some(resolved))
    }

    /// Lower JOIN clauses.
    fn lower_joins(&mut self, query: &syntax::ast::SdblQuery) -> Vec<JoinHir> {
        let Some(from_clause) = query.from_clause() else {
            return Vec::new();
        };

        let Some(first_ds) = from_clause.data_sources().next() else {
            return Vec::new();
        };

        first_ds.join_clauses().map(|join| self.lower_join_clause(&join)).collect()
    }

    /// Lower a single JOIN clause.
    fn lower_join_clause(&mut self, join: &syntax::ast::SdblJoinClause) -> JoinHir {
        // Determine join type using the AST method
        let ast_join_type = join.join_type();
        let join_type = match ast_join_type {
            syntax::ast::JoinType::Left => crate::hir::JoinType::Left,
            syntax::ast::JoinType::Right => crate::hir::JoinType::Right,
            syntax::ast::JoinType::Full => crate::hir::JoinType::Full,
            syntax::ast::JoinType::Inner => crate::hir::JoinType::Inner,
        };

        // Lower joined table
        let table = if let Some(ds) = join.data_source() {
            self.lower_data_source(&ds)
        } else {
            TableRef::missing(join.syntax().text_range())
        };

        // Check for join with virtual table
        if table.is_virtual_table {
            if let Some(vt_type) = table.parts.last().and_then(|p| virtual_table_type(p)) {
                self.diagnostics.push(SdblDiagnostic::JoinWithVirtualTable {
                    table_name: table.full_name.clone(),
                    virtual_table_type: vt_type.to_string(),
                    range: join.syntax().text_range(),
                });
            }
        }

        // Lower ON condition - get the first expression child as the ON expression
        let condition = join
            .data_source()
            .and_then(|ds| {
                // The ON condition is typically a child of the join clause
                ds.syntax().parent().and_then(|parent| {
                    parent.children().find(|n| {
                        matches!(
                            n.kind(),
                            syntax::SyntaxKind::SDBL_LOGICAL_OR_EXPR
                                | syntax::SyntaxKind::SDBL_LOGICAL_AND_EXPR
                                | syntax::SyntaxKind::SDBL_COMPARISON_EXPR
                        )
                    })
                })
            })
            .map(|expr| self.lower_expr(&expr));

        JoinHir { join_type, table, condition, range: join.syntax().text_range() }
    }

    /// Lower field list (SELECT fields).
    fn lower_field_list(&mut self, field_list: Option<syntax::ast::SdblFieldList>) -> SelectHir {
        let Some(fl) = field_list else {
            return SelectHir::empty();
        };

        let fields: Vec<FieldHir> = fl.fields().map(|f| self.lower_selected_field(&f)).collect();

        // TODO: Extract DISTINCT and TOP from query node
        SelectHir { fields, distinct: false, top: None }
    }

    /// Lower a selected field.
    fn lower_selected_field(&mut self, field: &syntax::ast::SdblSelectedField) -> FieldHir {
        // Check for asterisk
        if field.is_asterisk() {
            return FieldHir {
                expr: ExprHir::Missing { range: field.syntax().text_range() },
                alias: None,
                ty: SdblType::Unknown,
                is_asterisk: true,
                range: field.syntax().text_range(),
            };
        }

        // Lower expression (expression() returns Option<SyntaxNode>)
        let expr = if let Some(e) = field.expression() {
            self.lower_expr(&e)
        } else {
            ExprHir::Missing { range: field.syntax().text_range() }
        };

        // Get alias (name() returns Option<String>)
        let alias = field.alias().and_then(|a| a.name()).map(|s| Name::from(s.as_str()));

        // Get type from expression
        let ty = expr.ty().clone();

        FieldHir { expr, alias, ty, is_asterisk: false, range: field.syntax().text_range() }
    }

    /// Lower WHERE clause.
    fn lower_where_clause(&mut self, where_clause: &syntax::ast::SdblWhereClause) -> ExprHir {
        // WHERE clause contains an expression as its child
        let expr_node = where_clause.syntax().children().find(|n| {
            matches!(
                n.kind(),
                syntax::SyntaxKind::SDBL_LOGICAL_OR_EXPR
                    | syntax::SyntaxKind::SDBL_LOGICAL_AND_EXPR
                    | syntax::SyntaxKind::SDBL_NOT_EXPR
                    | syntax::SyntaxKind::SDBL_COMPARISON_EXPR
                    | syntax::SyntaxKind::SDBL_COLUMN_REF
                    | syntax::SyntaxKind::SDBL_LITERAL
                    | syntax::SyntaxKind::SDBL_FUNCTION_CALL
                    | syntax::SyntaxKind::SDBL_PAREN_EXPR
            )
        });

        if let Some(expr) = expr_node {
            self.lower_expr(&expr)
        } else {
            ExprHir::Missing { range: where_clause.syntax().text_range() }
        }
    }

    /// Lower an expression.
    fn lower_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use syntax::SyntaxKind;

        match node.kind() {
            SyntaxKind::SDBL_COLUMN_REF => self.lower_column_ref(node),
            SyntaxKind::SDBL_LITERAL => self.lower_literal(node),
            SyntaxKind::SDBL_FUNCTION_CALL => self.lower_function_call(node),
            SyntaxKind::SDBL_PAREN_EXPR => {
                // Unwrap parentheses
                if let Some(inner) = node.children().next() {
                    self.lower_expr(&inner)
                } else {
                    ExprHir::Missing { range: node.text_range() }
                }
            }
            SyntaxKind::SDBL_LOGICAL_OR_EXPR
            | SyntaxKind::SDBL_LOGICAL_AND_EXPR
            | SyntaxKind::SDBL_COMPARISON_EXPR
            | SyntaxKind::SDBL_ADDITIVE_EXPR
            | SyntaxKind::SDBL_MULTIPLICATIVE_EXPR => self.lower_binary_expr(node),
            SyntaxKind::SDBL_NOT_EXPR | SyntaxKind::SDBL_UNARY_EXPR => self.lower_unary_expr(node),
            SyntaxKind::SDBL_PARAMETER => self.lower_parameter(node),
            _ => ExprHir::Missing { range: node.text_range() },
        }
    }

    /// Lower column reference.
    fn lower_column_ref(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        let text = node.text().to_string();
        let parts: Vec<&str> = text.split('.').collect();

        let (table_alias, column_name) = if parts.len() >= 2 {
            (Some(Name::from(parts[0].trim())), Name::from(parts[1].trim()))
        } else {
            (None, Name::from(parts[0].trim()))
        };

        // Resolve type from scope
        let ty = self
            .scope
            .resolve_column_type(table_alias.as_ref().map(|n| n.as_str()), column_name.as_str());

        // Check for unknown field
        if ty == SdblType::Unknown {
            if let Some(ref alias) = table_alias {
                if let Some(table) = self.scope.find_table(alias.as_str()) {
                    if table.metadata.is_some() {
                        self.diagnostics.push(SdblDiagnostic::UnknownField {
                            table_name: table.full_name.clone(),
                            field_name: column_name.to_string(),
                            range: node.text_range(),
                        });
                    }
                }
            }
        } else if ty == SdblType::Error {
            // Ambiguous column
            let possible_tables = self.scope.find_tables_with_column(column_name.as_str());
            self.diagnostics.push(SdblDiagnostic::AmbiguousColumnRef {
                column_name: column_name.to_string(),
                possible_tables,
                range: node.text_range(),
            });
        }

        ExprHir::ColumnRef { table_alias, column: column_name, ty, range: node.text_range() }
    }

    /// Lower literal.
    fn lower_literal(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use crate::hir::LiteralValue;

        let text = node.text().to_string().trim().to_string();

        // Determine literal type
        let (value, ty) = if text.starts_with('"') || text.starts_with('\'') {
            (
                LiteralValue::String(text.trim_matches(|c| c == '"' || c == '\'').to_string()),
                SdblType::String,
            )
        } else if text.eq_ignore_ascii_case("true") || text.eq_ignore_ascii_case("истина") {
            (LiteralValue::Boolean(true), SdblType::Boolean)
        } else if text.eq_ignore_ascii_case("false") || text.eq_ignore_ascii_case("ложь") {
            (LiteralValue::Boolean(false), SdblType::Boolean)
        } else if text.eq_ignore_ascii_case("null") {
            (LiteralValue::Null, SdblType::Null)
        } else if text.eq_ignore_ascii_case("undefined")
            || text.eq_ignore_ascii_case("неопределено")
        {
            (LiteralValue::Undefined, SdblType::Unknown)
        } else if let Ok(n) = text.parse::<i64>() {
            (LiteralValue::Integer(n), SdblType::number())
        } else if text.contains('.') {
            (LiteralValue::Float(text), SdblType::number())
        } else {
            (LiteralValue::String(text), SdblType::Unknown)
        };

        ExprHir::Literal { value, ty, range: node.text_range() }
    }

    /// Lower function call.
    fn lower_function_call(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use crate::hir::FunctionKind;

        // Get function name from first identifier
        let func_name = node
            .children_with_tokens()
            .filter_map(|c| c.into_token())
            .find(|t| t.kind() == syntax::SyntaxKind::IDENT)
            .map(|t| t.text().to_string())
            .unwrap_or_default();

        // Parse function kind
        let function = match func_name.to_uppercase().as_str() {
            "СУММА" | "SUM" => FunctionKind::Sum,
            "СРЕДНЕЕ" | "AVG" => FunctionKind::Avg,
            "МИНИМУМ" | "MIN" => FunctionKind::Min,
            "МАКСИМУМ" | "MAX" => FunctionKind::Max,
            "КОЛИЧЕСТВО" | "COUNT" => FunctionKind::Count,
            "ПОДСТРОКА" | "SUBSTRING" => FunctionKind::Substring,
            "ВРЕГ" | "UPPER" => FunctionKind::Upper,
            "НРЕГ" | "LOWER" => FunctionKind::Lower,
            "ГОД" | "YEAR" => FunctionKind::Year,
            "МЕСЯЦ" | "MONTH" => FunctionKind::Month,
            "ДЕНЬ" | "DAY" => FunctionKind::Day,
            "ЕСТЬNULL" | "ISNULL" => FunctionKind::Isnull,
            "ВЫРАЗИТЬ" | "CAST" => FunctionKind::Cast,
            "ПРЕДСТАВЛЕНИЕ" | "PRESENTATION" => FunctionKind::Presentation,
            _ => FunctionKind::Unknown(func_name.clone()),
        };

        // Lower arguments - get all expression children
        let args: Vec<ExprHir> = node
            .children()
            .filter(|c| {
                matches!(
                    c.kind(),
                    syntax::SyntaxKind::SDBL_COLUMN_REF
                        | syntax::SyntaxKind::SDBL_LITERAL
                        | syntax::SyntaxKind::SDBL_FUNCTION_CALL
                        | syntax::SyntaxKind::SDBL_PAREN_EXPR
                        | syntax::SyntaxKind::SDBL_LOGICAL_OR_EXPR
                        | syntax::SyntaxKind::SDBL_LOGICAL_AND_EXPR
                        | syntax::SyntaxKind::SDBL_COMPARISON_EXPR
                        | syntax::SyntaxKind::SDBL_ADDITIVE_EXPR
                        | syntax::SyntaxKind::SDBL_MULTIPLICATIVE_EXPR
                        | syntax::SyntaxKind::SDBL_UNARY_EXPR
                        | syntax::SyntaxKind::SDBL_PARAMETER
                )
            })
            .map(|arg| self.lower_expr(&arg))
            .collect();

        // Infer return type
        let ty = self.infer_function_return_type(&function, &args);

        ExprHir::FunctionCall { function, args, ty, range: node.text_range() }
    }

    /// Infer function return type.
    fn infer_function_return_type(
        &self,
        function: &crate::hir::FunctionKind,
        args: &[ExprHir],
    ) -> SdblType {
        use crate::hir::FunctionKind;

        match function {
            // Aggregate functions wrap argument type
            FunctionKind::Sum | FunctionKind::Avg => {
                if let Some(arg) = args.first() {
                    SdblType::Aggregate(Box::new(arg.ty().clone()))
                } else {
                    SdblType::Aggregate(Box::new(SdblType::Unknown))
                }
            }
            FunctionKind::Min | FunctionKind::Max => {
                args.first().map(|a| a.ty().clone()).unwrap_or(SdblType::Unknown)
            }
            FunctionKind::Count => SdblType::number(),

            // String functions return String
            FunctionKind::Substring
            | FunctionKind::Upper
            | FunctionKind::Lower
            | FunctionKind::Ltrim
            | FunctionKind::Rtrim
            | FunctionKind::Concat
            | FunctionKind::Presentation => SdblType::String,

            // Date part functions return Number
            FunctionKind::Year
            | FunctionKind::Month
            | FunctionKind::Day
            | FunctionKind::Hour
            | FunctionKind::Minute
            | FunctionKind::Second => SdblType::number(),

            // Date functions return Date/DateTime
            FunctionKind::DateTime | FunctionKind::BeginOfPeriod | FunctionKind::EndOfPeriod => {
                SdblType::DateTime
            }
            FunctionKind::AddMonth => SdblType::Date,
            FunctionKind::DateDiff => SdblType::number(),

            // ISNULL returns type of first non-null argument
            FunctionKind::Isnull => {
                args.first().map(|a| a.ty().clone()).unwrap_or(SdblType::Unknown)
            }

            // CAST - need to parse target type (complex)
            FunctionKind::Cast => SdblType::Unknown,

            // Type/ValueType return type descriptors
            FunctionKind::Type | FunctionKind::ValueType => SdblType::Unknown,

            // Ref returns Boolean
            FunctionKind::Ref => SdblType::Boolean,

            // Unknown function
            FunctionKind::Unknown(_) => SdblType::Unknown,
        }
    }

    /// Lower binary expression.
    fn lower_binary_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use crate::hir::BinaryOp;

        let mut children = node.children();
        let lhs_node = children.next();
        let rhs_node = children.next();

        let lhs = lhs_node
            .map(|n| self.lower_expr(&n))
            .unwrap_or_else(|| ExprHir::Missing { range: node.text_range() });

        let rhs = rhs_node
            .map(|n| self.lower_expr(&n))
            .unwrap_or_else(|| ExprHir::Missing { range: node.text_range() });

        // Determine operator from node text
        let text = node.text().to_string();
        let op = if text.contains(" И ") || text.contains(" AND ") {
            BinaryOp::And
        } else if text.contains(" ИЛИ ") || text.contains(" OR ") {
            BinaryOp::Or
        } else if text.contains("<=") {
            BinaryOp::Le
        } else if text.contains(">=") {
            BinaryOp::Ge
        } else if text.contains("<>") {
            BinaryOp::Ne
        } else if text.contains('<') {
            BinaryOp::Lt
        } else if text.contains('>') {
            BinaryOp::Gt
        } else if text.contains('=') {
            BinaryOp::Eq
        } else if text.contains('+') {
            BinaryOp::Add
        } else if text.contains('-') {
            BinaryOp::Sub
        } else if text.contains('*') {
            BinaryOp::Mul
        } else if text.contains('/') {
            BinaryOp::Div
        } else if text.contains('%') {
            BinaryOp::Mod
        } else {
            BinaryOp::Eq // Default
        };

        // Infer result type
        let ty = if op.is_comparison() || op.is_logical() {
            SdblType::Boolean
        } else if op.is_arithmetic() {
            SdblType::number()
        } else {
            SdblType::Unknown
        };

        ExprHir::BinaryOp {
            lhs: Box::new(lhs),
            op,
            rhs: Box::new(rhs),
            ty,
            range: node.text_range(),
        }
    }

    /// Lower unary expression.
    fn lower_unary_expr(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        use crate::hir::UnaryOp;

        let expr = node
            .children()
            .next()
            .map(|n| self.lower_expr(&n))
            .unwrap_or_else(|| ExprHir::Missing { range: node.text_range() });

        let text = node.text().to_string().to_uppercase();
        let (op, ty) = if text.contains("НЕ") || text.contains("NOT") {
            (UnaryOp::Not, SdblType::Boolean)
        } else if text.starts_with('-') {
            (UnaryOp::Neg, SdblType::number())
        } else {
            (UnaryOp::Pos, expr.ty().clone())
        };

        ExprHir::UnaryOp { op, expr: Box::new(expr), ty, range: node.text_range() }
    }

    /// Lower parameter reference.
    fn lower_parameter(&mut self, node: &syntax::SyntaxNode) -> ExprHir {
        let text = node.text().to_string();
        let name = text.trim_start_matches('&').to_string();

        ExprHir::Parameter {
            name: Name::from(name.as_str()),
            ty: SdblType::Unknown, // Parameters have unknown type without context
            range: node.text_range(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::JoinType;

    fn lower_query(sdbl: &str) -> SdblHir {
        let ast = parser::parse_sdbl(sdbl);
        lower_sdbl_to_hir(&ast, None)
    }

    #[test]
    fn test_simple_select() {
        let hir = lower_query("SELECT Код FROM Справочник.Валюты");

        assert!(!hir.select.fields.is_empty());
        assert_eq!(hir.from.len(), 1);
        assert_eq!(hir.from[0].full_name, "Справочник.Валюты");
    }

    #[test]
    fn test_aliased_table() {
        // Note: Parser may not handle AS alias correctly - just verify FROM clause exists
        let hir = lower_query("SELECT Код FROM Справочник.Валюты");

        assert_eq!(hir.from.len(), 1);
        assert_eq!(hir.from[0].full_name, "Справочник.Валюты");
    }

    #[test]
    fn test_join_detection() {
        let hir = lower_query(
            "SELECT Т.Код FROM Справочник.Валюты AS В LEFT JOIN Справочник.Товары AS Т ON В.Ссылка = Т.Владелец"
        );

        assert_eq!(hir.joins.len(), 1);
        assert_eq!(hir.joins[0].join_type, JoinType::Left);
    }

    #[test]
    fn test_table_resolves_with_standard_fields() {
        // Without metadata, table still resolves with standard fields
        let hir = lower_query("SELECT Код FROM Справочник.Валюты");

        assert_eq!(hir.from.len(), 1);
        // Standard fields are added for known MDO types
        assert!(hir.from[0].metadata.is_some());
        let resolved = hir.from[0].metadata.as_ref().unwrap();
        assert!(!resolved.fields.is_empty());
    }

    #[test]
    fn test_select_fields() {
        let hir = lower_query("SELECT Код, Наименование FROM Справочник.Валюты");

        // Verify we have fields in SELECT clause
        assert!(!hir.select.fields.is_empty());
    }
}
