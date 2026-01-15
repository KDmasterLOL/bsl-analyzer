//! Virtual table detection utilities for SDBL queries.

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

/// Check if table name part is a virtual table.
pub fn is_virtual_table_name(name: &str) -> bool {
    let name_lower = name.to_lowercase();
    VIRTUAL_TABLES.iter().any(|(ru, en)| *ru == name_lower || *en == name_lower)
}

/// Get virtual table type from name (for diagnostics).
pub fn virtual_table_type(name: &str) -> Option<&'static str> {
    let name_lower = name.to_lowercase();
    for (ru, en) in VIRTUAL_TABLES {
        if *ru == name_lower {
            return Some(ru);
        }
        if *en == name_lower {
            return Some(ru); // Return Russian name for diagnostics
        }
    }
    None
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
        assert_eq!(virtual_table_type("СрезПоследних"), Some("срезпоследних"));
        assert_eq!(virtual_table_type("SliceLast"), Some("срезпоследних"));
        assert_eq!(virtual_table_type("Остатки"), Some("остатки"));
        assert_eq!(virtual_table_type("Balance"), Some("остатки"));
        assert_eq!(virtual_table_type("Unknown"), None);
    }
}
