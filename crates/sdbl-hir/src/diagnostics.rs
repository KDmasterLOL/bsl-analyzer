use text_size::TextRange;

use crate::types::SdblType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SdblDiagnostic {
    QueryToMissingMetadata {
        table_name: String,
        range: TextRange,
    },

    JoinWithVirtualTable {
        table_name: String,
        virtual_table_type: String,
        range: TextRange,
    },

    VirtualTableCallWithoutParameters {
        table_name: String,
        expected_params: Vec<String>,
        range: TextRange,
    },

    UnknownField {
        table_name: String,
        field_name: String,
        range: TextRange,
    },

    AmbiguousColumnRef {
        column_name: String,
        possible_tables: Vec<String>,
        range: TextRange,
    },

    TypeMismatch {
        expected: SdblType,
        actual: SdblType,
        context: String,
        range: TextRange,
    },

    UnknownFunction {
        function_name: String,
        range: TextRange,
    },

    InvalidArgumentCount {
        function_name: String,
        expected: String,
        actual: usize,
        range: TextRange,
    },

    AliasRequired {
        name: String,
        range: TextRange,
    },

    DuplicateAlias {
        alias: String,
        range: TextRange,
    },

    FullOuterJoin {
        range: TextRange,
    },

    JoinWithSubQuery {
        range: TextRange,
    },

    AliasWithoutAsKeyword {
        field_name: Option<String>,
        raw_name: Option<String>,
        range: TextRange,
    },

    LogicalOrInWhere {
        range: TextRange,
    },

    LogicalOrInJoin {
        range: TextRange,
    },

    FieldsFromJoinWithoutNullCheck {
        join_type: crate::hir::JoinType,
        range: TextRange,
        unprotected_fields: Vec<UnprotectedFieldRef>,
    },

    MultilineString {
        range: TextRange,
    },

    QueryNestedFieldsByDot {
        range: TextRange,
        parts_count: Option<u32>,
    },

    RefOveruse {
        range: TextRange,
    },

    UnionWithoutAll {
        range: TextRange,
    },

    LikeUsage {
        range: TextRange,
        kind: LikeUsageKind,
    },

    SelectTopWithoutOrderBy {
        top_value: u32,
        in_union: bool,
        has_where: bool,
        range: TextRange,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LikeUsageKind {
    Allowed,
    Incorrect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnprotectedFieldRef {
    pub table_alias: String,
    pub field_name: String,
    pub range: TextRange,
}

impl SdblDiagnostic {
    pub fn code(&self) -> Option<u32> {
        match self {
            Self::QueryToMissingMetadata { .. } => Some(122),
            Self::JoinWithVirtualTable { .. } => Some(79),
            Self::VirtualTableCallWithoutParameters { .. } => Some(174),
            Self::QueryNestedFieldsByDot { .. } => None,
            Self::RefOveruse { .. } => None,
            Self::LikeUsage { .. } => None,
            _ => None,
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::QueryToMissingMetadata { table_name, .. } => {
                format!(
                    "Исправьте обращение к несуществующему метаданному \"{}\" в запросе",
                    table_name
                )
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
                    crate::hir::JoinType::Left => "ЛЕВОГО СОЕДИНЕНИЯ",
                    crate::hir::JoinType::Right => "ПРАВОГО СОЕДИНЕНИЯ",
                    crate::hir::JoinType::Full => "ПОЛНОГО СОЕДИНЕНИЯ",
                    _ => "СОЕДИНЕНИЯ",
                };
                format!(
                    "Для полей из {} добавьте проверку через ЕСТЬ NULL или используйте функцию ЕСТЬNULL, либо замените на ВНУТРЕННЕЕ СОЕДИНЕНИЕ",
                    join_str
                )
            }
            Self::MultilineString { .. } => {
                "Проверьте корректность многострочного литерала".to_string()
            }
            Self::QueryNestedFieldsByDot { .. } => {
                "Обнаружено разыменование ссылочного поля".to_string()
            }
            Self::RefOveruse { .. } => {
                "Избавьтесь от получения поля \"Ссылка\" в запросе.".to_string()
            }
            Self::UnionWithoutAll { .. } => {
                "Использование ключевого слова ОБЪЕДИНИТЬ без ВСЕ приводит к \
                 излишней обработке для удаления дубликатов. Используйте ОБЪЕДИНИТЬ ВСЕ"
                    .to_string()
            }
            Self::LikeUsage { kind: LikeUsageKind::Allowed, .. } => {
                "Измените выражение, чтобы не использовать 'ПОДОБНО'".to_string()
            }
            Self::LikeUsage { kind: LikeUsageKind::Incorrect, .. } => {
                "Нужно исправить выражение в соответствии со стандартом".to_string()
            }
            Self::SelectTopWithoutOrderBy { .. } => {
                "Измените запрос, добавив сортировку".to_string()
            }
        }
    }

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
            Self::QueryNestedFieldsByDot { range, .. } => *range,
            Self::RefOveruse { range } => *range,
            Self::UnionWithoutAll { range } => *range,
            Self::LikeUsage { range, .. } => *range,
            Self::SelectTopWithoutOrderBy { range, .. } => *range,
        }
    }

    pub fn is_error(&self) -> bool {
        match self {
            Self::QueryToMissingMetadata { .. } => true,
            Self::UnknownField { .. } => true,
            Self::AmbiguousColumnRef { .. } => true,
            Self::TypeMismatch { .. } => true,
            Self::UnknownFunction { .. } => true,
            Self::InvalidArgumentCount { .. } => true,

            Self::JoinWithVirtualTable { .. } => false,
            Self::VirtualTableCallWithoutParameters { .. } => false,
            Self::AliasRequired { .. } => false,
            Self::DuplicateAlias { .. } => false,
            Self::FullOuterJoin { .. } => false,
            Self::JoinWithSubQuery { .. } => false,
            Self::AliasWithoutAsKeyword { .. } => false,
            Self::LogicalOrInWhere { .. } => false,
            Self::LogicalOrInJoin { .. } => false,
            Self::FieldsFromJoinWithoutNullCheck { .. } => false,
            Self::MultilineString { .. } => false,
            Self::QueryNestedFieldsByDot { .. } => false,
            Self::RefOveruse { .. } => false,
            Self::UnionWithoutAll { .. } => false,
            Self::LikeUsage { kind: LikeUsageKind::Allowed, .. } => false,
            Self::LikeUsage { kind: LikeUsageKind::Incorrect, .. } => true,
            Self::SelectTopWithoutOrderBy { .. } => false,
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
            range: syntax::MODULE_RANGE,
        };
        assert_eq!(diag.code(), Some(122));

        let diag = SdblDiagnostic::JoinWithVirtualTable {
            table_name: "Test".to_string(),
            virtual_table_type: "СрезПоследних".to_string(),
            range: syntax::MODULE_RANGE,
        };
        assert_eq!(diag.code(), Some(79));
    }

    #[test]
    fn test_diagnostic_messages() {
        let diag = SdblDiagnostic::UnknownField {
            table_name: "Справочник.Валюты".to_string(),
            field_name: "НесуществующееПоле".to_string(),
            range: syntax::MODULE_RANGE,
        };
        assert!(diag.message().contains("НесуществующееПоле"));
        assert!(diag.message().contains("Справочник.Валюты"));
    }
}
