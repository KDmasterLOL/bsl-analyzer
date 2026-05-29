use bsl_metadata::ModuleType;

#[derive(Debug, Clone, PartialEq, Eq)]
enum RegionPattern {
    Exact(&'static str, &'static str),
    Prefix(&'static str, &'static str),
}

fn standard_patterns(module_type: ModuleType) -> Vec<RegionPattern> {
    use RegionPattern::*;
    let mut patterns = vec![];

    match module_type {
        ModuleType::FormModule => {
            patterns.extend([
                Exact("ОписаниеПеременных", "Variables"),
                Exact("ОбработчикиСобытийФормы", "FormEventHandlers"),
                Exact("ОбработчикиСобытийЭлементовШапкиФормы", "FormHeaderItemsEventHandlers"),
                Prefix("ОбработчикиСобытийЭлементовТаблицыФормы", "FormTableItemsEventHandlers"),
                Exact("ОбработчикиКомандФормы", "FormCommandsEventHandlers"),
                Exact("Инициализация", "Initialize"),
            ]);
        }
        ModuleType::ObjectModule | ModuleType::RecordSetModule => {
            patterns.extend([
                Exact("ОписаниеПеременных", "Variables"),
                Exact("ПрограммныйИнтерфейс", "Public"),
                Exact("ОбработчикиСобытий", "EventHandlers"),
                Exact("СлужебныйПрограммныйИнтерфейс", "Internal"),
                Exact("Инициализация", "Initialize"),
            ]);
        }
        ModuleType::ValueManagerModule => {
            patterns.extend([
                Exact("ОписаниеПеременных", "Variables"),
                Exact("ПрограммныйИнтерфейс", "Public"),
                Exact("ОбработчикиСобытий", "EventHandlers"),
                Exact("СлужебныйПрограммныйИнтерфейс", "Internal"),
            ]);
        }
        ModuleType::CommonModule => {
            patterns.extend([
                Exact("ПрограммныйИнтерфейс", "Public"),
                Exact("СлужебныйПрограммныйИнтерфейс", "Internal"),
            ]);
        }
        ModuleType::ApplicationModule
        | ModuleType::ManagedApplicationModule
        | ModuleType::OrdinaryApplicationModule => {
            patterns.extend([
                Exact("ОписаниеПеременных", "Variables"),
                Exact("ПрограммныйИнтерфейс", "Public"),
                Exact("ОбработчикиСобытий", "EventHandlers"),
            ]);
        }
        ModuleType::CommandModule
        | ModuleType::SessionModule
        | ModuleType::HTTPServiceModule
        | ModuleType::WebServiceModule => {
            patterns.extend([Exact("ОбработчикиСобытий", "EventHandlers")]);
        }
        ModuleType::ExternalConnectionModule => {
            patterns.extend([
                Exact("ПрограммныйИнтерфейс", "Public"),
                Exact("ОбработчикиСобытий", "EventHandlers"),
            ]);
        }
        ModuleType::ManagerModule => {
            patterns.extend([
                Exact("ПрограммныйИнтерфейс", "Public"),
                Exact("ОбработчикиСобытий", "EventHandlers"),
                Exact("СлужебныйПрограммныйИнтерфейс", "Internal"),
                Exact("Инициализация", "Initialize"),
            ]);
        }
        ModuleType::Unknown => return patterns,
    }

    patterns.push(Exact("СлужебныеПроцедурыИФункции", "Private"));
    patterns
}

pub fn is_standard_region(module_type: ModuleType, name: &str) -> bool {
    let patterns = standard_patterns(module_type);

    for pattern in &patterns {
        match pattern {
            RegionPattern::Exact(ru, en) => {
                if eq_ignore_case(name, ru) || eq_ignore_case(name, en) {
                    return true;
                }
            }
            RegionPattern::Prefix(ru, en) => {
                if let Some(suffix) = strip_prefix_ignore_case(name, ru) {
                    if is_valid_suffix(suffix) {
                        return true;
                    }
                }
                if let Some(suffix) = strip_prefix_ignore_case(name, en) {
                    if is_valid_suffix(suffix) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn eq_ignore_case(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

fn strip_prefix_ignore_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let s_lower = s.to_lowercase();
    let prefix_lower = prefix.to_lowercase();

    if !s_lower.starts_with(&prefix_lower) {
        return None;
    }

    let prefix_char_count = prefix.chars().count();
    let byte_offset =
        s.char_indices().nth(prefix_char_count).map(|(idx, _)| idx).unwrap_or(s.len());

    Some(&s[byte_offset..])
}

fn is_valid_suffix(suffix: &str) -> bool {
    suffix.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '_' || matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_russian() {
        assert!(is_standard_region(ModuleType::CommonModule, "ПрограммныйИнтерфейс"));
    }

    #[test]
    fn exact_match_english() {
        assert!(is_standard_region(ModuleType::CommonModule, "Public"));
    }

    #[test]
    fn case_insensitive() {
        assert!(is_standard_region(ModuleType::CommonModule, "public"));
        assert!(is_standard_region(ModuleType::CommonModule, "PUBLIC"));
        assert!(is_standard_region(ModuleType::CommonModule, "программныйинтерфейс"));
    }

    #[test]
    fn prefix_match() {
        assert!(is_standard_region(ModuleType::FormModule, "FormTableItemsEventHandlersProducts"));
        assert!(is_standard_region(
            ModuleType::FormModule,
            "ОбработчикиСобытийЭлементовТаблицыФормыТовары"
        ));
    }

    #[test]
    fn prefix_exact() {
        assert!(is_standard_region(ModuleType::FormModule, "FormTableItemsEventHandlers"));
    }

    #[test]
    fn non_standard_rejected() {
        assert!(!is_standard_region(ModuleType::CommonModule, "CustomRegion"));
        assert!(!is_standard_region(ModuleType::FormModule, "Переменные"));
    }

    #[test]
    fn unknown_module_type_admits_nothing() {
        assert!(!is_standard_region(ModuleType::Unknown, "Public"));
    }

    #[test]
    fn private_universal_across_types() {
        assert!(is_standard_region(ModuleType::CommonModule, "СлужебныеПроцедурыИФункции"));
        assert!(is_standard_region(ModuleType::CommonModule, "Private"));
        assert!(is_standard_region(ModuleType::FormModule, "СлужебныеПроцедурыИФункции"));
        assert!(is_standard_region(ModuleType::ObjectModule, "Private"));
    }

    #[test]
    fn module_specific_isolation() {
        assert!(is_standard_region(ModuleType::FormModule, "FormEventHandlers"));
        assert!(!is_standard_region(ModuleType::CommonModule, "FormEventHandlers"));

        assert!(is_standard_region(ModuleType::FormModule, "Variables"));
        assert!(!is_standard_region(ModuleType::CommonModule, "Variables"));
    }
}
