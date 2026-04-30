//! Lookup port for type definitions referenced from `AttributeType`.
//!
//! [`MetadataResolver`] is the single abstraction both `hir-ty` (BSL inference)
//! and `sdbl-hir` (query-language inference) consume to follow
//! `AttributeType::DefinedType { name }` to its underlying type. Living in
//! `bsl-metadata` keeps the trait at the lowest layer that knows about
//! `AttributeType`, so neither HIR crate has to depend on the other.
//!
//! Implementations:
//! - [`Configuration`] — single-config lookup, used by SDBL.
//! - `&[VisibleConfig]` (in `hir-ty`) — multi-config lookup with
//!   extension-wins semantics, used by BSL field enumeration.

use std::collections::HashSet;

use crate::metadata_object::AttributeType;
use crate::Configuration;

/// Looks up `<DefinedType>` entries by name.
///
/// The single method is intentionally narrow — callers compose recursive
/// resolution and cycle detection through [`resolve_defined_type_terminal`]
/// instead of pushing those concerns into each implementation.
///
/// `Debug` is a supertrait so containers that hold a `&dyn MetadataResolver`
/// (e.g. `TyLoweringContext` in `hir-ty`) can themselves derive `Debug`. Both
/// production implementations (`Configuration`, `ConfigsResolver`) already
/// derive it.
pub trait MetadataResolver: std::fmt::Debug {
    /// Returns the underlying [`AttributeType`] of the `<DefinedType>` named
    /// `name`, or `None` if no such DefinedType exists in this resolver's
    /// scope. Lookup is case-insensitive (delegated to the underlying
    /// `Configuration::find_defined_type`).
    fn resolve_defined_type(&self, name: &str) -> Option<&AttributeType>;
}

impl MetadataResolver for Configuration {
    fn resolve_defined_type(&self, name: &str) -> Option<&AttributeType> {
        self.find_defined_type(name).map(|dt| dt.underlying_type())
    }
}

/// Walk a `DefinedType → DefinedType → …` chain to its terminal
/// (non-`DefinedType`) base, with cycle protection.
///
/// `visited` accumulates the lowercase names of every DefinedType entered;
/// when a name is encountered twice the walk returns `None` rather than
/// recursing forever. The caller owns the set so the same guard composes
/// with outer recursion (e.g. `TyLoweringContext` lowering a `Composite`
/// whose arms are themselves `DefinedType`s).
///
/// Returns `None` when:
/// - the next DefinedType in the chain is not present in the resolver, or
/// - a cycle is detected.
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
        // A → B → Number
        let cfg = cfg_with(vec![
            dt("A", AttributeType::DefinedType { name: "B".to_string() }),
            dt("B", AttributeType::Number { precision: 10, scale: 0 }),
        ]);
        let mut visited = HashSet::new();
        let underlying = resolve_defined_type_terminal(&cfg, "A", &mut visited)
            .expect("chain must terminate at Number");
        assert!(matches!(underlying, AttributeType::Number { .. }));
        // visited records every entered DefinedType (canonical lowercase).
        assert!(visited.contains("a"));
        assert!(visited.contains("b"));
    }

    #[test]
    fn terminal_walk_breaks_simple_cycle() {
        // A → A — self-cycle must yield None, not stack overflow.
        let cfg = cfg_with(vec![dt("A", AttributeType::DefinedType { name: "A".to_string() })]);
        let mut visited = HashSet::new();
        assert!(resolve_defined_type_terminal(&cfg, "A", &mut visited).is_none());
    }

    #[test]
    fn terminal_walk_breaks_indirect_cycle() {
        // A → B → A — indirect cycle.
        let cfg = cfg_with(vec![
            dt("A", AttributeType::DefinedType { name: "B".to_string() }),
            dt("B", AttributeType::DefinedType { name: "A".to_string() }),
        ]);
        let mut visited = HashSet::new();
        assert!(resolve_defined_type_terminal(&cfg, "A", &mut visited).is_none());
    }

    #[test]
    fn terminal_walk_returns_none_when_chain_breaks() {
        // A → Missing — second hop unresolvable.
        let cfg =
            cfg_with(vec![dt("A", AttributeType::DefinedType { name: "Missing".to_string() })]);
        let mut visited = HashSet::new();
        assert!(resolve_defined_type_terminal(&cfg, "A", &mut visited).is_none());
    }

    #[test]
    fn terminal_walk_lookup_is_case_insensitive() {
        // Configuration::find_defined_type does case-insensitive lookup; the
        // walk must inherit that — `cfg:DefinedType.X` references in XML do
        // not always match the declared name's casing exactly.
        let cfg = cfg_with(vec![dt("ДенежнаяСумма", AttributeType::Boolean)]);
        let mut visited = HashSet::new();
        let underlying = resolve_defined_type_terminal(&cfg, "денежнаясумма", &mut visited)
            .expect("case-insensitive lookup");
        assert_eq!(underlying, &AttributeType::Boolean);
    }
}
