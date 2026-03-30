//! Virtual table detection utilities for SDBL queries.

use std::fmt;

/// Virtual table types and their Russian/English names.
pub const VIRTUAL_TABLES: &[(&str, &str)] = &[
    ("срезпоследних", "slicelast"),
    ("срезпервых", "slicefirst"),
    ("остатки", "balance"),
    ("обороты", "turnovers"),
    ("остаткииобороты", "balanceandturnovers"),
    ("движениясостороккорреспонденциями", "recordswithextdimensions"),
    ("движениясубконто", "extdimensiondr"),
    ("субконто", "extdimensions"),
    ("изменения", "changes"),
];

/// Virtual table type enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtualTableType {
    SliceLast,
    SliceFirst,
    Balance,
    Turnovers,
    BalanceAndTurnovers,
    RecordsWithExtDimensions,
    ExtDimensionDr,
    ExtDimensions,
    Changes,
}

impl VirtualTableType {
    /// Russian lowercase name (for diagnostics).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SliceLast => "срезпоследних",
            Self::SliceFirst => "срезпервых",
            Self::Balance => "остатки",
            Self::Turnovers => "обороты",
            Self::BalanceAndTurnovers => "остаткииобороты",
            Self::RecordsWithExtDimensions => "движениясостороккорреспонденциями",
            Self::ExtDimensionDr => "движениясубконто",
            Self::ExtDimensions => "субконто",
            Self::Changes => "изменения",
        }
    }

    /// Whether this VT type has a periodicity parameter (3rd param, index 2).
    pub fn has_periodicity(self) -> bool {
        matches!(self, Self::Turnovers | Self::BalanceAndTurnovers)
    }
}

impl fmt::Display for VirtualTableType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Check if table name part is a virtual table.
pub fn is_virtual_table_name(name: &str) -> bool {
    let name_lower = name.to_lowercase();
    VIRTUAL_TABLES.iter().any(|(ru, en)| *ru == name_lower || *en == name_lower)
}

/// Get virtual table type from name.
pub fn virtual_table_type(name: &str) -> Option<VirtualTableType> {
    let name_lower = name.to_lowercase();
    match name_lower.as_str() {
        "срезпоследних" | "slicelast" => Some(VirtualTableType::SliceLast),
        "срезпервых" | "slicefirst" => Some(VirtualTableType::SliceFirst),
        "остатки" | "balance" => Some(VirtualTableType::Balance),
        "обороты" | "turnovers" => Some(VirtualTableType::Turnovers),
        "остаткииобороты" | "balanceandturnovers" => {
            Some(VirtualTableType::BalanceAndTurnovers)
        }
        "движениясостороккорреспонденциями" | "recordswithextdimensions" => {
            Some(VirtualTableType::RecordsWithExtDimensions)
        }
        "движениясубконто" | "extdimensiondr" => {
            Some(VirtualTableType::ExtDimensionDr)
        }
        "субконто" | "extdimensions" => Some(VirtualTableType::ExtDimensions),
        "изменения" | "changes" => Some(VirtualTableType::Changes),
        _ => None,
    }
}

/// Periodicity enum values for virtual table parameters.
pub const PERIODICITY_VALUES: &[(&str, &str)] = &[
    ("авто", "auto"),
    ("год", "year"),
    ("полугодие", "halfyear"),
    ("квартал", "quarter"),
    ("месяц", "month"),
    ("декада", "tendays"),
    ("неделя", "week"),
    ("день", "day"),
    ("секунда", "second"),
    ("запись", "record"),
];

/// Check if a name is a known periodicity enum value.
pub fn is_periodicity_value(name: &str) -> bool {
    let name_lower = name.to_lowercase();
    PERIODICITY_VALUES.iter().any(|(ru, en)| *ru == name_lower || *en == name_lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_virtual_table_detection() {
        assert!(is_virtual_table_name("СрезПоследних"));
        assert!(is_virtual_table_name("срезпоследних"));
        assert!(is_virtual_table_name("SliceLast"));
        assert!(is_virtual_table_name("slicelast"));
        assert!(is_virtual_table_name("Остатки"));
        assert!(is_virtual_table_name("Balance"));
        assert!(is_virtual_table_name("Изменения"));
        assert!(is_virtual_table_name("Changes"));

        assert!(!is_virtual_table_name("Справочник"));
        assert!(!is_virtual_table_name("Random"));
    }

    #[test]
    fn test_virtual_table_type() {
        assert_eq!(virtual_table_type("СрезПоследних"), Some(VirtualTableType::SliceLast));
        assert_eq!(virtual_table_type("SliceLast"), Some(VirtualTableType::SliceLast));
        assert_eq!(virtual_table_type("Остатки"), Some(VirtualTableType::Balance));
        assert_eq!(virtual_table_type("Balance"), Some(VirtualTableType::Balance));
        assert_eq!(virtual_table_type("Обороты"), Some(VirtualTableType::Turnovers));
        assert_eq!(virtual_table_type("Turnovers"), Some(VirtualTableType::Turnovers));
        assert_eq!(virtual_table_type("Unknown"), None);
    }

    #[test]
    fn test_virtual_table_type_display() {
        assert_eq!(VirtualTableType::SliceLast.to_string(), "срезпоследних");
        assert_eq!(VirtualTableType::Balance.to_string(), "остатки");
        assert_eq!(VirtualTableType::Turnovers.to_string(), "обороты");
    }

    #[test]
    fn test_periodicity_values() {
        assert!(is_periodicity_value("Авто"));
        assert!(is_periodicity_value("авто"));
        assert!(is_periodicity_value("Auto"));
        assert!(is_periodicity_value("День"));
        assert!(is_periodicity_value("Day"));
        assert!(is_periodicity_value("Месяц"));
        assert!(is_periodicity_value("Month"));
        assert!(is_periodicity_value("Запись"));
        assert!(is_periodicity_value("Record"));

        assert!(!is_periodicity_value("Партнер"));
        assert!(!is_periodicity_value("Random"));
    }

    #[test]
    fn test_has_periodicity() {
        assert!(VirtualTableType::Turnovers.has_periodicity());
        assert!(VirtualTableType::BalanceAndTurnovers.has_periodicity());
        assert!(!VirtualTableType::Balance.has_periodicity());
        assert!(!VirtualTableType::SliceLast.has_periodicity());
        assert!(!VirtualTableType::Changes.has_periodicity());
    }
}
