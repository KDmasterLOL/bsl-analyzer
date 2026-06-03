use std::collections::HashSet;

use crate::metadata_object::AttributeType;
use crate::Configuration;

pub trait MetadataResolver: std::fmt::Debug {
    fn resolve_defined_type(&self, name: &str) -> Option<&AttributeType>;
}

impl MetadataResolver for Configuration {
    fn resolve_defined_type(&self, name: &str) -> Option<&AttributeType> {
        self.find_defined_type(name).map(|dt| dt.underlying_type())
    }
}

pub fn resolve_defined_type_terminal<'a, R>(
    resolver: &'a R,
    name: &str,
    visited: &mut HashSet<String>,
) -> Option<&'a AttributeType>
where
    R: MetadataResolver + ?Sized,
{
    let mut current = name.to_string();
    loop {
        if !visited.insert(current.to_lowercase()) {
            return None;
        }
        let underlying = resolver.resolve_defined_type(&current)?;
        match underlying {
            AttributeType::DefinedType { name: next } => {
                current = next.clone();
            }
            _ => return Some(underlying),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defined_type::DefinedType;
    use crate::Configuration;
    use uuid::Uuid;

    fn dt(name: &str, underlying: AttributeType) -> DefinedType {
        DefinedType::builder().uuid(Uuid::new_v4()).name(name).underlying_type(underlying).build()
    }

    fn cfg_with(types: Vec<DefinedType>) -> Configuration {
        let mut cfg = Configuration::new("Test");
        for t in types {
            cfg.add_defined_type(t);
        }
        cfg
    }

    #[test]
    fn configuration_resolves_existing_defined_type() {
        let cfg =
            cfg_with(vec![dt("ДенежнаяСумма", AttributeType::Number { precision: 15, scale: 2 })]);
        let underlying = cfg.resolve_defined_type("ДенежнаяСумма").expect("present");
        assert!(matches!(underlying, AttributeType::Number { .. }));
    }

    #[test]
    fn configuration_returns_none_for_unknown_name() {
        let cfg = cfg_with(Vec::new());
        assert!(cfg.resolve_defined_type("Несуществует").is_none());
    }

    #[test]
    fn terminal_walk_unwraps_chain() {
        let cfg = cfg_with(vec![
            dt("A", AttributeType::DefinedType { name: "B".to_string() }),
            dt("B", AttributeType::Number { precision: 10, scale: 0 }),
        ]);
        let mut visited = HashSet::new();
        let underlying = resolve_defined_type_terminal(&cfg, "A", &mut visited)
            .expect("chain must terminate at Number");
        assert!(matches!(underlying, AttributeType::Number { .. }));
        assert!(visited.contains("a"));
        assert!(visited.contains("b"));
    }

    #[test]
    fn terminal_walk_breaks_simple_cycle() {
        let cfg = cfg_with(vec![dt("A", AttributeType::DefinedType { name: "A".to_string() })]);
        let mut visited = HashSet::new();
        assert!(resolve_defined_type_terminal(&cfg, "A", &mut visited).is_none());
    }

    #[test]
    fn terminal_walk_breaks_indirect_cycle() {
        let cfg = cfg_with(vec![
            dt("A", AttributeType::DefinedType { name: "B".to_string() }),
            dt("B", AttributeType::DefinedType { name: "A".to_string() }),
        ]);
        let mut visited = HashSet::new();
        assert!(resolve_defined_type_terminal(&cfg, "A", &mut visited).is_none());
    }

    #[test]
    fn terminal_walk_returns_none_when_chain_breaks() {
        let cfg =
            cfg_with(vec![dt("A", AttributeType::DefinedType { name: "Missing".to_string() })]);
        let mut visited = HashSet::new();
        assert!(resolve_defined_type_terminal(&cfg, "A", &mut visited).is_none());
    }

    #[test]
    fn terminal_walk_lookup_is_case_insensitive() {
        let cfg = cfg_with(vec![dt("ДенежнаяСумма", AttributeType::Boolean)]);
        let mut visited = HashSet::new();
        let underlying = resolve_defined_type_terminal(&cfg, "денежнаясумма", &mut visited)
            .expect("case-insensitive lookup");
        assert_eq!(underlying, &AttributeType::Boolean);
    }
}
