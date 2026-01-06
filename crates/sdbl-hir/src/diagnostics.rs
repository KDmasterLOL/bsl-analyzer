//! SDBL semantic diagnostics.
//!
//! Diagnostics collected during SDBL HIR lowering.

use text_size::TextRange;

use crate::types::SdblType;

/// Semantic diagnostic collected during SDBL HIR lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SdblDiagnostic {
    /// Table doesn't exist in metadata.
    ///
    /// BSL-LS diagnostic code: 122
    QueryToMissingMetadata {
        /// Table name that wasn't found.
        table_name: String,
        /// Source range.
        range: TextRange,
    },

    /// JOIN with virtual table (not recommended).
    ///
    /// BSL-LS diagnostic code: 79
    JoinWithVirtualTable {
        /// Table name.
        table_name: String,
        /// Virtual table type (e.g., "СрезПоследних").
        virtual_table_type: String,
        /// Source range.
        range: TextRange,
    },

    /// Virtual table call without required parameters.
    ///
    /// BSL-LS diagnostic code: 174
    VirtualTableCallWithoutParameters {
        /// Table name.
        table_name: String,
        /// Expected parameters.
        expected_params: Vec<String>,
        /// Source range.
        range: TextRange,
    },

    /// Unknown field in table.
    UnknownField {
        /// Table name.
        table_name: String,
        /// Field name that wasn't found.
        field_name: String,
        /// Source range.
        range: TextRange,
    },

    /// Ambiguous column reference (multiple tables have same field).
    AmbiguousColumnRef {
        /// Column name.
        column_name: String,
        /// Possible tables with this column.
        possible_tables: Vec<String>,
        /// Source range.
        range: TextRange,
    },

    /// Type mismatch in expression.
    TypeMismatch {
        /// Expected type.
        expected: SdblType,
        /// Actual type found.
        actual: SdblType,
        /// Context description.
        context: String,
        /// Source range.
        range: TextRange,
    },

    /// Unknown function.
    UnknownFunction {
        /// Function name.
        function_name: String,
        /// Source range.
        range: TextRange,
    },

    /// Invalid function argument count.
    InvalidArgumentCount {
        /// Function name.
        function_name: String,
        /// Expected count (or range).
        expected: String,
        /// Actual count.
        actual: usize,
        /// Source range.
        range: TextRange,
    },

    /// Alias required (AS keyword missing).
    ///
    /// Note: This is a syntax-level check that can also be done at HIR level.
    AliasRequired {
        /// Field or column name.
        name: String,
        /// Source range.
        range: TextRange,
    },

    /// Duplicate alias in SELECT.
    DuplicateAlias {
        /// Alias name.
        alias: String,
        /// Source range of duplicate.
        range: TextRange,
    },

    /// FULL OUTER JOIN detected (performance issue).
    ///
    /// BSL-LS diagnostic code: FullOuterJoinQuery
    FullOuterJoin {
        /// Source range.
        range: TextRange,
    },

    /// JOIN with subquery (severe performance issue).
    ///
    /// BSL-LS diagnostic code: JoinWithSubQuery
    JoinWithSubQuery {
        /// Source range.
        range: TextRange,
    },

    /// Field alias without AS keyword (or no alias).
    ///
    /// BSL-LS diagnostic code: AssignAliasFieldsInQuery
    AliasWithoutAsKeyword {
        /// Field name (for message).
        field_name: Option<String>,
        /// Source range.
        range: TextRange,
    },

    /// OR operator in WHERE clause (performance issue).
    ///
    /// BSL-LS diagnostic code: LogicalOrInTheWhereSectionOfQuery
    LogicalOrInWhere {
        /// Source range.
        range: TextRange,
    },

    /// OR operator in JOIN condition with multiple fields (performance issue).
    ///
    /// BSL-LS diagnostic code: LogicalOrInJoinQuerySection
    LogicalOrInJoin {
        /// Source range.
        range: TextRange,
    },

    /// Fields from LEFT/RIGHT/FULL JOIN without NULL protection.
    ///
    /// BSL-LS diagnostic code: FieldsFromJoinsWithoutIsNull
    FieldsFromJoinWithoutNullCheck {
        /// JOIN type (for message generation).
        join_type: crate::hir::JoinType,
        /// Source range of the JOIN clause.
        range: TextRange,
        /// Unprotected field references (for future LSP RelatedInformation).
        unprotected_fields: Vec<UnprotectedFieldRef>,
    },

    /// Multiline string literal in query (likely incorrect quoting).
    ///
    /// BSL-LS diagnostic code: MultilineStringInQuery
    ///
    /// In SDBL, empty string is """" (4 quotes), not "" (2 quotes).
    /// Two quotes create a multiline string which is usually unintended.
    MultilineString {
        /// Source range.
        range: TextRange,
    },
}

/// Reference to an unprotected field from JOIN.
///
/// Used for detailed error reporting and future LSP RelatedInformation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnprotectedFieldRef {
    /// Table alias.
    pub table_alias: String,
    /// Field name (for debugging/messages).
    pub field_name: String,
    /// Source range in SDBL.
    pub range: TextRange,
}

impl SdblDiagnostic {
    /// Get diagnostic code (for BSL-LS compatibility).
    pub fn code(&self) -> Option<u32> {
        match self {
            Self::QueryToMissingMetadata { .. } => Some(122),
            Self::JoinWithVirtualTable { .. } => Some(79),
            Self::VirtualTableCallWithoutParameters { .. } => Some(174),
            _ => None, // Other diagnostics don't have BSL-LS codes
        }
    }

    /// Get diagnostic message.
    pub fn message(&self) -> String {
        match self {
            Self::QueryToMissingMetadata { table_name, .. } => {
                format!("Таблица '{}' не найдена в метаданных", table_name)
            }
            Self::JoinWithVirtualTable { table_name, virtual_table_type, .. } => {
                format!(
                    "Не рекомендуется использовать соединение с виртуальной таблицей '{}' ({})",
                    table_name, virtual_table_type
                )
            }
            Self::VirtualTableCallWithoutParameters { table_name, expected_params, .. } => {
                format!(
                    "Виртуальная таблица '{}' требует параметры: {}",
                    table_name,
                    expected_params.join(", ")
                )
            }
            Self::UnknownField { table_name, field_name, .. } => {
                format!("Поле '{}' не найдено в таблице '{}'", field_name, table_name)
            }
            Self::AmbiguousColumnRef { column_name, possible_tables, .. } => {
                format!(
                    "Неоднозначная ссылка на колонку '{}'. Возможные таблицы: {}",
                    column_name,
                    possible_tables.join(", ")
                )
            }
            Self::TypeMismatch { expected, actual, context, .. } => {
                format!(
                    "Несоответствие типов в {}: ожидается {}, получен {}",
                    context, expected, actual
                )
            }
            Self::UnknownFunction { function_name, .. } => {
                format!("Неизвестная функция '{}'", function_name)
            }
            Self::InvalidArgumentCount { function_name, expected, actual, .. } => {
                format!(
                    "Функция '{}' ожидает {} аргументов, передано {}",
                    function_name, expected, actual
                )
            }
            Self::AliasRequired { name, .. } => {
                format!("Для поля '{}' требуется псевдоним (используйте AS)", name)
            }
            Self::DuplicateAlias { alias, .. } => {
                format!("Дублирующийся псевдоним '{}' в SELECT", alias)
            }
            Self::FullOuterJoin { .. } => {
                "Использование FULL OUTER JOIN значительно снижает производительность запроса. \
                 Рассмотрите возможность переписать с использованием UNION и LEFT JOIN"
                    .to_string()
            }
            Self::JoinWithSubQuery { .. } => "Не используйте соединение с подзапросами. \
                 Соединения с подзапросами вызывают серьезные проблемы с производительностью"
                .to_string(),
            Self::AliasWithoutAsKeyword { field_name, .. } => {
                if let Some(name) = field_name {
                    format!("Поле '{}' должно иметь явный псевдоним с ключевым словом AS/КАК", name)
                } else {
                    "Поле в подзапросе должно иметь псевдоним с ключевым словом AS/КАК".to_string()
                }
            }
            Self::LogicalOrInWhere { .. } => {
                "Использование оператора ИЛИ (OR) в условии WHERE существенно снижает производительность запроса. \
                 Рассмотрите возможность переписать с использованием UNION или изменить структуру условий"
                    .to_string()
            }
            Self::LogicalOrInJoin { .. } => {
                "Использование ИЛИ (OR) в условии соединения приводит к низкой производительности запроса"
                    .to_string()
            }
            Self::FieldsFromJoinWithoutNullCheck { join_type, .. } => {
                let join_str = match join_type {
                    crate::hir::JoinType::Left => "LEFT JOIN",
                    crate::hir::JoinType::Right => "RIGHT JOIN",
                    crate::hir::JoinType::Full => "FULL JOIN",
                    _ => "JOIN",
                };
                format!(
                    "For fields from {} add field checks via IS NULL or use conversion via ISNULL or use INNER JOIN",
                    join_str
                )
            }
            Self::MultilineString { .. } => {
                "Check if multiline literal is correct".to_string()
            }
        }
    }

    /// Get source range.
    pub fn range(&self) -> TextRange {
        match self {
            Self::QueryToMissingMetadata { range, .. } => *range,
            Self::JoinWithVirtualTable { range, .. } => *range,
            Self::VirtualTableCallWithoutParameters { range, .. } => *range,
            Self::UnknownField { range, .. } => *range,
            Self::AmbiguousColumnRef { range, .. } => *range,
            Self::TypeMismatch { range, .. } => *range,
            Self::UnknownFunction { range, .. } => *range,
            Self::InvalidArgumentCount { range, .. } => *range,
            Self::AliasRequired { range, .. } => *range,
            Self::DuplicateAlias { range, .. } => *range,
            Self::FullOuterJoin { range, .. } => *range,
            Self::JoinWithSubQuery { range, .. } => *range,
            Self::AliasWithoutAsKeyword { range, .. } => *range,
            Self::LogicalOrInWhere { range } => *range,
            Self::LogicalOrInJoin { range } => *range,
            Self::FieldsFromJoinWithoutNullCheck { range, .. } => *range,
            Self::MultilineString { range } => *range,
        }
    }

    /// Check if this is an error (vs warning).
    pub fn is_error(&self) -> bool {
        match self {
            // Errors - prevent correct execution
            Self::QueryToMissingMetadata { .. } => true,
            Self::UnknownField { .. } => true,
            Self::AmbiguousColumnRef { .. } => true,
            Self::TypeMismatch { .. } => true,
            Self::UnknownFunction { .. } => true,
            Self::InvalidArgumentCount { .. } => true,

            // Warnings - code may work but has issues
            Self::JoinWithVirtualTable { .. } => false,
            Self::VirtualTableCallWithoutParameters { .. } => false,
            Self::AliasRequired { .. } => false,
            Self::DuplicateAlias { .. } => false,
            Self::FullOuterJoin { .. } => false,
            Self::JoinWithSubQuery { .. } => false,
            Self::AliasWithoutAsKeyword { .. } => false,
            Self::LogicalOrInWhere { .. } => false,
            Self::LogicalOrInJoin { .. } => false,
            Self::FieldsFromJoinWithoutNullCheck { .. } => false, // Warning/Critical, not an error
            Self::MultilineString { .. } => false, // Warning - likely incorrect quoting
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnostic_codes() {
        let diag = SdblDiagnostic::QueryToMissingMetadata {
            table_name: "Test".to_string(),
            range: TextRange::empty(0.into()),
        };
        assert_eq!(diag.code(), Some(122));

        let diag = SdblDiagnostic::JoinWithVirtualTable {
            table_name: "Test".to_string(),
            virtual_table_type: "СрезПоследних".to_string(),
            range: TextRange::empty(0.into()),
        };
        assert_eq!(diag.code(), Some(79));
    }

    #[test]
    fn test_diagnostic_messages() {
        let diag = SdblDiagnostic::UnknownField {
            table_name: "Справочник.Валюты".to_string(),
            field_name: "НесуществующееПоле".to_string(),
            range: TextRange::empty(0.into()),
        };
        assert!(diag.message().contains("НесуществующееПоле"));
        assert!(diag.message().contains("Справочник.Валюты"));
    }
}
