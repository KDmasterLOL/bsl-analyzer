use std::collections::HashSet;
use std::sync::Arc;

use crate::metadata_object::AttributeType;
use crate::{Configuration, MdoType, MetadataObject, Register};

pub trait MetadataResolver: std::fmt::Debug {
    /// The underlying type of the defined type `name`, or `None` if unknown.
    ///
    /// Returns an owned [`AttributeType`] (not a borrow) so a db-backed resolver
    /// can hand back a value composed fresh from a per-defined-type Salsa cell,
    /// rather than borrowing from a long-lived `Configuration`.
    fn resolve_defined_type(&self, name: &str) -> Option<AttributeType>;
}

impl MetadataResolver for Configuration {
    fn resolve_defined_type(&self, name: &str) -> Option<AttributeType> {
        self.find_defined_type(name).map(|dt| dt.underlying_type().clone())
    }
}

/// Metadata-object and register resolution surface for query (SDBL) lowering.
///
/// Returning owned `Arc`s (not borrows) lets a db-backed resolver hand back
/// per-MDO objects composed fresh from their own Salsa cells, so lowering a
/// query depends on just the metadata objects it references instead of the whole
/// `Configuration`. The [`Configuration`] impl serves the cold call sites (graph
/// build, streaming, tests) that still carry a whole config.
pub trait QueryMetadataResolver: MetadataResolver {
    fn resolve_metadata_object(&self, mdo_type: MdoType, name: &str)
        -> Option<Arc<MetadataObject>>;

    fn resolve_register(&self, mdo_type: MdoType, name: &str) -> Option<Arc<Register>>;
}

impl QueryMetadataResolver for Configuration {
    fn resolve_metadata_object(
        &self,
        mdo_type: MdoType,
        name: &str,
    ) -> Option<Arc<MetadataObject>> {
        self.find_metadata_object(mdo_type, name).cloned().map(Arc::new)
    }

    fn resolve_register(&self, mdo_type: MdoType, name: &str) -> Option<Arc<Register>> {
        self.find_register_by_type_and_name(mdo_type, name).cloned().map(Arc::new)
    }
}

pub fn resolve_defined_type_terminal<R>(
    resolver: &R,
    name: &str,
    visited: &mut HashSet<String>,
) -> Option<AttributeType>
where
    R: MetadataResolver + ?Sized,
{
    let mut current = name.to_string();
    loop {
        if !visited.insert(current.to_lowercase()) {
            return None;
        }
        match resolver.resolve_defined_type(&current)? {
            AttributeType::DefinedType { name: next } => {
                current = next;
            }
            other => return Some(other),
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
        assert_eq!(underlying, AttributeType::Boolean);
    }
}
