//! SDBL HIR structures.
//!
//! High-level Intermediate Representation of SDBL queries.

use smol_str::SmolStr;
use text_size::TextRange;

use crate::diagnostics::SdblDiagnostic;
use crate::types::SdblType;
use bsl_metadata::MdoType;

/// Name in SDBL (field, alias, table name).
pub type Name = SmolStr;

/// HIR representation of a complete SDBL query.
///
/// Created by lowering SDBL AST with metadata context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdblHir {
    /// SELECT clause with typed fields.
    pub select: SelectHir,

    /// INTO clause - temporary table name (if present).
    /// Example: SELECT ... INTO TemporaryTable FROM ...
    pub into_table: Option<Name>,

    /// FROM clause with resolved tables.
    pub from: Vec<TableRef>,

    /// JOIN clauses.
    pub joins: Vec<JoinHir>,

    /// WHERE clause expression (optional).
    pub where_clause: Option<ExprHir>,

    /// GROUP BY clause (optional).
    pub group_by: Option<GroupByHir>,

    /// HAVING clause (optional).
    pub having: Option<ExprHir>,

    /// ORDER BY clause (optional).
    pub order_by: Option<OrderByHir>,

    /// UNION queries (if any).
    pub unions: Vec<UnionHir>,

    /// Semantic diagnostics collected during lowering.
    pub diagnostics: Vec<SdblDiagnostic>,

    /// Source range in SDBL text.
    pub range: TextRange,
}

impl SdblHir {
    /// Create an empty HIR (for error cases).
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
            range: TextRange::empty(0.into()),
        }
    }

    /// Check if HIR has any semantic errors.
    pub fn has_errors(&self) -> bool {
        !self.diagnostics.is_empty()
    }

    /// Get all tables in scope (FROM + JOINs).
    pub fn all_tables(&self) -> impl Iterator<Item = &TableRef> {
        self.from.iter().chain(self.joins.iter().map(|j| &j.table))
    }
}

/// SELECT clause HIR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectHir {
    /// Selected fields.
    pub fields: Vec<FieldHir>,

    /// DISTINCT modifier.
    pub distinct: bool,

    /// TOP N modifier.
    pub top: Option<u32>,
}

impl SelectHir {
    /// Create an empty SELECT clause.
    pub fn empty() -> Self {
        Self { fields: Vec::new(), distinct: false, top: None }
    }
}

/// SELECT field with inferred type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldHir {
    /// Field expression (column ref, literal, function call, etc.).
    pub expr: ExprHir,

    /// Field alias (if specified with AS).
    pub alias: Option<Name>,

    /// Inferred type from metadata.
    pub ty: SdblType,

    /// Is this an asterisk field (* or Table.*).
    pub is_asterisk: bool,

    /// Source range in SDBL.
    pub range: TextRange,
}

impl FieldHir {
    /// Get alias if present, otherwise try to get name from expression.
    pub fn alias_or_name(&self) -> Option<&Name> {
        self.alias.as_ref().or_else(|| self.expr.column_name())
    }
}

/// Table reference with metadata link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableRef {
    /// Table identifier parts (e.g., ["Справочник", "Валюты"]).
    pub parts: Vec<Name>,

    /// Full table name (e.g., "Справочник.Валюты").
    pub full_name: String,

    /// Table alias.
    pub alias: Option<Name>,

    /// Resolved metadata reference.
    pub metadata: Option<ResolvedTable>,

    /// Is this a virtual table (SliceLast, Balance, etc.).
    pub is_virtual_table: bool,

    /// Virtual table parameters (if virtual).
    pub virtual_table_params: Vec<ExprHir>,

    /// Source range.
    pub range: TextRange,
}

impl TableRef {
    /// Create a missing/error table reference.
    pub fn missing(range: TextRange) -> Self {
        Self {
            parts: Vec::new(),
            full_name: String::new(),
            alias: None,
            metadata: None,
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            range,
        }
    }

    /// Get effective name (alias if present, otherwise full name).
    pub fn effective_name(&self) -> &str {
        self.alias.as_ref().map(|a| a.as_str()).unwrap_or(&self.full_name)
    }

    /// Check if table is resolved to metadata.
    pub fn is_resolved(&self) -> bool {
        self.metadata.is_some()
    }
}

/// Resolved table reference - either from metadata or temporary table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedTable {
    /// Table from 1C metadata (Catalog, Document, Register, etc.).
    Metadata {
        /// MDO type (Catalog, Document, InformationRegister, etc.).
        mdo_type: MdoType,
        /// Object name.
        name: String,
        /// Available fields from metadata.
        fields: Vec<FieldDef>,
    },

    /// Temporary table created with INTO clause.
    TempTable {
        /// Table name.
        name: String,
        /// Fields from SELECT clause of the query that created this table.
        fields: Vec<FieldDef>,
    },
}

impl ResolvedTable {
    /// Find field by name (case-insensitive).
    pub fn find_field(&self, name: &str) -> Option<&FieldDef> {
        let name_lower = name.to_lowercase();
        let fields = match self {
            ResolvedTable::Metadata { fields, .. } => fields,
            ResolvedTable::TempTable { fields, .. } => fields,
        };
        fields.iter().find(|f| f.name.to_lowercase() == name_lower)
    }

    /// Get table name.
    pub fn name(&self) -> &str {
        match self {
            ResolvedTable::Metadata { name, .. } => name,
            ResolvedTable::TempTable { name, .. } => name,
        }
    }

    /// Get all fields.
    pub fn fields(&self) -> &[FieldDef] {
        match self {
            ResolvedTable::Metadata { fields, .. } => fields,
            ResolvedTable::TempTable { fields, .. } => fields,
        }
    }
}

/// Field definition from metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDef {
    /// Field name (Russian).
    pub name: String,

    /// English name (if available).
    pub name_en: Option<String>,

    /// Field type.
    pub ty: SdblType,

    /// Is standard attribute (Ссылка, Код, Наименование, etc.).
    pub is_standard: bool,
}

impl FieldDef {
    /// Create a new field definition.
    pub fn new(name: impl Into<String>, ty: SdblType) -> Self {
        Self { name: name.into(), name_en: None, ty, is_standard: false }
    }

    /// Create a standard attribute field.
    pub fn standard(name: impl Into<String>, name_en: impl Into<String>, ty: SdblType) -> Self {
        Self { name: name.into(), name_en: Some(name_en.into()), ty, is_standard: true }
    }

    /// Check if name matches (case-insensitive, bilingual).
    pub fn matches_name(&self, name: &str) -> bool {
        let name_lower = name.to_lowercase();
        self.name.to_lowercase() == name_lower
            || self.name_en.as_ref().map(|n| n.to_lowercase() == name_lower).unwrap_or(false)
    }
}

/// JOIN clause HIR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinHir {
    /// Join type (LEFT, RIGHT, FULL, INNER).
    pub join_type: JoinType,

    /// Joined table.
    pub table: TableRef,

    /// Join condition (ON clause).
    pub condition: Option<ExprHir>,

    /// Source range.
    pub range: TextRange,
}

/// JOIN type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JoinType {
    Inner,
    Left,
    Right,
    Full,
    Cross,
}

impl JoinType {
    /// Check if this is an outer join (LEFT, RIGHT, or FULL).
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

/// GROUP BY clause HIR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupByHir {
    /// Grouping expressions.
    pub exprs: Vec<ExprHir>,

    /// Source range.
    pub range: TextRange,
}

/// ORDER BY clause HIR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderByHir {
    /// Ordering items.
    pub items: Vec<OrderByItem>,

    /// Source range.
    pub range: TextRange,
}

/// ORDER BY item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderByItem {
    /// Expression to order by.
    pub expr: ExprHir,

    /// Sort direction.
    pub direction: SortDirection,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SortDirection {
    #[default]
    Asc,
    Desc,
}

/// UNION clause HIR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnionHir {
    /// Is this UNION ALL?
    pub all: bool,

    /// Unioned query.
    pub query: Box<SdblHir>,

    /// Source range.
    pub range: TextRange,
}

/// SDBL expression HIR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprHir {
    /// Column reference (Table.Field or Field).
    ColumnRef {
        /// Table alias (if qualified).
        table_alias: Option<Name>,
        /// Column name.
        column: Name,
        /// Inferred type.
        ty: SdblType,
        /// Source range.
        range: TextRange,
    },

    /// Literal value.
    Literal {
        /// Literal value.
        value: LiteralValue,
        /// Type.
        ty: SdblType,
        /// Source range.
        range: TextRange,
    },

    /// Binary operation.
    BinaryOp {
        /// Left operand.
        lhs: Box<ExprHir>,
        /// Operator.
        op: BinaryOp,
        /// Right operand.
        rhs: Box<ExprHir>,
        /// Result type.
        ty: SdblType,
        /// Source range.
        range: TextRange,
    },

    /// Unary operation.
    UnaryOp {
        /// Operator.
        op: UnaryOp,
        /// Operand.
        expr: Box<ExprHir>,
        /// Result type.
        ty: SdblType,
        /// Source range.
        range: TextRange,
    },

    /// Function call.
    FunctionCall {
        /// Function.
        function: FunctionKind,
        /// Arguments.
        args: Vec<ExprHir>,
        /// Return type.
        ty: SdblType,
        /// Source range.
        range: TextRange,
    },

    /// CASE expression.
    Case {
        /// Case operand (for simple CASE).
        operand: Option<Box<ExprHir>>,
        /// WHEN clauses.
        when_clauses: Vec<WhenClause>,
        /// ELSE expression.
        else_expr: Option<Box<ExprHir>>,
        /// Result type.
        ty: SdblType,
        /// Source range.
        range: TextRange,
    },

    /// Subquery expression.
    Subquery {
        /// Inner query.
        query: Box<SdblHir>,
        /// Result type.
        ty: SdblType,
        /// Source range.
        range: TextRange,
    },

    /// Parameter reference (&ParameterName).
    Parameter {
        /// Parameter name.
        name: Name,
        /// Inferred type.
        ty: SdblType,
        /// Source range.
        range: TextRange,
    },

    /// IN expression.
    In {
        /// Expression being tested.
        expr: Box<ExprHir>,
        /// Is NOT IN?
        negated: bool,
        /// Values or subquery.
        values: InValues,
        /// Result type (always Boolean).
        ty: SdblType,
        /// Source range.
        range: TextRange,
    },

    /// BETWEEN expression.
    Between {
        /// Expression being tested.
        expr: Box<ExprHir>,
        /// Is NOT BETWEEN?
        negated: bool,
        /// Lower bound.
        low: Box<ExprHir>,
        /// Upper bound.
        high: Box<ExprHir>,
        /// Result type (always Boolean).
        ty: SdblType,
        /// Source range.
        range: TextRange,
    },

    /// LIKE expression.
    Like {
        /// Expression being tested.
        expr: Box<ExprHir>,
        /// Is NOT LIKE?
        negated: bool,
        /// Pattern.
        pattern: Box<ExprHir>,
        /// Escape character.
        escape: Option<Box<ExprHir>>,
        /// Result type (always Boolean).
        ty: SdblType,
        /// Source range.
        range: TextRange,
    },

    /// IS NULL expression.
    IsNull {
        /// Expression being tested.
        expr: Box<ExprHir>,
        /// Is NOT NULL?
        negated: bool,
        /// Result type (always Boolean).
        ty: SdblType,
        /// Source range.
        range: TextRange,
    },

    /// Missing/error expression.
    Missing {
        /// Source range.
        range: TextRange,
    },
}

impl ExprHir {
    /// Get the type of this expression.
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
            Self::Missing { .. } => &SdblType::Error,
        }
    }

    /// Get the source range.
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
            Self::Missing { range } => *range,
        }
    }

    /// Get column name if this is a column reference.
    pub fn column_name(&self) -> Option<&Name> {
        match self {
            Self::ColumnRef { column, .. } => Some(column),
            _ => None,
        }
    }
}

/// Literal value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiteralValue {
    /// Integer literal.
    Integer(i64),
    /// Float literal (stored as string to preserve precision).
    Float(String),
    /// String literal.
    String(String),
    /// Boolean literal.
    Boolean(bool),
    /// Date literal.
    Date { year: u16, month: u8, day: u8 },
    /// DateTime literal.
    DateTime { year: u16, month: u8, day: u8, hour: u8, minute: u8, second: u8 },
    /// NULL literal.
    Null,
    /// Undefined literal.
    Undefined,
}

/// Binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,

    // Logical
    And,
    Or,

    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

impl BinaryOp {
    /// Check if this is a comparison operator.
    pub fn is_comparison(&self) -> bool {
        matches!(self, Self::Eq | Self::Ne | Self::Lt | Self::Le | Self::Gt | Self::Ge)
    }

    /// Check if this is a logical operator.
    pub fn is_logical(&self) -> bool {
        matches!(self, Self::And | Self::Or)
    }

    /// Check if this is an arithmetic operator.
    pub fn is_arithmetic(&self) -> bool {
        matches!(self, Self::Add | Self::Sub | Self::Mul | Self::Div | Self::Mod)
    }
}

/// Unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    /// Logical NOT.
    Not,
    /// Unary minus.
    Neg,
    /// Unary plus.
    Pos,
}

/// SDBL function kind.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FunctionKind {
    // Aggregate functions
    Sum,
    Avg,
    Min,
    Max,
    Count,

    // String functions
    Substring,
    Upper,
    Lower,
    Ltrim,
    Rtrim,
    Concat,

    // Date functions
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

    // Type conversion
    Cast,
    Isnull,
    Type,
    ValueType,
    Presentation,

    // Other
    Ref,

    /// Unknown function (not recognized).
    Unknown(String),
}

impl FunctionKind {
    /// Check if this is an aggregate function.
    pub fn is_aggregate(&self) -> bool {
        matches!(self, Self::Sum | Self::Avg | Self::Min | Self::Max | Self::Count)
    }

    /// Get function name (Russian).
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
            Self::Ref => "ССЫЛКА",
            Self::Unknown(name) => name.as_str(),
        }
    }
}

/// WHEN clause in CASE expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhenClause {
    /// Condition expression.
    pub condition: ExprHir,
    /// Result expression.
    pub result: ExprHir,
}

/// IN values (list or subquery).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InValues {
    /// List of values.
    List(Vec<ExprHir>),
    /// Subquery.
    Subquery(Box<SdblHir>),
}
