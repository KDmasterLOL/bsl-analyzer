use sdbl_hir::SdblType;

pub struct FieldFormatter;

impl FieldFormatter {
    pub fn format_field_type(
        ty: &SdblType,
        table_name: &str,
        is_standard: bool,
    ) -> (String, String) {
        let standard_marker = if is_standard { " (стандартный)" } else { "" };

        match ty {
            SdblType::Composite { types } => {
                let detail = format!("Составной тип:{}", standard_marker);
                let types_list = types.iter().map(|t| t.to_string()).collect::<Vec<_>>().join("\n");
                let doc = format!("{}\n\nТаблица: {}", types_list, table_name);
                (detail, doc)
            }

            SdblType::DefinedType { name, underlying_type: Some(underlying) } => {
                if let SdblType::Composite { types } = underlying.as_ref() {
                    let detail = format!("ОпределяемыйТип.{}:{}", name, standard_marker);
                    let types_list =
                        types.iter().map(|t| t.to_string()).collect::<Vec<_>>().join("\n");
                    let doc = format!("{}\n\nТаблица: {}", types_list, table_name);
                    (detail, doc)
                } else {
                    let detail = format!("{}{}", ty, standard_marker);
                    let doc = format!("Таблица: {}", table_name);
                    (detail, doc)
                }
            }

            _ => {
                let detail = format!("{}{}", ty, standard_marker);
                let doc = format!("Таблица: {}", table_name);
                (detail, doc)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::MdoType;

    #[test]
    fn test_format_simple_type() {
        let ty = SdblType::string();
        let (detail, doc) = FieldFormatter::format_field_type(&ty, "Справочник.Валюты", false);

        assert_eq!(detail, "Строка");
        assert_eq!(doc, "Таблица: Справочник.Валюты");
    }

    #[test]
    fn test_format_standard_field() {
        let ty = SdblType::string();
        let (detail, doc) = FieldFormatter::format_field_type(&ty, "Справочник.Валюты", true);

        assert_eq!(detail, "Строка (стандартный)");
        assert_eq!(doc, "Таблица: Справочник.Валюты");
    }

    #[test]
    fn test_format_composite_type() {
        let ty = SdblType::Composite {
            types: vec![
                SdblType::reference(MdoType::Catalog, "Валюты"),
                SdblType::reference(MdoType::Document, "ПриходнаяНакладная"),
            ],
        };
        let (detail, doc) = FieldFormatter::format_field_type(&ty, "Справочник.Контрагенты", false);

        assert_eq!(detail, "Составной тип:");
        assert!(doc.contains("Справочник.Валюты"));
        assert!(doc.contains("Документ.ПриходнаяНакладная"));
        assert!(doc.contains("Таблица: Справочник.Контрагенты"));
    }

    #[test]
    fn test_format_defined_type() {
        let ty = SdblType::DefinedType {
            name: "ВидНоменклатуры".to_string(),
            underlying_type: Some(Box::new(SdblType::string())),
        };
        let (detail, doc) =
            FieldFormatter::format_field_type(&ty, "Справочник.Номенклатура", false);

        assert!(detail.contains("ОпределяемыйТип.ВидНоменклатуры"));
        assert_eq!(doc, "Таблица: Справочник.Номенклатура");
    }
}
