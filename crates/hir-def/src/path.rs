//! Qualified names (paths) for BSL.
//!
//! This module provides types for representing multi-segment qualified names
//! like `Module.Method()` or `Documents.PKO.Create()`.

use smallvec::SmallVec;

use crate::{MethodId, Name, VariableId};

/// Qualified name representing a multi-segment path (e.g., Module.Method).
///
/// # Examples
///
/// - Two-level: `ОбщийМодуль.Метод()` → `["ОбщийМодуль", "Метод"]`
/// - Three-level: `Документы.ПКО.Создать()` → `["Документы", "ПКО", "Создать"]`
///
/// # Implementation Notes
///
/// Uses `SmallVec<[Name; 2]>` to optimize for the common case of two segments
/// (Module.Method) without heap allocation. Three-segment paths are less common
/// and will use heap allocation when needed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QualifiedName {
    segments: SmallVec<[Name; 2]>,
}

impl QualifiedName {
    /// Creates a qualified name from an iterator of segments.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let name = QualifiedName::from_segments([
    ///     Name::new("ОбщийМодуль"),
    ///     Name::new("Метод"),
    /// ]);
    /// ```
    pub fn from_segments(segments: impl IntoIterator<Item = Name>) -> Self {
        Self { segments: segments.into_iter().collect() }
    }

    /// Returns all segments of this qualified name.
    pub fn segments(&self) -> &[Name] {
        &self.segments
    }

    /// Returns the first segment (typically the module name).
    ///
    /// # Panics
    ///
    /// Panics if the qualified name has no segments.
    pub fn first(&self) -> &Name {
        &self.segments[0]
    }

    /// Returns the last segment (typically the method or field name).
    ///
    /// # Panics
    ///
    /// Panics if the qualified name has no segments.
    pub fn last(&self) -> &Name {
        self.segments.last().expect("QualifiedName must have at least one segment")
    }

    /// Returns the number of segments in this qualified name.
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// Returns whether this qualified name is empty.
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

/// Result of path resolution.
///
/// Represents what a qualified name resolved to, or indicates that
/// resolution failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathResolution {
    /// Resolved to a method (procedure or function) in the workspace.
    Method(MethodId),

    /// Resolved to a variable (module-level variable).
    Variable(VariableId),

    /// Resolved to a platform builtin (global function, e.g. `Сообщить`).
    ///
    /// In BSL, platform globals have higher priority than local variables
    /// and cannot be shadowed by user code. Only single-segment paths may
    /// resolve to a `Builtin`.
    Builtin(Name),

    /// Could not resolve - the path is invalid or refers to a non-existent item.
    ///
    /// This can happen when:
    /// - The module doesn't exist
    /// - The method/variable doesn't exist in the module
    /// - The path is malformed
    Unresolved(QualifiedName),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qualified_name_creation() {
        let name = QualifiedName::from_segments([Name::new("Module"), Name::new("Method")]);

        assert_eq!(name.len(), 2);
        assert_eq!(name.first(), &Name::new("Module"));
        assert_eq!(name.last(), &Name::new("Method"));
    }

    #[test]
    fn test_qualified_name_three_segments() {
        let name = QualifiedName::from_segments([
            Name::new("Документы"),
            Name::new("ПКО"),
            Name::new("Создать"),
        ]);

        assert_eq!(name.len(), 3);
        assert_eq!(name.segments()[0], Name::new("Документы"));
        assert_eq!(name.segments()[1], Name::new("ПКО"));
        assert_eq!(name.segments()[2], Name::new("Создать"));
    }

    #[test]
    fn test_qualified_name_preserves_case() {
        // Name preserves original case for display purposes
        let name1 = QualifiedName::from_segments([Name::new("ОбщийМодуль"), Name::new("Метод")]);
        let name2 = QualifiedName::from_segments([Name::new("общиймодуль"), Name::new("метод")]);

        // Names with different cases are not equal (use Name::eq_ignore_case for case-insensitive comparison)
        assert_ne!(name1, name2);

        // But same case names are equal
        let name3 = QualifiedName::from_segments([Name::new("ОбщийМодуль"), Name::new("Метод")]);
        assert_eq!(name1, name3);
    }
}
