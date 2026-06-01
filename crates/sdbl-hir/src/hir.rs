use smol_str::SmolStr;
use text_size::{TextRange, TextSize};

use crate::diagnostics::SdblDiagnostic;
use crate::types::SdblType;
use bsl_metadata::MdoType;

pub type Name = SmolStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdblHir {
    pub select: SelectHir,

    pub into_table: Option<Name>,

    pub from: Vec<TableRef>,

    pub joins: Vec<JoinHir>,

    pub where_clause: Option<ExprHir>,

    pub group_by: Option<GroupByHir>,

    pub having: Option<ExprHir>,

    pub order_by: Option<OrderByHir>,

    pub unions: Vec<UnionHir>,

    pub diagnostics: Vec<SdblDiagnostic>,

    pub range: TextRange,
}

impl SdblHir {
    pub fn empty() -> Self {
        Self {
            select: SelectHir::empty(),
            into_table: None,
            from: Vec::new(),
            joins: Vec::new(),
            where_clause: None,
            group_by: None,
            having: None,
            order_by: None,
            unions: Vec::new(),
            diagnostics: Vec::new(),
            range: syntax::MODULE_RANGE,
        }
    }

    pub fn has_errors(&self) -> bool {
        !self.diagnostics.is_empty()
    }

    pub fn all_tables(&self) -> impl Iterator<Item = &TableRef> {
        self.from.iter().chain(self.joins.iter().map(|j| &j.table))
    }

    /// Every metadata-resolved table read anywhere in this query tree: top-level
    /// FROM/JOIN sources plus tables nested in subqueries, unions, and expression
    /// subqueries (WHERE/HAVING/SELECT/JOIN-ON/IN/CASE/virtual-table params).
    /// Unresolved (by-name-only) and temp tables carry no metadata and are absent.
    pub fn collect_resolved_tables<'a>(&'a self, out: &mut Vec<&'a ResolvedTable>) {
        for table in self.all_tables() {
            if let Some(meta) = &table.metadata {
                out.push(meta);
            }
            for param in &table.virtual_table_params {
                collect_expr_tables(param, out);
            }
            for sub in &table.subquery {
                sub.collect_resolved_tables(out);
            }
        }
        for field in &self.select.fields {
            collect_expr_tables(&field.expr, out);
        }
        for join in &self.joins {
            if let Some(cond) = &join.condition {
                collect_expr_tables(cond, out);
            }
        }
        if let Some(where_expr) = &self.where_clause {
            collect_expr_tables(where_expr, out);
        }
        if let Some(group_by) = &self.group_by {
            for expr in &group_by.exprs {
                collect_expr_tables(expr, out);
            }
        }
        if let Some(having) = &self.having {
            collect_expr_tables(having, out);
        }
        if let Some(order_by) = &self.order_by {
            for item in &order_by.items {
                collect_expr_tables(&item.expr, out);
            }
        }
        for union in &self.unions {
            union.query.collect_resolved_tables(out);
        }
    }
}

/// Recurse an expression, descending into embedded subqueries to collect their
/// metadata-resolved tables.
fn collect_expr_tables<'a>(expr: &'a ExprHir, out: &mut Vec<&'a ResolvedTable>) {
    match expr {
        ExprHir::Subquery { query, .. } => query.collect_resolved_tables(out),
        ExprHir::In { expr: inner, values, .. } => {
            collect_expr_tables(inner, out);
            match values {
                InValues::List(items) => {
                    for item in items {
                        collect_expr_tables(item, out);
                    }
                }
                InValues::Subquery(sq) => sq.collect_resolved_tables(out),
            }
        }
        ExprHir::BinaryOp { lhs, rhs, .. } => {
            collect_expr_tables(lhs, out);
            collect_expr_tables(rhs, out);
        }
        ExprHir::UnaryOp { expr: inner, .. } => collect_expr_tables(inner, out),
        ExprHir::FunctionCall { args, .. } => {
            for arg in args {
                collect_expr_tables(arg, out);
            }
        }
        ExprHir::Case { operand, when_clauses, else_expr, .. } => {
            if let Some(op) = operand {
                collect_expr_tables(op, out);
            }
            for clause in when_clauses {
                collect_expr_tables(&clause.condition, out);
                collect_expr_tables(&clause.result, out);
            }
            if let Some(else_e) = else_expr {
                collect_expr_tables(else_e, out);
            }
        }
        ExprHir::Between { expr: inner, low, high, .. } => {
            collect_expr_tables(inner, out);
            collect_expr_tables(low, out);
            collect_expr_tables(high, out);
        }
        ExprHir::Like { expr: inner, pattern, escape, .. } => {
            collect_expr_tables(inner, out);
            collect_expr_tables(pattern, out);
            if let Some(esc) = escape {
                collect_expr_tables(esc, out);
            }
        }
        ExprHir::IsNull { expr: inner, .. } => collect_expr_tables(inner, out),
        ExprHir::Tuple { elements, .. } => {
            for elem in elements {
                collect_expr_tables(elem, out);
            }
        }
        ExprHir::ColumnRef { .. }
        | ExprHir::Literal { .. }
        | ExprHir::Parameter { .. }
        | ExprHir::Missing { .. } => {}
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectHir {
    pub fields: Vec<FieldHir>,

    pub distinct: bool,

    pub top: Option<u32>,
}

impl SelectHir {
    pub fn empty() -> Self {
        Self { fields: Vec::new(), distinct: false, top: None }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldHir {
    pub expr: ExprHir,

    pub alias: Option<Name>,

    pub has_as_keyword: bool,

    pub has_parse_error: bool,

    pub raw_name: Option<Name>,

    pub ty: SdblType,

    pub is_asterisk: bool,

    pub asterisk_qualifier: Option<String>,

    pub diagnostic_range: TextRange,

    pub range: TextRange,
}

impl FieldHir {
    pub fn alias_or_name(&self) -> Option<&Name> {
        self.alias.as_ref().or_else(|| self.expr.column_name()).or(self.raw_name.as_ref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRef {
    pub parts: Vec<Name>,

    pub full_name: String,

    pub alias: Option<Name>,

    pub metadata: Option<ResolvedTable>,

    pub is_virtual_table: bool,

    pub virtual_table_params: Vec<ExprHir>,

    pub subquery: Vec<Box<SdblHir>>,

    pub range: TextRange,
}

impl TableRef {
    pub fn missing(range: TextRange) -> Self {
        Self {
            parts: Vec::new(),
            full_name: String::new(),
            alias: None,
            metadata: None,
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            subquery: Vec::new(),
            range,
        }
    }

    pub fn effective_name(&self) -> &str {
        self.alias.as_ref().map(|a| a.as_str()).unwrap_or(&self.full_name)
    }

    pub fn is_resolved(&self) -> bool {
        self.metadata.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedTable {
    Metadata {
        mdo_type: MdoType,
        name: String,
        fields: Vec<FieldDef>,
    },

    Register {
        mdo_type: MdoType,
        name: String,
        fields: Vec<FieldDef>,
        dimensions: Vec<FieldDef>,
        resources: Vec<FieldDef>,
        attributes: Vec<FieldDef>,
    },

    TempTable {
        name: String,
        fields: Vec<FieldDef>,
    },
}

impl ResolvedTable {
    pub fn find_field(&self, name: &str) -> Option<&FieldDef> {
        let name_lower = name.to_lowercase();
        self.fields().iter().find(|f| {
            f.name.to_lowercase() == name_lower
                || f.name_en.as_ref().map(|en| en.to_lowercase() == name_lower).unwrap_or(false)
        })
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Metadata { name, .. } => name,
            Self::Register { name, .. } => name,
            Self::TempTable { name, .. } => name,
        }
    }

    pub fn fields(&self) -> &[FieldDef] {
        match self {
            Self::Metadata { fields, .. } => fields,
            Self::Register { fields, .. } => fields,
            Self::TempTable { fields, .. } => fields,
        }
    }

    pub fn dimensions(&self) -> &[FieldDef] {
        match self {
            Self::Register { dimensions, .. } => dimensions,
            _ => &[],
        }
    }

    pub fn resources(&self) -> &[FieldDef] {
        match self {
            Self::Register { resources, .. } => resources,
            _ => &[],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDef {
    pub name: String,

    pub name_en: Option<String>,

    pub ty: SdblType,

    pub is_standard: bool,
}

impl FieldDef {
    pub fn new(name: impl Into<String>, ty: SdblType) -> Self {
        Self { name: name.into(), name_en: None, ty, is_standard: false }
    }

    pub fn standard(name: impl Into<String>, name_en: impl Into<String>, ty: SdblType) -> Self {
        Self { name: name.into(), name_en: Some(name_en.into()), ty, is_standard: true }
    }

    pub fn new_with_names(
        name: String,
        name_en: Option<String>,
        ty: SdblType,
        is_standard: bool,
    ) -> Self {
        Self { name, name_en, ty, is_standard }
    }

    pub fn matches_name(&self, name: &str) -> bool {
        let name_lower = name.to_lowercase();
        self.name.to_lowercase() == name_lower
            || self.name_en.as_ref().map(|n| n.to_lowercase() == name_lower).unwrap_or(false)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinHir {
    pub join_type: JoinType,

    pub table: TableRef,

    pub condition: Option<ExprHir>,

    pub range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JoinType {
    Inner,
    Left,
    Right,
    Full,
    Cross,
}

impl JoinType {
    pub fn is_outer(&self) -> bool {
        matches!(self, Self::Left | Self::Right | Self::Full)
    }
}

impl std::fmt::Display for JoinType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inner => write!(f, "INNER"),
            Self::Left => write!(f, "LEFT"),
            Self::Right => write!(f, "RIGHT"),
            Self::Full => write!(f, "FULL"),
            Self::Cross => write!(f, "CROSS"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupByHir {
    pub exprs: Vec<ExprHir>,

    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderByHir {
    pub items: Vec<OrderByItem>,

    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderByItem {
    pub expr: ExprHir,

    pub direction: SortDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SortDirection {
    #[default]
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnionHir {
    pub all: bool,

    pub query: Box<SdblHir>,

    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprHir {
    ColumnRef {
        parts: Vec<Name>,
        ty: SdblType,
        range: TextRange,
    },

    Literal {
        value: LiteralValue,
        ty: SdblType,
        range: TextRange,
    },

    BinaryOp {
        lhs: Box<ExprHir>,
        op: BinaryOp,
        rhs: Box<ExprHir>,
        ty: SdblType,
        range: TextRange,
    },

    UnaryOp {
        op: UnaryOp,
        expr: Box<ExprHir>,
        ty: SdblType,
        range: TextRange,
    },

    FunctionCall {
        function: FunctionKind,
        args: Vec<ExprHir>,
        member_access: Vec<Name>,
        ty: SdblType,
        range: TextRange,
    },

    Case {
        operand: Option<Box<ExprHir>>,
        when_clauses: Vec<WhenClause>,
        else_expr: Option<Box<ExprHir>>,
        ty: SdblType,
        range: TextRange,
    },

    Subquery {
        query: Box<SdblHir>,
        ty: SdblType,
        range: TextRange,
    },

    Parameter {
        name: Name,
        ty: SdblType,
        range: TextRange,
    },

    In {
        expr: Box<ExprHir>,
        negated: bool,
        values: InValues,
        ty: SdblType,
        range: TextRange,
    },

    Between {
        expr: Box<ExprHir>,
        negated: bool,
        low: Box<ExprHir>,
        high: Box<ExprHir>,
        ty: SdblType,
        range: TextRange,
    },

    Like {
        expr: Box<ExprHir>,
        negated: bool,
        pattern: Box<ExprHir>,
        escape: Option<Box<ExprHir>>,
        ty: SdblType,
        range: TextRange,
    },

    IsNull {
        expr: Box<ExprHir>,
        negated: bool,
        ty: SdblType,
        range: TextRange,
    },

    Tuple {
        elements: Vec<ExprHir>,
        range: TextRange,
    },

    Missing {
        range: TextRange,
    },
}

impl ExprHir {
    pub fn ty(&self) -> &SdblType {
        match self {
            Self::ColumnRef { ty, .. } => ty,
            Self::Literal { ty, .. } => ty,
            Self::BinaryOp { ty, .. } => ty,
            Self::UnaryOp { ty, .. } => ty,
            Self::FunctionCall { ty, .. } => ty,
            Self::Case { ty, .. } => ty,
            Self::Subquery { ty, .. } => ty,
            Self::Parameter { ty, .. } => ty,
            Self::In { ty, .. } => ty,
            Self::Between { ty, .. } => ty,
            Self::Like { ty, .. } => ty,
            Self::IsNull { ty, .. } => ty,
            Self::Tuple { .. } => &SdblType::Unknown,
            Self::Missing { .. } => &SdblType::Error,
        }
    }

    pub fn range(&self) -> TextRange {
        match self {
            Self::ColumnRef { range, .. } => *range,
            Self::Literal { range, .. } => *range,
            Self::BinaryOp { range, .. } => *range,
            Self::UnaryOp { range, .. } => *range,
            Self::FunctionCall { range, .. } => *range,
            Self::Case { range, .. } => *range,
            Self::Subquery { range, .. } => *range,
            Self::Parameter { range, .. } => *range,
            Self::In { range, .. } => *range,
            Self::Between { range, .. } => *range,
            Self::Like { range, .. } => *range,
            Self::IsNull { range, .. } => *range,
            Self::Tuple { range, .. } => *range,
            Self::Missing { range } => *range,
        }
    }

    pub fn column_name(&self) -> Option<&Name> {
        match self {
            Self::ColumnRef { parts, .. } => parts.last(),
            _ => None,
        }
    }

    pub fn table_alias(&self) -> Option<&Name> {
        match self {
            Self::ColumnRef { parts, .. } if parts.len() >= 2 => parts.first(),
            _ => None,
        }
    }

    pub fn is_nested_field_access(&self) -> bool {
        match self {
            Self::ColumnRef { parts, .. } => parts.len() >= 3,
            _ => false,
        }
    }

    pub fn column_parts(&self) -> Option<&[Name]> {
        match self {
            Self::ColumnRef { parts, .. } => Some(parts),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiteralValue {
    Integer(i64),
    Float(String),
    String(String),
    Boolean(bool),
    Date { year: u16, month: u8, day: u8 },
    DateTime { year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8 },
    Null,
    Undefined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,

    And,
    Or,

    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

impl BinaryOp {
    pub fn is_comparison(&self) -> bool {
        matches!(self, Self::Eq | Self::Ne | Self::Lt | Self::Le | Self::Gt | Self::Ge)
    }

    pub fn is_logical(&self) -> bool {
        matches!(self, Self::And | Self::Or)
    }

    pub fn is_arithmetic(&self) -> bool {
        matches!(self, Self::Add | Self::Sub | Self::Mul | Self::Div | Self::Mod)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    Not,
    Neg,
    Pos,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FunctionKind {
    Sum,
    Avg,
    Min,
    Max,
    Count,

    Substring,
    Upper,
    Lower,
    Ltrim,
    Rtrim,
    Concat,

    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
    AddMonth,
    BeginOfPeriod,
    EndOfPeriod,
    DateTime,
    DateDiff,

    Cast,
    Isnull,
    Type,
    ValueType,
    Presentation,
    Value,

    Ref,

    Unknown(String),
}

impl FunctionKind {
    pub fn is_aggregate(&self) -> bool {
        matches!(self, Self::Sum | Self::Avg | Self::Min | Self::Max | Self::Count)
    }

    pub fn name_ru(&self) -> &str {
        match self {
            Self::Sum => "СУММА",
            Self::Avg => "СРЕДНЕЕ",
            Self::Min => "МИНИМУМ",
            Self::Max => "МАКСИМУМ",
            Self::Count => "КОЛИЧЕСТВО",
            Self::Substring => "ПОДСТРОКА",
            Self::Upper => "ВРЕГ",
            Self::Lower => "НРЕГ",
            Self::Ltrim => "СОКРЛ",
            Self::Rtrim => "СОКРП",
            Self::Concat => "КОНКАТ",
            Self::Year => "ГОД",
            Self::Month => "МЕСЯЦ",
            Self::Day => "ДЕНЬ",
            Self::Hour => "ЧАС",
            Self::Minute => "МИНУТА",
            Self::Second => "СЕКУНДА",
            Self::AddMonth => "ДОБАВИТЬМЕСЯЦ",
            Self::BeginOfPeriod => "НАЧАЛОПЕРИОДА",
            Self::EndOfPeriod => "КОНЕЦПЕРИОДА",
            Self::DateTime => "ДАТАВРЕМЯ",
            Self::DateDiff => "РАЗНОСТЬДАТ",
            Self::Cast => "ВЫРАЗИТЬ",
            Self::Isnull => "ЕСТЬNULL",
            Self::Type => "ТИП",
            Self::ValueType => "ТИПЗНАЧЕНИЯ",
            Self::Presentation => "ПРЕДСТАВЛЕНИЕ",
            Self::Value => "ЗНАЧЕНИЕ",
            Self::Ref => "ССЫЛКА",
            Self::Unknown(name) => name.as_str(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhenClause {
    pub condition: ExprHir,
    pub result: ExprHir,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InValues {
    List(Vec<ExprHir>),
    Subquery(Box<SdblHir>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdblQuery {
    pub hir: SdblHir,

    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdblPackage {
    pub(crate) queries: Vec<SdblQuery>,

    pub source_map: crate::source_map::SdblSourceMap,
}

impl SdblPackage {
    pub fn query_at_offset(&self, offset: TextSize) -> Option<&SdblQuery> {
        self.queries.iter().find(|q| q.range.start() <= offset && offset <= q.range.end())
    }

    pub fn queries(&self) -> &[SdblQuery] {
        &self.queries
    }

    pub fn all_diagnostics(&self) -> impl Iterator<Item = &crate::diagnostics::SdblDiagnostic> {
        let mut all = Vec::new();
        for q in &self.queries {
            Self::collect_diagnostics_recursive(&q.hir, &mut all);
        }
        all.into_iter()
    }

    fn collect_diagnostics_recursive<'a>(
        hir: &'a SdblHir,
        diagnostics: &mut Vec<&'a crate::diagnostics::SdblDiagnostic>,
    ) {
        diagnostics.extend(hir.diagnostics.iter());

        for field in &hir.select.fields {
            Self::collect_expr_diagnostics(&field.expr, diagnostics);
        }

        for table in &hir.from {
            for subquery in &table.subquery {
                Self::collect_diagnostics_recursive(subquery, diagnostics);
            }
        }

        for join in &hir.joins {
            for subquery in &join.table.subquery {
                Self::collect_diagnostics_recursive(subquery, diagnostics);
            }
            if let Some(ref cond) = join.condition {
                Self::collect_expr_diagnostics(cond, diagnostics);
            }
        }

        if let Some(ref where_expr) = hir.where_clause {
            Self::collect_expr_diagnostics(where_expr, diagnostics);
        }

        if let Some(ref group_by) = hir.group_by {
            for expr in &group_by.exprs {
                Self::collect_expr_diagnostics(expr, diagnostics);
            }
        }

        if let Some(ref having) = hir.having {
            Self::collect_expr_diagnostics(having, diagnostics);
        }

        if let Some(ref order_by) = hir.order_by {
            for item in &order_by.items {
                Self::collect_expr_diagnostics(&item.expr, diagnostics);
            }
        }

        for union in &hir.unions {
            Self::collect_diagnostics_recursive(&union.query, diagnostics);
        }
    }

    fn collect_expr_diagnostics<'a>(
        expr: &'a ExprHir,
        diagnostics: &mut Vec<&'a crate::diagnostics::SdblDiagnostic>,
    ) {
        match expr {
            ExprHir::In { expr: inner, values, .. } => {
                Self::collect_expr_diagnostics(inner, diagnostics);
                match values {
                    InValues::List(items) => {
                        for item in items {
                            Self::collect_expr_diagnostics(item, diagnostics);
                        }
                    }
                    InValues::Subquery(sq) => {
                        Self::collect_diagnostics_recursive(sq, diagnostics);
                    }
                }
            }
            ExprHir::Subquery { query, .. } => {
                Self::collect_diagnostics_recursive(query, diagnostics);
            }
            ExprHir::BinaryOp { lhs, rhs, .. } => {
                Self::collect_expr_diagnostics(lhs, diagnostics);
                Self::collect_expr_diagnostics(rhs, diagnostics);
            }
            ExprHir::UnaryOp { expr: inner, .. } => {
                Self::collect_expr_diagnostics(inner, diagnostics);
            }
            ExprHir::FunctionCall { args, .. } => {
                for arg in args {
                    Self::collect_expr_diagnostics(arg, diagnostics);
                }
            }
            ExprHir::Case { operand, when_clauses, else_expr, .. } => {
                if let Some(op) = operand {
                    Self::collect_expr_diagnostics(op, diagnostics);
                }
                for clause in when_clauses {
                    Self::collect_expr_diagnostics(&clause.condition, diagnostics);
                    Self::collect_expr_diagnostics(&clause.result, diagnostics);
                }
                if let Some(else_e) = else_expr {
                    Self::collect_expr_diagnostics(else_e, diagnostics);
                }
            }
            ExprHir::Between { expr: inner, low, high, .. } => {
                Self::collect_expr_diagnostics(inner, diagnostics);
                Self::collect_expr_diagnostics(low, diagnostics);
                Self::collect_expr_diagnostics(high, diagnostics);
            }
            ExprHir::Like { expr: inner, pattern, escape, .. } => {
                Self::collect_expr_diagnostics(inner, diagnostics);
                Self::collect_expr_diagnostics(pattern, diagnostics);
                if let Some(esc) = escape {
                    Self::collect_expr_diagnostics(esc, diagnostics);
                }
            }
            ExprHir::IsNull { expr: inner, .. } => {
                Self::collect_expr_diagnostics(inner, diagnostics);
            }
            ExprHir::Tuple { elements, .. } => {
                for elem in elements {
                    Self::collect_expr_diagnostics(elem, diagnostics);
                }
            }
            ExprHir::ColumnRef { .. }
            | ExprHir::Literal { .. }
            | ExprHir::Parameter { .. }
            | ExprHir::Missing { .. } => {}
        }
    }

    pub fn empty() -> Self {
        Self { queries: Vec::new(), source_map: crate::source_map::SdblSourceMap::new() }
    }
}
